//! PLC port: accepts connections of active PLCs, sniffs the framing (FRAME / NDJSON → session, HTTP →
//! `POST /api/plc/{plc}/{msg}` and `GET /api/plc/{plc}/command`).

use std::sync::Arc;
use std::time::Duration;

use plc_link::framing::http::CONTINUE_100;
use plc_link::framing::{ContentLength, HttpRequestDecoder, Sniff, sniff_detail, write_response};
use plc_link::{ErrCode, Format, Framing, LinkError, RawFrame, Role, Strictness};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::hub::{Hub, Mode};
use crate::log::{Dir, LogEntry};
use crate::session::{self, SessionParams, error_entry, hello_info, report_warnings};
use crate::wire::ref_of;

/// Accept loop (runs until the task is dropped).
pub async fn serve(listener: TcpListener, hub: Arc<Hub>) {
    loop {
        match listener.accept().await {
            Ok((s, peer)) => {
                let hub = hub.clone();
                tokio::spawn(async move { handle(hub, s, peer.to_string()).await });
            }
            Err(e) => {
                tracing::warn!(%e, "PLC port accept");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

async fn handle(hub: Arc<Hub>, mut s: TcpStream, peer: String) {
    let _ = s.set_nodelay(true);
    let mut initial = Vec::new();
    let mut buf = [0u8; 4096];
    let framing = loop {
        match sniff_detail(&initial) {
            Sniff::Found(f) => break f,
            Sniff::Unknown => {
                let mut e = LogEntry::new(&peer, Dir::Rx);
                e.ok = false;
                e.error = Some(format!("unknown framing (first bytes {})", crate::util::hex(&initial[..initial.len().min(8)])));
                e.peer = Some(peer.clone());
                e.len = initial.len();
                hub.log(e, None);
                return;
            }
            Sniff::NeedMore => {}
        }
        match tokio::time::timeout(Duration::from_secs(30), s.read(&mut buf)).await {
            Ok(Ok(n)) if n > 0 => initial.extend_from_slice(&buf[..n]),
            _ => return,
        }
    };
    tracing::debug!(%peer, ?framing, "PLC connection");
    match framing {
        Framing::Frame | Framing::Ndjson => {
            let end = session::run(hub, s, initial, SessionParams { framing, mode: Mode::Active, opener: false, plc: None, format: Format::Json, peer: peer.clone() }).await;
            tracing::info!(%peer, reason = %end.reason, "PLC session ended");
        }
        Framing::Http => http_active(hub, s, initial, peer).await,
    }
}

fn plain(status: u16) -> Vec<u8> {
    write_response(status, &[], Some(b""), ContentLength::Plain)
}

async fn http_active(hub: Arc<Hub>, stream: TcpStream, initial: Vec<u8>, peer: String) {
    let (mut rd, mut wr) = stream.into_split();
    let mut dec = HttpRequestDecoder::pc();
    dec.feed(&initial);
    let idle = (hub.heartbeat() * 3).max(Duration::from_secs(30));
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        loop {
            match dec.next() {
                Ok(Some(raw)) => {
                    let close = raw.http.as_ref().is_some_and(|h| h.close);
                    let resp = handle_request(&hub, &raw, &peer);
                    if wr.write_all(&resp).await.is_err() || close {
                        let _ = wr.shutdown().await;
                        return;
                    }
                }
                Ok(None) => {
                    if dec.take_continue() && wr.write_all(CONTINUE_100).await.is_err() {
                        return;
                    }
                    break;
                }
                Err(e) => {
                    let raw = RawFrame { framing: Framing::Http, format: Format::Json, msg_type: None, seq: None, sig: None, payload: Vec::new(), http: None };
                    hub.log(error_entry(&hub, &peer, &raw, &e, 0, &peer), None);
                    let status = e.http_status.unwrap_or(400);
                    let resp = hub.codec().ack(0, 0, e.code.code(), &e.msg, 0).and_then(|a| hub.codec().to_http_response(status, Some(&a), Format::Json)).unwrap_or_else(|_| plain(status));
                    let _ = wr.write_all(&resp).await;
                    let _ = wr.shutdown().await;
                    return;
                }
            }
        }
        match tokio::time::timeout(idle, rd.read(&mut buf)).await {
            Ok(Ok(n)) if n > 0 => dec.feed(&buf[..n]),
            _ => {
                let _ = wr.shutdown().await;
                return;
            }
        }
    }
}

/// One request of an HTTP-active PLC → complete response bytes.
fn handle_request(hub: &Hub, raw: &RawFrame, peer: &str) -> Vec<u8> {
    let codec = hub.codec();
    let Some(meta) = raw.http.as_ref() else { return plain(400) };
    let method = meta.method.as_deref().unwrap_or("");
    let path = meta.path.as_deref().unwrap_or("").split('?').next().unwrap_or("");
    let segs: Vec<&str> = path.trim_matches('/').split('/').collect();
    let ["api", "plc", plc, msg] = segs[..] else { return plain(404) };
    if plc.is_empty() || msg.is_empty() {
        return plain(404);
    }
    let fmt_for = |f: Format| if hub.registry_ok(plc) == Some(false) { Format::Json } else { f };
    if msg == "command" {
        if method != "GET" {
            return plain(405);
        }
        hub.touch_http(plc, peer, raw.format);
        return match hub.pop_http_command(plc) {
            Some(m) => {
                let fmt = fmt_for(raw.format);
                match codec.to_http_response(200, Some(&m), fmt) {
                    Ok(b) => {
                        let body_len = codec.http_parts(&m, fmt).map(|p| p.1.len()).unwrap_or(0);
                        hub.log(hub.msg_entry(plc, Dir::Tx, Framing::Http, fmt, &m, body_len, Some(peer)), Some(&m));
                        b
                    }
                    Err(_) => plain(500),
                }
            }
            None => plain(204),
        };
    }
    if method != "POST" {
        return plain(405);
    }
    hub.touch_http(plc, peer, raw.format);
    let wire_len = raw.payload.len();
    let decoded = if raw.format == Format::Bin && hub.registry_ok(plc) == Some(false) && !msg.eq_ignore_ascii_case("hello") {
        Err(LinkError::new(ErrCode::FormatNotAllowed, "registry hash mismatch (contract_mismatch): BIN refused, send JSON"))
    } else {
        codec.decode_raw_report(raw, Role::Pc, Strictness::Lenient).and_then(|(m, r)| {
            let spec = codec.spec(m.id)?;
            if spec.http_name() != msg { Err(LinkError::parse(format!("route /{msg} but the body is {}", spec.name))) } else { Ok((m, r)) }
        })
    };
    let (ref_t, ref_s, code, text) = match decoded {
        Ok((m, report)) => {
            let mut entry = hub.msg_entry(plc, Dir::Rx, Framing::Http, raw.format, &m, wire_len, Some(peer));
            entry.warnings = report_warnings(&report);
            if codec.registry().by_name("Hello").is_some_and(|h| h.id == m.id) {
                let (data, _, ok, _) = hello_info(codec, &m);
                if !ok {
                    entry.warnings.push(format!("registry hash mismatch: PC 0x{:08X} (contract_mismatch, BIN refused)", codec.registry().hash()));
                }
                hub.log(entry, Some(&m));
                hub.set_hello(plc, data, ok);
            } else {
                hub.log(entry, Some(&m));
                hub.command_reply(plc, &m);
            }
            (m.id, m.seq, 0i16, String::new())
        }
        Err(e) => {
            hub.log(error_entry(hub, plc, raw, &e, wire_len, peer), None);
            let (t, s) = ref_of(codec, raw);
            (t, s, e.code.code(), e.msg)
        }
    };
    let fmt = fmt_for(raw.format);
    match codec.ack(ref_t, ref_s, code, &text, hub.next_seq(plc)) {
        Ok(ack) => match codec.to_http_response(200, Some(&ack), fmt) {
            Ok(b) => {
                let body_len = codec.http_parts(&ack, fmt).map(|p| p.1.len()).unwrap_or(0);
                hub.log(hub.msg_entry(plc, Dir::Tx, Framing::Http, fmt, &ack, body_len, Some(peer)), Some(&ack));
                b
            }
            Err(_) => plain(500),
        },
        Err(_) => plain(if code == 0 { 200 } else { 400 }),
    }
}
