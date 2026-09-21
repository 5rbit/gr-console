//! Trace sessions: start / stop, chunk ingest, on-disk storage and the live stream.
//!
//! One session at a time, like the recorder. A session owns a `CfgId`; chunks that carry a different one are
//! ignored, so a late chunk of a previous configuration never lands in a new file. Rows are appended to
//! `<data_dir>/traces/<id>/samples.bin` as fixed-width little-endian records (`cycle`, `tick`, one word per
//! channel); `meta.json` carries the channel list, the timing and the statistics, so a session reads back without
//! the PLC.

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use plc_layout::Contract;
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use tokio::sync::{Mutex, broadcast, mpsc, oneshot};

use plc_link::TraceLayout;

use crate::trace::channels::{self, CHAN_MAX, Channel, FLUSH_MIN_MS};
use crate::util::now_str;

/// Ack code the PLC answers a valid configuration with.
const ACK_OK: i16 = 0;
/// Largest number of rows a read returns before decimation.
pub const READ_MAX: usize = 200_000;
/// Default number of rows a read returns.
pub const READ_DEFAULT: usize = 12_000;

/// A request the trace store makes of whoever owns the PLC socket link.
#[derive(Debug)]
pub enum LinkRequest {
    /// Send a `TraceCfg` to `plc`; the reply carries the sequence number it went out with.
    Config { plc: String, cfg: Json, reply: oneshot::Sender<Result<u16, String>> },
}

/// One annotation inside a session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mark {
    pub t_ms: i64,
    pub at: String,
    pub text: String,
}

/// `meta.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Meta {
    pub id: String,
    pub label: String,
    pub note: String,
    pub plc: String,
    pub robot: Option<u8>,
    pub cfg_id: u16,
    pub divider: u16,
    pub flush_ms: u16,
    pub started_at: String,
    pub stopped_at: Option<String>,
    /// PLC wall clock of the first row, when the CPU clock is set.
    pub plc_time0: Option<String>,
    pub channels: Vec<ChannelMeta>,
    pub rows: u64,
    pub chunks: u64,
    /// Samples the PLC dropped because the link could not keep up.
    pub overrun: u64,
    /// Chunks whose `FirstCycle` did not continue the previous one.
    pub gaps: u64,
    pub errors: Vec<String>,
}

/// A channel as recorded, enough to decode `samples.bin` without the contract.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChannelMeta {
    pub path: String,
    pub type_name: String,
    pub kind: channels::Kind,
    pub bits: u8,
    pub db_no: u16,
    pub offset: u32,
    pub bit: Option<u8>,
}

impl From<&Channel> for ChannelMeta {
    fn from(c: &Channel) -> Self {
        ChannelMeta { path: c.path.clone(), type_name: c.type_name.clone(), kind: c.kind, bits: c.bits, db_no: c.db_no, offset: c.offset, bit: c.bit }
    }
}

impl ChannelMeta {
    /// Decodes one raw word, mirroring [`Channel::value`].
    pub fn value(&self, word: u32) -> f64 {
        match self.kind {
            channels::Kind::Bool => f64::from(word & 1),
            channels::Kind::Uint => f64::from(word),
            channels::Kind::Int | channels::Kind::TimeMs => match self.bits {
                8 => f64::from(word as u8 as i8),
                16 => f64::from(word as u16 as i16),
                _ => f64::from(word as i32),
            },
            channels::Kind::Real => f64::from(f32::from_bits(word)),
        }
    }
}

/// A stored session as read back: metadata, marks and the decimated rows.
pub type SessionRead = (Meta, Vec<Mark>, Vec<Vec<f64>>);

/// Parameters of a start request.
#[derive(Clone, Debug, Deserialize)]
pub struct StartReq {
    pub plc: String,
    #[serde(default)]
    pub robot: Option<u8>,
    pub channels: Vec<String>,
    #[serde(default = "one")]
    pub divider: u16,
    #[serde(default = "default_flush")]
    pub flush_ms: u16,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub note: String,
}

fn one() -> u16 {
    1
}

