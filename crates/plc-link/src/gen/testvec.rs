//! `LNK_TestVectors` standard-access global DB for the PLC self-test: per payload message the golden value
//! (`<Msg>_Val`, BEGIN start values), its Serialize bytes (`<Msg>_Bin`) and the canonical envelope text with seq 1
//! (`<Msg>_JsonLen`, `<Msg>_Json` in 200-character chunks). Values come from [`crate::vectors`].

use plc_layout::Contract;
use plc_layout::ast::{Field, Prim, TypeRef};
use plc_layout::layout::resolve_dims;
use serde_json::Value;

use crate::envelope::write_envelope;
use crate::error::{ErrCode, LinkError};
use crate::registry::MessageSpec;
use crate::vectors::{golden_bytes, pattern_value};
use crate::wire_json::{DTL_EPOCH_TEXT, parse_dtl_text, udt_bytes_to_json_text};

use super::names::{member, scl_str};
use super::text::GENERATED_MARKER;

/// Upper limit of the DB's standard-access size.
pub const TEST_DB_MAX_BYTES: u32 = 60_000;
/// Characters per `<Msg>_Json` element.
pub const JSON_CHUNK: usize = 200;

/// Seq of the envelope text in the vectors.
pub const VECTOR_SEQ: u16 = 1;

struct Leaf {
    /// Golden value path (`Cmd.Position[1]`, `Grid[1][0]`).
    vpath: String,
    /// SCL start value path (`MeasLog_Val.Cmd.Position[1]`, `Fixture_Val.Grid[1,0]`).
    spath: String,
    prim: Prim,
    /// The declaration has no start value (or a zero one), so a zero golden value needs no BEGIN line.
    default_zero: bool,
}

fn init_is_zero(init: Option<&str>) -> bool {
    let Some(s) = init else { return true };
    let t = s.trim();
    if t == "''" || t.eq_ignore_ascii_case("false") {
        return true;
    }
    if let Ok(f) = t.replace('_', "").parse::<f64>() {
        return f == 0.0;
    }
    t.contains('#') && plc_layout::consts::parse_literal(t) == Some(0)
}

fn flatten_fields(c: &Contract, fields: &[Field], vpath: &str, spath: &str, out: &mut Vec<Leaf>) -> Result<(), LinkError> {
    for f in fields {
        let vp = if vpath.is_empty() { f.name.clone() } else { format!("{vpath}.{}", f.name) };
        let sp = format!("{spath}.{}", member(&f.name));
        flatten(c, &f.ty, &vp, &sp, init_is_zero(f.init.as_deref()), out)?;
    }
    Ok(())
}

