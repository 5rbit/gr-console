//! TIA Portal SCL source → standard-access memory layout → JSON decode/encode + layout signature.
//!
//! Rules (S7-300 compatible "standard access"): Bool bit-packed in consecutive bits; 1-byte types byte
//! aligned; 2-byte and larger scalars, Struct, Array, String and DTL word aligned; Struct/Array end padded
//! to an even byte. Declaration order == memory order.

pub mod ast;
pub mod consts;
pub mod contract;
pub mod decode;
pub mod docs;
pub mod encode;
pub mod layout;
pub mod parse;
pub mod signature;

pub use ast::{Bound, DbDecl, Field, Prim, TypeRef};
pub use contract::Contract;
pub use decode::Value;
pub use layout::{Layout, Member};

#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("parse error{}: {msg}", .line.map(|l| format!(" at line {l}")).unwrap_or_default())]
    Parse { line: Option<usize>, msg: String },
    #[error("unknown UDT \"{0}\"")]
    UnknownUdt(String),
    #[error("unknown constant \"{0}\" used as array bound")]
    UnknownConst(String),
    #[error("unknown DB \"{0}\"")]
    UnknownDb(String),
    #[error("unknown member path \"{0}\"")]
    UnknownPath(String),
    #[error("buffer too short: need {need} bytes, have {have}")]
    Short { need: usize, have: usize },
    #[error("value error at {path}: {msg}")]
    Value { path: String, msg: String },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}
