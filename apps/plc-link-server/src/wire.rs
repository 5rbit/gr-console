//! Stream decoders / encoders over FRAME and NDJSON, and writers with injectable faults (simulator).

use std::time::Duration;

use plc_link::framing::{FrameDecoder, NdjsonDecoder, PC_MAX_PAYLOAD};
use plc_link::{Codec, ErrCode, Format, Framing, LinkError, Message, RawFrame};
use tokio::io::{AsyncWrite, AsyncWriteExt};

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

    pub fn pc(framing: Framing) -> Result<Self, LinkError> {
        Self::new(framing, PC_MAX_PAYLOAD)
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
        && let Ok(env) = plc_link::parse_envelope(&raw.payload)
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

/// Simulator write faults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// Every message written in 1–3 byte pieces with a short pause.
    SplitWrites,
    /// Messages collected and written together.
    Coalesce,
    /// BIN signature (FRAME header / X-GR-Sig / envelope sig) wrong on MeasLog.
    BadSig,
    /// One frame with a bad magic per first connection.
    BadMagic,
}

impl std::str::FromStr for Fault {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "split-writes" => Ok(Fault::SplitWrites),
            "coalesce" => Ok(Fault::Coalesce),
            "bad-sig" => Ok(Fault::BadSig),
            "bad-magic" => Ok(Fault::BadMagic),
            _ => Err(format!("unknown fault {s:?} (split-writes|coalesce|bad-sig|bad-magic)")),
        }
    }
}

/// Writes `bytes`, split into small pieces when `split`.
pub async fn write_bytes<W: AsyncWrite + Unpin>(w: &mut W, bytes: &[u8], split: bool) -> std::io::Result<()> {
    if !split {
        return w.write_all(bytes).await;
    }
    let mut i = 0;
    let mut k = 0usize;
    while i < bytes.len() {
        let n = (1 + k % 3).min(bytes.len() - i);
        w.write_all(&bytes[i..i + n]).await?;
        w.flush().await?;
        i += n;
        k += 1;
        // flush per piece splits the TCP segments; pause rarely (a Windows sleep lasts ~15 ms)
        if k.is_multiple_of(512) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        } else if k.is_multiple_of(16) {
            tokio::task::yield_now().await;
        }
    }
    Ok(())
}
