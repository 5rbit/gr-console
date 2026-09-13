//! Byte-exact wire JSON value text (wire-spec section 2) <-> standard-layout UDT bytes.
//!
//! The writer walks the UDT declaration together with the standard-layout bytes (same alignment rules as
//! `plc_layout::layout`) and writes compact JSON in declaration order. The reader converts any valid JSON
//! value into the UDT bytes with [`Strictness::Strict`] (API input: reject anything questionable) or
//! [`Strictness::Lenient`] (coerce and warn).

use std::ops::Range;

use serde::Serialize;
use serde_json::Value;

use plc_layout::Contract;
use plc_layout::ast::{Bound, Field, Prim, TypeRef, UdtDecl};
use plc_layout::encode::{dtl_bytes, fraction_ns, latin1_byte};
use plc_layout::layout::{Cursor, resolve_dims, size_of};

use crate::error::{ErrCode, LinkError};

/// DTL text used for `""` / `null` / defaulted DTL members.
pub const DTL_EPOCH_TEXT: &str = "1970-01-01T00:00:00.000";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Strictness {
    Strict,
    Lenient,
}

/// Conversion notes: members missing from the input (zero value written) and lenient coercions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    pub defaulted: Vec<String>,
    pub warnings: Vec<String>,
}

impl Report {
    pub fn is_clean(&self) -> bool {
        self.defaulted.is_empty() && self.warnings.is_empty()
    }
}

// ---------------------------------------------------------------------------------------------------------
// scalar text

/// Real/LReal text (wire-spec section 2): `null` for NaN/Inf, 4 fraction digits trimmed (min one),
/// exponent form for |x| >= 1e9.
pub fn fmt_real(x: f64) -> String {
    if !x.is_finite() {
        return "null".to_string();
    }
    let mut out = String::with_capacity(16);
    if x.abs() < 1e9 {
        let n = (x * 10000.0).round() as i64;
        if n == 0 {
            return "0.0".to_string();
        }
        if n < 0 {
            out.push('-');
        }
        let a = n.unsigned_abs();
        out.push_str(&(a / 10000).to_string());
        out.push('.');
        push_frac4(&mut out, a % 10000);
    } else {
        let a = x.abs();
        // e and 10^e by repeated ×10 from 1.0, exactly like the PLC (Json_WLReal)
        let (mut e, mut p) = (0i32, 1.0f64);
        while p * 10.0 <= a && e < 400 {
            p *= 10.0;
            e += 1;
        }
        let m = a / p;
        let mut k = (m * 10000.0).round() as i64;
        if k >= 100_000 {
            k /= 10;
            e += 1;
        }
        if x < 0.0 {
            out.push('-');
        }
        out.push_str(&(k / 10000).to_string());
        out.push('.');
        push_frac4(&mut out, (k % 10000) as u64);
        out.push('e');
        out.push_str(&e.to_string());
    }
    out
}

fn push_frac4(out: &mut String, fp: u64) {
    let s = format!("{fp:04}");
    let t = s.trim_end_matches('0');
    out.push_str(if t.is_empty() { "0" } else { t });
}

/// DTL text without the quotes: `YYYY-MM-DDTHH:MM:SS.mmm`. Every field has its fixed width (out-of-range
/// bytes are written modulo the field width, so the text is always 23 characters).
pub fn fmt_dtl(b: &[u8; 12]) -> String {
    let year = u16::from_be_bytes([b[0], b[1]]) % 10000;
    let ns = u32::from_be_bytes([b[8], b[9], b[10], b[11]]);
    format!("{year:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}", b[2] % 100, b[3] % 100, b[5] % 100, b[6] % 100, b[7] % 100, (ns / 1_000_000) % 1000)
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

fn push_escaped_byte(out: &mut String, b: u8) {
    match b {
        b'"' => out.push_str("\\\""),
        b'\\' => out.push_str("\\\\"),
        0x20..=0x7E => out.push(b as char),
        _ => {
            out.push_str("\\u00");
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0F) as usize] as char);
        }
    }
}

