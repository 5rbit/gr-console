//! JSON / values → bytes (in place, so unspecified fields keep their current content).

use serde_json::Value as Json;

use crate::LayoutError;
use crate::ast::{Field, Prim, TypeRef};
use crate::contract::Contract;
use crate::decode::Value;
use crate::layout::{Cursor, Member, resolve_dims};

fn verr(path: &str, msg: impl Into<String>) -> LayoutError {
    LayoutError::Value { path: path.to_string(), msg: msg.into() }
}

/// Encodes one scalar into `buf` at `offset`/`bit`.
pub fn encode_prim(buf: &mut [u8], prim: Prim, offset: u32, bit: Option<u8>, v: &Value, path: &str) -> Result<(), LayoutError> {
    let at = offset as usize;
    let need = at + prim.size().max(1) as usize;
    if need > buf.len() {
        return Err(LayoutError::Short { need, have: buf.len() });
    }
    let f = v.as_f64();
    let u = |max: u64| -> Result<u64, LayoutError> {
        let x = v.as_u64().ok_or_else(|| verr(path, "expected unsigned integer"))?;
        if x > max { Err(verr(path, format!("{x} exceeds {max}"))) } else { Ok(x) }
    };
    let i = |min: i64, max: i64| -> Result<i64, LayoutError> {
        let x = match v {
            Value::I64(i) => *i,
            Value::U64(u) => *u as i64,
            Value::F64(f) => *f as i64,
            Value::Bool(b) => *b as i64,
            Value::Str(_) => return Err(verr(path, "expected integer")),
        };
        if x < min || x > max { Err(verr(path, format!("{x} out of range"))) } else { Ok(x) }
    };
    let b = &mut buf[at..];
    match prim {
        Prim::Bool => {
            let on = match v {
                Value::Bool(x) => *x,
                other => other.as_f64().map(|x| x != 0.0).unwrap_or(false),
            };
            let bit = bit.unwrap_or(0);
            if on { b[0] |= 1 << bit } else { b[0] &= !(1 << bit) }
        }
        Prim::Byte | Prim::USInt => b[0] = u(255)? as u8,
        Prim::SInt => b[0] = i(-128, 127)? as i8 as u8,
        Prim::Char => {
            b[0] = match v {
                Value::Str(s) => s.bytes().next().unwrap_or(0),
                _ => u(255)? as u8,
            }
        }
        Prim::Word | Prim::UInt | Prim::Date => b[..2].copy_from_slice(&(u(65535)? as u16).to_be_bytes()),
        Prim::Int => b[..2].copy_from_slice(&(i(-32768, 32767)? as i16).to_be_bytes()),
        Prim::DWord | Prim::UDInt | Prim::Time | Prim::Tod => b[..4].copy_from_slice(&(u(u32::MAX as u64)? as u32).to_be_bytes()),
        Prim::DInt => b[..4].copy_from_slice(&(i(i32::MIN as i64, i32::MAX as i64)? as i32).to_be_bytes()),
        Prim::Real => b[..4].copy_from_slice(&(f.ok_or_else(|| verr(path, "expected number"))? as f32).to_bits().to_be_bytes()),
        Prim::LReal => b[..8].copy_from_slice(&f.ok_or_else(|| verr(path, "expected number"))?.to_bits().to_be_bytes()),
        Prim::LInt => b[..8].copy_from_slice(&i(i64::MIN, i64::MAX)?.to_be_bytes()),
        Prim::ULInt | Prim::LWord | Prim::LTime => b[..8].copy_from_slice(&u(u64::MAX)?.to_be_bytes()),
        Prim::DateAndTime => {
            // "AA-BB-.." hex dump form (as decoded) or anything else -> zero
            let s = match v {
                Value::Str(s) => s.clone(),
                _ => String::new(),
            };
            b[..8].fill(0);
            for (i, part) in s.split('-').take(8).enumerate() {
                if let Ok(x) = u8::from_str_radix(part.trim(), 16) {
                    b[i] = x;
                }
            }
        }
        Prim::Dtl => {
            // "YYYY-MM-DD HH:MM:SS.mmm" or empty → zero
            let s = match v {
                Value::Str(s) => s.clone(),
                _ => String::new(),
            };
            b[..12].fill(0);
            if s.len() >= 19 {
                let num = |r: std::ops::Range<usize>| s.get(r).and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
                b[..2].copy_from_slice(&(num(0..4) as u16).to_be_bytes());
                b[2] = num(5..7) as u8;
                b[3] = num(8..10) as u8;
                b[4] = 0;
                b[5] = num(11..13) as u8;
                b[6] = num(14..16) as u8;
                b[7] = num(17..19) as u8;
                let ms = s.get(20..23).and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
                b[8..12].copy_from_slice(&(ms * 1_000_000).to_be_bytes());
            }
        }
        Prim::String(n) => {
            let s = match v {
                Value::Str(s) => s.clone(),
                other => other.as_f64().map(|x| x.to_string()).unwrap_or_default(),
            };
            let bytes: Vec<u8> = s.bytes().take(n as usize).collect();
            b[0] = n as u8;
            b[1] = bytes.len() as u8;
            b[2..2 + n as usize].fill(0);
            b[2..2 + bytes.len()].copy_from_slice(&bytes);
        }
    }
    Ok(())
}

