//! NDJSON framing: one envelope per `\n`-terminated line (a preceding `\r` is stripped, empty lines ignored).

use crate::error::{ErrCode, LinkError};
use crate::framing::{Framing, RawFrame};
use crate::header::Format;

#[derive(Debug)]
pub struct NdjsonDecoder {
    buf: Vec<u8>,
    max_line: usize,
    scanned: usize,
    failed: Option<LinkError>,
}

impl NdjsonDecoder {
    /// `max_line` = maximum line length without the terminator (PC 65535, PLC 8176).
    pub fn new(max_line: usize) -> Self {
        NdjsonDecoder { buf: Vec::new(), max_line, scanned: 0, failed: None }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    pub fn buffered(&self) -> usize {
        self.buf.len()
    }

    /// Returns the next non-empty line as a JSON-format raw frame. A line longer than the maximum is a sticky
    /// `BAD_LENGTH` error (detected as soon as the buffered partial line exceeds it).
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<RawFrame>, LinkError> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        loop {
            let Some(rel) = self.buf[self.scanned..].iter().position(|b| *b == b'\n') else {
                self.scanned = self.buf.len();
                // a trailing '\r' may still belong to the terminator
                let partial = self.buf.len() - usize::from(self.buf.last() == Some(&b'\r'));
                if partial > self.max_line {
                    return Err(self.fail(partial));
                }
                return Ok(None);
            };
            let nl = self.scanned + rel;
            let mut line: Vec<u8> = self.buf.drain(..=nl).collect();
            self.scanned = 0;
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.len() > self.max_line {
                return Err(self.fail(line.len()));
            }
            if line.iter().all(|b| b.is_ascii_whitespace()) {
                continue;
            }
            return Ok(Some(RawFrame { framing: Framing::Ndjson, format: Format::Json, msg_type: None, seq: None, sig: None, payload: line, http: None }));
        }
    }

    fn fail(&mut self, len: usize) -> LinkError {
        let e = LinkError::new(ErrCode::BadLength, format!("NDJSON line of {len} bytes exceeds maximum {}", self.max_line));
        self.failed = Some(e.clone());
        e
    }

    /// End of stream: an unterminated non-empty line is reported as truncated.
    pub fn finish(&self) -> Result<(), LinkError> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        if self.buf.iter().all(|b| b.is_ascii_whitespace()) { Ok(()) } else { Err(LinkError::new(ErrCode::BadLength, "unterminated NDJSON line at end of stream")) }
    }
}

/// Envelope text + `\n`.
pub fn encode_ndjson(json_text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(json_text.len() + 1);
    out.extend_from_slice(json_text.as_bytes());
    out.push(b'\n');
    out
}
