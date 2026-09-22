//! PLC socket link listener.
//!
//! The console listens on a TCP port; a PLC slot configured as Active connects to it, says Hello and then
//! pushes messages (`docs/link/wire-spec.md`). The transport itself is `plc_link::io`; this module only keeps
//! one task per connection, routes `Trace` chunks and `TraceCfg` acknowledgements into the trace store, and
//! lets the store send a `TraceCfg` to a named PLC.
//!
//! Commands (`Command` / `CommandResult`) still go over OPC UA; nothing here writes to a PLC except the
//! read-only trace configuration.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use plc_layout::Contract;
use plc_link::io::session::{LinkSession, SessionCommand, SessionEvent, SessionParams, sniff_framing};
use plc_link::{Codec, Format, Message, Registry, RegistryDoc};
use serde_json::Value as Json;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::trace::TraceStore;
use crate::trace::store::LinkRequest;

/// The message registry, embedded so a deployed executable needs no `plc/` directory.
const MESSAGES_TOML: &str = include_str!("../../../../plc/link/messages.toml");

/// How long an accepted connection may take to reveal its framing.
const SNIFF_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// What a connection task accepts from the rest of the console.
enum AppReq {
    /// Send a `TraceCfg`; the reply carries the sequence number the session assigned.
    TraceCfg { cfg: Json, reply: oneshot::Sender<Result<u16, String>> },
}

/// Live connections by PLC name (from `Hello.Plc`).
type Sessions = Arc<Mutex<HashMap<String, mpsc::Sender<AppReq>>>>;

/// Starts the listener. Returns without a listener (after logging) when the port cannot be bound, so a busy
/// port never stops the console from starting.
pub async fn spawn(bind: &str, contract: Arc<Contract>, trace: Arc<TraceStore>) {
    let doc: RegistryDoc = match MESSAGES_TOML.parse() {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("PLC link disabled: message registry: {e}");
            return;
        }
    };
    let registry: Registry = match doc.resolve(&contract) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("PLC link disabled: {e}");
            return;
        }
    };
    for m in &registry.messages {
        if !m.available {
            tracing::warn!(message = %m.name, "PLC link: message not available in the contract");
        }
    }
    let codec = Codec::new(contract, Arc::new(registry));
    let listener = match TcpListener::bind(bind).await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("PLC link disabled: bind {bind}: {e}");
            return;
        }
    };
    let addr = listener.local_addr().map(|a| a.to_string()).unwrap_or_else(|_| bind.to_string());
    tracing::info!(addr = %addr, "PLC link listening");

    let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));
    let (req_tx, req_rx) = mpsc::channel::<LinkRequest>(16);
    trace.set_link(Some(req_tx)).await;
    tokio::spawn(route_requests(req_rx, sessions.clone()));

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    let (codec, trace, sessions) = (codec.clone(), trace.clone(), sessions.clone());
                    tokio::spawn(async move { connection(stream, peer, codec, trace, sessions).await });
                }
                Err(e) => {
                    tracing::warn!("PLC link accept: {e}");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    });
}

/// Forwards store requests to the connection of the named PLC.
async fn route_requests(mut rx: mpsc::Receiver<LinkRequest>, sessions: Sessions) {
    while let Some(req) = rx.recv().await {
        let LinkRequest::Config { plc, cfg, reply } = req;
        let tx = {
            let map = sessions.lock().await;
            map.get(&plc).cloned().or_else(|| map.iter().find(|(k, _)| k.eq_ignore_ascii_case(&plc)).map(|(_, v)| v.clone()))
        };
        match tx {
            Some(tx) => {
                if tx.send(AppReq::TraceCfg { cfg, reply }).await.is_err() {
                    tracing::warn!(plc = %plc, "PLC link: the connection closed while sending TraceCfg");
                }
            }
            None => {
                let _ = reply.send(Err(format!("no PLC link from {plc}")));
            }
        }
    }
}

/// One accepted connection: sniff, run the session and translate its events.
async fn connection(mut stream: tokio::net::TcpStream, peer: SocketAddr, codec: Codec, trace: Arc<TraceStore>, sessions: Sessions) {
    let framing = match sniff_framing(&mut stream, SNIFF_TIMEOUT).await {
        Ok((f, initial)) => {
            let (events_tx, events_rx) = mpsc::unbounded_channel();
            let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
            let params = SessionParams::new(f, peer.to_string());
            let session = LinkSession::new(codec.clone(), params, events_tx);
            let pump = tokio::spawn(session.run(stream, initial, cmd_rx));
            drive(events_rx, cmd_tx, codec, trace, sessions, peer).await;
            let _ = pump.await;
            f
        }
        Err(e) => {
            tracing::info!(peer = %peer, "PLC link: {e}");
            return;
        }
    };
    tracing::info!(peer = %peer, framing = ?framing, "PLC link closed");
}

