//! PLC simulator. Active: connects to the server (FRAME / NDJSON session with Hello first, or HTTP POST/GET
//! polling). Passive: listens for the PC (FRAME / NDJSON session, or the HTTP routes `POST /api/command`,
//! `GET /api/status`, `/api/measlog/last`, `/api/hello`). Writes `Content-Length` with 5 digits like the PLC.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
use plc_layout::Contract;
use plc_layout::layout::size_of;
use plc_link::framing::http::{CONTINUE_100, write_get};
use plc_link::framing::{ContentLength, HttpRequestDecoder, PLC_MAX_PAYLOAD, encode_frame, encode_ndjson, write_request, write_response};
use plc_link::trace::{TRACE_CH_MAX, TraceChunk, TraceLayout};
use plc_link::vectors::{golden_value, zero_value};
use plc_link::wire_json::zero_udt_bytes;
use plc_link::{Codec, ErrCode, Format, Framing, Message, RawFrame, Role, Strictness};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use crate::client::check_combo;
use crate::httpc::HttpConn;
use crate::session::hello_formats;
use crate::util::{SeqGen, dtl_now_text};
use crate::wire::{Fault, StreamDecoder, write_bytes};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimTarget {
    /// Active PLC: connect to the server's PLC port.
    Server(String),
    /// Passive PLC: listen (single peer).
    Listen(String),
}

#[derive(Clone, Debug)]
pub struct SimConfig {
    pub plc: String,
    pub framing: Framing,
    pub format: Format,
    pub target: SimTarget,
    pub measlog_ms: u64,
    pub status_ms: u64,
    /// HTTP-active command poll interval.
    pub poll_ms: u64,
    /// Stop after this many MeasLogs were acknowledged (passive HTTP: generated).
    pub count: Option<u64>,
    pub exit_after: Option<Duration>,
    /// Every N-th command result is a rejection (`Data[0]` = 16#80).
    pub reject_every: Option<u64>,
    /// Every N-th command is answered with Ack(`ack_code`) instead of a CommandResult (HTTP passive: 200 Ack).
    pub ack_every: Option<u64>,
    pub ack_code: i16,
    /// Every N-th command is answered with Ack(BUSY) (HTTP passive: 503 Ack).
    pub busy_every: Option<u64>,
    /// Trace chunk period in ms; 0 = the simulator traces only while a `TraceCfg` runs one, and then uses
    /// the configuration's `FlushMs`.
    pub trace_ms: u64,
    /// Channels of the trace the simulator starts by itself (only with `trace_ms` > 0); a `TraceCfg`
    /// replaces it with its own channel count.
    pub trace_channels: u16,
    pub fault: Option<Fault>,
    pub reconnect: Duration,
}

