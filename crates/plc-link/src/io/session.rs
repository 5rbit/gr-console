//! FRAME / NDJSON session over a `TcpStream`, seen from the side that speaks the PC half of the contract
//! (`docs/link/wire-spec.md` section 4): framing sniff, Hello exchange with the registry hash check,
//! heartbeat and drop after 3 × heartbeat, automatic Ack of framing / payload errors and of messages that
//! ask for one.
//!
//! The session owns nothing but the socket: every message it receives or sends is reported on the event
//! channel, and everything the application wants to send arrives on the command channel. Logging, command
//! bookkeeping and connection state stay in the application.

use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::codec::{Codec, HelloInfo, Message, Role, next_seq};
use crate::error::{ErrCode, LinkError};
use crate::framing::{Framing, PC_MAX_PAYLOAD, RawFrame, Sniff, sniff_detail};
use crate::header::Format;
use crate::io::stream::{StreamDecoder, encode, ref_of, wire_len};
use crate::wire_json::{Report, Strictness};

/// `Hello.Plc` the PC sends.
pub const PC_NAME: &str = "PC";

/// How often the session checks the heartbeat timers.
const TICK: Duration = Duration::from_millis(100);

/// `Hello.Formats` offered on a framing (NDJSON: JSON only).
pub fn hello_formats(framing: Framing) -> u8 {
    match framing {
        Framing::Ndjson => Format::Json.bit(),
        _ => Format::Json.bit() | Format::Bin.bit(),
    }
}

/// Why the framing of an incoming connection could not be determined. The bytes read so far are kept for
/// the caller's log.
#[derive(Clone, Debug)]
pub enum SniffFailure {
    /// The first bytes match no framing.
    Unknown(Vec<u8>),
    /// The peer closed before the framing was clear.
    Closed(Vec<u8>),
    /// Nothing more arrived within the timeout.
    Timeout(Vec<u8>),
    Io(String),
}

impl SniffFailure {
    /// Bytes received before the failure.
    pub fn initial(&self) -> &[u8] {
        match self {
            SniffFailure::Unknown(b) | SniffFailure::Closed(b) | SniffFailure::Timeout(b) => b,
            SniffFailure::Io(_) => &[],
        }
    }
}

impl std::fmt::Display for SniffFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SniffFailure::Unknown(_) => write!(f, "unknown framing"),
            SniffFailure::Closed(_) => write!(f, "closed before the framing was clear"),
            SniffFailure::Timeout(_) => write!(f, "no data"),
            SniffFailure::Io(e) => write!(f, "read: {e}"),
        }
    }
}

/// Reads from an accepted connection until its framing is known; the bytes read are returned with it and
/// must be handed to [`LinkSession::run`] as `initial`.
pub async fn sniff_framing(stream: &mut TcpStream, timeout: Duration) -> Result<(Framing, Vec<u8>), SniffFailure> {
    let mut initial = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match sniff_detail(&initial) {
            Sniff::Found(f) => return Ok((f, initial)),
            Sniff::Unknown => return Err(SniffFailure::Unknown(initial)),
            Sniff::NeedMore => {}
        }
        match tokio::time::timeout(timeout, stream.read(&mut buf)).await {
            Ok(Ok(0)) => return Err(SniffFailure::Closed(initial)),
            Ok(Ok(n)) => initial.extend_from_slice(&buf[..n]),
            Ok(Err(e)) => return Err(SniffFailure::Io(e.to_string())),
            Err(_) => return Err(SniffFailure::Timeout(initial)),
        }
    }
}

/// How a session is set up. Build it with [`SessionParams::new`] and override what differs.
#[derive(Clone, Debug)]
pub struct SessionParams {
    pub framing: Framing,
    /// This side opened the connection → it sends Hello first.
    pub opener: bool,
    /// Configured peer name; an accepted connection learns it from the Hello instead.
    pub plc: Option<String>,
    /// Format used before the peer's first message.
    pub format: Format,
    /// Peer address, for the application's log.
    pub peer: String,
    /// `Hello.Plc` this side sends.
    pub local_name: String,
    /// Receiving role of this side (direction checks).
    pub role: Role,
    pub strictness: Strictness,
    /// Heartbeat interval; `None` = the registry's `heartbeat_ms`.
    pub heartbeat: Option<Duration>,
    /// Receive maximum of this side.
    pub max_payload: usize,
}

