//! Deterministic golden values for test vectors (PC tests and the PLC `LNK_TestVectors` DB).
//!
//! Every elementary member gets a value derived from `crc32(member path)`; paths use dots for members and
//! `[i]` per array dimension with PLC indices (`Cmd.Position[1]`, `Grid[1][0]`).

use serde_json::{Map, Value};

use plc_layout::Contract;
use plc_layout::ast::{Field, Prim, TypeRef};

use crate::error::{ErrCode, LinkError};
use crate::wire_json::{DTL_EPOCH_TEXT, Strictness, check_supported, json_to_udt_bytes};

/// DTL value used for every DTL member.
pub const GOLDEN_DTL: &str = "2026-09-13T10:11:12.345";

/// Golden value of one elementary member.
pub fn pattern_value(path: &str, prim: Prim) -> Value {
    let h = crc32fast::hash(path.as_bytes());
    match prim {
        Prim::Bool => Value::Bool(h % 2 == 1),
        Prim::Byte | Prim::USInt => Value::from(h % 256),
        Prim::SInt => Value::from((h % 256) as i64 - 128),
        Prim::Char => Value::String(((b'A' + (h % 26) as u8) as char).to_string()),
        Prim::Word | Prim::UInt => Value::from(h % 65536),
        Prim::Int => Value::from((h % 65536) as i64 - 32768),
        Prim::DWord | Prim::UDInt => Value::from(h),
        Prim::DInt => Value::from(h as i32),
        Prim::Time => Value::from((h % 2_000_001) as i64 - 1_000_000),
        Prim::Real | Prim::LReal => Value::from(((h % 80001) as i64 - 40000) as f64 * 0.25),
        Prim::LInt => {
            let x = (h as i64) << 31;
            Value::from(if h & 1 == 1 { -x } else { x })
        }
        Prim::ULInt | Prim::LWord => Value::from(((h as u64) << 32) | h as u64),
        Prim::Dtl => Value::String(GOLDEN_DTL.to_string()),
        Prim::String(n) => Value::String(format!("S{h:08X}").chars().take(n as usize).collect()),
        Prim::Date | Prim::Tod | Prim::DateAndTime | Prim::LTime => Value::Null,
    }
}

/// Zero value of one elementary member (what the wire text of zeroed memory parses to, DTL = 1970-01-01).
pub fn zero_prim_value(prim: Prim) -> Value {
    match prim {
        Prim::Bool => Value::Bool(false),
        Prim::Real | Prim::LReal => Value::from(0.0),
        Prim::Char => Value::String("\u{0}".to_string()),
        Prim::String(_) => Value::String(String::new()),
        Prim::Dtl => Value::String(DTL_EPOCH_TEXT.to_string()),
        Prim::Date | Prim::Tod | Prim::DateAndTime | Prim::LTime => Value::Null,
        _ => Value::from(0),
    }
}

fn tree(c: &Contract, ty: &TypeRef, path: &str, leaf: &dyn Fn(&str, Prim) -> Value) -> Result<Value, LinkError> {
    Ok(match ty {
        TypeRef::Prim(p) => leaf(path, *p),
        TypeRef::Udt(n) => {
            let u = c.udt(n).map_err(|_| LinkError::at(ErrCode::UnknownType, path, format!("unknown UDT \"{n}\"")))?;
            fields(c, &u.fields, path, leaf)?
        }
        TypeRef::Struct(f) => fields(c, f, path, leaf)?,
        TypeRef::Array { dims, elem } => {
            let dims = plc_layout::layout::resolve_dims(c, dims).map_err(LinkError::from)?;
            array(c, elem, &dims, path, leaf)?
        }
    })
}

fn fields(c: &Contract, fs: &[Field], path: &str, leaf: &dyn Fn(&str, Prim) -> Value) -> Result<Value, LinkError> {
    let mut m = Map::new();
    for f in fs {
        let p = if path.is_empty() { f.name.clone() } else { format!("{path}.{}", f.name) };
        m.insert(f.name.clone(), tree(c, &f.ty, &p, leaf)?);
    }
    Ok(Value::Object(m))
}

fn array(c: &Contract, elem: &TypeRef, dims: &[(i64, i64)], path: &str, leaf: &dyn Fn(&str, Prim) -> Value) -> Result<Value, LinkError> {
    let (lo, hi) = dims[0];
    let mut out = Vec::new();
    for i in lo..=hi {
        let p = format!("{path}[{i}]");
        out.push(if dims.len() > 1 { array(c, elem, &dims[1..], &p, leaf)? } else { tree(c, elem, &p, leaf)? });
    }
    Ok(Value::Array(out))
}

/// Golden value of a whole UDT (every member present).
pub fn golden_value(c: &Contract, udt: &str) -> Result<Value, LinkError> {
    let ty = TypeRef::Udt(udt.to_string());
    check_supported(c, &ty)?;
    tree(c, &ty, "", &pattern_value)
}

/// Zero value of a whole UDT.
pub fn zero_value(c: &Contract, udt: &str) -> Result<Value, LinkError> {
    let ty = TypeRef::Udt(udt.to_string());
    check_supported(c, &ty)?;
    tree(c, &ty, "", &|_, p| zero_prim_value(p))
}

/// Standard-layout bytes of the golden value.
pub fn golden_bytes(c: &Contract, udt: &str) -> Result<Vec<u8>, LinkError> {
    Ok(json_to_udt_bytes(c, udt, &golden_value(c, udt)?, Strictness::Strict)?.0)
}
