//! Bytes → values / nested JSON.

use serde::Serialize;
use serde_json::{Map, Value as Json};

use crate::LayoutError;
use crate::ast::{Bound, Field, Prim, TypeRef};
use crate::contract::Contract;
use crate::layout::{Cursor, Member, resolve_dims};

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Value {
    Bool(bool),
    U64(u64),
    I64(i64),
    F64(f64),
    Str(String),
}

impl Value {
    pub fn to_json(&self) -> Json {
        match self {
            Value::Bool(b) => Json::Bool(*b),
            Value::U64(u) => Json::from(*u),
            Value::I64(i) => Json::from(*i),
            Value::F64(f) => serde_json::Number::from_f64(*f).map(Json::Number).unwrap_or(Json::Null),
            Value::Str(s) => Json::String(s.clone()),
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::U64(u) => Some(*u as f64),
            Value::I64(i) => Some(*i as f64),
            Value::F64(f) => Some(*f),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Value::Str(_) => None,
        }
    }
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::U64(u) => Some(*u),
            Value::I64(i) if *i >= 0 => Some(*i as u64),
            Value::F64(f) if *f >= 0.0 => Some(*f as u64),
            Value::Bool(b) => Some(*b as u64),
            _ => None,
        }
    }
}

fn be16(b: &[u8]) -> u16 {
    u16::from_be_bytes([b[0], b[1]])
}
fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}
fn be64(b: &[u8]) -> u64 {
    u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

/// Decodes one scalar at `offset` (bit for Bool).
pub fn decode_prim(buf: &[u8], prim: Prim, offset: u32, bit: Option<u8>) -> Result<Value, LayoutError> {
    let at = offset as usize;
    let need = at + prim.size().max(1) as usize;
    if need > buf.len() {
        return Err(LayoutError::Short { need, have: buf.len() });
    }
    let b = &buf[at..];
    Ok(match prim {
        Prim::Bool => Value::Bool((b[0] >> bit.unwrap_or(0)) & 1 == 1),
        Prim::Byte | Prim::USInt => Value::U64(b[0] as u64),
        Prim::SInt => Value::I64(b[0] as i8 as i64),
        Prim::Char => Value::Str((b[0] as char).to_string()),
        Prim::Word | Prim::UInt | Prim::Date => Value::U64(be16(b) as u64),
        Prim::Int => Value::I64(be16(b) as i16 as i64),
        Prim::DWord | Prim::UDInt | Prim::Time | Prim::Tod => Value::U64(be32(b) as u64),
        Prim::DInt => Value::I64(be32(b) as i32 as i64),
        Prim::Real => Value::F64(round4(f32::from_bits(be32(b)) as f64)),
        Prim::LReal => Value::F64(f64::from_bits(be64(b))),
        Prim::LInt => Value::I64(be64(b) as i64),
        Prim::ULInt | Prim::LWord | Prim::LTime => Value::U64(be64(b)),
        Prim::DateAndTime => Value::Str(b[..8].iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join("-")),
        Prim::Dtl => {
            let year = be16(b);
            let (mon, day, hour, min, sec) = (b[2], b[3], b[5], b[6], b[7]);
            let ns = be32(&b[8..]);
            if year < 1970 || !(1..=12).contains(&mon) || !(1..=31).contains(&day) {
                Value::Str(String::new())
            } else {
                Value::Str(format!("{year:04}-{mon:02}-{day:02} {hour:02}:{min:02}:{sec:02}.{:03}", ns / 1_000_000))
            }
        }
        Prim::String(n) => {
            let len = (b[1] as usize).min(n as usize);
            Value::Str(b[2..2 + len].iter().map(|&c| c as char).collect())
        }
    })
}

fn round4(v: f64) -> f64 {
    if !v.is_finite() {
        return v;
    }
    (v * 10000.0).round() / 10000.0
}

pub fn decode_member(buf: &[u8], m: &Member) -> Result<Value, LayoutError> {
    decode_prim(buf, m.prim, m.offset, m.bit)
}

/// Decodes a whole type into nested JSON, walking with the same alignment rules as the layout.
pub fn decode_tree(contract: &Contract, ty: &TypeRef, buf: &[u8], cur: &mut Cursor) -> Result<Json, LayoutError> {
    match ty {
        TypeRef::Prim(p) => {
            match p.align() {
                0 => {}
                1 => cur.align_byte(),
                _ => cur.align_word(),
            }
            let v = decode_prim(buf, *p, cur.byte, Some(cur.bit))?;
            if *p == Prim::Bool {
                cur.bit += 1;
                if cur.bit == 8 {
                    cur.bit = 0;
                    cur.byte += 1;
                }
            } else {
                cur.byte += p.size();
            }
            Ok(v.to_json())
        }
        TypeRef::Udt(name) => {
            let udt = contract.udt(name)?;
            decode_fields(contract, &udt.fields, buf, cur)
        }
        TypeRef::Struct(fields) => decode_fields(contract, fields, buf, cur),
        TypeRef::Array { dims, elem } => {
            cur.align_word();
            let dims = resolve_dims(contract, dims)?;
            let v = decode_array(contract, elem, &dims, buf, cur)?;
            cur.align_word();
            Ok(v)
        }
    }
}

fn decode_array(contract: &Contract, elem: &TypeRef, dims: &[(i64, i64)], buf: &[u8], cur: &mut Cursor) -> Result<Json, LayoutError> {
    let (lo, hi) = dims[0];
    let mut arr = Vec::with_capacity((hi - lo + 1).max(0) as usize);
    for _ in lo..=hi {
        if dims.len() > 1 {
            arr.push(decode_array(contract, elem, &dims[1..], buf, cur)?);
        } else {
            arr.push(decode_tree(contract, elem, buf, cur)?);
        }
    }
    Ok(Json::Array(arr))
}

pub fn decode_fields(contract: &Contract, fields: &[Field], buf: &[u8], cur: &mut Cursor) -> Result<Json, LayoutError> {
    cur.align_word();
    let mut obj = Map::with_capacity(fields.len());
    for f in fields {
        obj.insert(f.name.clone(), decode_tree(contract, &f.ty, buf, cur)?);
    }
    cur.align_word();
    Ok(Json::Object(obj))
}

/// Convenience: resolved lower bound of an array type's first dimension (for index arithmetic).
pub fn first_lower_bound(contract: &Contract, ty: &TypeRef) -> Option<i64> {
    match ty {
        TypeRef::Array { dims, .. } => match &dims[0].0 {
            Bound::Int(i) => Some(*i),
            Bound::Const(c) => contract.consts.get(c).copied(),
        },
        _ => None,
    }
}
