//! Shared server state: PLC links, last messages, command queue / in-flight / history, message log ring and
//! the event channel feeding SSE.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use plc_link::{Codec, Format, Framing, Message};
use serde::Serialize;
use serde_json::value::RawValue;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::log::{Dir, LogEntry};
use crate::util::{SeqGen, hex, now_ts};

#[derive(Clone, Debug)]
pub struct HubConfig {
    /// Log ring size.
    pub log_cap: usize,
    /// Queued / sent commands expire after this.
    pub cmd_ttl: Duration,
    /// Finished commands kept per PLC.
    pub history_cap: usize,
    /// Reconnect backoff of passive PLC clients (doubles up to `reconnect_max`).
    pub reconnect_min: Duration,
    pub reconnect_max: Duration,
    /// JSONL log directory (`None` = no file).
    pub log_dir: Option<PathBuf>,
}

impl Default for HubConfig {
    fn default() -> Self {
        HubConfig { log_cap: 5000, cmd_ttl: Duration::from_secs(60), history_cap: 200, reconnect_min: Duration::from_secs(1), reconnect_max: Duration::from_secs(10), log_dir: None }
    }
}

/// PLC socket role as seen from the PLC: Active = the PLC connects to this server, Passive = we connect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Active,
    Passive,
}

/// Instruction to the task owning a PLC connection.
#[derive(Debug)]
pub enum Outbound {
    /// Send a message (the session assigns the seq).
    Send(Message),
    /// Send a queued command; the session assigns the seq and reports it with [`Hub::command_sent`].
    Command {
        id: u64,
        msg: Message,
    },
    Disconnect,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Counters {
    pub rx_msgs: u64,
    pub tx_msgs: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    pub acks_rx: u64,
    pub acks_tx: u64,
    pub heartbeats_rx: u64,
    pub heartbeats_tx: u64,
    pub commands_sent: u64,
    pub results_rx: u64,
    pub connects: u64,
    pub disconnects: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CmdState {
    Queued,
    Sent,
    Done,
    Rejected,
    Expired,
    Cancelled,
    Failed,
}

impl CmdState {
    pub fn is_terminal(self) -> bool {
        !matches!(self, CmdState::Queued | CmdState::Sent)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CmdView {
    pub id: u64,
    pub plc: String,
    /// Command seq (0 until sent).
    pub seq: u16,
    pub state: CmdState,
    pub created: String,
    pub sent: Option<String>,
    pub finished: Option<String>,
    /// Milliseconds from submission to the final state.
    pub elapsed_ms: Option<u64>,
    /// `CommandResult` or `Ack`.
    pub result_type: Option<String>,
    pub result: Option<Box<RawValue>>,
    pub result_hex: Option<String>,
    /// Ack code of a rejection.
    pub code: Option<i16>,
    /// HTTP status of a passive HTTP PLC's `POST /api/command` response (200, 400, 503, …).
    pub http_status: Option<u16>,
    pub error: Option<String>,
}

#[derive(Debug)]
struct Cmd {
    view: CmdView,
    msg: Message,
    created_at: Instant,
}

#[derive(Clone, Debug)]
pub struct LastMsg {
    pub msg: Message,
    pub ts: String,
    pub log_id: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct LastInfo {
    pub seq: u16,
    pub ts: String,
    pub log_id: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlcView {
    pub name: String,
    pub mode: Mode,
    pub framing: Option<Framing>,
    pub format: Option<Format>,
    pub peer: Option<String>,
    /// Configured address of a passive PLC.
    pub target: Option<String>,
    pub connected: bool,
    pub since: Option<String>,
    pub last_rx: Option<String>,
    /// `None` until a Hello was received; `false` = registry hash mismatch (contract_mismatch).
    pub registry_ok: Option<bool>,
    pub contract_mismatch: bool,
    pub hello: Option<Box<RawValue>>,
    pub last_error: Option<String>,
    pub counters: Counters,
    /// Last received message per type name.
    pub last: BTreeMap<String, LastInfo>,
    pub queued: usize,
    pub inflight: usize,
    pub conn_id: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CommandsView {
    pub queue: Vec<CmdView>,
    pub inflight: Vec<CmdView>,
    pub history: Vec<CmdView>,
}

#[derive(Clone, Debug)]
pub enum HubEvent {
    Msg(Arc<LogEntry>),
    Plc(Box<PlcView>),
    Cmd(Box<CmdView>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HubError {
    NotFound,
    NotConnected,
    Unsupported(String),
}

impl std::fmt::Display for HubError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HubError::NotFound => write!(f, "unknown PLC"),
            HubError::NotConnected => write!(f, "PLC is not connected"),
            HubError::Unsupported(s) => write!(f, "{s}"),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct LogQuery {
    pub plc: Option<String>,
    /// Message name (case-insensitive) or id.
    pub msg: Option<String>,
    pub dir: Option<Dir>,
    pub since_id: Option<u64>,
    pub limit: usize,
}

#[derive(Debug)]
struct PlcState {
    name: String,
    mode: Mode,
    framing: Option<Framing>,
    format: Option<Format>,
    peer: Option<String>,
    target: Option<String>,
    connected: bool,
    since: Option<String>,
    last_rx: Option<String>,
    last_rx_at: Option<Instant>,
    registry_ok: Option<bool>,
    hello: Option<Box<RawValue>>,
    last_error: Option<String>,
    counters: Counters,
    last: BTreeMap<u16, LastMsg>,
    cmds: VecDeque<Cmd>,
    history: VecDeque<CmdView>,
    conn_id: u64,
    tx: Option<mpsc::UnboundedSender<Outbound>>,
    seq: SeqGen,
}

impl PlcState {
    fn new(name: &str, mode: Mode) -> Self {
        PlcState {
            name: name.to_string(),
            mode,
            framing: None,
            format: None,
            peer: None,
            target: None,
            connected: false,
            since: None,
            last_rx: None,
            last_rx_at: None,
            registry_ok: None,
            hello: None,
            last_error: None,
            counters: Counters::default(),
            last: BTreeMap::new(),
            cmds: VecDeque::new(),
            history: VecDeque::new(),
            conn_id: 0,
            tx: None,
            seq: SeqGen::default(),
        }
    }

    fn view(&self, codec: &Codec) -> PlcView {
        PlcView {
            name: self.name.clone(),
            mode: self.mode,
            framing: self.framing,
            format: self.format,
            peer: self.peer.clone(),
            target: self.target.clone(),
            connected: self.connected,
            since: self.since.clone(),
            last_rx: self.last_rx.clone(),
            registry_ok: self.registry_ok,
            contract_mismatch: self.registry_ok == Some(false),
            hello: self.hello.clone(),
            last_error: self.last_error.clone(),
            counters: self.counters.clone(),
            last: self
                .last
                .iter()
                .map(|(id, l)| (codec.registry().by_id(*id).map(|s| s.name.clone()).unwrap_or_else(|| id.to_string()), LastInfo { seq: l.msg.seq, ts: l.ts.clone(), log_id: l.log_id }))
                .collect(),
            queued: self.cmds.iter().filter(|c| c.view.state == CmdState::Queued).count(),
            inflight: self.cmds.iter().filter(|c| c.view.state == CmdState::Sent).count(),
            conn_id: self.conn_id,
        }
    }

    /// Removes command `idx`, sets its final state and records it in the history.
    fn finish(&mut self, idx: usize, state: CmdState, cap: usize) -> Option<CmdView> {
        let mut c = self.cmds.remove(idx)?;
        c.view.state = state;
        c.view.finished = Some(now_ts());
        c.view.elapsed_ms = Some(c.created_at.elapsed().as_millis() as u64);
        self.history.push_back(c.view.clone());
        while self.history.len() > cap {
            self.history.pop_front();
        }
        Some(c.view)
    }
}

#[derive(Debug, Default)]
struct Inner {
    plcs: BTreeMap<String, PlcState>,
    log: VecDeque<Arc<LogEntry>>,
    next_log_id: u64,
    next_cmd_id: u64,
    next_conn_id: u64,
    waiters: HashMap<u64, Vec<oneshot::Sender<CmdView>>>,
}

pub struct Hub {
    codec: Codec,
    pub contract_name: String,
    pub cfg: HubConfig,
    pub started: String,
    started_at: Instant,
    inner: Mutex<Inner>,
    events: broadcast::Sender<HubEvent>,
    file: Mutex<Option<std::sync::mpsc::Sender<Arc<LogEntry>>>>,
    command_id: u16,
    ack_id: u16,
    result_id: u16,
}

fn raw_json(text: String) -> Option<Box<RawValue>> {
    RawValue::from_string(text).ok()
}

impl Hub {
    pub fn new(codec: Codec, contract_name: &str, cfg: HubConfig) -> Arc<Hub> {
        let file = cfg.log_dir.as_ref().and_then(|d| match crate::log::spawn_file_writer(d.clone()) {
            Ok(tx) => Some(tx),
            Err(e) => {
                tracing::warn!(dir = %d.display(), %e, "log file writer disabled");
                None
            }
        });
        let id = |n: &str| codec.registry().by_name(n).map(|s| s.id).unwrap_or(0);
        let (command_id, ack_id, result_id) = (id("Command"), id("Ack"), id("CommandResult"));
        let (events, _) = broadcast::channel(4096);
        Arc::new(Hub {
            codec,
            contract_name: contract_name.to_string(),
            cfg,
            started: now_ts(),
            started_at: Instant::now(),
            inner: Mutex::new(Inner::default()),
            events,
            file: Mutex::new(file),
            command_id,
            ack_id,
            result_id,
        })
    }

    pub fn codec(&self) -> &Codec {
        &self.codec
    }

    pub fn heartbeat(&self) -> Duration {
        Duration::from_millis(self.codec.registry().heartbeat_ms() as u64)
    }

    pub fn uptime(&self) -> Duration {
        self.started_at.elapsed()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<HubEvent> {
        self.events.subscribe()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn emit(&self, e: HubEvent) {
        let _ = self.events.send(e);
    }

    fn emit_plc(&self, name: &str) {
        if let Some(v) = self.plc(name) {
            self.emit(HubEvent::Plc(Box::new(v)));
        }
    }

    fn deliver(&self, finished: Vec<CmdView>) {
        if finished.is_empty() {
            return;
        }
        let mut senders = Vec::new();
        {
            let mut g = self.lock();
            for v in &finished {
                if let Some(ws) = g.waiters.remove(&v.id) {
                    senders.push((v.clone(), ws));
                }
            }
        }
        for (v, ws) in senders {
            for w in ws {
                let _ = w.send(v.clone());
            }
        }
        for v in finished {
            self.emit(HubEvent::Cmd(Box::new(v)));
        }
    }

    // ---------------------------------------------------------------------------------------------------
    // links

    /// Creates the PLC entry if missing (passive PLCs from `--connect`).
    pub fn ensure_plc(&self, name: &str, mode: Mode, framing: Option<Framing>, format: Option<Format>, target: Option<String>) {
        {
            let mut g = self.lock();
            let st = g.plcs.entry(name.to_string()).or_insert_with(|| PlcState::new(name, mode));
            st.mode = mode;
            st.framing = framing.or(st.framing);
            st.format = format.or(st.format);
            st.target = target.or(st.target.take());
        }
        self.emit_plc(name);
    }

    /// Marks a new connection; a previous connection of the same PLC is told to disconnect. Returns the
    /// connection id for [`Hub::detach`].
    pub fn attach(&self, name: &str, mode: Mode, framing: Framing, format: Option<Format>, peer: &str, tx: Option<mpsc::UnboundedSender<Outbound>>) -> u64 {
        let old = {
            let mut g = self.lock();
            g.next_conn_id += 1;
            let conn_id = g.next_conn_id;
            let st = g.plcs.entry(name.to_string()).or_insert_with(|| PlcState::new(name, mode));
            let old = st.tx.take();
            st.mode = mode;
            st.framing = Some(framing);
            if format.is_some() {
                st.format = format;
            }
            st.peer = Some(peer.to_string());
            st.connected = true;
            st.since = Some(now_ts());
            st.registry_ok = None;
            st.last_error = None;
            st.counters.connects += 1;
            st.conn_id = conn_id;
            st.tx = tx;
            (old, conn_id)
        };
        if let Some(tx) = old.0 {
            let _ = tx.send(Outbound::Disconnect);
        }
        self.emit_plc(name);
        old.1
    }

    /// Connection `conn_id` ended; commands sent over it fail.
    pub fn detach(&self, name: &str, conn_id: u64, reason: Option<String>) {
        let finished = {
            let mut g = self.lock();
            let cap = self.cfg.history_cap;
            let Some(st) = g.plcs.get_mut(name) else { return };
            if st.conn_id != conn_id || !st.connected {
                return;
            }
            st.connected = false;
            st.tx = None;
            st.counters.disconnects += 1;
            st.last_error = reason.clone();
            let mut finished = Vec::new();
            while let Some(idx) = st.cmds.iter().position(|c| c.view.state == CmdState::Sent) {
                st.cmds[idx].view.error = Some(format!("link closed{}", reason.as_ref().map(|r| format!(": {r}")).unwrap_or_default()));
                finished.extend(st.finish(idx, CmdState::Failed, cap));
            }
            finished
        };
        self.emit_plc(name);
        self.deliver(finished);
    }

    pub fn connect_failed(&self, name: &str, err: String) {
        {
            let mut g = self.lock();
            if let Some(st) = g.plcs.get_mut(name) {
                st.last_error = Some(err);
            }
        }
        self.emit_plc(name);
    }

    pub fn set_format(&self, name: &str, format: Format) {
        let mut g = self.lock();
        if let Some(st) = g.plcs.get_mut(name) {
            st.format = Some(format);
        }
    }

    pub fn set_hello(&self, name: &str, hello: Option<Box<RawValue>>, registry_ok: bool) {
        {
            let mut g = self.lock();
            if let Some(st) = g.plcs.get_mut(name) {
                st.hello = hello;
                st.registry_ok = Some(registry_ok);
            }
        }
        self.emit_plc(name);
    }

    pub fn registry_ok(&self, name: &str) -> Option<bool> {
        self.lock().plcs.get(name).and_then(|s| s.registry_ok)
    }

    /// A request of an HTTP-active PLC: creates the entry / marks it connected.
    pub fn touch_http(&self, name: &str, peer: &str, format: Format) {
        let changed = {
            let mut g = self.lock();
            g.next_conn_id += 1;
            let conn_id = g.next_conn_id;
            let st = g.plcs.entry(name.to_string()).or_insert_with(|| PlcState::new(name, Mode::Active));
            st.peer = Some(peer.to_string());
            st.format = Some(format);
            st.last_rx_at = Some(Instant::now());
            if st.connected && st.framing == Some(Framing::Http) && st.mode == Mode::Active {
                false
            } else {
                if let Some(tx) = st.tx.take() {
                    let _ = tx.send(Outbound::Disconnect);
                }
                st.mode = Mode::Active;
                st.framing = Some(Framing::Http);
                st.connected = true;
                st.since = Some(now_ts());
                st.last_error = None;
                st.counters.connects += 1;
                st.conn_id = conn_id;
                true
            }
        };
        if changed {
            self.emit_plc(name);
        }
    }

    /// Next PC sequence number of this PLC (HTTP-active replies).
    pub fn next_seq(&self, name: &str) -> u16 {
        let mut g = self.lock();
        match g.plcs.get_mut(name) {
            Some(st) => st.seq.next(),
            None => 1,
        }
    }

    pub fn disconnect(&self, name: &str) -> Result<(), HubError> {
        let g = self.lock();
        let st = g.plcs.get(name).ok_or(HubError::NotFound)?;
        match (&st.tx, st.connected) {
            (Some(tx), true) => tx.send(Outbound::Disconnect).map_err(|_| HubError::NotConnected),
            _ => Err(HubError::NotConnected),
        }
    }

    /// Sends a message over the PLC's live connection.
    pub fn send(&self, name: &str, msg: Message) -> Result<(), HubError> {
        let g = self.lock();
        let st = g.plcs.get(name).ok_or(HubError::NotFound)?;
        if st.mode == Mode::Active && st.framing == Some(Framing::Http) {
            return Err(HubError::Unsupported("an HTTP-active PLC only receives commands (GET /api/plc/{plc}/command)".into()));
        }
        match (&st.tx, st.connected) {
            (Some(tx), true) => tx.send(Outbound::Send(msg)).map_err(|_| HubError::NotConnected),
            _ => Err(HubError::NotConnected),
        }
    }

    // ---------------------------------------------------------------------------------------------------
    // log

    /// Records a log entry (assigns id and time stamp), updates counters and the last message of an OK rx.
    pub fn log(&self, mut e: LogEntry, msg: Option<&Message>) -> Arc<LogEntry> {
        let entry = {
            let mut g = self.lock();
            g.next_log_id += 1;
            e.id = g.next_log_id;
            e.ts = now_ts();
            let (ack, result, command) = (self.ack_id, self.result_id, self.command_id);
            let heartbeat = self.codec.registry().by_name("Heartbeat").map(|s| s.id).unwrap_or(0);
            if let Some(st) = g.plcs.get_mut(&e.plc) {
                let c = &mut st.counters;
                let t = e.msg_type.unwrap_or(0);
                match (e.dir, e.ok) {
                    (Dir::Rx, true) => {
                        c.rx_msgs += 1;
                        c.acks_rx += u64::from(t == ack);
                        c.results_rx += u64::from(t == result);
                        c.heartbeats_rx += u64::from(t == heartbeat);
                    }
                    (Dir::Rx, false) => c.rx_errors += 1,
                    (Dir::Tx, true) => {
                        c.tx_msgs += 1;
                        c.acks_tx += u64::from(t == ack);
                        c.commands_sent += u64::from(t == command);
                        c.heartbeats_tx += u64::from(t == heartbeat);
                    }
                    (Dir::Tx, false) => c.tx_errors += 1,
                }
                if e.dir == Dir::Rx {
                    st.last_rx = Some(e.ts.clone());
                    st.last_rx_at = Some(Instant::now());
                    if e.ok
                        && let Some(m) = msg
                    {
                        st.last.insert(m.id, LastMsg { msg: m.clone(), ts: e.ts.clone(), log_id: e.id });
                    }
                }
            }
            let entry = Arc::new(e);
            g.log.push_back(entry.clone());
            while g.log.len() > self.cfg.log_cap.max(1) {
                g.log.pop_front();
            }
            entry
        };
        if let Some(tx) = self.file.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            let _ = tx.send(entry.clone());
        }
        self.emit(HubEvent::Msg(entry.clone()));
        entry
    }

    /// Log entry for a message (name, sig, data text and hex filled from the codec).
    #[allow(clippy::too_many_arguments)]
    pub fn msg_entry(&self, plc: &str, dir: Dir, framing: Framing, format: Format, m: &Message, wire_len: usize, peer: Option<&str>) -> LogEntry {
        let mut e = LogEntry::new(plc, dir);
        e.framing = Some(framing);
        e.format = Some(format);
        e.msg_type = Some(m.id);
        e.seq = Some(m.seq);
        e.len = wire_len;
        e.peer = peer.map(str::to_string);
        if let Some(spec) = self.codec.registry().by_id(m.id) {
            e.name = Some(spec.name.clone());
            e.sig = Some(spec.sig);
        }
        match self.codec.data_json_text(m) {
            Ok(t) => {
                e.data = raw_json(t.clone());
                e.data_text = Some(t);
            }
            Err(err) => {
                e.ok = false;
                e.error = Some(err.to_string());
            }
        }
        if !m.payload.is_empty() {
            e.hex = Some(hex(&m.payload));
        }
        e
    }

    pub fn log_query(&self, q: &LogQuery) -> Vec<Arc<LogEntry>> {
        let g = self.lock();
        let msg = q.msg.as_ref().map(|m| m.trim().to_string());
        let msg_id = msg.as_ref().and_then(|m| self.codec.registry().lookup(m).map(|s| s.id).or_else(|| m.parse().ok()));
        let limit = if q.limit == 0 { 200 } else { q.limit };
        let mut out: Vec<Arc<LogEntry>> = g
            .log
            .iter()
            .rev()
            .filter(|e| q.since_id.is_none_or(|s| e.id > s))
            .filter(|e| q.plc.as_ref().is_none_or(|p| e.plc == *p))
            .filter(|e| q.dir.is_none_or(|d| e.dir == d))
            .filter(|e| match (&msg, msg_id) {
                (None, _) => true,
                (Some(_), Some(id)) => e.msg_type == Some(id),
                (Some(m), None) => e.name.as_ref().is_some_and(|n| n.eq_ignore_ascii_case(m)),
            })
            .take(limit)
            .cloned()
            .collect();
        out.reverse();
        out
    }

    pub fn log_len(&self) -> usize {
        self.lock().log.len()
    }

    // ---------------------------------------------------------------------------------------------------
    // views

    pub fn plcs(&self) -> Vec<PlcView> {
        let g = self.lock();
        g.plcs.values().map(|s| s.view(&self.codec)).collect()
    }

    pub fn plc(&self, name: &str) -> Option<PlcView> {
        self.lock().plcs.get(name).map(|s| s.view(&self.codec))
    }

    pub fn last(&self, name: &str, id: u16) -> Option<LastMsg> {
        self.lock().plcs.get(name).and_then(|s| s.last.get(&id).cloned())
    }

    // ---------------------------------------------------------------------------------------------------
    // commands

    /// Submits a Command. Live FRAME/NDJSON/passive-HTTP links send it at once; HTTP-active PLCs always queue
    /// (the PLC polls); a disconnected PLC queues only with `queue` (else `NotConnected`; unknown → `NotFound`).
    pub fn submit(&self, name: &str, msg: Message, queue: bool, wait: bool) -> Result<(CmdView, Option<oneshot::Receiver<CmdView>>), HubError> {
        let (view, rx) = {
            let mut g = self.lock();
            if !g.plcs.contains_key(name) {
                if !queue {
                    return Err(HubError::NotFound);
                }
                g.plcs.insert(name.to_string(), PlcState::new(name, Mode::Active));
            }
            g.next_cmd_id += 1;
            let id = g.next_cmd_id;
            let rx = wait.then(|| {
                let (tx, rx) = oneshot::channel();
                g.waiters.entry(id).or_default().push(tx);
                rx
            });
            let Some(st) = g.plcs.get_mut(name) else { return Err(HubError::NotFound) };
            let http_active = st.mode == Mode::Active && st.framing == Some(Framing::Http);
            let live = st.connected && st.tx.is_some() && !http_active;
            if !live && !http_active && !queue {
                g.waiters.remove(&id);
                return Err(HubError::NotConnected);
            }
            let mut view = CmdView {
                id,
                plc: name.to_string(),
                seq: 0,
                state: CmdState::Queued,
                created: now_ts(),
                sent: None,
                finished: None,
                elapsed_ms: None,
                result_type: None,
                result: None,
                result_hex: None,
                code: None,
                http_status: None,
                error: None,
            };
            if live {
                let sent = st.tx.as_ref().is_some_and(|tx| tx.send(Outbound::Command { id, msg: msg.clone() }).is_ok());
                if sent {
                    view.state = CmdState::Sent;
                    view.sent = Some(now_ts());
                }
            }
            st.cmds.push_back(Cmd { view: view.clone(), msg, created_at: Instant::now() });
            (view, rx)
        };
        self.emit(HubEvent::Cmd(Box::new(view.clone())));
        self.emit_plc(name);
        Ok((view, rx))
    }

    /// All queued commands (now handed to the connection, state `sent`, seq assigned via `command_sent`).
    pub fn take_queued(&self, name: &str) -> Vec<(u64, Message)> {
        let mut g = self.lock();
        let Some(st) = g.plcs.get_mut(name) else { return Vec::new() };
        let mut out = Vec::new();
        for c in st.cmds.iter_mut().filter(|c| c.view.state == CmdState::Queued) {
            c.view.state = CmdState::Sent;
            c.view.sent = Some(now_ts());
            out.push((c.view.id, c.msg.clone()));
        }
        out
    }

    pub fn command_sent(&self, name: &str, id: u64, seq: u16) {
        let view = {
            let mut g = self.lock();
            let Some(st) = g.plcs.get_mut(name) else { return };
            let Some(c) = st.cmds.iter_mut().find(|c| c.view.id == id) else { return };
            c.view.seq = seq;
            c.view.state = CmdState::Sent;
            c.view.sent = Some(now_ts());
            c.view.clone()
        };
        self.emit(HubEvent::Cmd(Box::new(view)));
    }

    /// Sending a command failed (write error).
    pub fn command_failed(&self, name: &str, id: u64, err: String) {
        self.command_failed_http(name, id, err, None);
    }

    /// A command failed; `http_status` of the response, if any.
    pub fn command_failed_http(&self, name: &str, id: u64, err: String, http_status: Option<u16>) {
        let finished = {
            let mut g = self.lock();
            let cap = self.cfg.history_cap;
            let Some(st) = g.plcs.get_mut(name) else { return };
            let Some(idx) = st.cmds.iter().position(|c| c.view.id == id) else { return };
            st.cmds[idx].view.error = Some(err);
            st.cmds[idx].view.http_status = http_status;
            st.finish(idx, CmdState::Failed, cap).into_iter().collect()
        };
        self.deliver(finished);
    }

    /// HTTP-active poll: oldest queued command with a fresh seq, now `sent`.
    pub fn pop_http_command(&self, name: &str) -> Option<Message> {
        let view = {
            let mut g = self.lock();
            let st = g.plcs.get_mut(name)?;
            let seq = st.seq.next();
            let c = st.cmds.iter_mut().find(|c| c.view.state == CmdState::Queued)?;
            c.view.state = CmdState::Sent;
            c.view.seq = seq;
            c.view.sent = Some(now_ts());
            c.msg.seq = seq;
            (c.view.clone(), c.msg.clone())
        };
        self.emit(HubEvent::Cmd(Box::new(view.0)));
        Some(view.1)
    }

    /// Matches a received CommandResult (by seq) or an Ack of a Command (by RefSeq) to a sent command.
    pub fn command_reply(&self, name: &str, m: &Message) -> Option<CmdView> {
        self.reply_inner(name, m, None, None)
    }

    /// The response of a passive HTTP PLC to `POST /api/command` for command `id` (one request, one response:
    /// matched by id). 200 CommandResult → done; 200 Ack(rc), 400 Ack(read error), 503 Ack(BUSY) → rejected.
    pub fn command_response(&self, name: &str, id: u64, m: &Message, http_status: u16) -> Option<CmdView> {
        self.reply_inner(name, m, Some(id), Some(http_status))
    }

    /// Registry name of an error / Ack code (`BUSY`, `UNSUPPORTED`, …).
    pub fn error_name(&self, code: i16) -> Option<String> {
        self.codec.registry().doc.errors.iter().find(|e| e.code == code).map(|e| e.name.clone())
    }

    fn reply_inner(&self, name: &str, m: &Message, id: Option<u64>, http_status: Option<u16>) -> Option<CmdView> {
        let http_ok = http_status.is_none_or(|s| s == 200);
        let (seq, state, code, text, result_type) = if m.id == self.result_id && m.id != 0 {
            (m.seq, if http_ok { CmdState::Done } else { CmdState::Failed }, None, None, "CommandResult")
        } else if m.id == self.ack_id && m.id != 0 {
            let v = self.codec.data_value(m).ok()?;
            if id.is_none() && v["RefType"].as_u64() != Some(self.command_id as u64) {
                return None;
            }
            let code = v["Code"].as_i64().unwrap_or(0) as i16;
            let state = if code == 0 && http_ok { CmdState::Done } else { CmdState::Rejected };
            (v["RefSeq"].as_u64().unwrap_or(0) as u16, state, Some(code), v["Text"].as_str().map(str::to_string), "Ack")
        } else {
            return None;
        };
        if id.is_none() && seq == 0 {
            return None;
        }
        let view = {
            let mut g = self.lock();
            let cap = self.cfg.history_cap;
            let st = g.plcs.get_mut(name)?;
            let idx = st.cmds.iter().position(|c| c.view.state == CmdState::Sent && id.map_or(c.view.seq == seq, |id| c.view.id == id))?;
            let c = &mut st.cmds[idx];
            c.view.result_type = Some(result_type.to_string());
            c.view.result = self.codec.data_json_text(m).ok().and_then(raw_json);
            c.view.result_hex = Some(hex(&m.payload));
            c.view.code = code;
            c.view.http_status = http_status;
            if state != CmdState::Done {
                let mut e = match code {
                    Some(k) => format!("rejected: {} ({k})", self.error_name(k).unwrap_or_else(|| "code".into())),
                    None => "failed".to_string(),
                };
                if let Some(t) = text.filter(|t| !t.is_empty()) {
                    e.push_str(&format!(": {t}"));
                }
                if !http_ok {
                    e.push_str(&format!(" [HTTP {}]", http_status.unwrap_or(0)));
                }
                c.view.error = Some(e);
            }
            st.finish(idx, state, cap)?
        };
        self.deliver(vec![view.clone()]);
        self.emit_plc(name);
        Some(view)
    }

    pub fn cancel(&self, name: &str, id: u64) -> Option<CmdView> {
        let view = {
            let mut g = self.lock();
            let cap = self.cfg.history_cap;
            let st = g.plcs.get_mut(name)?;
            let idx = st.cmds.iter().position(|c| c.view.id == id)?;
            st.cmds[idx].view.error = Some("cancelled via API".into());
            st.finish(idx, CmdState::Cancelled, cap)?
        };
        self.deliver(vec![view.clone()]);
        Some(view)
    }

    pub fn commands(&self, name: &str) -> Option<CommandsView> {
        let g = self.lock();
        let st = g.plcs.get(name)?;
        let pick = |s: CmdState| st.cmds.iter().filter(|c| c.view.state == s).map(|c| c.view.clone()).collect::<Vec<_>>();
        Some(CommandsView { queue: pick(CmdState::Queued), inflight: pick(CmdState::Sent), history: st.history.iter().rev().cloned().collect() })
    }

    /// Expires old commands; marks HTTP-active PLCs without a request for 3 × heartbeat as disconnected.
    pub fn tick(&self) {
        let hb3 = self.heartbeat() * 3;
        let mut changed = Vec::new();
        let finished = {
            let mut g = self.lock();
            let cap = self.cfg.history_cap;
            let mut finished = Vec::new();
            for st in g.plcs.values_mut() {
                while let Some(idx) = st.cmds.iter().position(|c| c.created_at.elapsed() >= self.cfg.cmd_ttl) {
                    st.cmds[idx].view.error = Some(format!("expired after {} s", self.cfg.cmd_ttl.as_secs()));
                    finished.extend(st.finish(idx, CmdState::Expired, cap));
                }
                if st.mode == Mode::Active && st.framing == Some(Framing::Http) && st.connected && st.last_rx_at.is_some_and(|t| t.elapsed() > hb3) {
                    st.connected = false;
                    st.counters.disconnects += 1;
                    st.last_error = Some(format!("no request for {} ms", hb3.as_millis()));
                    changed.push(st.name.clone());
                }
            }
            finished
        };
        for n in changed {
            self.emit_plc(&n);
        }
        self.deliver(finished);
    }

    pub fn spawn_maintenance(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let hub = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut iv = tokio::time::interval(Duration::from_millis(250));
            loop {
                iv.tick().await;
                match hub.upgrade() {
                    Some(h) => h.tick(),
                    None => return,
                }
            }
        })
    }
}