impl SessionParams {
    /// PC defaults: accepted connection (the peer sends Hello first), JSON until the peer chooses.
    pub fn new(framing: Framing, peer: impl Into<String>) -> Self {
        SessionParams {
            framing,
            opener: false,
            plc: None,
            format: Format::Json,
            peer: peer.into(),
            local_name: PC_NAME.to_string(),
            role: Role::Pc,
            strictness: Strictness::Lenient,
            heartbeat: None,
            max_payload: PC_MAX_PAYLOAD,
        }
    }
}

/// Wire metadata of one message, for the application's log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameInfo {
    pub framing: Framing,
    pub format: Format,
    /// Bytes on the wire (FRAME: header + payload, NDJSON: line + newline).
    pub wire_len: usize,
    pub seq: u16,
    /// Received: the frame's signature. Sent: the message's declared signature.
    pub sig: Option<u32>,
}

/// Why a send did not happen.
#[derive(Clone, Debug)]
pub enum SendError {
    /// The message could not be encoded; the session continues.
    Encode(LinkError),
    /// Writing to the socket failed; the session is ending with this reason.
    Write(String),
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SendError::Encode(e) => write!(f, "{e}"),
            SendError::Write(e) => write!(f, "{e}"),
        }
    }
}

/// How the session ended.
#[derive(Clone, Debug, Default)]
pub struct SessionEnd {
    pub reason: String,
    /// A Hello of the peer was seen (used to reset a reconnect backoff).
    pub hello_seen: bool,
}

/// Everything the session observes, in the order it happened.
#[derive(Debug)]
pub enum SessionEvent {
    /// The socket is up and the framing is fixed; nothing has been exchanged yet.
    Connected { framing: Framing, peer: String },
    /// A decoded message. `hello` is set for the peer's Hello and carries the registry hash check.
    Received { msg: Message, frame: FrameInfo, report: Report, hello: Option<HelloInfo> },
    /// A frame that could not be decoded; the session has already answered with an Ack.
    Rejected { raw: Box<RawFrame>, error: LinkError, wire_len: usize },
    /// A message was written (with the seq the session assigned).
    Sent { msg: Message, frame: FrameInfo, tag: Option<u64> },
    /// A message was not written. [`SendError::Write`] means the session is ending.
    SendFailed { msg: Message, format: Format, error: SendError, tag: Option<u64> },
    /// Last event of a session.
    Closed(SessionEnd),
}

/// What the application asks the session to do.
#[derive(Debug)]
pub enum SessionCommand {
    /// Send a message; the session assigns the seq. `format` overrides the session format (NDJSON always
    /// sends JSON), `tag` is echoed in [`SessionEvent::Sent`] / [`SessionEvent::SendFailed`].
    Send { msg: Message, format: Option<Format>, tag: Option<u64> },
    /// End the session ("disconnect requested").
    Disconnect,
}

impl SessionCommand {
    /// Send in the session's current format, untagged.
    pub fn send(msg: Message) -> Self {
        SessionCommand::Send { msg, format: None, tag: None }
    }
}

type Flow = Result<(), String>;

/// A FRAME / NDJSON session. [`LinkSession::run`] owns the socket until the session ends.
pub struct LinkSession {
    codec: Codec,
    p: SessionParams,
    events: mpsc::UnboundedSender<SessionEvent>,
    heartbeat: Duration,
    /// Format of the peer's first message.
    format: Option<Format>,
    registry_ok: Option<bool>,
    seq: u16,
    last_tx: Instant,
    last_rx: Instant,
    peer_hb: Option<Duration>,
    hello_seen: bool,
}