fn default_flush() -> u16 {
    100
}

struct Active {
    meta: Meta,
    dir: PathBuf,
    file: BufWriter<File>,
    channels: Vec<Channel>,
    /// Cycle the next chunk should start at, for gap detection.
    next_cycle: Option<u32>,
    marks: Vec<Mark>,
    /// Tick of the first row, so `t_ms` starts at 0.
    tick0: Option<i32>,
    /// `t_ms` of the row written last, so a mark lands on the trace time axis.
    last_t_ms: i64,
    /// Sequence number of the `TraceCfg` still waiting for its Ack.
    pending: Option<u16>,
    ack: Option<oneshot::Sender<i16>>,
}

/// Owns the current session and the link channel.
pub struct TraceStore {
    contract: Arc<Contract>,
    layout: TraceLayout,
    root: PathBuf,
    active: Mutex<Option<Active>>,
    link: Mutex<Option<mpsc::Sender<LinkRequest>>>,
    events: broadcast::Sender<Json>,
    next_cfg_id: std::sync::atomic::AtomicU16,
}

impl TraceStore {
    /// Fails when the contract has no usable `LNK_Trace`, which means the console cannot decode chunks at all.
    pub fn new(contract: Arc<Contract>, root: PathBuf) -> Result<Arc<TraceStore>, String> {
        let layout = TraceLayout::new(&contract).map_err(|e| e.to_string())?;
        let (events, _) = broadcast::channel(256);
        Ok(Arc::new(TraceStore { contract, layout, root, active: Mutex::new(None), link: Mutex::new(None), events, next_cfg_id: std::sync::atomic::AtomicU16::new(1) }))
    }

    pub fn contract(&self) -> &Contract {
        &self.contract
    }

    /// The same contract, for the link listener that decodes the messages.
    pub fn contract_arc(&self) -> Arc<Contract> {
        self.contract.clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Json> {
        self.events.subscribe()
    }

    /// Installs the channel the store sends `TraceCfg` requests on. Called by the link host when it starts.
    pub async fn set_link(&self, tx: Option<mpsc::Sender<LinkRequest>>) {
        *self.link.lock().await = tx;
    }

    pub async fn link_ready(&self) -> bool {
        self.link.lock().await.is_some()
    }

    /// Current session metadata, or `None`.
    pub async fn current(&self) -> Option<Meta> {
        self.active.lock().await.as_ref().map(|a| a.meta.clone())
    }

    fn cfg_id(&self) -> u16 {
        let v = self.next_cfg_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if v == 0 { 1 } else { v }
    }

    async fn send_cfg(&self, plc: &str, cfg: Json) -> Result<u16, String> {
        let tx = self.link.lock().await.clone().ok_or_else(|| "no PLC link is running".to_string())?;
        let (reply, rx) = oneshot::channel();
        tx.send(LinkRequest::Config { plc: plc.to_string(), cfg, reply }).await.map_err(|_| "the PLC link stopped".to_string())?;
        rx.await.map_err(|_| "the PLC link did not answer".to_string())?
    }

    /// Starts a session: resolves the channels, sends `TraceCfg` and waits for the PLC's Ack.
    pub async fn start(&self, req: StartReq) -> Result<Meta, String> {
        if self.active.lock().await.is_some() {
            return Err("a trace session is already running".to_string());
        }
        if req.channels.is_empty() || req.channels.len() > CHAN_MAX {
            return Err(format!("select 1 to {CHAN_MAX} channels, not {}", req.channels.len()));
        }
        let mut chans = Vec::with_capacity(req.channels.len());
        for p in &req.channels {
            chans.push(channels::resolve(&self.contract, p)?);
        }
        let divider = req.divider.max(1);
        let flush_ms = req.flush_ms.max(FLUSH_MIN_MS);
        let cfg_id = self.cfg_id();

        let id = format!("{}-{cfg_id:04x}", now_str().chars().filter(char::is_ascii_digit).take(14).collect::<String>());
        let dir = self.root.join(&id);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let file = BufWriter::new(File::create(dir.join("samples.bin")).map_err(|e| format!("samples.bin: {e}"))?);

        let meta = Meta {
            id: id.clone(),
            label: req.label.clone(),
            note: req.note.clone(),
            plc: req.plc.clone(),
            robot: req.robot,
            cfg_id,
            divider,
            flush_ms,
            started_at: now_str(),
            stopped_at: None,
            plc_time0: None,
            channels: chans.iter().map(ChannelMeta::from).collect(),
            rows: 0,
            chunks: 0,
            overrun: 0,
            gaps: 0,
            errors: Vec::new(),
        };

        let (ack_tx, ack_rx) = oneshot::channel();
        let cfg = channels::config_json(1, cfg_id, divider, flush_ms, &chans);
        let seq = match self.send_cfg(&req.plc, cfg).await {
            Ok(s) => s,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(e);
            }
        };
        *self.active.lock().await =
            Some(Active { meta: meta.clone(), dir: dir.clone(), file, channels: chans, next_cycle: None, marks: Vec::new(), tick0: None, last_t_ms: 0, pending: Some(seq), ack: Some(ack_tx) });

        match tokio::time::timeout(std::time::Duration::from_secs(5), ack_rx).await {
            Ok(Ok(ACK_OK)) => {
                self.write_meta().await;
                Ok(meta)
            }
            Ok(Ok(code)) => {
                self.abandon(&dir).await;
                Err(match code {
                    110 => "the PLC refused the channel list (code 110): an address is out of range or cannot be read".to_string(),
                    111 => "the PLC slot does not allow trace (code 111): set AllowTrace and FRAME framing in SOCK_CFG".to_string(),
                    other => format!("the PLC refused the trace configuration with code {other}"),
                })
            }
            _ => {
                self.abandon(&dir).await;
                Err("the PLC did not acknowledge the trace configuration".to_string())
            }
        }
    }