/// String/Char bytes as JSON string content without the quotes: `"` → `\"`, `\` → `\\`, bytes `< 0x20` or
/// `>= 0x7F` → `\u00XX` (upper-case hex). Output is pure ASCII.
pub fn escape_str(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        push_escaped_byte(&mut out, b);
    }
    out
}

/// Member name as a JSON object key followed by `:` (names are written verbatim except `"`, `\`, controls).
fn push_key(out: &mut String, name: &str) {
    out.push('"');
    push_name(out, name);
    out.push_str("\":");
}

pub(crate) fn push_name(out: &mut String, name: &str) {
    for ch in name.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                out.push_str("\\u00");
                out.push(HEX[(c as usize) >> 4] as char);
                out.push(HEX[(c as usize) & 0x0F] as char);
            }
            c => out.push(c),
        }
    }
}

fn key_len(name: &str) -> usize {
    let mut s = String::new();
    push_key(&mut s, name);
    s.len()
}

/// Parses DTL text: `YYYY-MM-DD` + `T` or space + `HH:MM:SS` + optional `.` and 0–9 fraction digits.
/// Validates ranges (year 1970..=2262, real calendar day) and computes the weekday.
pub fn parse_dtl_text(s: &str) -> Option<[u8; 12]> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let num = |r: Range<usize>| -> Option<u32> {
        let t = &b[r];
        if !t.iter().all(u8::is_ascii_digit) {
            return None;
        }
        Some(t.iter().fold(0u32, |acc, d| acc * 10 + (d - b'0') as u32))
    };
    if b[4] != b'-' || b[7] != b'-' || !(b[10] == b'T' || b[10] == b' ') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (year, mon, day, hour, min, sec) = (num(0..4)?, num(5..7)?, num(8..10)?, num(11..13)?, num(14..16)?, num(17..19)?);
    let rest = &b[19..];
    let ns = if rest.is_empty() {
        0
    } else {
        let frac = rest.strip_prefix(b".")?;
        if frac.len() > 9 || !frac.iter().all(u8::is_ascii_digit) {
            return None;
        }
        fraction_ns(std::str::from_utf8(frac).ok()?)
    };
    if !(1970..=2262).contains(&year) || !(1..=12).contains(&mon) || day < 1 || day > days_in_month(year, mon) || hour > 23 || min > 59 || sec > 59 {
        return None;
    }
    Some(dtl_bytes(year as u16, mon as u8, day as u8, hour as u8, min as u8, sec as u8, ns))
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400) => 29,
        _ => 28,
    }
}

/// DTL bytes of `DTL#1970-01-01-00:00:00` (weekday Thursday).
pub fn dtl_epoch() -> [u8; 12] {
    dtl_bytes(1970, 1, 1, 0, 0, 0, 0)
}

// ---------------------------------------------------------------------------------------------------------
// shared helpers

/// Date, TOD, DATE_AND_TIME, LTime (and LDT, which parses as LTime) cannot be carried.
pub fn is_supported(p: Prim) -> bool {
    !matches!(p, Prim::Date | Prim::Tod | Prim::DateAndTime | Prim::LTime)
}

fn unsupported(path: &str, p: Prim) -> LinkError {
    LinkError::at(ErrCode::Parse, display_path(path), format!("type {} is not supported on the link", p.name()))
}

fn display_path(path: &str) -> String {
    if path.is_empty() { "<root>".to_string() } else { path.to_string() }
}

fn join(path: &str, name: &str) -> String {
    if path.is_empty() { name.to_string() } else { format!("{path}.{name}") }
}

fn udt_decl<'c>(c: &'c Contract, name: &str, path: &str) -> Result<&'c UdtDecl, LinkError> {
    c.udt(name).map_err(|_| LinkError::at(ErrCode::UnknownType, display_path(path), format!("unknown UDT \"{name}\"")))
}

