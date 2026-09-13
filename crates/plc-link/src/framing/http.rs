//! HTTP/1.1 subset framing (wire-spec section 1.3): incremental request / response decoders and writers.

use crate::error::{ErrCode, LinkError};
use crate::framing::{Framing, HttpMeta, PC_MAX_PAYLOAD, PLC_MAX_PAYLOAD, RawFrame};
use crate::header::Format;

/// Header section maximum on the PC.
pub const PC_MAX_HEADER: usize = 8 * 1024;
/// Header section maximum on the PLC.
pub const PLC_MAX_HEADER: usize = 1024;
/// Interim response for `Expect: 100-continue`.
pub const CONTINUE_100: &[u8] = b"HTTP/1.1 100 Continue\r\n\r\n";

/// `Content-Length` value style: plain decimal or exactly 5 digits with leading zeros (what the PLC writes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContentLength {
    #[default]
    Plain,
    Pad5,
}

pub fn status_text(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

/// MIME type of a payload format.
pub fn mime(format: Format) -> &'static str {
    match format {
        Format::Json => "application/json",
        Format::Bin => "application/octet-stream",
    }
}

fn bad(msg: impl Into<String>) -> LinkError {
    LinkError::http(400, ErrCode::Parse, msg)
}

#[derive(Debug)]
struct Pending {
    meta: HttpMeta,
    format: Format,
    msg_type: Option<u16>,
    seq: Option<u16>,
    sig: Option<u32>,
    head_len: usize,
    body_len: usize,
    continue_taken: bool,
}

#[derive(Debug)]
struct Core {
    buf: Vec<u8>,
    request: bool,
    max_header: usize,
    max_body: usize,
    pending: Option<Pending>,
    failed: Option<LinkError>,
}

fn find_head_end(buf: &[u8]) -> Option<usize> {
    for (i, b) in buf.iter().enumerate() {
        if *b != b'\n' {
            continue;
        }
        match (buf.get(i + 1), buf.get(i + 2)) {
            (Some(b'\n'), _) => return Some(i + 2),
            (Some(b'\r'), Some(b'\n')) => return Some(i + 3),
            _ => {}
        }
    }
    None
}

fn parse_version(v: &str) -> Result<bool, LinkError> {
    match v {
        "HTTP/1.1" => Ok(false),
        "HTTP/1.0" => Ok(true),
        _ => Err(bad(format!("unsupported HTTP version {v:?}"))),
    }
}

/// `0xHHHHHHHH` (also without prefix or with `16#`).
pub fn parse_sig_header(s: &str) -> Option<u32> {
    let t = s.trim();
    let hex = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).or_else(|| t.strip_prefix("16#")).unwrap_or(t);
    if hex.is_empty() || hex.len() > 8 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

impl Core {
    fn new(request: bool, max_header: usize, max_body: usize) -> Self {
        Core { buf: Vec::new(), request, max_header, max_body, pending: None, failed: None }
    }

    fn next(&mut self) -> Result<Option<RawFrame>, LinkError> {
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
        if self.pending.is_none() {
            let skip = self.buf.iter().take_while(|b| **b == b'\r' || **b == b'\n').count();
            if skip > 0 {
                self.buf.drain(..skip);
            }
            let Some(head_len) = find_head_end(&self.buf) else {
                if self.buf.len() > self.max_header {
                    return Err(LinkError::http(431, ErrCode::BadLength, format!("header section exceeds {} bytes", self.max_header)));
                }
                return Ok(None);
            };
            if head_len > self.max_header {
                return Err(LinkError::http(431, ErrCode::BadLength, format!("header section of {head_len} bytes exceeds {}", self.max_header)));
            }
            let head = String::from_utf8_lossy(&self.buf[..head_len]).into_owned();
            self.pending = Some(self.parse_head(&head, head_len)?);
        }
        let Some(p) = &self.pending else { return Ok(None) };
        let total = p.head_len + p.body_len;
        if self.buf.len() < total {
            return Ok(None);
        }
        let Some(p) = self.pending.take() else { return Ok(None) };
        let payload = self.buf[p.head_len..total].to_vec();
        self.buf.drain(..total);
        Ok(Some(RawFrame { framing: Framing::Http, format: p.format, msg_type: p.msg_type, seq: p.seq, sig: p.sig, payload, http: Some(p.meta) }))
    }

