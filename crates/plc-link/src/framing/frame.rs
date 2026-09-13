//! FRAME framing: 16-byte header + payload.

use crate::error::{ErrCode, LinkError};
use crate::framing::{Framing, RawFrame};
use crate::header::{Format, MAGIC};
use crate::header::{FrameHeader, HEADER_LEN, VERSION};

/// Incremental FRAME decoder. A framing error (bad magic / version / oversize) is sticky: there is no
/// resync, every later `next` returns the same error.
#[derive(Debug)]
pub struct FrameDecoder {
    buf: Vec<u8>,
    max: u32,
    failed: Option<LinkError>,
}

impl FrameDecoder {
    /// `max` = receiver maximum payload length (PC 65535, PLC 8176).
    pub fn new(max: u32) -> Self {
        FrameDecoder { buf: Vec::new(), max, failed: None }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Bytes buffered but not yet returned.
    pub fn buffered(&self) -> usize {
        self.buf.len()
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<RawFrame>, LinkError> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        let r = self.step();
        if let Err(e) = &r {
            self.failed = Some(e.clone());
        }
        r
    }

    fn step(&mut self) -> Result<Option<RawFrame>, LinkError> {
        if self.buf.len() < HEADER_LEN {
            // check magic / version early so garbage is rejected without waiting for 16 bytes
            if (self.buf.len() >= 2 && u16::from_be_bytes([self.buf[0], self.buf[1]]) != MAGIC) || (self.buf.len() >= 3 && self.buf[2] != VERSION) {
                FrameHeader::decode(&self.buf)?;
            }
            return Ok(None);
        }
        let h = FrameHeader::decode(&self.buf)?;
        if h.len > self.max {
            return Err(LinkError::new(ErrCode::BadLength, format!("frame length {} exceeds maximum {}", h.len, self.max)));
        }
        let total = HEADER_LEN + h.len as usize;
        if self.buf.len() < total {
            return Ok(None);
        }
        let payload = self.buf[HEADER_LEN..total].to_vec();
        self.buf.drain(..total);
        Ok(Some(RawFrame { framing: Framing::Frame, format: h.format, msg_type: Some(h.msg_type), seq: Some(h.seq), sig: Some(h.sig), payload, http: None }))
    }

    /// Call at end of stream: buffered bytes of an incomplete frame are a `BAD_LENGTH` (truncated) error.
    pub fn finish(&self) -> Result<(), LinkError> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        if self.buf.is_empty() { Ok(()) } else { Err(LinkError::new(ErrCode::BadLength, format!("truncated frame: {} bytes left at end of stream", self.buf.len()))) }
    }
}

/// Header + payload.
pub fn encode_frame(format: Format, msg_type: u16, seq: u16, sig: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(&FrameHeader::new(format, msg_type, seq, sig, payload.len() as u32).encode());
    out.extend_from_slice(payload);
    out
}