fn dims_of(c: &Contract, dims: &[(Bound, Bound)], path: &str) -> Result<Vec<(i64, i64)>, LinkError> {
    resolve_dims(c, dims).map_err(|e| LinkError::at(ErrCode::Parse, display_path(path), e.to_string()))
}

fn count(dims: &[(i64, i64)]) -> usize {
    dims.iter().map(|(lo, hi)| (hi - lo + 1).max(0) as usize).product()
}

fn align(cur: &mut Cursor, p: Prim) {
    match p.align() {
        0 => {}
        1 => cur.align_byte(),
        _ => cur.align_word(),
    }
}

fn advance(cur: &mut Cursor, p: Prim) {
    if p == Prim::Bool {
        cur.bit += 1;
        if cur.bit == 8 {
            cur.bit = 0;
            cur.byte += 1;
        }
    } else {
        cur.byte += p.size();
    }
}

/// Fails with the member path of the first unsupported elementary type (or unknown UDT / constant).
pub fn check_supported(c: &Contract, ty: &TypeRef) -> Result<(), LinkError> {
    fn go(c: &Contract, ty: &TypeRef, path: &str) -> Result<(), LinkError> {
        match ty {
            TypeRef::Prim(p) => {
                if is_supported(*p) {
                    Ok(())
                } else {
                    Err(unsupported(path, *p))
                }
            }
            TypeRef::Udt(n) => udt_decl(c, n, path)?.fields.iter().try_for_each(|f| go(c, &f.ty, &join(path, &f.name))),
            TypeRef::Struct(fields) => fields.iter().try_for_each(|f| go(c, &f.ty, &join(path, &f.name))),
            TypeRef::Array { dims, elem } => {
                dims_of(c, dims, path)?;
                go(c, elem, &format!("{path}[]"))
            }
        }
    }
    go(c, ty, "")
}

// ---------------------------------------------------------------------------------------------------------
// writer

/// Wire JSON text of a value of type `ty` laid out from byte 0 of `buf`.
pub fn bytes_to_json_text(c: &Contract, ty: &TypeRef, buf: &[u8]) -> Result<String, LinkError> {
    let mut w = Writer { c, buf, out: String::with_capacity(256) };
    let mut cur = Cursor::default();
    w.ty(ty, &mut cur, "")?;
    Ok(w.out)
}

/// Wire JSON text of a UDT value (`buf` = the UDT bytes).
pub fn udt_bytes_to_json_text(c: &Contract, udt: &str, buf: &[u8]) -> Result<String, LinkError> {
    bytes_to_json_text(c, &TypeRef::Udt(udt.to_string()), buf)
}

struct Writer<'a> {
    c: &'a Contract,
    buf: &'a [u8],
    out: String,
}

fn slice<'b>(buf: &'b [u8], at: u32, n: u32, path: &str) -> Result<&'b [u8], LinkError> {
    let (s, e) = (at as usize, at as usize + n.max(1) as usize);
    buf.get(s..e).ok_or_else(|| LinkError::at(ErrCode::BadLength, display_path(path), format!("buffer too short: need {e} bytes, have {}", buf.len())))
}