    fn parse_head(&self, head: &str, head_len: usize) -> Result<Pending, LinkError> {
        let mut lines = head.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l));
        let start = lines.next().unwrap_or("");
        let mut meta = HttpMeta::default();
        let http10 = if self.request {
            let parts: Vec<&str> = start.split(' ').collect();
            let [method, path, version] = parts[..] else {
                return Err(bad(format!("bad request line {start:?}")));
            };
            if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) || path.is_empty() {
                return Err(bad(format!("bad request line {start:?}")));
            }
            meta.method = Some(method.to_string());
            meta.path = Some(path.to_string());
            parse_version(version)?
        } else {
            let mut parts = start.splitn(3, ' ');
            let version = parts.next().unwrap_or("");
            let code = parts.next().unwrap_or("");
            let http10 = parse_version(version)?;
            if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad(format!("bad status line {start:?}")));
            }
            meta.status = code.parse().ok();
            http10
        };
        for line in lines {
            if line.is_empty() {
                continue;
            }
            let Some((k, v)) = line.split_once(':') else {
                return Err(bad(format!("bad header line {line:?}")));
            };
            if k.is_empty() || k.bytes().any(|b| b.is_ascii_whitespace()) {
                return Err(bad(format!("bad header name {k:?}")));
            }
            meta.headers.push((k.to_string(), v.trim().to_string()));
        }
        if meta.header("Transfer-Encoding").is_some() {
            return Err(LinkError::http(501, ErrCode::Parse, "Transfer-Encoding is not supported (Content-Length required)"));
        }
        let mut cl: Option<u64> = None;
        for (k, v) in &meta.headers {
            if !k.eq_ignore_ascii_case("Content-Length") {
                continue;
            }
            if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad(format!("bad Content-Length {v:?}")));
            }
            let digits = v.trim_start_matches('0');
            if digits.len() > 18 {
                return Err(LinkError::http(413, ErrCode::BadLength, format!("Content-Length {v} too large")));
            }
            let n: u64 = if digits.is_empty() { 0 } else { digits.parse().map_err(|_| bad("bad Content-Length"))? };
            if cl.is_some_and(|prev| prev != n) {
                return Err(bad("conflicting Content-Length headers"));
            }
            cl = Some(n);
        }
        // 1xx / 204 / 304 responses never have a body; the PLC still writes `Content-Length: 00000` on 204
        let bodyless_status = !self.request && meta.status.is_some_and(|s| (100..200).contains(&s) || s == 204 || s == 304);
        let body_len = match cl {
            _ if bodyless_status => 0,
            Some(n) if n > self.max_body as u64 => return Err(LinkError::http(413, ErrCode::BadLength, format!("body of {n} bytes exceeds maximum {}", self.max_body))),
            Some(n) => n as usize,
            None if self.request => {
                if matches!(meta.method.as_deref(), Some("POST" | "PUT" | "PATCH")) {
                    return Err(LinkError::http(411, ErrCode::BadLength, "Content-Length required"));
                }
                0
            }
            None => {
                let s = meta.status.unwrap_or(0);
                if (100..200).contains(&s) || s == 204 || s == 304 {
                    0
                } else {
                    return Err(LinkError::http(411, ErrCode::BadLength, format!("response {s} without Content-Length")));
                }
            }
        };
        meta.expect_continue = self.request && meta.header("Expect").is_some_and(|v| v.eq_ignore_ascii_case("100-continue"));
        let conn = meta.header("Connection").map(|v| v.to_ascii_lowercase()).unwrap_or_default();
        let tokens: Vec<&str> = conn.split(',').map(str::trim).collect();
        meta.close = tokens.contains(&"close") || (http10 && !tokens.contains(&"keep-alive"));
        let ctype = meta.header("Content-Type").or(if body_len == 0 { meta.header("Accept") } else { None });
        let format = if ctype.is_some_and(|t| t.trim().to_ascii_lowercase().starts_with("application/octet-stream")) { Format::Bin } else { Format::Json };
        let msg_type = meta.header("X-GR-Type").and_then(|t| t.trim().parse::<u16>().ok());
        let seq = match meta.header("X-GR-Seq") {
            Some(s) => Some(s.trim().parse::<u16>().map_err(|_| bad(format!("bad X-GR-Seq {s:?}")))?),
            None => None,
        };
        let sig = match meta.header("X-GR-Sig") {
            Some(s) => Some(parse_sig_header(s).ok_or_else(|| bad(format!("bad X-GR-Sig {s:?}")))?),
            None => None,
        };
        Ok(Pending { meta, format, msg_type, seq, sig, head_len, body_len, continue_taken: false })
    }
}