impl LinkSession {
    pub fn new(codec: Codec, p: SessionParams, events: mpsc::UnboundedSender<SessionEvent>) -> Self {
        let heartbeat = p.heartbeat.unwrap_or_else(|| Duration::from_millis(codec.registry().heartbeat_ms() as u64));
        LinkSession { codec, p, events, heartbeat, format: None, registry_ok: None, seq: 0, last_tx: Instant::now(), last_rx: Instant::now(), peer_hb: None, hello_seen: false }
    }

    /// Runs until the connection ends. `initial` = bytes already read (framing sniff).
    pub async fn run(mut self, stream: TcpStream, initial: Vec<u8>, mut commands: mpsc::UnboundedReceiver<SessionCommand>) -> SessionEnd {
        let mut dec = match StreamDecoder::new(self.p.framing, self.p.max_payload) {
            Ok(d) => d,
            Err(e) => return self.finish(e.to_string()),
        };
        let _ = stream.set_nodelay(true);
        let (mut rd, mut wr) = stream.into_split();
        self.emit(SessionEvent::Connected { framing: self.p.framing, peer: self.p.peer.clone() });
        let result: Flow = async {
            if self.p.opener
                && let Ok(h) = self.codec.hello(&self.p.local_name, hello_formats(self.p.framing), 0)
            {
                let fmt = self.p.format;
                self.send(&mut wr, h, Some(fmt), None).await?;
            }
            dec.feed(&initial);
            let mut buf = vec![0u8; 16 * 1024];
            let mut tick = tokio::time::interval(TICK);
            loop {
                loop {
                    match dec.next() {
                        Ok(Some(raw)) => self.on_frame(&mut wr, raw).await?,
                        Ok(None) => break,
                        Err(e) => {
                            let raw = RawFrame { framing: self.p.framing, format: Format::Json, msg_type: None, seq: None, sig: None, payload: Vec::new(), http: None };
                            let (code, text) = (e.code.code(), e.msg.clone());
                            let reason = format!("framing error: {e}");
                            self.emit(SessionEvent::Rejected { raw: Box::new(raw), error: e, wire_len: 0 });
                            if let Ok(ack) = self.codec.ack(0, 0, code, &text, 0) {
                                let _ = self.send(&mut wr, ack, None, None).await;
                            }
                            let _ = wr.flush().await;
                            return Err(reason);
                        }
                    }
                }
                tokio::select! {
                    r = rd.read(&mut buf) => match r {
                        Ok(0) => return Err("closed by peer".into()),
                        Ok(n) => dec.feed(&buf[..n]),
                        Err(e) => return Err(format!("read: {e}")),
                    },
                    Some(c) = commands.recv() => match c {
                        SessionCommand::Send { msg, format, tag } => self.send(&mut wr, msg, format, tag).await?,
                        SessionCommand::Disconnect => return Err("disconnect requested".into()),
                    },
                    _ = tick.tick() => self.on_tick(&mut wr).await?,
                }
            }
        }
        .await;
        let _ = wr.shutdown().await;
        self.finish(result.err().unwrap_or_default())
    }

    fn finish(&self, reason: String) -> SessionEnd {
        let end = SessionEnd { reason, hello_seen: self.hello_seen };
        self.emit(SessionEvent::Closed(end.clone()));
        end
    }

    fn emit(&self, e: SessionEvent) {
        let _ = self.events.send(e);
    }

    /// Format of the next send: JSON while the framing is NDJSON or the registry hash did not match, else
    /// the format the peer chose.
    fn send_format(&self) -> Format {
        if self.p.framing == Framing::Ndjson || self.registry_ok == Some(false) { Format::Json } else { self.format.unwrap_or(self.p.format) }
    }