    async fn abandon(&self, dir: &Path) {
        *self.active.lock().await = None;
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Stops the session and returns its final metadata. Sends `TraceCfg` with `Cmd` 0; a failure there is
    /// recorded but does not keep the session open.
    pub async fn stop(&self) -> Result<Meta, String> {
        let (plc, cfg_id) = {
            let a = self.active.lock().await;
            let a = a.as_ref().ok_or_else(|| "no trace session is running".to_string())?;
            (a.meta.plc.clone(), a.meta.cfg_id)
        };
        let stop_err = self.send_cfg(&plc, channels::config_json(0, cfg_id, 1, FLUSH_MIN_MS, &[])).await.err();
        let mut guard = self.active.lock().await;
        let Some(mut a) = guard.take() else { return Err("no trace session is running".to_string()) };
        drop(guard);
        let _ = a.file.flush();
        a.meta.stopped_at = Some(now_str());
        if let Some(e) = stop_err {
            a.meta.errors.push(format!("stop: {e}"));
        }
        write_meta_file(&a.dir, &a.meta, &a.marks);
        let _ = self.events.send(json!({"event": "stopped", "id": a.meta.id}));
        Ok(a.meta)
    }

    /// Adds an annotation at the current position.
    pub async fn mark(&self, text: &str) -> Result<Mark, String> {
        let mut guard = self.active.lock().await;
        let a = guard.as_mut().ok_or_else(|| "no trace session is running".to_string())?;
        let m = Mark { t_ms: a.last_t_ms, at: now_str(), text: text.to_string() };
        a.marks.push(m.clone());
        let _ = self.events.send(json!({"event": "mark", "mark": m}));
        Ok(m)
    }

    async fn write_meta(&self) {
        let guard = self.active.lock().await;
        if let Some(a) = guard.as_ref() {
            write_meta_file(&a.dir, &a.meta, &a.marks);
        }
    }

    /// Feeds the Ack of a `TraceCfg` back to a waiting [`TraceStore::start`].
    pub async fn on_cfg_ack(&self, plc: &str, ref_seq: u16, code: i16) {
        let mut guard = self.active.lock().await;
        let Some(a) = guard.as_mut() else { return };
        if a.meta.plc != plc || a.pending != Some(ref_seq) {
            return;
        }
        a.pending = None;
        if let Some(tx) = a.ack.take() {
            let _ = tx.send(code);
        }
    }

    /// Feeds one received `Trace` payload into the session. Chunks of another PLC or another `CfgId` are dropped.
    pub async fn on_chunk(&self, plc: &str, payload: &[u8]) {
        let mut guard = self.active.lock().await;
        let Some(a) = guard.as_mut() else { return };
        if a.meta.plc != plc {
            return;
        }
        let chunk = match self.layout.decode(payload) {
            Ok(v) => v,
            Err(e) => {
                if a.meta.errors.len() < 20 {
                    a.meta.errors.push(e.to_string());
                }
                return;
            }
        };
        let head = &chunk.header;
        let rows = &chunk.rows;
        if head.cfg_id != a.meta.cfg_id || head.chan_count as usize != a.channels.len() {
            return;
        }
        if a.meta.plc_time0.is_none() && !head.time0.is_empty() {
            a.meta.plc_time0 = Some(head.time0.clone());
        }
        if head.overrun > 0 {
            a.meta.overrun += u64::from(head.overrun);
        }
        if let Some(want) = a.next_cycle
            && want != head.first_cycle
        {
            a.meta.gaps += 1;
        }
        a.next_cycle = Some(head.first_cycle.wrapping_add(u32::from(head.count) * u32::from(head.divider)));

        let mut cycle = head.first_cycle;
        let mut live = Vec::with_capacity(rows.len());
        for row in rows {
            let tick = row[0] as i32;
            if a.tick0.is_none() {
                a.tick0 = Some(tick);
            }
            let t_ms = tick.wrapping_sub(a.tick0.unwrap_or(tick));
            let mut rec = Vec::with_capacity(8 + row.len() * 4);
            rec.extend_from_slice(&cycle.to_le_bytes());
            rec.extend_from_slice(&t_ms.to_le_bytes());
            for w in &row[1..] {
                rec.extend_from_slice(&w.to_le_bytes());
            }
            if let Err(e) = a.file.write_all(&rec)
                && a.meta.errors.len() < 20
            {
                a.meta.errors.push(format!("write: {e}"));
            }
            let mut out = Vec::with_capacity(row.len());
            out.push(f64::from(t_ms));
            for (i, w) in row[1..].iter().enumerate() {
                out.push(a.channels[i].value(*w));
            }
            live.push(out);
            a.last_t_ms = i64::from(t_ms);
            cycle = cycle.wrapping_add(u32::from(head.divider));
        }
        a.meta.rows += rows.len() as u64;
        a.meta.chunks += 1;
        let _ = a.file.flush();

        let _ = self.events.send(json!({
            "event": "chunk",
            "id": a.meta.id,
            "cfg_id": head.cfg_id,
            "first_cycle": head.first_cycle,
            "divider": head.divider,
            "overrun": head.overrun,
            "rows": live,
            "total": a.meta.rows,
        }));
    }

    /// Drops the session when its PLC link goes away, so a later chunk cannot append to a stale file.
    pub async fn on_link_down(&self, plc: &str) {
        let mut guard = self.active.lock().await;
        let Some(a) = guard.as_mut() else { return };
        if a.meta.plc != plc {
            return;
        }
        a.meta.errors.push("the PLC link closed while the session was running".to_string());
        let mut a = guard.take().expect("checked above");
        let _ = a.file.flush();
        a.meta.stopped_at = Some(now_str());
        write_meta_file(&a.dir, &a.meta, &a.marks);
        let _ = self.events.send(json!({"event": "stopped", "id": a.meta.id, "reason": "link down"}));
    }

    /// Stored sessions, newest first.
    pub fn list(&self) -> Vec<Meta> {
        let mut out = Vec::new();
        let Ok(dir) = std::fs::read_dir(&self.root) else { return out };
        for e in dir.flatten() {
            if let Some(m) = read_meta(&e.path()) {
                out.push(m);
            }
        }
        out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        out
    }

    fn session_dir(&self, id: &str) -> Result<PathBuf, String> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(format!("{id}: not a session id"));
        }
        let dir = self.root.join(id);
        if !dir.join("meta.json").is_file() {
            return Err(format!("{id}: no such session"));
        }
        Ok(dir)
    }

    /// Metadata, marks and rows of a stored session. `max` decimates by a fixed stride, keeping the first and
    /// last row.
    pub fn read(&self, id: &str, from_ms: Option<i64>, to_ms: Option<i64>, max: usize) -> Result<SessionRead, String> {
        let dir = self.session_dir(id)?;
        let meta = read_meta(&dir).ok_or_else(|| format!("{id}: meta.json is unreadable"))?;
        let rows = read_rows(&dir, &meta, from_ms, to_ms)?;
        let max = max.clamp(100, READ_MAX);
        Ok((meta, read_marks(&dir), thin(rows, max)))
    }

    /// Deletes a stored session.
    pub fn delete(&self, id: &str) -> Result<(), String> {
        let dir = self.session_dir(id)?;
        std::fs::remove_dir_all(&dir).map_err(|e| format!("{id}: {e}"))
    }

    /// CSV of a stored session, every row, UTF-8 with a BOM so Excel reads Korean labels.
    pub fn csv(&self, id: &str) -> Result<String, String> {
        let dir = self.session_dir(id)?;
        let meta = read_meta(&dir).ok_or_else(|| format!("{id}: meta.json is unreadable"))?;
        let rows = read_rows(&dir, &meta, None, None)?;
        let mut s = String::from("\u{feff}cycle,t_ms");
        for c in &meta.channels {
            s.push(',');
            s.push_str(&c.path);
        }
        s.push('\n');
        for r in rows {
            s.push_str(&format!("{}", r[0]));
            for v in &r[1..] {
                s.push(',');
                s.push_str(&fmt_num(*v));
            }
            s.push('\n');
        }
        Ok(s)
    }
}