/// Server side: incremental request decoder (pipelined requests are returned one by one).
#[derive(Debug)]
pub struct HttpRequestDecoder {
    core: Core,
}

impl HttpRequestDecoder {
    pub fn new(max_header: usize, max_body: usize) -> Self {
        HttpRequestDecoder { core: Core::new(true, max_header, max_body) }
    }

    /// PC limits (8 KB header section, 65535 B body).
    pub fn pc() -> Self {
        Self::new(PC_MAX_HEADER, PC_MAX_PAYLOAD)
    }

    /// PLC limits (1024 B header section, 8176 B body).
    pub fn plc() -> Self {
        Self::new(PLC_MAX_HEADER, PLC_MAX_PAYLOAD)
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.core.buf.extend_from_slice(bytes);
    }

    pub fn buffered(&self) -> usize {
        self.core.buf.len()
    }

    /// Next complete request. Errors carry `http_status` (400, 411, 413, 431, 501) and are sticky.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<RawFrame>, LinkError> {
        self.core.next()
    }

    /// After `next` returned `None`: true exactly once when the request being received sent
    /// `Expect: 100-continue` and its body is still missing → write [`CONTINUE_100`].
    pub fn take_continue(&mut self) -> bool {
        let have = self.core.buf.len();
        match &mut self.core.pending {
            Some(p) if p.meta.expect_continue && !p.continue_taken && p.body_len > 0 && have < p.head_len + p.body_len => {
                p.continue_taken = true;
                true
            }
            _ => false,
        }
    }
}

/// Client side: incremental response decoder (`100 Continue` interim responses are returned as frames too).
#[derive(Debug)]
pub struct HttpResponseDecoder {
    core: Core,
}

impl HttpResponseDecoder {
    pub fn new(max_header: usize, max_body: usize) -> Self {
        HttpResponseDecoder { core: Core::new(false, max_header, max_body) }
    }

    pub fn pc() -> Self {
        Self::new(PC_MAX_HEADER, PC_MAX_PAYLOAD)
    }

    pub fn plc() -> Self {
        Self::new(PLC_MAX_HEADER, PLC_MAX_PAYLOAD)
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.core.buf.extend_from_slice(bytes);
    }

    pub fn buffered(&self) -> usize {
        self.core.buf.len()
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<RawFrame>, LinkError> {
        self.core.next()
    }
}

fn push_content_length(out: &mut Vec<u8>, n: usize, cl: ContentLength) {
    let line = match cl {
        ContentLength::Plain => format!("Content-Length: {n}\r\n"),
        ContentLength::Pad5 => format!("Content-Length: {n:05}\r\n"),
    };
    out.extend_from_slice(line.as_bytes());
}

/// `METHOD path HTTP/1.1`, `Host`, `headers` in order, `Content-Length` (only with a body), blank line, body.
pub fn write_request(method: &str, path: &str, host: &str, headers: &[(&str, &str)], body: Option<&[u8]>, cl: ContentLength) -> Vec<u8> {
    let mut out = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\n").into_bytes();
    for (k, v) in headers {
        out.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
    }
    if let Some(b) = body {
        push_content_length(&mut out, b.len(), cl);
    }
    out.extend_from_slice(b"\r\n");
    if let Some(b) = body {
        out.extend_from_slice(b);
    }
    out
}

/// `GET path HTTP/1.1`, `Host`, `Accept: <mime>`, blank line.
pub fn write_get(path: &str, host: &str, accept: Format) -> Vec<u8> {
    write_request("GET", path, host, &[("Accept", mime(accept))], None, ContentLength::Plain)
}

/// `HTTP/1.1 <code> <text>`, `headers` in order, `Content-Length` (omitted for 1xx / 204 / 304), blank line, body.
pub fn write_response(status: u16, headers: &[(&str, &str)], body: Option<&[u8]>, cl: ContentLength) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status} {}\r\n", status_text(status)).into_bytes();
    for (k, v) in headers {
        out.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
    }
    let bodyless = (100..200).contains(&status) || status == 204 || status == 304;
    if !bodyless {
        push_content_length(&mut out, body.map_or(0, <[u8]>::len), cl);
    }
    out.extend_from_slice(b"\r\n");
    if !bodyless && let Some(b) = body {
        out.extend_from_slice(b);
    }
    out
}
