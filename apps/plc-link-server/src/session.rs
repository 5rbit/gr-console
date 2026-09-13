//! PC side of a FRAME / NDJSON session (accepted from an active PLC, or opened to a passive PLC):
//! Hello exchange + registry hash check, Heartbeat, Ack of MeasLog / CommandResult, command delivery.

use std::sync::Arc;
use std::time::{Duration, Instant};

use plc_link::{Codec, Format, Framing, LinkError, Message, RawFrame, Report, Role, Strictness};
use serde_json::value::RawValue;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::hub::{Hub, Mode, Outbound};
use crate::log::{Dir, LogEntry, preview_text};
use crate::util::SeqGen;
use crate::wire::{StreamDecoder, encode, ref_of};

/// Hello.Plc sent by the PC.
pub const PC_NAME: &str = "PC";

#[derive(Clone, Debug)]
pub struct SessionParams {
    pub framing: Framing,
    pub mode: Mode,
    /// This side opened the connection → it sends Hello first.
    pub opener: bool,
    /// Configured PLC name (passive client); accepted connections learn it from the Hello.
    pub plc: Option<String>,
    /// Format used before the PLC's first message.
    pub format: Format,
    pub peer: String,
}

#[derive(Clone, Debug)]
pub struct SessionEnd {
    pub reason: String,
    pub hello_seen: bool,
}

/// Report warnings for the log (defaulted members summarized).
pub fn report_warnings(r: &Report) -> Vec<String> {
    let mut w = r.warnings.clone();
    if !r.defaulted.is_empty() {
        let first: Vec<&str> = r.defaulted.iter().take(5).map(String::as_str).collect();
        w.push(format!("{} members missing (zero value): {}{}", r.defaulted.len(), first.join(", "), if r.defaulted.len() > 5 { ", …" } else { "" }));
    }
    w
}

/// Log entry of a received message that failed to decode.
pub fn error_entry(hub: &Hub, plc: &str, raw: &RawFrame, e: &LinkError, wire_len: usize, peer: &str) -> LogEntry {
    let codec = hub.codec();
    let mut entry = LogEntry::new(plc, Dir::Rx);
    let (t, s) = ref_of(codec, raw);
    entry.framing = Some(raw.framing);
    entry.format = Some(raw.format);
    entry.msg_type = (t != 0).then_some(t);
    entry.name = codec.registry().by_id(t).map(|m| m.name.clone());
    entry.seq = Some(s);
    entry.sig = raw.sig;
    entry.len = wire_len;
    entry.ok = false;
    entry.error = Some(e.to_string());
    entry.peer = Some(peer.to_string());
    entry.data_text = Some(if raw.format == Format::Bin { crate::util::hex(&raw.payload) } else { preview_text(&raw.payload) });
    entry
}

