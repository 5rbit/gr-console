//! Link error / Ack codes (wire-spec section 3) and the error type.

use std::fmt;

use serde::Serialize;

/// Wire error codes carried in `Ack.Code`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum ErrCode {
    Ok,
    BadMagic,
    BadVersion,
    UnknownType,
    SigMismatch,
    BadLength,
    Parse,
    FormatNotAllowed,
    DirNotAllowed,
    Busy,
    NoPending,
}

impl ErrCode {
    pub const ALL: [ErrCode; 11] = [
        ErrCode::Ok,
        ErrCode::BadMagic,
        ErrCode::BadVersion,
        ErrCode::UnknownType,
        ErrCode::SigMismatch,
        ErrCode::BadLength,
        ErrCode::Parse,
        ErrCode::FormatNotAllowed,
        ErrCode::DirNotAllowed,
        ErrCode::Busy,
        ErrCode::NoPending,
    ];

    pub fn code(self) -> i16 {
        match self {
            ErrCode::Ok => 0,
            ErrCode::BadMagic => 1,
            ErrCode::BadVersion => 2,
            ErrCode::UnknownType => 3,
            ErrCode::SigMismatch => 4,
            ErrCode::BadLength => 5,
            ErrCode::Parse => 6,
            ErrCode::FormatNotAllowed => 7,
            ErrCode::DirNotAllowed => 8,
            ErrCode::Busy => 9,
            ErrCode::NoPending => 10,
        }
    }

    pub fn from_code(code: i16) -> Option<ErrCode> {
        ErrCode::ALL.into_iter().find(|c| c.code() == code)
    }

    /// Registry / constant name (`BAD_MAGIC`, ...).
    pub fn name(self) -> &'static str {
        match self {
            ErrCode::Ok => "OK",
            ErrCode::BadMagic => "BAD_MAGIC",
            ErrCode::BadVersion => "BAD_VERSION",
            ErrCode::UnknownType => "UNKNOWN_TYPE",
            ErrCode::SigMismatch => "SIG_MISMATCH",
            ErrCode::BadLength => "BAD_LENGTH",
            ErrCode::Parse => "PARSE",
            ErrCode::FormatNotAllowed => "FORMAT_NOT_ALLOWED",
            ErrCode::DirNotAllowed => "DIR_NOT_ALLOWED",
            ErrCode::Busy => "BUSY",
            ErrCode::NoPending => "NO_PENDING",
        }
    }
}

/// Link error: wire code, message, optional member path (JSON conversion) and, for HTTP framing errors,
/// the status the server should answer with (400, 411, 413, 431, 501).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LinkError {
    pub code: ErrCode,
    pub msg: String,
    pub path: Option<String>,
    pub http_status: Option<u16>,
}

impl LinkError {
    pub fn new(code: ErrCode, msg: impl Into<String>) -> Self {
        LinkError { code, msg: msg.into(), path: None, http_status: None }
    }

    pub fn at(code: ErrCode, path: impl Into<String>, msg: impl Into<String>) -> Self {
        LinkError { code, msg: msg.into(), path: Some(path.into()), http_status: None }
    }

    pub fn http(status: u16, code: ErrCode, msg: impl Into<String>) -> Self {
        LinkError { code, msg: msg.into(), path: None, http_status: Some(status) }
    }

    pub fn parse(msg: impl Into<String>) -> Self {
        LinkError::new(ErrCode::Parse, msg)
    }
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code.name())?;
        if let Some(s) = self.http_status {
            write!(f, " (HTTP {s})")?;
        }
        if let Some(p) = &self.path {
            write!(f, " at {p}")?;
        }
        write!(f, ": {}", self.msg)
    }
}

impl std::error::Error for LinkError {}

impl From<plc_layout::LayoutError> for LinkError {
    fn from(e: plc_layout::LayoutError) -> Self {
        match e {
            plc_layout::LayoutError::Value { path, msg } => LinkError::at(ErrCode::Parse, path, msg),
            plc_layout::LayoutError::UnknownUdt(n) => LinkError::new(ErrCode::UnknownType, format!("unknown UDT \"{n}\"")),
            other => LinkError::parse(other.to_string()),
        }
    }
}
