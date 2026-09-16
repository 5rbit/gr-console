//! Hub side of a FRAME / NDJSON session: drives a [`plc_link::io::LinkSession`] (which owns the socket and
//! keeps the wire rules) and turns its events into hub state — log entries, counters, last messages, command
//! bookkeeping — while the hub's [`Outbound`] queue becomes session commands.

use std::sync::Arc;

use plc_link::io::{LinkSession, SendError, SessionCommand, SessionEvent};
use plc_link::{Codec, Format, Framing, HelloInfo, LinkError, Message, RawFrame, Report, Role, Strictness};
use serde_json::value::RawValue;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::hub::{Hub, Mode, Outbound};
use crate::log::{Dir, LogEntry, preview_text};
use crate::wire::ref_of;

pub use plc_link::io::{PC_NAME, SessionEnd, hello_formats};

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

/// Hello data as a raw JSON value for the PLC view.
pub fn hello_data(h: &HelloInfo) -> Option<Box<RawValue>> {
    h.data.clone().and_then(|t| RawValue::from_string(t).ok())
}

/// Hub state of one session: which PLC the connection belongs to and its hub connection id.
struct Driver {
    hub: Arc<Hub>,
    p: SessionParams,
    plc: Option<String>,
    conn_id: u64,
    tx: mpsc::UnboundedSender<Outbound>,
}

impl Driver {
    fn name(&self) -> String {
        self.plc.clone().unwrap_or_else(|| self.p.peer.clone())
    }

    fn attach(&mut self, name: &str, format: Option<Format>) {
        if self.conn_id != 0 && self.plc.as_deref() == Some(name) {
            return;
        }
        if let Some(old) = self.plc.clone()
            && self.conn_id != 0
        {
            self.hub.detach(&old, self.conn_id, Some(format!("renamed to {name} by Hello")));
        }
        self.plc = Some(name.to_string());
        self.conn_id = self.hub.attach(name, self.p.mode, self.p.framing, format, &self.p.peer, Some(self.tx.clone()));
    }

    fn on_hello(&mut self, msg: &Message, framing: Framing, format: Format, wire_len: usize, report: &Report, h: &HelloInfo) {
        let name = self.p.plc.clone().or_else(|| h.plc.clone()).unwrap_or_else(|| self.p.peer.clone());
        self.attach(&name, Some(format));
        self.hub.set_format(&name, format);
        let mut entry = self.hub.msg_entry(&name, Dir::Rx, framing, format, msg, wire_len, Some(&self.p.peer));
        entry.warnings = report_warnings(report);
        if !h.registry_ok {
            entry.warnings.push(format!("registry hash mismatch: PC 0x{:08X} (contract_mismatch, BIN refused)", self.hub.codec().registry().hash()));
        }
        self.hub.log(entry, Some(msg));
        self.hub.set_hello(&name, hello_data(h), h.registry_ok);
        if !h.registry_ok {
            tracing::warn!(plc = %name, "registry hash mismatch: BIN refused, JSON accepted");
        }
    }

    fn on_event(&mut self, ev: SessionEvent, cmds: &mpsc::UnboundedSender<SessionCommand>) {
        match ev {
            SessionEvent::Connected { .. } | SessionEvent::Closed(_) => {}
            SessionEvent::Received { msg, frame, report, hello: Some(h) } => {
                self.on_hello(&msg, frame.framing, frame.format, frame.wire_len, &report, &h);
                let name = self.name();
                for (id, cmd) in self.hub.take_queued(&name) {
                    let _ = cmds.send(SessionCommand::Send { msg: cmd, format: None, tag: Some(id) });
                }
            }
            SessionEvent::Received { msg, frame, report, hello: None } => {
                if self.conn_id == 0 {
                    let peer = self.p.peer.clone();
                    self.attach(&peer, Some(frame.format));
                }
                let name = self.name();
                let mut entry = self.hub.msg_entry(&name, Dir::Rx, frame.framing, frame.format, &msg, frame.wire_len, Some(&self.p.peer));
                entry.warnings = report_warnings(&report);
                self.hub.log(entry, Some(&msg));
                self.hub.command_reply(&name, &msg);
            }
            SessionEvent::Rejected { raw, error, wire_len } => {
                let entry = error_entry(&self.hub, &self.name(), &raw, &error, wire_len, &self.p.peer);
                self.hub.log(entry, None);
            }
            SessionEvent::Sent { msg, frame, tag } => {
                let name = self.name();
                let entry = self.hub.msg_entry(&name, Dir::Tx, frame.framing, frame.format, &msg, frame.wire_len, Some(&self.p.peer));
                self.hub.log(entry, Some(&msg));
                if let Some(id) = tag {
                    self.hub.command_sent(&name, id, msg.seq);
                }
            }
            SessionEvent::SendFailed { msg, format, error, tag } => {
                let name = self.name();
                if let SendError::Encode(e) = &error {
                    let mut entry = self.hub.msg_entry(&name, Dir::Tx, self.p.framing, format, &msg, 0, Some(&self.p.peer));
                    entry.ok = false;
                    entry.error = Some(e.to_string());
                    self.hub.log(entry, None);
                }
                if let Some(id) = tag {
                    self.hub.command_failed(&name, id, error.to_string());
                }
            }
        }
    }
}

fn to_command(o: Outbound) -> SessionCommand {
    match o {
        Outbound::Send(msg) => SessionCommand::Send { msg, format: None, tag: None },
        Outbound::Command { id, msg } => SessionCommand::Send { msg, format: None, tag: Some(id) },
        Outbound::Disconnect => SessionCommand::Disconnect,
    }
}

fn session_params(hub: &Hub, p: &SessionParams) -> plc_link::io::SessionParams {
    plc_link::io::SessionParams {
        opener: p.opener,
        plc: p.plc.clone(),
        format: p.format,
        role: Role::Pc,
        strictness: Strictness::Lenient,
        heartbeat: Some(hub.heartbeat()),
        ..plc_link::io::SessionParams::new(p.framing, p.peer.clone())
    }
}

/// Runs a session until the connection ends. `initial` = bytes already read (sniffing).
pub async fn run(hub: Arc<Hub>, stream: TcpStream, initial: Vec<u8>, p: SessionParams) -> SessionEnd {
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Outbound>();
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<SessionCommand>();
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel::<SessionEvent>();
    let codec: Codec = hub.codec().clone();
    let params = session_params(&hub, &p);
    let mut d = Driver { hub: hub.clone(), p, plc: None, conn_id: 0, tx: out_tx };
    if let Some(name) = d.p.plc.clone() {
        d.attach(&name, None);
    }
    let session = LinkSession::new(codec, params, ev_tx).run(stream, initial, cmd_rx);
    tokio::pin!(session);
    let end = loop {
        tokio::select! {
            end = &mut session => break end,
            Some(ev) = ev_rx.recv() => d.on_event(ev, &cmd_tx),
            Some(o) = out_rx.recv() => {
                let _ = cmd_tx.send(to_command(o));
            }
        }
    };
    while let Ok(ev) = ev_rx.try_recv() {
        d.on_event(ev, &cmd_tx);
    }
    if let Some(name) = d.plc.clone()
        && d.conn_id != 0
    {
        hub.detach(&name, d.conn_id, Some(end.reason.clone()));
    }
    end
}
