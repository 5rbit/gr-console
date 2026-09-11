use thiserror::Error;

#[derive(Debug, Error)]
pub enum S7Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("connect timeout to {0}")]
    ConnectTimeout(String),
    #[error("request timeout")]
    Timeout,
    #[error("COTP connect refused (code 0x{0:02X}); check rack/slot and connection type")]
    Cotp(u8),
    #[error("protocol error: {0}")]
    Pdu(String),
    #[error("S7 error class 0x{class:02X} code 0x{code:02X}")]
    S7 { class: u8, code: u8 },
    /// Per-item return code from the PLC (0x05 address out of range, 0x0A object does not exist,
    /// 0x03 access denied / optimized block, 0x07 data type inconsistent).
    #[error("item error 0x{code:02X}: {}", item_code_text(*code))]
    Item { code: u8 },
    #[error("not connected")]
    Disconnected,
}

impl S7Error {
    pub fn is_item(&self, code: u8) -> bool {
        matches!(self, S7Error::Item { code: c } if *c == code)
    }
}

pub fn item_code_text(code: u8) -> &'static str {
    match code {
        0xFF => "ok",
        0x01 => "hardware fault",
        0x03 => "access denied (optimized block or PUT/GET not permitted)",
        0x05 => "address out of range",
        0x06 => "data type not supported",
        0x07 => "data type inconsistent",
        0x0A => "object does not exist (DB missing)",
        _ => "unknown",
    }
}