    /// Assigns a seq and writes the message. `Err` = write failure (the session ends); an encode failure is
    /// reported as [`SessionEvent::SendFailed`] and the session continues.
    async fn send<W: AsyncWrite + Unpin>(&mut self, w: &mut W, mut m: Message, format: Option<Format>, tag: Option<u64>) -> Flow {
        self.seq = next_seq(self.seq);
        m.seq = self.seq;
        let fmt = if self.p.framing == Framing::Ndjson { Format::Json } else { format.unwrap_or_else(|| self.send_format()) };
        let bytes = match encode(&self.codec, self.p.framing, fmt, &m) {
            Ok(b) => b,
            Err(e) => {
                self.emit(SessionEvent::SendFailed { msg: m, format: fmt, error: SendError::Encode(e), tag });
                return Ok(());
            }
        };
        if let Err(e) = w.write_all(&bytes).await {
            let reason = format!("write: {e}");
            self.emit(SessionEvent::SendFailed { msg: m, format: fmt, error: SendError::Write(reason.clone()), tag });
            return Err(reason);
        }
        self.last_tx = Instant::now();
        let frame = FrameInfo { framing: self.p.framing, format: fmt, wire_len: bytes.len(), seq: m.seq, sig: self.codec.registry().by_id(m.id).map(|s| s.sig) };
        self.emit(SessionEvent::Sent { msg: m, frame, tag });
        Ok(())
    }

    /// Reports a frame that failed to decode and answers it with an Ack.
    async fn reject<W: AsyncWrite + Unpin>(&mut self, w: &mut W, raw: RawFrame, err: LinkError, wire_len: usize) -> Flow {
        let (t, s) = ref_of(&self.codec, &raw);
        let ack = self.codec.ack(t, s, err.code.code(), &err.msg, 0).ok();
        self.emit(SessionEvent::Rejected { raw: Box::new(raw), error: err, wire_len });
        match ack {
            Some(ack) => self.send(w, ack, None, None).await,
            None => Ok(()),
        }
    }

    async fn on_frame<W: AsyncWrite + Unpin>(&mut self, w: &mut W, raw: RawFrame) -> Flow {
        self.last_rx = Instant::now();
        let len = wire_len(self.p.framing, raw.payload.len());
        let hello_id = self.codec.registry().by_name("Hello").map(|m| m.id);
        if raw.format == Format::Bin && self.registry_ok == Some(false) && raw.msg_type != hello_id {
            let e = LinkError::new(ErrCode::FormatNotAllowed, "registry hash mismatch (contract_mismatch): BIN refused, send JSON");
            return self.reject(w, raw, e, len).await;
        }
        let (m, report) = match self.codec.decode_raw_report(&raw, self.p.role, self.p.strictness) {
            Ok(x) => x,
            Err(e) => return self.reject(w, raw, e, len).await,
        };
        if self.format.is_none() {
            self.format = Some(raw.format);
        }
        let Ok(wants_ack) = self.codec.spec(m.id).map(|s| s.ack) else { return Ok(()) };
        let frame = FrameInfo { framing: raw.framing, format: raw.format, wire_len: len, seq: m.seq, sig: raw.sig };
        let (id, seq) = (m.id, m.seq);
        if Some(id) == hello_id {
            let hello = self.codec.hello_info(&m);
            self.registry_ok = Some(hello.registry_ok);
            self.peer_hb = hello.heartbeat_ms.map(Duration::from_millis);
            self.emit(SessionEvent::Received { msg: m, frame, report, hello: Some(hello) });
            if !self.p.opener
                && let Ok(h) = self.codec.hello(&self.p.local_name, hello_formats(self.p.framing), 0)
            {
                self.send(w, h, Some(raw.format), None).await?;
            }
            self.hello_seen = true;
            return Ok(());
        }
        self.emit(SessionEvent::Received { msg: m, frame, report, hello: None });
        if wants_ack && let Ok(ack) = self.codec.ack(id, seq, 0, "", 0) {
            self.send(w, ack, None, None).await?;
        }
        Ok(())
    }

    async fn on_tick<W: AsyncWrite + Unpin>(&mut self, w: &mut W) -> Flow {
        let rx_limit = self.peer_hb.unwrap_or(self.heartbeat) * 3;
        if self.last_rx.elapsed() >= rx_limit {
            return Err(format!("nothing received for {} ms (3 × heartbeat)", rx_limit.as_millis()));
        }
        if self.last_tx.elapsed() >= self.heartbeat
            && let Ok(m) = self.codec.heartbeat(0)
        {
            self.send(w, m, None, None).await?;
        }
        Ok(())
    }
}