impl SimConfig {
    pub fn new(plc: &str, framing: Framing, format: Format, target: SimTarget) -> Self {
        SimConfig {
            plc: plc.to_string(),
            framing,
            format,
            target,
            measlog_ms: 2000,
            status_ms: 1000,
            poll_ms: 500,
            count: None,
            exit_after: None,
            reject_every: None,
            ack_every: None,
            ack_code: 104,
            busy_every: None,
            trace_ms: 0,
            trace_channels: 4,
            fault: None,
            reconnect: Duration::from_secs(1),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        check_combo(self.framing, self.format)?;
        if self.plc.is_empty() || self.plc.len() > 16 {
            return Err(format!("PLC name {:?} must be 1..16 characters (Hello.Plc String[16])", self.plc));
        }
        if self.measlog_ms == 0 || self.status_ms == 0 || self.poll_ms == 0 {
            return Err("intervals must be > 0 ms".into());
        }
        if self.trace_channels == 0 || self.trace_channels > TRACE_CH_MAX {
            return Err(format!("--trace-channels {} must be 1..{TRACE_CH_MAX}", self.trace_channels));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SimStats {
    pub connects: u64,
    pub hello_rx: u64,
    pub measlog_sent: u64,
    pub measlog_acked: u64,
    pub status_sent: u64,
    pub acks_rx: u64,
    pub ack_errors: u64,
    pub last_ack_code: Option<i64>,
    pub commands_rx: u64,
    pub results_sent: u64,
    pub rejects: u64,
    /// Commands answered with an Ack (`--ack-every`, `--busy-every`).
    pub command_acks: u64,
    pub heartbeats_rx: u64,
    pub trace_cfg_rx: u64,
    /// TraceCfg answered with Ack 110 / 111.
    pub trace_cfg_rejected: u64,
    pub trace_sent: u64,
    pub trace_rows_sent: u64,
    pub bad_magic_sent: u64,
    pub errors: u64,
    pub last_error: Option<String>,
}

type Stats = Arc<Mutex<SimStats>>;
type SimFuture = std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send>>;

fn with<R>(stats: &Stats, f: impl FnOnce(&mut SimStats) -> R) -> R {
    f(&mut stats.lock().unwrap_or_else(|p| p.into_inner()))
}

pub struct Sim {
    stats: Stats,
    pub local_addr: Option<SocketAddr>,
    handle: Option<JoinHandle<anyhow::Result<()>>>,
}

impl Sim {
    pub fn stats(&self) -> SimStats {
        with(&self.stats, |s| s.clone())
    }

    /// Shared counters (stay readable after `wait` consumed the simulator).
    pub fn stats_handle(&self) -> Arc<Mutex<SimStats>> {
        self.stats.clone()
    }

    pub fn stop(&mut self) {
        if let Some(h) = self.handle.take() {
            h.abort();
        }
    }

    /// Waits for the simulator to finish (`--count` / `--exit-after-s`).
    pub async fn wait(mut self) -> anyhow::Result<()> {
        match self.handle.take() {
            Some(h) => h.await.map_err(|e| anyhow!("simulator task: {e}"))?,
            None => Ok(()),
        }
    }
}

impl Drop for Sim {
    fn drop(&mut self) {
        self.stop();
    }
}

pub async fn start(codec: Codec, cfg: SimConfig) -> anyhow::Result<Sim> {
    cfg.validate().map_err(|e| anyhow!(e))?;
    let codec = codec.with_content_length(ContentLength::Pad5);
    let stats: Stats = Arc::default();
    let gen_ = Gen::new(codec, &cfg.plc)?;
    let (local_addr, fut): (Option<SocketAddr>, SimFuture) = match cfg.target.clone() {
        SimTarget::Server(addr) => (None, Box::pin(run_active(gen_, cfg.clone(), stats.clone(), addr))),
        SimTarget::Listen(addr) => {
            let l = TcpListener::bind(&addr).await.with_context(|| format!("listen {addr}"))?;
            (Some(l.local_addr()?), Box::pin(run_passive(l, gen_, cfg.clone(), stats.clone())))
        }
    };
    let exit_after = cfg.exit_after;
    let handle = tokio::spawn(async move {
        match exit_after {
            Some(d) => tokio::time::timeout(d, fut).await.unwrap_or(Ok(())),
            None => fut.await,
        }
    });
    Ok(Sim { stats, local_addr, handle: Some(handle) })
}

fn note_error(stats: &Stats, e: &anyhow::Error) {
    with(stats, |s| {
        s.errors += 1;
        s.last_error = Some(format!("{e:#}"));
    });
}

async fn run_active(mut g: Gen, cfg: SimConfig, stats: Stats, addr: String) -> anyhow::Result<()> {
    let mut first = true;
    loop {
        match TcpStream::connect(&addr).await {
            Ok(s) => {
                let _ = s.set_nodelay(true);
                with(&stats, |x| x.connects += 1);
                let r = match cfg.framing {
                    Framing::Http => http_active(s, &addr, &mut g, &cfg, &stats, first).await,
                    _ => stream_session(s, &mut g, &cfg, &stats, true, first).await,
                };
                first = false;
                match r {
                    Ok(true) => return Ok(()),
                    Ok(false) => {}
                    Err(e) => note_error(&stats, &e),
                }
            }
            Err(e) => note_error(&stats, &anyhow!("connect {addr}: {e}")),
        }
        tokio::time::sleep(cfg.reconnect).await;
    }
}

async fn run_passive(listener: TcpListener, mut g: Gen, cfg: SimConfig, stats: Stats) -> anyhow::Result<()> {
    let mut first = true;
    loop {
        let (s, _) = listener.accept().await?;
        let _ = s.set_nodelay(true);
        with(&stats, |x| x.connects += 1);
        let r = match cfg.framing {
            Framing::Http => http_passive(s, &mut g, &cfg, &stats).await,
            _ => stream_session(s, &mut g, &cfg, &stats, false, first).await,
        };
        first = false;
        match r {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(e) => note_error(&stats, &e),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------
// message generation

struct Gen {
    codec: Codec,
    plc: String,
    meas_n: u32,
    cmd_n: u64,
    latest_meas: Option<Message>,
    seq: SeqGen,
    /// Trace message id and chunk layout, when the contract has `LNK_Trace`.
    trace_msg: Option<(u16, TraceLayout)>,
    trace: Option<TraceRun>,
}

/// Ack codes of a rejected trace configuration (wire-spec section 6.1).
const TRACE_BAD_CFG: i16 = 110;
const TRACE_NOT_ALLOWED: i16 = 111;
/// Simulated PLC cycle: one sample every 10 ms × Divider.
const TRACE_CYCLE_MS: i32 = 10;

/// A trace the simulator is pushing.
#[derive(Clone, Debug)]
struct TraceRun {
    cfg_id: u16,
    chan_count: u16,
    divider: u16,
    period: Duration,
    /// `FirstCycle` of the next chunk.
    cycle: u32,
    /// Tick of the next row in ms.
    tick: i32,
}

/// Plausible sample of channel `c` at `tick` ms: even channels are a Real sine (IEEE-754 bit pattern), odd
/// channels a zero-extended integer ramp.
fn trace_sample(c: u16, tick: i32) -> u32 {
    if c.is_multiple_of(2) {
        let hz = 0.5 + 0.25 * c as f32;
        ((tick as f32 / 1000.0 * hz * std::f32::consts::TAU).sin() * (10 * (c + 1)) as f32).to_bits()
    } else {
        (tick / TRACE_CYCLE_MS + c as i32).rem_euclid(1000) as u32
    }
}

/// Byte offset and size of a member path inside a UDT.
pub fn region(c: &Contract, udt: &str, path: &str) -> anyhow::Result<(usize, usize)> {
    let u = c.udt(udt)?;
    let (ty, off) = c.locate(&u.fields, path)?;
    Ok((off as usize, size_of(c, &ty)? as usize))
}

fn udt_of(codec: &Codec, msg: &str) -> anyhow::Result<(u16, String)> {
    let spec = codec.spec_by_name(msg)?;
    Ok((spec.id, spec.udt.clone().ok_or_else(|| anyhow!("{msg} has no payload UDT"))?))
}

fn set_key(v: &mut Value, key: &str, x: Value) {
    if let Value::Object(o) = v
        && o.contains_key(key)
    {
        o.insert(key.to_string(), x);
    }
}

impl Gen {
    fn new(codec: Codec, plc: &str) -> anyhow::Result<Self> {
        for m in ["Hello", "Ack", "MeasLog", "Status", "Command", "CommandResult"] {
            codec.spec_by_name(m).with_context(|| format!("simulator needs message {m}"))?;
        }
        // Trace is optional: a contract without LNK_Trace answers every TraceCfg with TRACE_NOT_ALLOWED
        let trace_id = codec.spec_by_name("Trace").ok().map(|s| s.id);
        let trace_msg = trace_id.zip(TraceLayout::for_message(&codec, "Trace").ok());
        Ok(Gen { codec, plc: plc.to_string(), meas_n: 0, cmd_n: 0, latest_meas: None, seq: SeqGen::default(), trace_msg, trace: None })
    }

    /// Chunk period of the running trace, if any.
    fn trace_period(&self) -> Option<Duration> {
        self.trace.as_ref().map(|t| t.period)
    }

    /// Starts (or stops) the trace a new session begins with: `--trace-ms` runs one without a TraceCfg.
    fn trace_reset(&mut self, cfg: &SimConfig) {
        self.trace = (cfg.trace_ms > 0 && self.trace_msg.is_some() && cfg.framing == Framing::Frame).then(|| TraceRun {
            cfg_id: 0,
            chan_count: cfg.trace_channels,
            divider: 1,
            period: Duration::from_millis(cfg.trace_ms),
            cycle: 0,
            tick: 0,
        });
    }

    /// Applies a received TraceCfg → the Ack code and text (0 = accepted).
    fn on_trace_cfg(&mut self, m: &Message, cfg: &SimConfig) -> anyhow::Result<(i16, String)> {
        if self.trace_msg.is_none() {
            return Ok((TRACE_NOT_ALLOWED, "contract without LNK_Trace".into()));
        }
        if cfg.framing != Framing::Frame {
            return Ok((TRACE_NOT_ALLOWED, "trace needs FRAME framing".into()));
        }
        let v = self.codec.data_value(m)?;
        let num = |k: &str| v[k].as_u64().unwrap_or(0);
        if num("Cmd") == 0 {
            self.trace = None;
            return Ok((0, String::new()));
        }
        let (chan, div) = (num("ChanCount"), num("Divider"));
        if chan == 0 || chan > TRACE_CH_MAX as u64 {
            return Ok((TRACE_BAD_CFG, format!("ChanCount {chan} (1..{TRACE_CH_MAX})")));
        }
        if div == 0 || div > u16::MAX as u64 {
            return Ok((TRACE_BAD_CFG, format!("Divider {div} (1..65535)")));
        }
        let flush = num("FlushMs").max(20);
        let period = Duration::from_millis(if cfg.trace_ms > 0 { cfg.trace_ms } else { flush });
        self.trace = Some(TraceRun { cfg_id: num("CfgId") as u16, chan_count: chan as u16, divider: div as u16, period, cycle: 0, tick: 0 });
        Ok((0, String::new()))
    }

    /// Next Trace chunk (seq 0: assigned when sent) and its row count, or `None` while no trace runs.
    fn trace_chunk(&mut self) -> anyhow::Result<Option<(Message, usize)>> {
        let (Some((id, layout)), Some(run)) = (&self.trace_msg, &mut self.trace) else { return Ok(None) };
        let step = TRACE_CYCLE_MS * run.divider.max(1) as i32;
        let want = (run.period.as_millis() as i32 / step).max(1) as usize;
        let n = want.min(layout.rows_per_chunk(run.chan_count));
        let rows: Vec<Vec<u32>> = (0..n)
            .map(|r| {
                let tick = run.tick.wrapping_add(r as i32 * step);
                std::iter::once(tick as u32).chain((0..run.chan_count).map(|c| trace_sample(c, tick))).collect()
            })
            .collect();
        let mut chunk = TraceChunk::from_rows(run.cfg_id, run.chan_count, run.divider, rows);
        chunk.header.first_cycle = run.cycle;
        chunk.header.time0 = dtl_now_text();
        let payload = layout.encode(&chunk)?;
        run.cycle = run.cycle.wrapping_add(n as u32 * run.divider.max(1) as u32);
        run.tick = run.tick.wrapping_add(n as i32 * step);
        Ok(Some((Message { id: *id, seq: 0, payload }, n)))
    }

    fn hello(&self, framing: Framing) -> anyhow::Result<Message> {
        Ok(self.codec.hello(&self.plc, hello_formats(framing), 0)?)
    }

    /// Next MeasLog: Seq increments, TimeStamp now, Kind cycles 1..5, deterministic data.
    fn measlog(&mut self) -> anyhow::Result<Message> {
        let (_, udt) = udt_of(&self.codec, "MeasLog")?;
        self.meas_n += 1;
        let n = self.meas_n;
        let mut v = golden_value(self.codec.contract(), &udt)?;
        set_key(&mut v, "TimeStamp", json!(dtl_now_text()));
        set_key(&mut v, "Seq", json!(n));
        set_key(&mut v, "Kind", json!((n - 1) % 5 + 1));
        set_key(&mut v, "Status", json!(2));
        if let Some(Value::Array(d)) = v.get_mut("Data") {
            for (i, x) in d.iter_mut().enumerate() {
                *x = json!(n as f64 * 0.5 + i as f64 * 0.25);
            }
        }
        let (mut m, _) = self.codec.from_json_data("MeasLog", &v, Strictness::Lenient)?;
        m.seq = self.seq.next();
        self.latest_meas = Some(m.clone());
        Ok(m)
    }

    fn status(&self) -> anyhow::Result<Message> {
        let (_, udt) = udt_of(&self.codec, "Status")?;
        let mut v = zero_value(self.codec.contract(), &udt)?;
        set_key(&mut v, "UpdateTime", json!(dtl_now_text()));
        set_key(&mut v, "Mode", json!(1));
        set_key(&mut v, "MeasTotal", json!(self.meas_n));
        Ok(self.codec.from_json_data("Status", &v, Strictness::Lenient)?.0)
    }

    /// Reply to a command per `busy_every`, `ack_every`, then `reject_every` (counted over all commands).
    fn reply(&mut self, cmd: &Message, cfg: &SimConfig) -> anyhow::Result<CmdReply> {
        let n = self.cmd_n + 1;
        let every = |k: Option<u64>| k.is_some_and(|k| k > 0 && n.is_multiple_of(k));
        if every(cfg.busy_every) {
            self.cmd_n = n;
            let code = ErrCode::Busy.code();
            return Ok(CmdReply::Ack(self.codec.ack(cmd.id, cmd.seq, code, "BUSY: not injected within CmdWaitMs", 0)?, code));
        }
        if every(cfg.ack_every) {
            self.cmd_n = n;
            return Ok(CmdReply::Ack(self.codec.ack(cmd.id, cmd.seq, cfg.ack_code, "rejected by simulator", 0)?, cfg.ack_code));
        }
        let (m, rejected) = self.result(cmd, cfg.reject_every)?;
        Ok(CmdReply::Result(m, rejected))
    }

    /// CommandResult: Header / Command copied, Task = TaskData bytes, Data[2] = 1, Data[0] = 16#80 on a
    /// rejection. Seq = the command's seq.
    fn result(&mut self, cmd: &Message, reject_every: Option<u64>) -> anyhow::Result<(Message, bool)> {
        let c = self.codec.contract();
        let (_, cmd_udt) = udt_of(&self.codec, "Command")?;
        let (res_id, res_udt) = udt_of(&self.codec, "CommandResult")?;
        let mut out = zero_udt_bytes(c, &res_udt)?;
        for (src, dst) in [("Header", "Header"), ("Command", "Command"), ("TaskData", "Task")] {
            let (so, ss) = region(c, &cmd_udt, src)?;
            let (d_o, ds) = region(c, &res_udt, dst)?;
            if ss != ds {
                bail!("{cmd_udt}.{src} ({ss} B) and {res_udt}.{dst} ({ds} B) differ");
            }
            out[d_o..d_o + ds].copy_from_slice(cmd.payload.get(so..so + ss).ok_or_else(|| anyhow!("short command payload"))?);
        }
        self.cmd_n += 1;
        let reject = reject_every.is_some_and(|k| k > 0 && self.cmd_n.is_multiple_of(k));
        out[region(c, &res_udt, "Data[2]")?.0] = 1;
        if reject {
            out[region(c, &res_udt, "Data[0]")?.0] = 0x80;
        }
        Ok((Message { id: res_id, seq: cmd.seq, payload: out }, reject))
    }
}

/// Simulator answer to a Command.
enum CmdReply {
    /// CommandResult (+ whether `Data[0]` marks a rejection).
    Result(Message, bool),
    /// Ack with this code (seq 0: assigned when sent).
    Ack(Message, i16),
}

/// Encodes a session message; `bad_sig` corrupts the signature (FRAME BIN header, JSON envelope `sig`).
fn encode_sim(codec: &Codec, framing: Framing, format: Format, m: &Message, bad_sig: bool) -> anyhow::Result<Vec<u8>> {
    if !bad_sig {
        return Ok(crate::wire::encode(codec, framing, format, m)?);
    }
    let sig = codec.spec(m.id)?.sig;
    let wrong_env = || -> anyhow::Result<String> { Ok(codec.json_text(m)?.replacen(&format!("\"sig\":{sig}"), &format!("\"sig\":{}", sig ^ 1), 1)) };
    Ok(match (framing, format) {
        (Framing::Frame, Format::Bin) => encode_frame(Format::Bin, m.id, m.seq, sig ^ 1, &m.payload),
        (Framing::Frame, Format::Json) => encode_frame(Format::Json, m.id, m.seq, 0, wrong_env()?.as_bytes()),
        _ => encode_ndjson(&wrong_env()?),
    })
}

// ---------------------------------------------------------------------------------------------------------
// FRAME / NDJSON session

struct StreamSim<'a> {
    g: &'a mut Gen,
    cfg: &'a SimConfig,
    stats: &'a Stats,
    w: OwnedWriteHalf,
    buf: Vec<u8>,
    seq: SeqGen,
    last_tx: Instant,
    opener: bool,
    hello_seen: bool,
}

impl StreamSim<'_> {
    async fn put(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.last_tx = Instant::now();
        match self.cfg.fault {
            Some(Fault::Coalesce) => self.buf.extend_from_slice(bytes),
            Some(Fault::SplitWrites) => write_bytes(&mut self.w, bytes, true).await?,
            _ => self.w.write_all(bytes).await?,
        }
        Ok(())
    }

    async fn flush(&mut self) -> anyhow::Result<()> {
        if !self.buf.is_empty() {
            let b = std::mem::take(&mut self.buf);
            self.w.write_all(&b).await?;
        }
        Ok(())
    }

    async fn send(&mut self, mut m: Message, keep_seq: bool, bad_sig: bool) -> anyhow::Result<()> {
        if !keep_seq {
            m.seq = self.seq.next();
        }
        let bytes = encode_sim(&self.g.codec, self.cfg.framing, self.cfg.format, &m, bad_sig)?;
        self.put(&bytes).await
    }

    async fn on_raw(&mut self, raw: RawFrame) -> anyhow::Result<()> {
        let codec = self.g.codec.clone();
        let m = match codec.decode_raw(&raw, Role::Plc, Strictness::Lenient) {
            Ok(m) => m,
            Err(e) => {
                note_error(self.stats, &anyhow!("received: {e}"));
                let (t, s) = crate::wire::ref_of(&codec, &raw);
                let ack = codec.ack(t, s, e.code.code(), &e.msg, 0)?;
                return self.send(ack, false, false).await;
            }
        };
        let name = codec.spec(m.id)?.name.clone();
        match name.as_str() {
            "Hello" => {
                with(self.stats, |s| s.hello_rx += 1);
                self.hello_seen = true;
                if !self.opener {
                    let h = self.g.hello(self.cfg.framing)?;
                    self.send(h, false, false).await?;
                }
            }
            "Heartbeat" => with(self.stats, |s| s.heartbeats_rx += 1),
            "Ack" => {
                let v = codec.data_value(&m)?;
                let code = v["Code"].as_i64().unwrap_or(0);
                let meas = codec.spec_by_name("MeasLog")?.id as u64;
                with(self.stats, |s| {
                    s.acks_rx += 1;
                    s.last_ack_code = Some(code);
                    if code != 0 {
                        s.ack_errors += 1;
                    } else if v["RefType"].as_u64() == Some(meas) {
                        s.measlog_acked += 1;
                    }
                });
            }
            "TraceCfg" => {
                with(self.stats, |s| s.trace_cfg_rx += 1);
                let (code, text) = self.g.on_trace_cfg(&m, self.cfg)?;
                if code != 0 {
                    with(self.stats, |s| s.trace_cfg_rejected += 1);
                }
                let ack = codec.ack(m.id, m.seq, code, &text, 0)?;
                self.send(ack, false, false).await?;
            }
            "Command" => {
                with(self.stats, |s| s.commands_rx += 1);
                match self.g.reply(&m, self.cfg)? {
                    CmdReply::Result(r, rejected) => {
                        self.send(r, true, false).await?;
                        with(self.stats, |s| {
                            s.results_sent += 1;
                            s.rejects += u64::from(rejected);
                        });
                    }
                    CmdReply::Ack(a, _) => {
                        self.send(a, false, false).await?;
                        with(self.stats, |s| s.command_acks += 1);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn measlog(&mut self, bad_magic: &mut bool) -> anyhow::Result<()> {
        let m = self.g.measlog()?;
        let bad_sig = self.cfg.fault == Some(Fault::BadSig);
        self.send(m, true, bad_sig).await?;
        with(self.stats, |s| s.measlog_sent += 1);
        if self.cfg.fault == Some(Fault::Coalesce) {
            self.status().await?;
        }
        if *bad_magic {
            *bad_magic = false;
            let mut f = encode_frame(Format::Json, 2, 1, 0, b"null");
            f[0] = 0;
            f[1] = 0;
            if self.cfg.framing == Framing::Ndjson {
                f = b"GS not json\n".to_vec();
            }
            self.put(&f).await?;
            with(self.stats, |s| s.bad_magic_sent += 1);
        }
        Ok(())
    }

    async fn status(&mut self) -> anyhow::Result<()> {
        let m = self.g.status()?;
        self.send(m, false, false).await?;
        with(self.stats, |s| s.status_sent += 1);
        Ok(())
    }

    /// Sends one chunk. Trace is BIN only, whatever format the rest of the session uses.
    async fn trace(&mut self) -> anyhow::Result<()> {
        let Some((mut m, rows)) = self.g.trace_chunk()? else { return Ok(()) };
        m.seq = self.seq.next();
        let bytes = crate::wire::encode(&self.g.codec, self.cfg.framing, Format::Bin, &m)?;
        self.put(&bytes).await?;
        with(self.stats, |s| {
            s.trace_sent += 1;
            s.trace_rows_sent += rows as u64;
        });
        Ok(())
    }
}

/// Returns Ok(true) when `--count` is reached.
async fn stream_session(s: TcpStream, g: &mut Gen, cfg: &SimConfig, stats: &Stats, opener: bool, first: bool) -> anyhow::Result<bool> {
    let (mut rd, w) = s.into_split();
    let hb = Duration::from_millis(g.codec.registry().heartbeat_ms() as u64);
    g.trace_reset(cfg);
    let mut dec = StreamDecoder::new(cfg.framing, PLC_MAX_PAYLOAD)?;
    let mut ss = StreamSim { g, cfg, stats, w, buf: Vec::new(), seq: SeqGen::default(), last_tx: Instant::now(), opener, hello_seen: false };
    if opener {
        let h = ss.g.hello(cfg.framing)?;
        ss.send(h, false, false).await?;
        ss.flush().await?;
    }
    let mut bad_magic = cfg.fault == Some(Fault::BadMagic) && first;
    let now = tokio::time::Instant::now();
    let mut meas = tokio::time::interval_at(now + Duration::from_millis(cfg.measlog_ms), Duration::from_millis(cfg.measlog_ms));
    let mut status = tokio::time::interval_at(now + Duration::from_millis(cfg.status_ms), Duration::from_millis(cfg.status_ms));
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    let mut last_rx = Instant::now();
    let mut buf = vec![0u8; 8192];
    // when the next Trace chunk is due; None while no trace runs
    let mut next_trace: Option<tokio::time::Instant> = None;
    loop {
        let due = next_trace;
        tokio::select! {
            r = rd.read(&mut buf) => {
                let n = r.context("read")?;
                if n == 0 {
                    bail!("connection closed by the PC");
                }
                last_rx = Instant::now();
                dec.feed(&buf[..n]);
                loop {
                    match dec.next() {
                        Ok(Some(raw)) => ss.on_raw(raw).await?,
                        Ok(None) => break,
                        Err(e) => {
                            // framing error (bad magic / version, Length above 8176): Ack if possible, then close
                            if let Ok(a) = ss.g.codec.ack(0, 0, e.code.code(), &e.msg, 0) {
                                let _ = ss.send(a, false, false).await;
                                let _ = ss.flush().await;
                            }
                            bail!("framing error: {e}");
                        }
                    }
                }
            }
            _ = meas.tick(), if ss.hello_seen => {
                match cfg.count {
                    Some(c) if with(stats, |s| s.measlog_sent) >= c => {
                        if with(stats, |s| s.measlog_acked) >= c {
                            ss.flush().await?;
                            return Ok(true);
                        }
                    }
                    _ => ss.measlog(&mut bad_magic).await?,
                }
            }
            _ = status.tick(), if ss.hello_seen => ss.status().await?,
            _ = async move {
                match due {
                    Some(t) => tokio::time::sleep_until(t).await,
                    None => std::future::pending().await,
                }
            }, if ss.hello_seen => {
                ss.trace().await?;
                next_trace = None;
            }
            _ = tick.tick() => {
                if ss.last_tx.elapsed() >= hb {
                    let m = ss.g.codec.heartbeat(0)?;
                    ss.send(m, false, false).await?;
                }
                if last_rx.elapsed() >= hb * 3 {
                    bail!("nothing received for 3 × heartbeat");
                }
            }
        }
        ss.flush().await?;
        // keep the trace timer in step with the running configuration (a TraceCfg may have started one)
        next_trace = match (ss.g.trace_period(), next_trace) {
            (Some(p), None) => Some(tokio::time::Instant::now() + p),
            (Some(_), t) => t,
            (None, _) => None,
        };
    }
}

// ---------------------------------------------------------------------------------------------------------
// HTTP active (PLC is the client)

struct HttpSim<'a> {
    g: &'a mut Gen,
    cfg: &'a SimConfig,
    stats: &'a Stats,
    c: HttpConn,
    seq: SeqGen,
}

const HTTP_TIMEOUT: Duration = Duration::from_secs(5);

impl HttpSim<'_> {
    fn request(&mut self, mut m: Message, keep_seq: bool, bad_sig: bool) -> anyhow::Result<Vec<u8>> {
        if !keep_seq {
            m.seq = self.seq.next();
        }
        let codec = &self.g.codec;
        let spec = codec.spec(m.id)?;
        let (mut headers, mut body) = codec.http_parts(&m, self.cfg.format)?;
        if bad_sig {
            for (k, v) in headers.iter_mut() {
                if k == "X-GR-Sig" {
                    *v = format!("0x{:08X}", spec.sig ^ 1);
                }
            }
            if self.cfg.format == Format::Json {
                body = String::from_utf8_lossy(&body).replacen(&format!("\"sig\":{}", spec.sig), &format!("\"sig\":{}", spec.sig ^ 1), 1).into_bytes();
            }
        }
        let path = format!("/api/plc/{}/{}", self.cfg.plc, spec.http_name());
        let h: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        Ok(write_request("POST", &path, &self.c.host, &h, Some(&body), ContentLength::Pad5))
    }

    async fn roundtrip(&mut self, req: &[u8]) -> anyhow::Result<RawFrame> {
        if self.cfg.fault == Some(Fault::SplitWrites) { self.c.roundtrip_split(req, HTTP_TIMEOUT).await } else { self.c.roundtrip(req, HTTP_TIMEOUT).await }
    }

    fn on_ack(&mut self, raw: &RawFrame) -> anyhow::Result<()> {
        let status = raw.http.as_ref().and_then(|h| h.status).unwrap_or(0);
        if status != 200 {
            bail!("HTTP {status}");
        }
        let codec = &self.g.codec;
        let m = codec.decode_raw(raw, Role::Plc, Strictness::Lenient)?;
        let v = codec.data_value(&m)?;
        let code = v["Code"].as_i64().unwrap_or(0);
        let meas = codec.spec_by_name("MeasLog")?.id as u64;
        with(self.stats, |s| {
            s.acks_rx += 1;
            s.last_ack_code = Some(code);
            if code != 0 {
                s.ack_errors += 1;
            } else if v["RefType"].as_u64() == Some(meas) {
                s.measlog_acked += 1;
            }
        });
        Ok(())
    }

    async fn post(&mut self, m: Message, keep_seq: bool, bad_sig: bool) -> anyhow::Result<()> {
        let req = self.request(m, keep_seq, bad_sig)?;
        let raw = self.roundtrip(&req).await?;
        self.on_ack(&raw)
    }

    async fn measlog(&mut self, bad_magic: &mut bool) -> anyhow::Result<()> {
        let m = self.g.measlog()?;
        let bad_sig = self.cfg.fault == Some(Fault::BadSig);
        if self.cfg.fault == Some(Fault::Coalesce) {
            // two pipelined requests in one write
            let mut req = self.request(m, true, false)?;
            let st = self.g.status()?;
            req.extend(self.request(st, false, false)?);
            self.c.stream_mut().write_all(&req).await?;
            for _ in 0..2 {
                let raw = self.c.read_response(HTTP_TIMEOUT).await?;
                self.on_ack(&raw)?;
            }
            with(self.stats, |s| {
                s.measlog_sent += 1;
                s.status_sent += 1;
            });
        } else {
            self.post(m, true, bad_sig).await?;
            with(self.stats, |s| s.measlog_sent += 1);
        }
        if *bad_magic {
            *bad_magic = false;
            with(self.stats, |s| s.bad_magic_sent += 1);
            let raw = self.c.roundtrip(b"GS\x00\x01\r\n\r\n", HTTP_TIMEOUT).await?;
            let status = raw.http.as_ref().and_then(|h| h.status).unwrap_or(0);
            bail!("bad request answered with HTTP {status}, reconnecting");
        }
        Ok(())
    }

    async fn poll(&mut self) -> anyhow::Result<()> {
        let path = format!("/api/plc/{}/command", self.cfg.plc);
        let req = write_get(&path, &self.c.host, self.cfg.format);
        let raw = self.roundtrip(&req).await?;
        match raw.http.as_ref().and_then(|h| h.status).unwrap_or(0) {
            204 => Ok(()),
            200 => {
                let m = self.g.codec.decode_raw(&raw, Role::Plc, Strictness::Lenient)?;
                with(self.stats, |s| s.commands_rx += 1);
                match self.g.reply(&m, self.cfg)? {
                    CmdReply::Result(r, rejected) => {
                        self.post(r, true, false).await?;
                        with(self.stats, |s| {
                            s.results_sent += 1;
                            s.rejects += u64::from(rejected);
                        });
                    }
                    CmdReply::Ack(a, _) => {
                        self.post(a, false, false).await?;
                        with(self.stats, |s| s.command_acks += 1);
                    }
                }
                Ok(())
            }
            s => bail!("GET {path}: HTTP {s}"),
        }
    }
}

async fn http_active(s: TcpStream, addr: &str, g: &mut Gen, cfg: &SimConfig, stats: &Stats, first: bool) -> anyhow::Result<bool> {
    let mut h = HttpSim { g, cfg, stats, c: HttpConn::new(s, addr, PLC_MAX_PAYLOAD), seq: SeqGen::default() };
    let hello = h.g.hello(Framing::Http)?;
    h.post(hello, false, false).await?;
    with(stats, |s| s.hello_rx += 1);
    let mut bad_magic = cfg.fault == Some(Fault::BadMagic) && first;
    let now = tokio::time::Instant::now();
    let mut meas = tokio::time::interval_at(now + Duration::from_millis(cfg.measlog_ms), Duration::from_millis(cfg.measlog_ms));
    let mut status = tokio::time::interval_at(now + Duration::from_millis(cfg.status_ms), Duration::from_millis(cfg.status_ms));
    let mut poll = tokio::time::interval(Duration::from_millis(cfg.poll_ms));
    let hb = Duration::from_millis(h.g.codec.registry().heartbeat_ms() as u64);
    let mut last_tx = Instant::now();
    loop {
        tokio::select! {
            _ = meas.tick() => {
                match cfg.count {
                    Some(c) if with(stats, |s| s.measlog_sent) >= c => {
                        if with(stats, |s| s.measlog_acked) >= c {
                            return Ok(true);
                        }
                    }
                    _ => h.measlog(&mut bad_magic).await?,
                }
                last_tx = Instant::now();
            }
            _ = status.tick() => {
                let m = h.g.status()?;
                h.post(m, false, false).await?;
                with(stats, |s| s.status_sent += 1);
                last_tx = Instant::now();
            }
            _ = poll.tick() => {
                h.poll().await?;
                if last_tx.elapsed() >= hb {
                    let m = h.g.codec.heartbeat(0)?;
                    h.post(m, false, false).await?;
                    last_tx = Instant::now();
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------
// HTTP passive (PLC is the server)

fn http_passive_response(g: &mut Gen, cfg: &SimConfig, stats: &Stats, raw: &RawFrame) -> Vec<u8> {
    let codec = g.codec.clone();
    let plain = |s: u16| write_response(s, &[], Some(b""), ContentLength::Pad5);
    let Some(meta) = raw.http.as_ref() else { return plain(400) };
    let path = meta.path.as_deref().unwrap_or("").split('?').next().unwrap_or("").to_string();
    let method = meta.method.as_deref().unwrap_or("");
    let fmt = raw.format;
    let respond = |m: &Message| codec.to_http_response(200, Some(m), fmt).unwrap_or_else(|_| plain(500));
    match (method, path.as_str()) {
        ("POST", "/api/command") => {
            // 200 CommandResult | 200 Ack(rc) | 400 Ack(read error) | 503 Ack(BUSY)
            let ack = |g: &mut Gen, status: u16, t: u16, s: u16, code: i16, text: &str| {
                let seq = g.seq.next();
                codec.ack(t, s, code, text, seq).and_then(|a| codec.to_http_response(status, Some(&a), fmt)).unwrap_or_else(|_| plain(status))
            };
            match codec.decode_raw(raw, Role::Plc, Strictness::Lenient) {
                Ok(m) if codec.spec(m.id).is_ok_and(|s| s.name == "Command") => {
                    with(stats, |s| s.commands_rx += 1);
                    match g.reply(&m, cfg) {
                        Ok(CmdReply::Result(r, rejected)) => {
                            with(stats, |s| {
                                s.results_sent += 1;
                                s.rejects += u64::from(rejected);
                            });
                            respond(&r)
                        }
                        Ok(CmdReply::Ack(mut a, code)) => {
                            with(stats, |s| s.command_acks += 1);
                            a.seq = g.seq.next();
                            let status = if code == ErrCode::Busy.code() { 503 } else { 200 };
                            codec.to_http_response(status, Some(&a), fmt).unwrap_or_else(|_| plain(500))
                        }
                        Err(e) => ack(g, 400, m.id, m.seq, ErrCode::Parse.code(), &e.to_string()),
                    }
                }
                Ok(m) => ack(g, 400, m.id, m.seq, ErrCode::DirNotAllowed.code(), "only Command"),
                Err(e) => {
                    let (t, s) = crate::wire::ref_of(&codec, raw);
                    ack(g, 400, t, s, e.code.code(), &e.msg)
                }
            }
        }
        ("GET", "/api/status") => match g.status() {
            Ok(mut m) => {
                m.seq = g.seq.next();
                with(stats, |s| s.status_sent += 1);
                respond(&m)
            }
            Err(_) => plain(503),
        },
        ("GET", "/api/measlog/last") => match &g.latest_meas {
            Some(m) => respond(m),
            // the PLC writes Content-Length: 00000 on 204
            None => b"HTTP/1.1 204 No Content\r\nContent-Length: 00000\r\n\r\n".to_vec(),
        },
        ("GET", "/api/hello") => match g.hello(Framing::Http) {
            Ok(mut m) => {
                m.seq = g.seq.next();
                with(stats, |s| s.hello_rx += 1);
                respond(&m)
            }
            Err(_) => plain(503),
        },
        (_, "/api/command" | "/api/status" | "/api/measlog/last" | "/api/hello") => plain(405),
        _ => plain(404),
    }
}

async fn http_passive(s: TcpStream, g: &mut Gen, cfg: &SimConfig, stats: &Stats) -> anyhow::Result<bool> {
    let (mut rd, mut w) = s.into_split();
    let mut dec = HttpRequestDecoder::plc();
    let mut meas = tokio::time::interval(Duration::from_millis(cfg.measlog_ms));
    let mut buf = vec![0u8; 8192];
    loop {
        tokio::select! {
            r = tokio::time::timeout(Duration::from_secs(30), rd.read(&mut buf)) => {
                let n = r.map_err(|_| anyhow!("idle 30 s, closing"))?.context("read")?;
                if n == 0 {
                    return Ok(false);
                }
                dec.feed(&buf[..n]);
                loop {
                    match dec.next() {
                        Ok(Some(raw)) => {
                            let resp = http_passive_response(g, cfg, stats, &raw);
                            write_bytes(&mut w, &resp, cfg.fault == Some(Fault::SplitWrites)).await?;
                            if raw.http.as_ref().is_some_and(|h| h.close) {
                                return Ok(false);
                            }
                        }
                        Ok(None) => {
                            if dec.take_continue() {
                                w.write_all(CONTINUE_100).await?;
                            }
                            break;
                        }
                        Err(e) => {
                            let status = e.http_status.unwrap_or(400);
                            w.write_all(&write_response(status, &[], Some(b""), ContentLength::Pad5)).await?;
                            bail!("bad request: {e}");
                        }
                    }
                }
            }
            _ = meas.tick() => {
                match cfg.count {
                    Some(c) if with(stats, |s| s.measlog_sent) >= c => return Ok(true),
                    _ => {
                        g.measlog()?;
                        with(stats, |s| s.measlog_sent += 1);
                    }
                }
            }
        }
    }
}