/// Number text for CSV: integers plain, others with four fraction digits like the wire format.
fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 { format!("{}", v as i64) } else { format!("{v:.4}") }
}

fn write_meta_file(dir: &Path, meta: &Meta, marks: &[Mark]) {
    let v = json!({"meta": meta, "marks": marks});
    if let Ok(text) = serde_json::to_string_pretty(&v) {
        let _ = std::fs::write(dir.join("meta.json"), text);
    }
}

fn read_meta(dir: &Path) -> Option<Meta> {
    let text = std::fs::read_to_string(dir.join("meta.json")).ok()?;
    let v: Json = serde_json::from_str(&text).ok()?;
    serde_json::from_value(v.get("meta")?.clone()).ok()
}

/// Marks of a stored session.
fn read_marks(dir: &Path) -> Vec<Mark> {
    let Ok(text) = std::fs::read_to_string(dir.join("meta.json")) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<Json>(&text) else { return Vec::new() };
    v.get("marks").and_then(|m| serde_json::from_value(m.clone()).ok()).unwrap_or_default()
}

/// Rows as `[cycle, t_ms, value...]`, filtered by the time window.
fn read_rows(dir: &Path, meta: &Meta, from_ms: Option<i64>, to_ms: Option<i64>) -> Result<Vec<Vec<f64>>, String> {
    let width = 8 + meta.channels.len() * 4;
    let mut f = File::open(dir.join("samples.bin")).map_err(|e| format!("samples.bin: {e}"))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).map_err(|e| format!("samples.bin: {e}"))?;
    let mut out = Vec::with_capacity(buf.len() / width.max(1));
    for rec in buf.chunks_exact(width) {
        let cycle = u32::from_le_bytes([rec[0], rec[1], rec[2], rec[3]]);
        let t_ms = i32::from_le_bytes([rec[4], rec[5], rec[6], rec[7]]);
        if from_ms.is_some_and(|f| i64::from(t_ms) < f) || to_ms.is_some_and(|t| i64::from(t_ms) > t) {
            continue;
        }
        let mut row = Vec::with_capacity(2 + meta.channels.len());
        row.push(f64::from(cycle));
        row.push(f64::from(t_ms));
        for (i, c) in meta.channels.iter().enumerate() {
            let o = 8 + i * 4;
            row.push(c.value(u32::from_le_bytes([rec[o], rec[o + 1], rec[o + 2], rec[o + 3]])));
        }
        out.push(row);
    }
    Ok(out)
}

