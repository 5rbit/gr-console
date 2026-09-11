//! PLC scalar kinds and `PlcValue` ↔ `Variant` coercion.

use opcua::types::{DataTypeId, NodeId, UAString, Variant};

use crate::{OpcError, PlcValue};

/// Scalar data type of a leaf member, derived from the node's `DataType` attribute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PlcKind {
    Bool,
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
    Str,
    /// Not a simple scalar (struct, DTL, unknown): the `PlcValue`'s own type is used.
    Unknown,
}

impl PlcKind {
    /// Map an OPC UA `DataType` node id (ns=0 builtin ids) to a kind.
    pub fn from_data_type(node: &NodeId) -> PlcKind {
        if node.namespace != 0 {
            return PlcKind::Unknown;
        }
        let Some(id) = node.as_u32() else {
            return PlcKind::Unknown;
        };
        match id {
            x if x == DataTypeId::Boolean as u32 => PlcKind::Bool,
            x if x == DataTypeId::SByte as u32 => PlcKind::I8,
            x if x == DataTypeId::Byte as u32 => PlcKind::U8,
            x if x == DataTypeId::Int16 as u32 => PlcKind::I16,
            x if x == DataTypeId::UInt16 as u32 => PlcKind::U16,
            x if x == DataTypeId::Int32 as u32 => PlcKind::I32,
            x if x == DataTypeId::UInt32 as u32 => PlcKind::U32,
            x if x == DataTypeId::Int64 as u32 => PlcKind::I64,
            x if x == DataTypeId::UInt64 as u32 => PlcKind::U64,
            x if x == DataTypeId::Float as u32 => PlcKind::F32,
            x if x == DataTypeId::Double as u32 => PlcKind::F64,
            x if x == DataTypeId::String as u32 => PlcKind::Str,
            _ => PlcKind::Unknown,
        }
    }

    /// Kind of a variant read from the server.
    pub fn from_variant(v: &Variant) -> PlcKind {
        match v {
            Variant::Boolean(_) => PlcKind::Bool,
            Variant::SByte(_) => PlcKind::I8,
            Variant::Byte(_) => PlcKind::U8,
            Variant::Int16(_) => PlcKind::I16,
            Variant::UInt16(_) => PlcKind::U16,
            Variant::Int32(_) => PlcKind::I32,
            Variant::UInt32(_) => PlcKind::U32,
            Variant::Int64(_) => PlcKind::I64,
            Variant::UInt64(_) => PlcKind::U64,
            Variant::Float(_) => PlcKind::F32,
            Variant::Double(_) => PlcKind::F64,
            Variant::String(_) => PlcKind::Str,
            _ => PlcKind::Unknown,
        }
    }

    /// The natural kind of a `PlcValue`.
    pub fn of(v: &PlcValue) -> PlcKind {
        match v {
            PlcValue::Bool(_) => PlcKind::Bool,
            PlcValue::U8(_) => PlcKind::U8,
            PlcValue::I8(_) => PlcKind::I8,
            PlcValue::U16(_) => PlcKind::U16,
            PlcValue::I16(_) => PlcKind::I16,
            PlcValue::U32(_) => PlcKind::U32,
            PlcValue::I32(_) => PlcKind::I32,
            PlcValue::F32(_) => PlcKind::F32,
            PlcValue::F64(_) => PlcKind::F64,
            PlcValue::Str(_) => PlcKind::Str,
        }
    }

    /// Siemens-style name for diagnostics.
    pub fn name(self) -> &'static str {
        match self {
            PlcKind::Bool => "Bool",
            PlcKind::I8 => "SInt",
            PlcKind::U8 => "Byte/USInt",
            PlcKind::I16 => "Int",
            PlcKind::U16 => "Word/UInt",
            PlcKind::I32 => "DInt",
            PlcKind::U32 => "DWord/UDInt",
            PlcKind::I64 => "LInt",
            PlcKind::U64 => "LWord/ULInt",
            PlcKind::F32 => "Real",
            PlcKind::F64 => "LReal",
            PlcKind::Str => "String",
            PlcKind::Unknown => "Unknown",
        }
    }
}

enum Num {
    Int(i128),
    Float(f64),
}