impl Writer<'_> {
    fn ty(&mut self, ty: &TypeRef, cur: &mut Cursor, path: &str) -> Result<(), LinkError> {
        match ty {
            TypeRef::Prim(p) => self.prim(*p, cur, path),
            TypeRef::Udt(n) => {
                let c = self.c;
                let u = udt_decl(c, n, path)?;
                self.fields(&u.fields, cur, path)
            }
            TypeRef::Struct(fields) => self.fields(fields, cur, path),
            TypeRef::Array { dims, elem } => {
                cur.align_word();
                let dims = dims_of(self.c, dims, path)?;
                self.array(elem, &dims, cur, path)?;
                cur.align_word();
                Ok(())
            }
        }
    }

    fn fields(&mut self, fields: &[Field], cur: &mut Cursor, path: &str) -> Result<(), LinkError> {
        cur.align_word();
        self.out.push('{');
        for (i, f) in fields.iter().enumerate() {
            if i > 0 {
                self.out.push(',');
            }
            push_key(&mut self.out, &f.name);
            self.ty(&f.ty, cur, &join(path, &f.name))?;
        }
        self.out.push('}');
        cur.align_word();
        Ok(())
    }

    fn array(&mut self, elem: &TypeRef, dims: &[(i64, i64)], cur: &mut Cursor, path: &str) -> Result<(), LinkError> {
        let (lo, hi) = dims[0];
        self.out.push('[');
        for i in lo..=hi {
            if i > lo {
                self.out.push(',');
            }
            let p = format!("{path}[{i}]");
            if dims.len() > 1 {
                self.array(elem, &dims[1..], cur, &p)?;
            } else {
                self.ty(elem, cur, &p)?;
            }
        }
        self.out.push(']');
        Ok(())
    }

    fn prim(&mut self, p: Prim, cur: &mut Cursor, path: &str) -> Result<(), LinkError> {
        if !is_supported(p) {
            return Err(unsupported(path, p));
        }
        align(cur, p);
        let b = slice(self.buf, cur.byte, p.size(), path)?;
        let be16 = || u16::from_be_bytes([b[0], b[1]]);
        let be32 = || u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        let be64 = || u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
        let out = &mut self.out;
        match p {
            Prim::Bool => out.push_str(if (b[0] >> cur.bit) & 1 == 1 { "true" } else { "false" }),
            Prim::Byte | Prim::USInt => out.push_str(&b[0].to_string()),
            Prim::SInt => out.push_str(&(b[0] as i8).to_string()),
            Prim::Char => {
                out.push('"');
                push_escaped_byte(out, b[0]);
                out.push('"');
            }
            Prim::Word | Prim::UInt => out.push_str(&be16().to_string()),
            Prim::Int => out.push_str(&(be16() as i16).to_string()),
            Prim::DWord | Prim::UDInt => out.push_str(&be32().to_string()),
            Prim::DInt | Prim::Time => out.push_str(&(be32() as i32).to_string()),
            Prim::Real => out.push_str(&fmt_real(f32::from_bits(be32()) as f64)),
            Prim::LReal => out.push_str(&fmt_real(f64::from_bits(be64()))),
            Prim::LInt => out.push_str(&(be64() as i64).to_string()),
            Prim::ULInt | Prim::LWord => out.push_str(&be64().to_string()),
            Prim::Dtl => {
                let mut d = [0u8; 12];
                d.copy_from_slice(&b[..12]);
                out.push('"');
                out.push_str(&fmt_dtl(&d));
                out.push('"');
            }
            Prim::String(n) => {
                let len = (b[1] as usize).min(n as usize);
                out.push('"');
                for &x in &b[2..2 + len] {
                    push_escaped_byte(out, x);
                }
                out.push('"');
            }
            Prim::Date | Prim::Tod | Prim::DateAndTime | Prim::LTime => unreachable!("checked above"),
        }
        advance(cur, p);
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------------------
// worst-case length

/// Longest wire text of an elementary type (`None` for unsupported types).
pub fn prim_max_len(p: Prim) -> Option<usize> {
    Some(match p {
        Prim::Bool => 5,
        Prim::Byte | Prim::USInt => 3,
        Prim::SInt => 4,
        Prim::Char => 8,
        Prim::Word | Prim::UInt => 5,
        Prim::Int => 6,
        Prim::DWord | Prim::UDInt => 10,
        Prim::DInt | Prim::Time => 11,
        Prim::Real | Prim::LReal => 15,
        Prim::LInt | Prim::ULInt | Prim::LWord => 20,
        Prim::Dtl => 25,
        Prim::String(n) => 2 + 6 * n as usize,
        Prim::Date | Prim::Tod | Prim::DateAndTime | Prim::LTime => return None,
    })
}

/// Longest possible wire text of a value of type `ty`.
pub fn max_json_len(c: &Contract, ty: &TypeRef) -> Result<usize, LinkError> {
    fn go(c: &Contract, ty: &TypeRef, path: &str) -> Result<usize, LinkError> {
        match ty {
            TypeRef::Prim(p) => prim_max_len(*p).ok_or_else(|| unsupported(path, *p)),
            TypeRef::Udt(n) => fields_len(c, &udt_decl(c, n, path)?.fields, path),
            TypeRef::Struct(fields) => fields_len(c, fields, path),
            TypeRef::Array { dims, elem } => {
                let dims = dims_of(c, dims, path)?;
                let mut len = go(c, elem, &format!("{path}[]"))?;
                for (lo, hi) in dims.iter().rev() {
                    let n = (hi - lo + 1).max(0) as usize;
                    len = 2 + n * len + n.saturating_sub(1);
                }
                Ok(len)
            }
        }
    }
    fn fields_len(c: &Contract, fields: &[Field], path: &str) -> Result<usize, LinkError> {
        let mut len = 2 + fields.len().saturating_sub(1);
        for f in fields {
            len += key_len(&f.name) + go(c, &f.ty, &join(path, &f.name))?;
        }
        Ok(len)
    }
    go(c, ty, "")
}

// ---------------------------------------------------------------------------------------------------------
// reader

/// JSON value → UDT bytes (standard layout). Missing keys get the type's zero value (String header max
/// length set, DTL = 1970-01-01) and are listed in `Report::defaulted`.
pub fn json_to_udt_bytes(c: &Contract, udt: &str, v: &Value, s: Strictness) -> Result<(Vec<u8>, Report), LinkError> {
    json_to_bytes(c, &TypeRef::Udt(udt.to_string()), v, s)
}

/// JSON value → bytes of any type (buffer size = `size_of(ty)`).
pub fn json_to_bytes(c: &Contract, ty: &TypeRef, v: &Value, s: Strictness) -> Result<(Vec<u8>, Report), LinkError> {
    check_supported(c, ty)?;
    let size = size_of(c, ty)? as usize;
    let mut r = Reader { c, strict: s == Strictness::Strict, buf: vec![0u8; size], report: Report::default() };
    let mut cur = Cursor::default();
    r.ty(ty, v, &mut cur, "")?;
    Ok((r.buf, r.report))
}

/// Zero value bytes of a UDT (what a missing key produces).
pub fn zero_udt_bytes(c: &Contract, udt: &str) -> Result<Vec<u8>, LinkError> {
    let ty = TypeRef::Udt(udt.to_string());
    check_supported(c, &ty)?;
    let size = size_of(c, &ty)? as usize;
    let mut r = Reader { c, strict: true, buf: vec![0u8; size], report: Report::default() };
    r.zero(&ty, &mut Cursor::default(), "")?;
    Ok(r.buf)
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

struct Reader<'a> {
    c: &'a Contract,
    strict: bool,
    buf: Vec<u8>,
    report: Report,
}

impl Reader<'_> {
    fn err(&self, path: &str, msg: impl Into<String>) -> LinkError {
        LinkError::at(ErrCode::Parse, display_path(path), msg)
    }

    /// Strict: error. Lenient: warning, continue.
    fn complain(&mut self, path: &str, msg: String) -> Result<(), LinkError> {
        if self.strict {
            Err(self.err(path, msg))
        } else {
            self.report.warnings.push(format!("{}: {msg}", display_path(path)));
            Ok(())
        }
    }

    fn ty(&mut self, ty: &TypeRef, v: &Value, cur: &mut Cursor, path: &str) -> Result<(), LinkError> {
        match ty {
            TypeRef::Prim(p) => self.prim(*p, v, cur, path),
            TypeRef::Udt(n) => {
                let c = self.c;
                let u = udt_decl(c, n, path)?;
                self.fields(&u.fields, v, cur, path)
            }
            TypeRef::Struct(fields) => self.fields(fields, v, cur, path),
            TypeRef::Array { dims, elem } => {
                cur.align_word();
                let dims = dims_of(self.c, dims, path)?;
                self.array(elem, &dims, v, cur, path)?;
                cur.align_word();
                Ok(())
            }
        }
    }

    fn fields(&mut self, fields: &[Field], v: &Value, cur: &mut Cursor, path: &str) -> Result<(), LinkError> {
        cur.align_word();
        let obj = match v {
            Value::Object(m) => Some(m),
            other => {
                self.complain(path, format!("expected object, got {}", kind(other)))?;
                self.report.defaulted.push(display_path(path));
                None
            }
        };
        if let Some(m) = obj {
            for k in m.keys() {
                if !fields.iter().any(|f| f.name == *k) {
                    self.complain(&join(path, k), "unknown key".to_string())?;
                }
            }
        }
        for f in fields {
            let p = join(path, &f.name);
            match obj.and_then(|m| m.get(&f.name)) {
                Some(x) => self.ty(&f.ty, x, cur, &p)?,
                None => {
                    if obj.is_some() {
                        self.report.defaulted.push(p.clone());
                    }
                    self.zero(&f.ty, cur, &p)?;
                }
            }
        }
        cur.align_word();
        Ok(())
    }

    fn array(&mut self, elem: &TypeRef, dims: &[(i64, i64)], v: &Value, cur: &mut Cursor, path: &str) -> Result<(), LinkError> {
        let (lo, hi) = dims[0];
        let n = (hi - lo + 1).max(0) as usize;
        let items = match v {
            Value::Array(a) => Some(a),
            other => {
                self.complain(path, format!("expected array, got {}", kind(other)))?;
                self.report.defaulted.push(display_path(path));
                None
            }
        };
        if let Some(a) = items
            && a.len() > n
        {
            self.complain(path, format!("array has {} elements, at most {n} allowed", a.len()))?;
        }
        for (k, i) in (lo..=hi).enumerate() {
            let p = format!("{path}[{i}]");
            match items.and_then(|a| a.get(k)) {
                Some(x) if dims.len() > 1 => self.array(elem, &dims[1..], x, cur, &p)?,
                Some(x) => self.ty(elem, x, cur, &p)?,
                None => {
                    for _ in 0..count(&dims[1..]) {
                        self.zero(elem, cur, &p)?;
                    }
                }
            }
        }
        if let Some(a) = items
            && a.len() < n
        {
            self.report.defaulted.push(format!("{path}[{}..{hi}]", lo + a.len() as i64));
        }
        Ok(())
    }

    /// Writes the zero value of `ty` (buffer is zero already except String headers and DTL).
    fn zero(&mut self, ty: &TypeRef, cur: &mut Cursor, path: &str) -> Result<(), LinkError> {
        match ty {
            TypeRef::Prim(p) => {
                align(cur, *p);
                let at = cur.byte as usize;
                match p {
                    Prim::String(n) => self.buf[at] = *n as u8,
                    Prim::Dtl => self.buf[at..at + 12].copy_from_slice(&dtl_epoch()),
                    _ => {}
                }
                advance(cur, *p);
            }
            TypeRef::Udt(n) => {
                let c = self.c;
                let u = udt_decl(c, n, path)?;
                cur.align_word();
                for f in &u.fields {
                    self.zero(&f.ty, cur, path)?;
                }
                cur.align_word();
            }
            TypeRef::Struct(fields) => {
                cur.align_word();
                for f in fields {
                    self.zero(&f.ty, cur, path)?;
                }
                cur.align_word();
            }
            TypeRef::Array { dims, elem } => {
                cur.align_word();
                let dims = dims_of(self.c, dims, path)?;
                for _ in 0..count(&dims) {
                    self.zero(elem, cur, path)?;
                }
                cur.align_word();
            }
        }
        Ok(())
    }

    fn prim(&mut self, p: Prim, v: &Value, cur: &mut Cursor, path: &str) -> Result<(), LinkError> {
        if !is_supported(p) {
            return Err(unsupported(path, p));
        }
        align(cur, p);
        let at = cur.byte as usize;
        match p {
            Prim::Bool => {
                if self.boolean(v, path)? {
                    self.buf[at] |= 1 << cur.bit;
                }
            }
            Prim::Byte | Prim::USInt => self.buf[at] = self.int(v, path, 0, 255)? as u8,
            Prim::SInt => self.buf[at] = self.int(v, path, -128, 127)? as i8 as u8,
            Prim::Char => self.buf[at] = self.character(v, path)?,
            Prim::Word | Prim::UInt => {
                let x = self.int(v, path, 0, u16::MAX as i128)? as u16;
                self.buf[at..at + 2].copy_from_slice(&x.to_be_bytes());
            }
            Prim::Int => {
                let x = self.int(v, path, i16::MIN as i128, i16::MAX as i128)? as i16;
                self.buf[at..at + 2].copy_from_slice(&x.to_be_bytes());
            }
            Prim::DWord | Prim::UDInt => {
                let x = self.int(v, path, 0, u32::MAX as i128)? as u32;
                self.buf[at..at + 4].copy_from_slice(&x.to_be_bytes());
            }
            Prim::DInt | Prim::Time => {
                let x = self.int(v, path, i32::MIN as i128, i32::MAX as i128)? as i32;
                self.buf[at..at + 4].copy_from_slice(&x.to_be_bytes());
            }
            Prim::LInt => {
                let x = self.int(v, path, i64::MIN as i128, i64::MAX as i128)? as i64;
                self.buf[at..at + 8].copy_from_slice(&x.to_be_bytes());
            }
            Prim::ULInt | Prim::LWord => {
                let x = self.int(v, path, 0, u64::MAX as i128)? as u64;
                self.buf[at..at + 8].copy_from_slice(&x.to_be_bytes());
            }
            Prim::Real => {
                let x = self.real(v, path, true)? as f32;
                self.buf[at..at + 4].copy_from_slice(&x.to_bits().to_be_bytes());
            }
            Prim::LReal => {
                let x = self.real(v, path, false)?;
                self.buf[at..at + 8].copy_from_slice(&x.to_bits().to_be_bytes());
            }
            Prim::Dtl => {
                let d = self.dtl(v, path)?;
                self.buf[at..at + 12].copy_from_slice(&d);
            }
            Prim::String(n) => {
                let bytes = self.string(v, path, n as usize)?;
                self.buf[at] = n as u8;
                self.buf[at + 1] = bytes.len() as u8;
                self.buf[at + 2..at + 2 + bytes.len()].copy_from_slice(&bytes);
            }
            Prim::Date | Prim::Tod | Prim::DateAndTime | Prim::LTime => unreachable!("checked above"),
        }
        advance(cur, p);
        Ok(())
    }

    fn boolean(&mut self, v: &Value, path: &str) -> Result<bool, LinkError> {
        if let Value::Bool(b) = v {
            return Ok(*b);
        }
        self.complain(path, format!("expected true/false, got {}", kind(v)))?;
        Ok(match v {
            Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
            Value::String(s) => matches!(s.trim().to_ascii_lowercase().as_str(), "true" | "1"),
            _ => false,
        })
    }

    fn int(&mut self, v: &Value, path: &str, min: i128, max: i128) -> Result<i128, LinkError> {
        let x: i128 = match v {
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    i as i128
                } else if let Some(u) = n.as_u64() {
                    u as i128
                } else {
                    let mut f = n.as_f64().unwrap_or(0.0);
                    if f.fract() != 0.0 {
                        self.complain(path, format!("expected integer, got {f}"))?;
                        f = f.round();
                    }
                    if f < min as f64 || f > max as f64 {
                        self.complain(path, format!("{f} out of range {min}..{max}"))?;
                        return Ok(if f < min as f64 { min } else { max });
                    }
                    f as i128
                }
            }
            Value::String(s) if !self.strict => {
                let t = s.trim();
                let parsed = t.parse::<i128>().ok().or_else(|| t.parse::<f64>().ok().filter(|f| f.is_finite()).map(|f| f.round() as i128));
                self.complain(path, format!("expected integer, got string {s:?}"))?;
                parsed.unwrap_or(0)
            }
            Value::Bool(b) if !self.strict => {
                self.complain(path, "expected integer, got boolean".to_string())?;
                *b as i128
            }
            other => {
                self.complain(path, format!("expected integer, got {}", kind(other)))?;
                0
            }
        };
        if x < min || x > max {
            self.complain(path, format!("{x} out of range {min}..{max}"))?;
            return Ok(x.clamp(min, max));
        }
        Ok(x)
    }

    fn real(&mut self, v: &Value, path: &str, single: bool) -> Result<f64, LinkError> {
        let x = match v {
            Value::Number(n) => n.as_f64().unwrap_or(0.0),
            Value::Null => {
                self.complain(path, "null is not a number".to_string())?;
                0.0
            }
            Value::String(s) if !self.strict => {
                self.complain(path, format!("expected number, got string {s:?}"))?;
                s.trim().parse::<f64>().unwrap_or(0.0)
            }
            Value::Bool(b) if !self.strict => {
                self.complain(path, "expected number, got boolean".to_string())?;
                if *b { 1.0 } else { 0.0 }
            }
            other => {
                self.complain(path, format!("expected number, got {}", kind(other)))?;
                0.0
            }
        };
        if single && x.is_finite() && x.abs() > f32::MAX as f64 {
            self.complain(path, format!("{x} out of Real range"))?;
            return Ok(x.signum() * f32::MAX as f64);
        }
        Ok(x)
    }

    fn character(&mut self, v: &Value, path: &str) -> Result<u8, LinkError> {
        match v {
            Value::String(s) => {
                let mut chars = s.chars();
                let first = chars.next();
                let extra = chars.next().is_some();
                match first {
                    Some(ch) if !extra && (ch as u32) <= 0xFF => Ok(ch as u32 as u8),
                    Some(ch) => {
                        if extra {
                            self.complain(path, format!("expected one character, got {s:?}"))?;
                        }
                        if (ch as u32) > 0xFF {
                            self.complain(path, format!("character U+{:04X} is not representable", ch as u32))?;
                        }
                        Ok(latin1_byte(ch))
                    }
                    None => {
                        self.complain(path, "expected one character, got empty string".to_string())?;
                        Ok(0)
                    }
                }
            }
            Value::Number(_) if !self.strict => self.int(v, path, 0, 255).map(|x| x as u8),
            other => {
                self.complain(path, format!("expected one-character string, got {}", kind(other)))?;
                Ok(0)
            }
        }
    }

    fn string(&mut self, v: &Value, path: &str, n: usize) -> Result<Vec<u8>, LinkError> {
        let text = match v {
            Value::String(s) => s.clone(),
            other => {
                self.complain(path, format!("expected string, got {}", kind(other)))?;
                match other {
                    Value::Number(x) => x.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => String::new(),
                }
            }
        };
        if let Some(ch) = text.chars().find(|c| (*c as u32) > 0xFF) {
            self.complain(path, format!("character U+{:04X} is not representable", ch as u32))?;
        }
        let len = text.chars().count();
        if len > n {
            self.complain(path, format!("string has {len} characters, at most {n} allowed"))?;
        }
        Ok(text.chars().take(n).map(latin1_byte).collect())
    }

    fn dtl(&mut self, v: &Value, path: &str) -> Result<[u8; 12], LinkError> {
        match v {
            Value::Null => Ok(dtl_epoch()),
            Value::String(s) if s.is_empty() => Ok(dtl_epoch()),
            Value::String(s) => match parse_dtl_text(s) {
                Some(d) => Ok(d),
                None => {
                    self.complain(path, format!("invalid DTL {s:?} (expected YYYY-MM-DDTHH:MM:SS.fff)"))?;
                    Ok(dtl_epoch())
                }
            },
            other => {
                self.complain(path, format!("expected DTL string, got {}", kind(other)))?;
                Ok(dtl_epoch())
            }
        }
    }
}