/// Consumes the session events until it closes, keeping the PLC name registration and the pending sends.
async fn drive(mut events: mpsc::UnboundedReceiver<SessionEvent>, cmds: mpsc::UnboundedSender<SessionCommand>, codec: Codec, trace: Arc<TraceStore>, sessions: Sessions, peer: SocketAddr) {
    let trace_id = codec.spec_by_name("Trace").ok().map(|s| s.id);
    let cfg_id = codec.spec_by_name("TraceCfg").ok().map(|s| s.id);
    let ack_id = codec.spec_by_name("Ack").ok().map(|s| s.id);
    let (app_tx, mut app_rx) = mpsc::channel::<AppReq>(8);
    let mut plc: Option<String> = None;
    let mut pending: HashMap<u64, oneshot::Sender<Result<u16, String>>> = HashMap::new();
    let mut next_tag = 1u64;

    loop {
        tokio::select! {
            ev = events.recv() => {
                let Some(ev) = ev else { break };
                match ev {
                    SessionEvent::Received { msg, hello, .. } => {
                        if let Some(h) = hello {
                            let name = h.plc.clone().unwrap_or_else(|| peer.to_string());
                            if !h.registry_ok {
                                tracing::warn!(plc = %name, "PLC link: registry hash mismatch — BIN is refused, trace will not work");
                            }
                            tracing::info!(plc = %name, peer = %peer, "PLC link up");
                            sessions.lock().await.insert(name.clone(), app_tx.clone());
                            plc = Some(name);
                        }
                        let Some(name) = plc.as_deref() else { continue };
                        if Some(msg.id) == trace_id {
                            trace.on_chunk(name, &msg.payload).await;
                        } else if Some(msg.id) == ack_id
                            && let Some((ref_type, ref_seq, code)) = ack_fields(&codec, &msg)
                            && Some(ref_type) == cfg_id
                        {
                            trace.on_cfg_ack(name, ref_seq, code).await;
                        }
                    }
                    SessionEvent::Rejected { error, .. } => tracing::debug!(peer = %peer, "PLC link rejected a frame: {error}"),
                    SessionEvent::Sent { frame, tag, .. } => {
                        if let Some(tx) = tag.and_then(|t| pending.remove(&t)) {
                            let _ = tx.send(Ok(frame.seq));
                        }
                    }
                    SessionEvent::SendFailed { error, tag, .. } => {
                        if let Some(tx) = tag.and_then(|t| pending.remove(&t)) {
                            let _ = tx.send(Err(error.to_string()));
                        }
                    }
                    SessionEvent::Connected { .. } => {}
                    SessionEvent::Closed(end) => {
                        tracing::info!(peer = %peer, "PLC link ended: {}", end.reason);
                        break;
                    }
                }
            }
            req = app_rx.recv() => {
                let Some(AppReq::TraceCfg { cfg, reply }) = req else { continue };
                match codec.from_json_data("TraceCfg", &cfg, plc_link::wire_json::Strictness::Strict) {
                    Ok((msg, _)) => {
                        let tag = next_tag;
                        next_tag += 1;
                        pending.insert(tag, reply);
                        if cmds.send(SessionCommand::Send { msg, format: Some(Format::Bin), tag: Some(tag) }).is_err()
                            && let Some(tx) = pending.remove(&tag)
                        {
                            let _ = tx.send(Err("the PLC link closed".to_string()));
                        }
                    }
                    Err(e) => {
                        let _ = reply.send(Err(format!("TraceCfg: {e}")));
                    }
                }
            }
        }
    }

    for (_, tx) in pending {
        let _ = tx.send(Err("the PLC link closed".to_string()));
    }
    if let Some(name) = plc {
        // PLC 가 다시 붙으면 새 연결이 같은 이름으로 먼저 등록된다 — 옛 연결이 늦게 닫히면서 그 등록을 지우고
        // 트레이스를 down 으로 만들던 경합(2026-09-21 통신 점검). 등록이 **내 것**일 때만 지운다.
        let mine = {
            let mut s = sessions.lock().await;
            if s.get(&name).is_some_and(|tx| tx.same_channel(&app_tx)) {
                s.remove(&name);
                true
            } else {
                false
            }
        };
        if mine {
            trace.on_link_down(&name).await;
        } else {
            tracing::info!(plc = %name, peer = %peer, "PLC link: old connection closed after a newer one took over");
        }
    }
}

/// `RefType`, `RefSeq` and `Code` of an Ack message.
fn ack_fields(codec: &Codec, m: &Message) -> Option<(u16, u16, i16)> {
    let v = codec.data_value(m).ok()?;
    Some((v.get("RefType")?.as_u64()? as u16, v.get("RefSeq")?.as_u64()? as u16, v.get("Code")?.as_i64()? as i16))
}