fn as_num(v: &PlcValue) -> Option<Num> {
    Some(match v {
        PlcValue::Bool(b) => Num::Int(i128::from(*b)),
        PlcValue::U8(x) => Num::Int(i128::from(*x)),
        PlcValue::I8(x) => Num::Int(i128::from(*x)),
        PlcValue::U16(x) => Num::Int(i128::from(*x)),
        PlcValue::I16(x) => Num::Int(i128::from(*x)),
        PlcValue::U32(x) => Num::Int(i128::from(*x)),
        PlcValue::I32(x) => Num::Int(i128::from(*x)),
        PlcValue::F32(x) => Num::Float(f64::from(*x)),
        PlcValue::F64(x) => Num::Float(*x),
        PlcValue::Str(_) => return None,
    })
}

fn int_in<T: TryFrom<i128>>(n: Num) -> Option<T> {
    match n {
        Num::Int(i) => T::try_from(i).ok(),
        Num::Float(f) => {
            if f.fract() != 0.0 || !f.is_finite() {
                return None;
            }
            T::try_from(f as i128).ok()
        }
    }
}

/// Coerce a `PlcValue` to the `Variant` matching the member's kind.
///
/// Integers convert between widths when the value fits; floats accept integers; `Bool`
/// accepts `0`/`1`. Anything else is a [`OpcError::Config`] naming the path.
pub fn coerce(value: &PlcValue, kind: PlcKind, path: &str) -> Result<Variant, OpcError> {
    let mismatch = || {
        OpcError::Config(format!(
            "{path}: cannot write {value:?} to a {} member",
            kind.name()
        ))
    };
    if kind == PlcKind::Unknown {
        return Ok(native(value));
    }
    if kind == PlcKind::Str {
        return match value {
            PlcValue::Str(s) => Ok(Variant::String(UAString::from(s.as_str()))),
            _ => Err(mismatch()),
        };
    }
    let num = as_num(value).ok_or_else(mismatch)?;
    let v = match kind {
        PlcKind::Bool => match num {
            Num::Int(0) => Variant::Boolean(false),
            Num::Int(1) => Variant::Boolean(true),
            _ => return Err(mismatch()),
        },
        PlcKind::I8 => Variant::SByte(int_in(num).ok_or_else(mismatch)?),
        PlcKind::U8 => Variant::Byte(int_in(num).ok_or_else(mismatch)?),
        PlcKind::I16 => Variant::Int16(int_in(num).ok_or_else(mismatch)?),
        PlcKind::U16 => Variant::UInt16(int_in(num).ok_or_else(mismatch)?),
        PlcKind::I32 => Variant::Int32(int_in(num).ok_or_else(mismatch)?),
        PlcKind::U32 => Variant::UInt32(int_in(num).ok_or_else(mismatch)?),
        PlcKind::I64 => Variant::Int64(int_in(num).ok_or_else(mismatch)?),
        PlcKind::U64 => Variant::UInt64(int_in(num).ok_or_else(mismatch)?),
        PlcKind::F32 => match num {
            Num::Int(i) => Variant::Float(i as f32),
            Num::Float(f) => Variant::Float(f as f32),
        },
        PlcKind::F64 => match num {
            Num::Int(i) => Variant::Double(i as f64),
            Num::Float(f) => Variant::Double(f),
        },
        PlcKind::Str | PlcKind::Unknown => unreachable!("handled above"),
    };
    Ok(v)
}

/// Variant with exactly the `PlcValue`'s own type.
pub fn native(value: &PlcValue) -> Variant {
    match value {
        PlcValue::Bool(b) => Variant::Boolean(*b),
        PlcValue::U8(x) => Variant::Byte(*x),
        PlcValue::I8(x) => Variant::SByte(*x),
        PlcValue::U16(x) => Variant::UInt16(*x),
        PlcValue::I16(x) => Variant::Int16(*x),
        PlcValue::U32(x) => Variant::UInt32(*x),
        PlcValue::I32(x) => Variant::Int32(*x),
        PlcValue::F32(x) => Variant::Float(*x),
        PlcValue::F64(x) => Variant::Double(*x),
        PlcValue::Str(s) => Variant::String(UAString::from(s.as_str())),
    }
}

