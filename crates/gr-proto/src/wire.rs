//! Leaf member values for OPC UA writes (converted to the OPC UA crate's variant by the app).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WireValue {
    Bool(bool),
    U8(u8),
    I8(i8),
    U16(u16),
    I16(i16),
    U32(u32),
    I32(i32),
    F32(f32),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemberValue {
    /// Path relative to the CMD struct, e.g. `TaskData.Position[3]`.
    pub path: String,
    pub value: WireValue,
}

impl MemberValue {
    pub fn new(path: impl Into<String>, value: WireValue) -> Self {
        Self { path: path.into(), value }
    }
}