pub fn encode_member(buf: &mut [u8], m: &Member, v: &Value) -> Result<(), LayoutError> {
    encode_prim(buf, m.prim, m.offset, m.bit, v, &m.path)
}

fn json_to_value(j: &Json) -> Option<Value> {
    match j {
        Json::Bool(b) => Some(Value::Bool(*b)),
        Json::Number(n) => n.as_u64().map(Value::U64).or_else(|| n.as_i64().map(Value::I64)).or_else(|| n.as_f64().map(Value::F64)),
        Json::String(s) => Some(Value::Str(s.clone())),
        Json::Null => None,
        _ => None,
    }
}

/// Encodes a JSON tree into `buf` following the type's layout. Missing object keys / short arrays are
/// left untouched (cursor still advances); `null` leaves the field untouched.
pub fn encode_tree(contract: &Contract, ty: &TypeRef, json: &Json, buf: &mut [u8], cur: &mut Cursor, path: &str) -> Result<(), LayoutError> {
    match ty {
        TypeRef::Prim(p) => {
            match p.align() {
                0 => {}
                1 => cur.align_byte(),
                _ => cur.align_word(),
            }
            if let Some(v) = json_to_value(json) {
                encode_prim(buf, *p, cur.byte, Some(cur.bit), &v, path)?;
            } else if !json.is_null() {
                return Err(verr(path, format!("cannot encode {json} as {}", p.name())));
            }
            if *p == Prim::Bool {
                cur.bit += 1;
                if cur.bit == 8 {
                    cur.bit = 0;
                    cur.byte += 1;
                }
            } else {
                cur.byte += p.size();
            }
        }
        TypeRef::Udt(name) => {
            let udt = contract.udt(name)?;
            encode_fields(contract, &udt.fields, json, buf, cur, path)?;
        }
        TypeRef::Struct(fields) => encode_fields(contract, fields, json, buf, cur, path)?,
        TypeRef::Array { dims, elem } => {
            cur.align_word();
            let dims = resolve_dims(contract, dims)?;
            encode_array(contract, elem, &dims, json, buf, cur, path)?;
            cur.align_word();
        }
    }
    Ok(())
}

fn encode_array(contract: &Contract, elem: &TypeRef, dims: &[(i64, i64)], json: &Json, buf: &mut [u8], cur: &mut Cursor, path: &str) -> Result<(), LayoutError> {
    let (lo, hi) = dims[0];
    let arr = json.as_array();
    for (k, idx) in (lo..=hi).enumerate() {
        let item = arr.and_then(|a| a.get(k)).unwrap_or(&Json::Null);
        let p = format!("{path}[{idx}]");
        if dims.len() > 1 {
            encode_array(contract, elem, &dims[1..], item, buf, cur, &p)?;
        } else {
            encode_tree(contract, elem, item, buf, cur, &p)?;
        }
    }
    Ok(())
}

pub fn encode_fields(contract: &Contract, fields: &[Field], json: &Json, buf: &mut [u8], cur: &mut Cursor, path: &str) -> Result<(), LayoutError> {
    cur.align_word();
    let obj = json.as_object();
    for f in fields {
        let item = obj.and_then(|o| o.get(&f.name)).unwrap_or(&Json::Null);
        let p = if path.is_empty() { f.name.clone() } else { format!("{path}.{}", f.name) };
        encode_tree(contract, &f.ty, item, buf, cur, &p)?;
    }
    cur.align_word();
    Ok(())
}