/// Parses a received Hello: (data JSON, Hello.Plc, registry hash matches, HeartbeatMs).
pub fn hello_info(codec: &Codec, m: &Message) -> (Option<Box<RawValue>>, Option<String>, bool, Option<u64>) {
    let text = codec.data_json_text(m).ok();
    let v: serde_json::Value = text.as_deref().and_then(|t| serde_json::from_str(t).ok()).unwrap_or_default();
    let plc = v["Plc"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
    let ok = v["RegistryHash"].as_u64() == Some(codec.registry().hash() as u64);
    let hb = v["HeartbeatMs"].as_u64().filter(|x| *x > 0);
    (text.and_then(|t| RawValue::from_string(t).ok()), plc, ok, hb)
}

struct Session {
    hub: Arc<Hub>,
    codec: Codec,
    p: SessionParams,
    plc: Option<String>,
    conn_id: u64,
    format: Option<Format>,
    registry_ok: Option<bool>,
    seq: SeqGen,
    last_tx: Instant,
    last_rx: Instant,
    peer_hb: Option<Duration>,
    hello_seen: bool,
    tx: mpsc::UnboundedSender<Outbound>,
}

type Flow = Result<(), String>;

impl Session {
    fn name(&self) -> String {
        self.plc.clone().unwrap_or_else(|| self.p.peer.clone())
    }

    fn attach(&mut self, name: &str) {
        if self.conn_id != 0 && self.plc.as_deref() == Some(name) {
            return;
        }
        if let Some(old) = self.plc.clone()
            && self.conn_id != 0
        {
            self.hub.detach(&old, self.conn_id, Some(format!("renamed to {name} by Hello")));
        }
        self.plc = Some(name.to_string());
        self.conn_id = self.hub.attach(name, self.p.mode, self.p.framing, self.format, &self.p.peer, Some(self.tx.clone()));
    }

    fn send_format(&self) -> Format {
        if self.p.framing == Framing::Ndjson || self.registry_ok == Some(false) { Format::Json } else { self.format.unwrap_or(self.p.format) }
    }

    /// Assigns a seq and writes the message. Outer error = write failure (end the session); inner error =
    /// the message could not be encoded (logged).
    async fn send<W: AsyncWrite + Unpin>(&mut self, w: &mut W, mut m: Message, format: Option<Format>) -> Result<Result<u16, LinkError>, String> {
        m.seq = self.seq.next();
        let fmt = if self.p.framing == Framing::Ndjson { Format::Json } else { format.unwrap_or_else(|| self.send_format()) };
        let name = self.name();
        match encode(&self.codec, self.p.framing, fmt, &m) {
            Ok(bytes) => {
                w.write_all(&bytes).await.map_err(|e| format!("write: {e}"))?;
                self.last_tx = Instant::now();
                let e = self.hub.msg_entry(&name, Dir::Tx, self.p.framing, fmt, &m, bytes.len(), Some(&self.p.peer));
                self.hub.log(e, Some(&m));
                Ok(Ok(m.seq))
            }
            Err(err) => {
                let mut e = self.hub.msg_entry(&name, Dir::Tx, self.p.framing, fmt, &m, 0, Some(&self.p.peer));
                e.ok = false;
                e.error = Some(err.to_string());
                self.hub.log(e, None);
                Ok(Err(err))
            }
        }
    }

    async fn reject<W: AsyncWrite + Unpin>(&mut self, w: &mut W, raw: &RawFrame, err: &LinkError, wire_len: usize) -> Flow {
        let entry = error_entry(&self.hub, &self.name(), raw, err, wire_len, &self.p.peer);
        self.hub.log(entry, None);
        let (t, s) = ref_of(&self.codec, raw);
        if let Ok(ack) = self.codec.ack(t, s, err.code.code(), &err.msg, 0) {
            self.send(w, ack, None).await?.ok();
        }
        Ok(())
    }

    async fn on_frame<W: AsyncWrite + Unpin>(&mut self, w: &mut W, raw: RawFrame) -> Flow {
        self.last_rx = Instant::now();
        let wire_len = raw.payload.len() + if self.p.framing == Framing::Frame { plc_link::header::HEADER_LEN } else { 1 };
        let hello_id = self.codec.registry().by_name("Hello").map(|m| m.id);
        if raw.format == Format::Bin && self.registry_ok == Some(false) && raw.msg_type != hello_id {
            let e = LinkError::new(plc_link::ErrCode::FormatNotAllowed, "registry hash mismatch (contract_mismatch): BIN refused, send JSON");
            return self.reject(w, &raw, &e, wire_len).await;
        }
        let (m, report) = match self.codec.decode_raw_report(&raw, Role::Pc, Strictness::Lenient) {
            Ok(x) => x,
            Err(e) => return self.reject(w, &raw, &e, wire_len).await,
        };
        if self.format.is_none() {
            self.format = Some(raw.format);
        }
        let Ok(spec) = self.codec.spec(m.id).cloned() else { return Ok(()) };
        if Some(m.id) == hello_id {
            let (data, hello_plc, ok, hb) = hello_info(&self.codec, &m);
            let name = self.p.plc.clone().or(hello_plc).unwrap_or_else(|| self.p.peer.clone());
            self.attach(&name);
            self.hub.set_format(&name, raw.format);
            let mut entry = self.hub.msg_entry(&name, Dir::Rx, raw.framing, raw.format, &m, wire_len, Some(&self.p.peer));
            entry.warnings = report_warnings(&report);
            if !ok {
                entry.warnings.push(format!("registry hash mismatch: PC 0x{:08X} (contract_mismatch, BIN refused)", self.codec.registry().hash()));
            }
            self.hub.log(entry, Some(&m));
            self.registry_ok = Some(ok);
            self.hub.set_hello(&name, data, ok);
            self.peer_hb = hb.map(Duration::from_millis);
            if !ok {
                tracing::warn!(plc = %name, "registry hash mismatch: BIN refused, JSON accepted");
            }
            if !self.p.opener
                && let Ok(h) = self.codec.hello(PC_NAME, hello_formats(self.p.framing), 0)
            {
                self.send(w, h, Some(raw.format)).await?.ok();
            }
            self.hello_seen = true;
            for (id, cmd) in self.hub.take_queued(&name) {
                match self.send(w, cmd, None).await? {
                    Ok(seq) => self.hub.command_sent(&name, id, seq),
                    Err(e) => self.hub.command_failed(&name, id, e.to_string()),
                }
            }
            return Ok(());
        }
        if self.conn_id == 0 {
            let peer = self.p.peer.clone();
            self.attach(&peer);
        }
        let name = self.name();
        let mut entry = self.hub.msg_entry(&name, Dir::Rx, raw.framing, raw.format, &m, wire_len, Some(&self.p.peer));
        entry.warnings = report_warnings(&report);
        self.hub.log(entry, Some(&m));
        if spec.ack
            && let Ok(ack) = self.codec.ack(m.id, m.seq, 0, "", 0)
        {
            self.send(w, ack, None).await?.ok();
        }
        self.hub.command_reply(&name, &m);
        Ok(())
    }

    async fn on_outbound<W: AsyncWrite + Unpin>(&mut self, w: &mut W, o: Outbound) -> Flow {
        let name = self.name();
        match o {
            Outbound::Send(m) => {
                self.send(w, m, None).await?.ok();
                Ok(())
            }
            Outbound::Command { id, msg } => match self.send(w, msg, None).await {
                Ok(Ok(seq)) => {
                    self.hub.command_sent(&name, id, seq);
                    Ok(())
                }
                Ok(Err(e)) => {
                    self.hub.command_failed(&name, id, e.to_string());
                    Ok(())
                }
                Err(e) => {
                    self.hub.command_failed(&name, id, e.clone());
                    Err(e)
                }
            },
            Outbound::Disconnect => Err("disconnect requested".into()),
        }
    }

    async fn on_tick<W: AsyncWrite + Unpin>(&mut self, w: &mut W) -> Flow {
        let hb = self.hub.heartbeat();
        let rx_limit = self.peer_hb.unwrap_or(hb) * 3;
        if self.last_rx.elapsed() >= rx_limit {
            return Err(format!("nothing received for {} ms (3 × heartbeat)", rx_limit.as_millis()));
        }
        if self.last_tx.elapsed() >= hb
            && let Ok(m) = self.codec.heartbeat(0)
        {
            self.send(w, m, None).await?.ok();
        }
        Ok(())
    }
}

/// Hello.Formats the PC offers on a framing (NDJSON: JSON only).
pub fn hello_formats(framing: Framing) -> u8 {
    match framing {
        Framing::Ndjson => Format::Json.bit(),
        _ => Format::Json.bit() | Format::Bin.bit(),
    }
}

/// Runs a session until the connection ends. `initial` = bytes already read (sniffing).
pub async fn run(hub: Arc<Hub>, stream: TcpStream, initial: Vec<u8>, p: SessionParams) -> SessionEnd {
    let _ = stream.set_nodelay(true);
    let (mut rd, mut wr) = stream.into_split();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let codec = hub.codec().clone();
    let mut dec = match StreamDecoder::pc(p.framing) {
        Ok(d) => d,
        Err(e) => return SessionEnd { reason: e.to_string(), hello_seen: false },
    };
    let mut s = Session {
        hub: hub.clone(),
        codec,
        plc: None,
        conn_id: 0,
        format: None,
        registry_ok: None,
        seq: SeqGen::default(),
        last_tx: Instant::now(),
        last_rx: Instant::now(),
        peer_hb: None,
        hello_seen: false,
        tx,
        p,
    };
    if let Some(name) = s.p.plc.clone() {
        s.attach(&name);
    }
    let result: Flow = async {
        if s.p.opener
            && let Ok(h) = s.codec.hello(PC_NAME, hello_formats(s.p.framing), 0)
        {
            let fmt = s.p.format;
            s.send(&mut wr, h, Some(fmt)).await?.ok();
        }
        dec.feed(&initial);
        let mut buf = vec![0u8; 16 * 1024];
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        loop {
            loop {
                match dec.next() {
                    Ok(Some(raw)) => s.on_frame(&mut wr, raw).await?,
                    Ok(None) => break,
                    Err(e) => {
                        let raw = RawFrame { framing: s.p.framing, format: Format::Json, msg_type: None, seq: None, sig: None, payload: Vec::new(), http: None };
                        let entry = error_entry(&s.hub, &s.name(), &raw, &e, 0, &s.p.peer);
                        s.hub.log(entry, None);
                        if let Ok(ack) = s.codec.ack(0, 0, e.code.code(), &e.msg, 0) {
                            let _ = s.send(&mut wr, ack, None).await;
                        }
                        let _ = wr.flush().await;
                        return Err(format!("framing error: {e}"));
                    }
                }
            }
            tokio::select! {
                r = rd.read(&mut buf) => match r {
                    Ok(0) => return Err("closed by peer".into()),
                    Ok(n) => dec.feed(&buf[..n]),
                    Err(e) => return Err(format!("read: {e}")),
                },
                Some(o) = rx.recv() => s.on_outbound(&mut wr, o).await?,
                _ = tick.tick() => s.on_tick(&mut wr).await?,
            }
        }
    }
    .await;
    let reason = result.err().unwrap_or_default();
    if let Some(name) = s.plc.clone()
        && s.conn_id != 0
    {
        hub.detach(&name, s.conn_id, Some(reason.clone()));
    }
    let _ = wr.shutdown().await;
    SessionEnd { reason, hello_seen: s.hello_seen }
}
