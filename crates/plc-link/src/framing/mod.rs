//! Framings (wire-spec section 1): incremental decoders (`feed` + `next`) and encoders.

pub mod frame;
pub mod http;
pub mod ndjson;
pub mod sniff;

use serde::Serialize;

use crate::header::Format;

pub use frame::{FrameDecoder, encode_frame};
pub use http::{ContentLength, HttpRequestDecoder, HttpResponseDecoder, status_text, write_request, write_response};
pub use ndjson::{NdjsonDecoder, encode_ndjson};
pub use sniff::{Sniff, sniff, sniff_detail};

/// PC receive maximum for FRAME Length / NDJSON line / HTTP body.
pub const PC_MAX_PAYLOAD: usize = 65535;
/// PLC receive maximum for FRAME Length / NDJSON line / HTTP body.
pub const PLC_MAX_PAYLOAD: usize = 8176;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Framing {
    Frame,
    Ndjson,
    Http,
}

/// HTTP request / response details.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct HttpMeta {
    /// Request method (requests only).
    pub method: Option<String>,
    /// Request target (requests only).
    pub path: Option<String>,
    /// Status code (responses only).
    pub status: Option<u16>,
    /// Header lines in order (names as received).
    pub headers: Vec<(String, String)>,
    /// `Expect: 100-continue` was present.
    pub expect_continue: bool,
    /// The connection is to be closed after this message (`Connection: close` or HTTP/1.0).
    pub close: bool,
}

impl HttpMeta {
    /// First header value with that name (case-insensitive).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// One complete message as cut from the byte stream (not yet interpreted by the codec).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RawFrame {
    pub framing: Framing,
    pub format: Format,
    /// FRAME header MsgType, or numeric `X-GR-Type`.
    pub msg_type: Option<u16>,
    /// FRAME header Seq, or `X-GR-Seq`.
    pub seq: Option<u16>,
    /// FRAME header LayoutSig, or `X-GR-Sig`.
    pub sig: Option<u32>,
    pub payload: Vec<u8>,
    pub http: Option<HttpMeta>,
}
