//! 16-byte FRAME header (wire-spec section 1.1, big endian).

use serde::{Deserialize, Serialize};

use crate::error::{ErrCode, LinkError};

pub const MAGIC: u16 = 0x4753;
pub const VERSION: u8 = 1;
pub const HEADER_LEN: usize = 16;

/// Payload format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Json,
    Bin,
}

impl Format {
    pub fn code(self) -> u8 {
        match self {
            Format::Json => 1,
            Format::Bin => 2,
        }
    }

    pub fn from_code(b: u8) -> Option<Format> {
        match b {
            1 => Some(Format::Json),
            2 => Some(Format::Bin),
            _ => None,
        }
    }

    /// Bit in `Hello.Formats` (bit0 JSON, bit1 BIN).
    pub fn bit(self) -> u8 {
        match self {
            Format::Json => 1,
            Format::Bin => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Json => "json",
            Format::Bin => "bin",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameHeader {
    pub version: u8,
    pub format: Format,
    pub msg_type: u16,
    pub seq: u16,
    pub sig: u32,
    pub len: u32,
}

impl FrameHeader {
    pub fn new(format: Format, msg_type: u16, seq: u16, sig: u32, len: u32) -> Self {
        FrameHeader { version: VERSION, format, msg_type, seq, sig, len }
    }

    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut b = [0u8; HEADER_LEN];
        b[0..2].copy_from_slice(&MAGIC.to_be_bytes());
        b[2] = self.version;
        b[3] = self.format.code();
        b[4..6].copy_from_slice(&self.msg_type.to_be_bytes());
        b[6..8].copy_from_slice(&self.seq.to_be_bytes());
        b[8..12].copy_from_slice(&self.sig.to_be_bytes());
        b[12..16].copy_from_slice(&self.len.to_be_bytes());
        b
    }

    /// Decodes the first 16 bytes. Magic and version are checked as soon as their bytes are present, so a
    /// short buffer with a bad magic still reports `BAD_MAGIC`; a short buffer otherwise is `BAD_LENGTH`.
    pub fn decode(b: &[u8]) -> Result<FrameHeader, LinkError> {
        if b.len() >= 2 && u16::from_be_bytes([b[0], b[1]]) != MAGIC {
            return Err(LinkError::new(ErrCode::BadMagic, format!("bad magic 0x{:02X}{:02X}", b[0], b[1])));
        }
        if b.len() >= 3 && b[2] != VERSION {
            return Err(LinkError::new(ErrCode::BadVersion, format!("unsupported version {}", b[2])));
        }
        if b.len() < HEADER_LEN {
            return Err(LinkError::new(ErrCode::BadLength, format!("truncated header: {} of {HEADER_LEN} bytes", b.len())));
        }
        let format = Format::from_code(b[3]).ok_or_else(|| LinkError::new(ErrCode::FormatNotAllowed, format!("unknown format {}", b[3])))?;
        Ok(FrameHeader {
            version: b[2],
            format,
            msg_type: u16::from_be_bytes([b[4], b[5]]),
            seq: u16::from_be_bytes([b[6], b[7]]),
            sig: u32::from_be_bytes([b[8], b[9], b[10], b[11]]),
            len: u32::from_be_bytes([b[12], b[13], b[14], b[15]]),
        })
    }
}