fn flatten(c: &Contract, ty: &TypeRef, vpath: &str, spath: &str, default_zero: bool, out: &mut Vec<Leaf>) -> Result<(), LinkError> {
    match ty {
        TypeRef::Prim(p) => out.push(Leaf { vpath: vpath.to_string(), spath: spath.to_string(), prim: *p, default_zero }),
        TypeRef::Udt(n) => {
            let u = c.udt(n)?;
            flatten_fields(c, &u.fields, vpath, spath, out)?;
        }
        TypeRef::Struct(fields) => flatten_fields(c, fields, vpath, spath, out)?,
        TypeRef::Array { dims, elem } => {
            let dims = resolve_dims(c, dims)?;
            flatten_array(c, &dims, elem, vpath, spath, &mut Vec::new(), default_zero, out)?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn flatten_array(c: &Contract, dims: &[(i64, i64)], elem: &TypeRef, vpath: &str, spath: &str, idx: &mut Vec<i64>, default_zero: bool, out: &mut Vec<Leaf>) -> Result<(), LinkError> {
    let (lo, hi) = dims[0];
    for i in lo..=hi {
        idx.push(i);
        let vp = format!("{vpath}[{i}]");
        if dims.len() > 1 {
            flatten_array(c, &dims[1..], elem, &vp, spath, idx, default_zero, out)?;
        } else {
            let sp = format!("{spath}[{}]", idx.iter().map(i64::to_string).collect::<Vec<_>>().join(","));
            flatten(c, elem, &vp, &sp, default_zero, out)?;
        }
        idx.pop();
    }
    Ok(())
}

fn is_zero(p: Prim, v: &Value) -> bool {
    match p {
        Prim::Bool => v == &Value::Bool(false),
        Prim::Char => v.as_str() == Some("\u{0}"),
        Prim::String(_) => v.as_str() == Some(""),
        Prim::Dtl => v.as_str() == Some(DTL_EPOCH_TEXT),
        _ => v.as_f64() == Some(0.0),
    }
}

/// TIA duration literal (`T#-1m_2s_345ms`, `T#0ms`).
fn tia_time(ms: i64) -> String {
    if ms == 0 {
        return "T#0ms".to_string();
    }
    let sign = if ms < 0 { "-" } else { "" };
    let mut a = ms.unsigned_abs();
    let mut parts = Vec::new();
    for (unit, suffix) in [(86_400_000u64, "d"), (3_600_000, "h"), (60_000, "m"), (1000, "s"), (1, "ms")] {
        let n = a / unit;
        a %= unit;
        if n > 0 {
            parts.push(format!("{n}{suffix}"));
        }
    }
    format!("T#{sign}{}", parts.join("_"))
}

fn literal(p: Prim, v: &Value, path: &str) -> Result<String, LinkError> {
    let u = v.as_u64().unwrap_or(0);
    let i = v.as_i64().unwrap_or(0);
    let s = v.as_str().unwrap_or("");
    Ok(match p {
        Prim::Bool => (if v.as_bool() == Some(true) { "TRUE" } else { "FALSE" }).to_string(),
        Prim::Byte => format!("16#{u:02X}"),
        Prim::Word => format!("16#{u:04X}"),
        Prim::DWord => format!("16#{:04X}_{:04X}", u >> 16, u & 0xFFFF),
        Prim::LWord => format!("16#{:04X}_{:04X}_{:04X}_{:04X}", u >> 48, (u >> 32) & 0xFFFF, (u >> 16) & 0xFFFF, u & 0xFFFF),
        Prim::USInt | Prim::UInt | Prim::UDInt | Prim::ULInt => u.to_string(),
        Prim::SInt | Prim::Int | Prim::DInt | Prim::LInt => i.to_string(),
        Prim::Real | Prim::LReal => format!("{:?}", v.as_f64().unwrap_or(0.0)),
        Prim::Char | Prim::String(_) => format!("'{}'", scl_str(s)),
        Prim::Time => tia_time(i),
        Prim::Dtl => format!("DTL#{}", s.replacen('T', "-", 1)),
        Prim::Date | Prim::Tod | Prim::DateAndTime | Prim::LTime => {
            return Err(LinkError::at(ErrCode::Parse, path, format!("type {} is not supported on the link", p.name())));
        }
    })
}

/// DB source; fails when the DB would exceed [`TEST_DB_MAX_BYTES`].
pub(crate) fn render(c: &Contract, msgs: &[&MessageSpec], db: &str, registry_hash: u32) -> Result<String, LinkError> {
    let mut decl = String::new();
    let mut begin = String::new();
    for m in msgs {
        let Some(udt) = m.udt.as_deref() else { continue };
        let n = &m.name;
        let bytes = golden_bytes(c, udt)?;
        let text = udt_bytes_to_json_text(c, udt, &bytes)?;
        let env = write_envelope(n, VECTOR_SEQ, m.sig, Some(&text));
        if !env.is_ascii() {
            return Err(LinkError::parse(format!("{db}: envelope text of {n} is not ASCII")));
        }
        let chunks: Vec<&str> = env.as_bytes().chunks(JSON_CHUNK).map(|b| std::str::from_utf8(b).expect("ASCII text")).collect();

        decl.push_str(&format!("      {n}_Val : \"{udt}\";   //   골든 값 (crates/plc-link vectors, sig 16#{:08X})\n", m.sig));
        decl.push_str(&format!("      {n}_Bin : Array[0..{}] of Byte;   //   {n}_Val 의 Serialize 바이트 ({} B)\n", bytes.len() - 1, bytes.len()));
        decl.push_str(&format!("      {n}_JsonLen : DInt;   //   {n}_Json 엔벨로프 텍스트 길이 (seq {VECTOR_SEQ})\n"));
        decl.push_str(&format!("      {n}_Json : Array[0..{}] of String[{JSON_CHUNK}];   //   엔벨로프 텍스트 ({JSON_CHUNK} 자씩)\n", chunks.len() - 1));

        let mut leaves = Vec::new();
        flatten_fields(c, &c.udt(udt)?.fields, "", &format!("{n}_Val"), &mut leaves)?;
        for l in &leaves {
            let v = pattern_value(&l.vpath, l.prim);
            if l.default_zero && is_zero(l.prim, &v) {
                continue;
            }
            let line = format!("   {} := {};\n", l.spath, literal(l.prim, &v, &format!("{udt}.{}", l.vpath))?);
            if l.prim == Prim::Dtl {
                // A TIA re-export writes the member start values followed by the DTL# line; emit the same so
                // `gen-link --check` stays clean after import + export.
                let b = parse_dtl_text(v.as_str().unwrap_or("")).unwrap_or_else(crate::wire_json::dtl_epoch);
                let year = u16::from_be_bytes([b[0], b[1]]);
                let ns = u32::from_be_bytes([b[8], b[9], b[10], b[11]]);
                for (member, value) in [
                    ("YEAR", u32::from(year)),
                    ("MONTH", u32::from(b[2])),
                    ("DAY", u32::from(b[3])),
                    ("WEEKDAY", u32::from(b[4])),
                    ("HOUR", u32::from(b[5])),
                    ("MINUTE", u32::from(b[6])),
                    ("SECOND", u32::from(b[7])),
                    ("NANOSECOND", ns),
                ] {
                    begin.push_str(&format!("   {}.{member} := {value};\n", l.spath));
                }
            }
            begin.push_str(&line);
        }
        for (i, b) in bytes.iter().enumerate() {
            if *b != 0 {
                begin.push_str(&format!("   {n}_Bin[{i}] := 16#{b:02X};\n"));
            }
        }
        begin.push_str(&format!("   {n}_JsonLen := {};\n", env.len()));
        for (i, chunk) in chunks.iter().enumerate() {
            begin.push_str(&format!("   {n}_Json[{i}] := '{}';\n", scl_str(chunk)));
        }
    }
    let source = format!(
        "DATA_BLOCK \"{db}\"\n{{ S7_Optimized_Access := 'FALSE' }}\nVERSION : 0.1\nNON_RETAIN\n{GENERATED_MARKER} (registry hash 16#{registry_hash:08X}) - do not edit. 표준 접근 : Lnk_SelfTest 골든 벡터\n   STRUCT\n{decl}   END_STRUCT;\n\n\nBEGIN\n{begin}\nEND_DATA_BLOCK\n"
    );
    let mut check = c.clone();
    let parsed = check.add_db_source(&source)?;
    let size = check.size_of_db(&parsed)?;
    if size > TEST_DB_MAX_BYTES {
        return Err(LinkError::new(ErrCode::BadLength, format!("{db} would be {size} bytes, limit {TEST_DB_MAX_BYTES}")));
    }
    Ok(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals() {
        assert_eq!(tia_time(0), "T#0ms");
        assert_eq!(tia_time(-62_345), "T#-1m_2s_345ms");
        assert_eq!(literal(Prim::DWord, &Value::from(0x1234_5678u32), "").unwrap(), "16#1234_5678");
        assert_eq!(literal(Prim::Real, &Value::from(3.0), "").unwrap(), "3.0");
        assert_eq!(literal(Prim::Dtl, &Value::from("2026-09-13T10:11:12.345"), "").unwrap(), "DTL#2026-09-13-10:11:12.345");
        assert!(init_is_zero(None) && init_is_zero(Some("0.0")) && init_is_zero(Some("16#0000")) && !init_is_zero(Some("0.5")));
    }
}