/// Keeps at most `max` rows by a fixed stride, always including the first and the last.
fn thin(rows: Vec<Vec<f64>>, max: usize) -> Vec<Vec<f64>> {
    if rows.len() <= max || max == 0 {
        return rows;
    }
    let stride = rows.len().div_ceil(max);
    let last = rows.len() - 1;
    let mut out: Vec<Vec<f64>> = rows.iter().step_by(stride).cloned().collect();
    if out.last().is_none_or(|r| r[1] != rows[last][1]) {
        out.push(rows[last].clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(n: usize) -> Meta {
        Meta {
            id: "t".into(),
            label: String::new(),
            note: String::new(),
            plc: "GR2".into(),
            robot: None,
            cfg_id: 1,
            divider: 1,
            flush_ms: 100,
            started_at: "2026-09-16 10:00:00".into(),
            stopped_at: None,
            plc_time0: None,
            channels: (0..n)
                .map(|i| ChannelMeta { path: format!("WEBMON.A{i}"), type_name: "Real".into(), kind: channels::Kind::Real, bits: 32, db_no: 43, offset: i as u32 * 4, bit: None })
                .collect(),
            rows: 0,
            chunks: 0,
            overrun: 0,
            gaps: 0,
            errors: Vec::new(),
        }
    }

    #[test]
    fn rows_round_trip_through_the_file() {
        let dir = std::env::temp_dir().join(format!("gr-trace-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let m = meta(2);
        let mut bytes = Vec::new();
        for (i, v) in [(1.5f32, 2.5f32), (3.5, 4.5), (5.5, 6.5)].iter().enumerate() {
            bytes.extend_from_slice(&(100u32 + i as u32).to_le_bytes());
            bytes.extend_from_slice(&(i as i32 * 5).to_le_bytes());
            bytes.extend_from_slice(&v.0.to_bits().to_le_bytes());
            bytes.extend_from_slice(&v.1.to_bits().to_le_bytes());
        }
        std::fs::write(dir.join("samples.bin"), &bytes).unwrap();
        let rows = read_rows(&dir, &m, None, None).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], vec![100.0, 0.0, 1.5, 2.5]);
        assert_eq!(rows[2], vec![102.0, 10.0, 5.5, 6.5]);
        let win = read_rows(&dir, &m, Some(5), Some(5)).unwrap();
        assert_eq!(win.len(), 1);
        assert_eq!(win[0][1], 5.0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn thinning_keeps_the_ends() {
        let rows: Vec<Vec<f64>> = (0..1000).map(|i| vec![f64::from(i), f64::from(i), 0.0]).collect();
        let out = thin(rows.clone(), 100);
        assert!(out.len() <= 101, "{}", out.len());
        assert_eq!(out[0][1], 0.0);
        assert_eq!(out.last().unwrap()[1], 999.0);
        assert_eq!(thin(rows.clone(), 5000).len(), 1000);
    }

    #[test]
    fn values_decode_by_kind() {
        let mut c = meta(1).channels.remove(0);
        assert_eq!(c.value(1.5f32.to_bits()), 1.5);
        c.kind = channels::Kind::Int;
        c.bits = 16;
        assert_eq!(c.value(0xFFFF), -1.0);
        c.kind = channels::Kind::Bool;
        c.bits = 1;
        assert_eq!(c.value(1), 1.0);
    }

    #[test]
    fn csv_numbers_stay_short() {
        assert_eq!(fmt_num(3.0), "3");
        assert_eq!(fmt_num(-1.25), "-1.2500");
    }
}
