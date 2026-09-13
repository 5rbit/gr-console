//! Minimal HTTP/1.1 client over one TCP connection (plc-link response decoder). Used for passive HTTP PLCs,
//! the HTTP-active simulator, the self test and the tests.

use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use plc_link::RawFrame;
use plc_link::framing::HttpResponseDecoder;
use plc_link::framing::http::PC_MAX_HEADER;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub struct HttpConn {
    stream: TcpStream,
    dec: HttpResponseDecoder,
    /// `Host` header value.
    pub host: String,
}

impl HttpConn {
    pub fn new(stream: TcpStream, host: &str, max_body: usize) -> Self {
        HttpConn { stream, dec: HttpResponseDecoder::new(PC_MAX_HEADER, max_body), host: host.to_string() }
    }

    pub async fn connect(addr: &str, max_body: usize, timeout: Duration) -> anyhow::Result<Self> {
        let s = tokio::time::timeout(timeout, TcpStream::connect(addr)).await.map_err(|_| anyhow!("connect {addr}: timeout"))?.with_context(|| format!("connect {addr}"))?;
        let _ = s.set_nodelay(true);
        Ok(Self::new(s, addr, max_body))
    }

    pub fn stream_mut(&mut self) -> &mut TcpStream {
        &mut self.stream
    }

    /// Writes a complete request and reads the final (non-1xx) response.
    pub async fn roundtrip(&mut self, request: &[u8], timeout: Duration) -> anyhow::Result<RawFrame> {
        self.stream.write_all(request).await.context("write request")?;
        self.read_response(timeout).await
    }

    /// Writes a request in small pieces (fault injection) and reads the response.
    pub async fn roundtrip_split(&mut self, request: &[u8], timeout: Duration) -> anyhow::Result<RawFrame> {
        crate::wire::write_bytes(&mut self.stream, request, true).await.context("write request")?;
        self.read_response(timeout).await
    }

    pub async fn read_response(&mut self, timeout: Duration) -> anyhow::Result<RawFrame> {
        let deadline = tokio::time::Instant::now() + timeout;
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            while let Some(r) = self.dec.next().map_err(|e| anyhow!("bad response: {e}"))? {
                let status = r.http.as_ref().and_then(|h| h.status).unwrap_or(0);
                if (100..200).contains(&status) {
                    continue;
                }
                return Ok(r);
            }
            let n = tokio::time::timeout_at(deadline, self.stream.read(&mut buf)).await.map_err(|_| anyhow!("response timeout"))?.context("read response")?;
            if n == 0 {
                bail!("connection closed before a complete response");
            }
            self.dec.feed(&buf[..n]);
        }
    }
}

/// A plain response for API calls.
#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn json(&self) -> anyhow::Result<serde_json::Value> {
        serde_json::from_slice(&self.body).with_context(|| format!("HTTP {} body is not JSON: {}", self.status, String::from_utf8_lossy(&self.body)))
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// One request on a fresh connection (`Connection: close`). `body` = (content type, bytes).
pub async fn call(addr: &str, method: &str, path: &str, body: Option<(&str, &[u8])>, timeout: Duration) -> anyhow::Result<Response> {
    let mut c = HttpConn::connect(addr, 64 * 1024 * 1024, timeout).await?;
    let mut headers: Vec<(&str, &str)> = vec![("Connection", "close"), ("Accept", "*/*")];
    let len_text;
    if let Some((ct, b)) = body {
        headers.push(("Content-Type", ct));
        len_text = b.len().to_string();
        headers.push(("Content-Length", &len_text));
    }
    let mut req = plc_link::framing::write_request(method, path, &c.host.clone(), &headers, None, plc_link::framing::ContentLength::Plain);
    if let Some((_, b)) = body {
        req.extend_from_slice(b);
    }
    let r = c.roundtrip(&req, timeout).await?;
    let meta = r.http.unwrap_or_default();
    Ok(Response { status: meta.status.unwrap_or(0), headers: meta.headers, body: r.payload })
}

pub async fn get_json(addr: &str, path: &str) -> anyhow::Result<(u16, serde_json::Value)> {
    let r = call(addr, "GET", path, None, Duration::from_secs(10)).await?;
    Ok((r.status, r.json()?))
}

pub async fn post_json(addr: &str, path: &str, body: &serde_json::Value, timeout: Duration) -> anyhow::Result<(u16, serde_json::Value)> {
    let text = serde_json::to_vec(body)?;
    let r = call(addr, "POST", path, Some(("application/json", &text)), timeout).await?;
    let v = if r.body.is_empty() { serde_json::Value::Null } else { r.json()? };
    Ok((r.status, v))
}