/// Convert a variant read from the server into a `PlcValue` (64-bit ints become `Str`).
pub fn from_variant(v: &Variant) -> Option<PlcValue> {
    Some(match v {
        Variant::Boolean(b) => PlcValue::Bool(*b),
        Variant::SByte(x) => PlcValue::I8(*x),
        Variant::Byte(x) => PlcValue::U8(*x),
        Variant::Int16(x) => PlcValue::I16(*x),
        Variant::UInt16(x) => PlcValue::U16(*x),
        Variant::Int32(x) => PlcValue::I32(*x),
        Variant::UInt32(x) => PlcValue::U32(*x),
        Variant::Int64(x) => PlcValue::Str(x.to_string()),
        Variant::UInt64(x) => PlcValue::Str(x.to_string()),
        Variant::Float(x) => PlcValue::F32(*x),
        Variant::Double(x) => PlcValue::F64(*x),
        Variant::String(s) => PlcValue::Str(s.as_ref().to_string()),
        Variant::Empty => return None,
        other => PlcValue::Str(format!("{other:?}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coerce_exact_and_widening() {
        assert_eq!(
            coerce(&PlcValue::U8(7), PlcKind::U8, "p").unwrap(),
            Variant::Byte(7)
        );
        assert_eq!(
            coerce(&PlcValue::U8(7), PlcKind::U16, "p").unwrap(),
            Variant::UInt16(7)
        );
        assert_eq!(
            coerce(&PlcValue::U8(7), PlcKind::U32, "p").unwrap(),
            Variant::UInt32(7)
        );
        assert_eq!(
            coerce(&PlcValue::I32(-5), PlcKind::I16, "p").unwrap(),
            Variant::Int16(-5)
        );
        assert_eq!(
            coerce(&PlcValue::U32(9), PlcKind::F32, "p").unwrap(),
            Variant::Float(9.0)
        );
        assert_eq!(
            coerce(&PlcValue::F32(1.5), PlcKind::F64, "p").unwrap(),
            Variant::Double(1.5)
        );
        assert_eq!(
            coerce(&PlcValue::F64(2.0), PlcKind::U8, "p").unwrap(),
            Variant::Byte(2)
        );
        assert_eq!(
            coerce(&PlcValue::Bool(true), PlcKind::Bool, "p").unwrap(),
            Variant::Boolean(true)
        );
        assert_eq!(
            coerce(&PlcValue::U8(1), PlcKind::Bool, "p").unwrap(),
            Variant::Boolean(true)
        );
        assert_eq!(
            coerce(&PlcValue::U8(0), PlcKind::Bool, "p").unwrap(),
            Variant::Boolean(false)
        );
        assert_eq!(
            coerce(&PlcValue::Str("x".into()), PlcKind::Str, "p").unwrap(),
            Variant::String("x".into())
        );
        // Unknown kind → native type.
        assert_eq!(
            coerce(&PlcValue::I16(3), PlcKind::Unknown, "p").unwrap(),
            Variant::Int16(3)
        );
    }

    #[test]
    fn coerce_rejects() {
        let err = coerce(&PlcValue::U16(300), PlcKind::U8, "Header.CMD").unwrap_err();
        assert!(matches!(err, OpcError::Config(ref m) if m.contains("Header.CMD")));
        assert!(coerce(&PlcValue::I8(-1), PlcKind::U8, "p").is_err());
        assert!(coerce(&PlcValue::F32(1.5), PlcKind::U8, "p").is_err());
        assert!(coerce(&PlcValue::U8(2), PlcKind::Bool, "p").is_err());
        assert!(coerce(&PlcValue::Str("1".into()), PlcKind::U8, "p").is_err());
        assert!(coerce(&PlcValue::U8(1), PlcKind::Str, "p").is_err());
    }

    #[test]
    fn kinds_from_data_type() {
        assert_eq!(PlcKind::from_data_type(&DataTypeId::Byte.into()), PlcKind::U8);
        assert_eq!(
            PlcKind::from_data_type(&DataTypeId::UInt16.into()),
            PlcKind::U16
        );
        assert_eq!(
            PlcKind::from_data_type(&DataTypeId::UInt32.into()),
            PlcKind::U32
        );
        assert_eq!(
            PlcKind::from_data_type(&DataTypeId::Float.into()),
            PlcKind::F32
        );
        assert_eq!(
            PlcKind::from_data_type(&DataTypeId::Boolean.into()),
            PlcKind::Bool
        );
        assert_eq!(
            PlcKind::from_data_type(&DataTypeId::Structure.into()),
            PlcKind::Unknown
        );
        assert_eq!(
            PlcKind::from_data_type(&NodeId::new(3, "x")),
            PlcKind::Unknown
        );
    }

    #[test]
    fn roundtrip_variant() {
        assert_eq!(from_variant(&Variant::UInt16(5)), Some(PlcValue::U16(5)));
        assert_eq!(from_variant(&Variant::Empty), None);
    }
}
