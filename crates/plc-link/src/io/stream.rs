//! Session framing over a byte stream: incremental decoder, message encoder and the best-effort reference
//! of a frame that failed to decode (used for the Ack).

use crate::codec::{Codec, Message};
use crate::error::{ErrCode, LinkError};
use crate::framing::{FrameDecoder, Framing, NdjsonDecoder, PC_MAX_PAYLOAD, PLC_MAX_PAYLOAD, RawFrame};
use crate::header::Format;

/// Incremental decoder of a session framing (HTTP has its own request / response decoders).
pub enum StreamDecoder {
    Frame(FrameDecoder),
    Ndjson(NdjsonDecoder),
}

impl StreamDecoder {
    /// `max` = receive maximum (PC 65535, PLC 8176).
    pub fn new(framing: Framing, max: usize) -> Result<Self, LinkError> {
        match framing {
            Framing::Frame => Ok(StreamDecoder::Frame(FrameDecoder::new(max as u32))),
            Framing::Ndjson => Ok(StreamDecoder::Ndjson(NdjsonDecoder::new(max))),
            Framing::Http => Err(LinkError::new(ErrCode::FormatNotAllowed, "HTTP is not a session framing")),
        }
    }

    /// Decoder with the PC receive maximum.
    pub fn pc(framing: Framing) -> Result<Self, LinkError> {
        Self::new(framing, PC_MAX_PAYLOAD)
    }

    /// Decoder with the PLC receive maximum.
    pub fn plc(framing: Framing) -> Result<Self, LinkError> {
        Self::new(framing, PLC_MAX_PAYLOAD)
    }

    pub fn feed(&mut self, b: &[u8]) {
        match self {
            StreamDecoder::Frame(d) => d.feed(b),
            StreamDecoder::Ndjson(d) => d.feed(b),
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<RawFrame>, LinkError> {
        match self {
            StreamDecoder::Frame(d) => d.next(),
            StreamDecoder::Ndjson(d) => d.next(),
        }
    }
}

/// Bytes on the wire of a received frame (FRAME: header + payload, NDJSON: line + newline).
pub fn wire_len(framing: Framing, payload_len: usize) -> usize {
    payload_len + if framing == Framing::Frame { crate::header::HEADER_LEN } else { 1 }
}

/// Message → bytes of a session framing. NDJSON always carries JSON.
pub fn encode(codec: &Codec, framing: Framing, format: Format, m: &Message) -> Result<Vec<u8>, LinkError> {
    match framing {
        Framing::Frame => codec.to_frame(m, format),
        Framing::Ndjson => {
            if format == Format::Bin {
                return Err(LinkError::new(ErrCode::FormatNotAllowed, "NDJSON carries JSON only"));
            }
            codec.to_ndjson(m)
        }
        Framing::Http => Err(LinkError::new(ErrCode::FormatNotAllowed, "HTTP is not a session framing")),
    }
}

/// Best-effort type / seq of a frame that failed to decode (for the Ack).
pub fn ref_of(codec: &Codec, raw: &RawFrame) -> (u16, u16) {
    let mut t = raw.msg_type.unwrap_or(0);
    let mut s = raw.seq.unwrap_or(0);
    if raw.format == Format::Json
        && let Ok(env) = crate::parse_envelope(&raw.payload)
    {
        if t == 0 {
            t = env.type_id.or_else(|| env.type_name.as_deref().and_then(|n| codec.registry().by_name(n)).map(|m| m.id)).unwrap_or(0);
        }
        if s == 0 {
            s = env.seq;
        }
    }
    if t == 0
        && let Some(h) = &raw.http
    {
        if let Some(n) = h.header("X-GR-Type").and_then(|x| codec.registry().lookup(x)) {
            t = n.id;
        } else if let Some(n) = h.path.as_deref().and_then(|p| p.split('?').next()).and_then(|p| p.rsplit('/').next()).and_then(|x| codec.registry().by_http_name(x)) {
            t = n.id;
        }
    }
    (t, s)
}
