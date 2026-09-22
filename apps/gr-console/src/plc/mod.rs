//! One `PlcSource` per S7 PLC: connect → verify layout → tiered polling → snapshots + events.
//! Also serves on-demand reads/writes (registry push, history sync) through a command channel.

pub mod routes;
pub mod verify;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use plc_layout::{Contract, Layout};
use s7::{DbRead, S7Client, S7Config, S7Error};
use serde::Serialize;
use serde_json::Value as Json;
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use crate::config::{PlcCfg, PlcRole, PollCfg};
use crate::util::now_str;
pub use verify::{CheckResult, DbCheck};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Fast,
    Webmon,
    Slow,
    #[allow(dead_code)]
    OnDemand,
}

#[derive(Clone, Debug, Serialize)]
pub struct DbSnap {
    #[serde(skip)]
    pub raw: Arc<Vec<u8>>,
    pub json: Arc<Json>,
    pub at: String,
    pub seq: u64,
    pub tier: Tier,
    /// 읽은 순간(단조 시계) — 게이트가 "이 값이 얼마나 오래됐나"를 본다(`at` 문자열은 벽시계라 못 쓴다).
    #[serde(skip)]
    pub read_at: Option<Instant>,
}

impl DbSnap {
    /// 읽은 지 얼마나 됐나(모르면 None — 게이트는 오래된 것으로 본다).
    pub fn age(&self) -> Option<Duration> {
        self.read_at.map(|t| t.elapsed())
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PlcSnapshot {
    pub plc: String,
    pub seq: u64,
    pub at: String,
    pub dbs: HashMap<String, DbSnap>,
}

impl PlcSnapshot {
    pub fn db(&self, name: &str) -> Option<&DbSnap> {
        self.dbs.get(name)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct PlcEvent {
    pub plc: String,
    pub tier: Tier,
    pub seq: u64,
    pub dbs: Vec<String>,
    pub at: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PlcHealth {
    pub plc: String,
    pub host: String,
    pub connected: bool,
    pub connecting: bool,
    pub last_error: Option<String>,
    pub last_ok_at: Option<String>,
    pub rtt_ms: Option<u64>,
    pub pdu: u16,
    pub checks: Vec<DbCheck>,
    pub verified_at: Option<String>,
    /// None = not verified yet; Some(true) = every DB passed.
    pub layout_ok: Option<bool>,
    /// 폴링 실패(연결 끊김·timeout) 누적 — 통신 품질을 숫자로 본다.
    pub error_count: u64,
    /// 다시 연결한 횟수(첫 연결 제외).
    pub reconnect_count: u64,
    /// 주기별 마지막 한 바퀴 시간(ms) — `fast` · `webmon` · `slow`.
    pub cycle_ms: std::collections::BTreeMap<String, u64>,
    /// 주기별 최대 한 바퀴 시간(ms, 연결 뒤).
    pub cycle_max_ms: std::collections::BTreeMap<String, u64>,
    /// 하트비트 비트가 마지막으로 바뀐 뒤 지난 시간(ms) — 설정에 경로가 있고 읽혔을 때만. 오래되면 PLC 프로그램 정지.
    pub heartbeat_age_ms: Option<u64>,
    /// 느린 주기 작업이 다음 주기까지 끝나지 않아 한 번 건너뛴 횟수(조각 읽기가 밀린다는 뜻).
    pub slow_overruns: u64,
}

/// 느린 주기를 쪼개 읽는 단위(PDU 수) — 한 조각 ≈ 수십 ms. 빠른 주기·WEBMON·명령이 조각 사이에 끼어든다.
const SLICE_PDUS: usize = 3;

/// 읽은 바이트를 JSON 으로 — 스냅샷의 값과 같으면 다시 풀지 않고 그 Arc 를 쓴다(매 tick 디코드가 CPU 의 대부분).
fn decode_or_reuse(name: &str, snap_tx: &watch::Sender<Arc<PlcSnapshot>>, contract: &Contract, db: &str, raw: &[u8]) -> Option<Arc<Json>> {
    if let Some(j) = snap_tx.borrow().db(db).filter(|p| p.raw.as_slice() == raw).map(|p| p.json.clone()) {
        return Some(j);
    }
    match contract.decode_db(db, raw) {
        Ok(j) => Some(Arc::new(j)),
        Err(e) => {
            tracing::error!(plc = %name, db, "decode failed: {e}");
            None
        }
    }
}

/// 읽은 DB 들을 스냅샷에 넣고 이벤트를 낸다. 게시 시각 문자열을 돌려준다.
fn publish_dbs(name: &str, snap_tx: &watch::Sender<Arc<PlcSnapshot>>, ev_tx: &broadcast::Sender<PlcEvent>, seq: &mut u64, tier: Tier, updated: Vec<(String, Vec<u8>, Arc<Json>)>) -> String {
    *seq += 1;
    let seq = *seq;
    let at = now_str();
    let read_at = Some(Instant::now());
    let names: Vec<String> = updated.iter().map(|(n, _, _)| n.clone()).collect();
    snap_tx.send_modify(|s| {
        let mut next = (**s).clone();
        next.seq = seq;
        next.at = at.clone();
        for (n, raw, json) in updated {
            next.dbs.insert(n, DbSnap { raw: Arc::new(raw), json, at: at.clone(), seq, tier, read_at });
        }
        *s = Arc::new(next);
    });
    let _ = ev_tx.send(PlcEvent { plc: name.to_string(), tier, seq, dbs: names, at: at.clone() });
    at
}

/// 바이트 구간을 정렬하고 겹치거나 맞닿은 것을 합친다.
fn merge_ranges(mut r: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    r.sort();
    let mut out: Vec<(u32, u32)> = Vec::with_capacity(r.len());
    for (a, b) in r {
        match out.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// `fast_ranges`(멤버 경로) → DB 별 바이트 구간. 찾지 못한 경로는 경고하고 뺀다.
fn resolve_fast_ranges(cfg: &PlcCfg, layouts: &HashMap<String, Layout>) -> HashMap<String, Vec<(u32, u32)>> {
    let mut out = HashMap::new();
    for (db, paths) in &cfg.fast_ranges {
        let Some(layout) = layouts.get(db) else { continue };
        let mut r = Vec::new();
        for p in paths {
            match layout.range_of(p) {
                Some(x) => r.push(x),
                None => tracing::warn!(plc = %cfg.name, db, path = %p, "fast_ranges: path not in layout — ignored"),
            }
        }
        if !r.is_empty() {
            let r = merge_ranges(r);
            let bytes: u32 = r.iter().map(|(a, b)| b - a).sum();
            tracing::info!(plc = %cfg.name, db, ranges = r.len(), bytes, of = layout.size, "fast tier reads ranges only");
            out.insert(db.clone(), r);
        }
    }
    out
}

/// `DB.a.b.c` 의 bool 을 스냅샷 JSON 에서.
fn json_bool(json: &Json, path: &str) -> Option<bool> {
    path.split('.').try_fold(json, |v, k| v.get(k)).and_then(Json::as_bool)
}

fn tier_name(t: Tier) -> &'static str {
    match t {
        Tier::Fast => "fast",
        Tier::Webmon => "webmon",
        Tier::Slow => "slow",
        Tier::OnDemand => "on_demand",
    }
}

impl PlcHealth {
    pub fn db_ok(&self, db: &str) -> bool {
        self.checks.iter().any(|c| c.db == db && matches!(c.result, CheckResult::Ok))
    }
    pub fn mismatch_detail(&self, db: &str) -> String {
        self.checks.iter().find(|c| c.db == db).map(|c| c.result.text()).unwrap_or_else(|| "not verified".into())
    }
}

pub enum PlcCommand {
    Reconnect,
    Reverify,
    Read {
        db: String,
        start: u32,
        len: usize,
        reply: oneshot::Sender<Result<Vec<u8>, String>>,
    },
    Write {
        db: String,
        start: u32,
        data: Vec<u8>,
        reply: oneshot::Sender<Result<(), String>>,
    },
    /// 종료 절차: 앞선 명령까지 처리한 뒤 S7 연결을 닫고 폴링을 끝낸다.
    Shutdown {
        done: oneshot::Sender<()>,
    },
}

#[derive(Clone)]
pub struct PlcHandle {
    pub cfg: Arc<PlcCfg>,
    pub contract: Arc<Contract>,
    pub layouts: Arc<HashMap<String, Layout>>,
    pub snapshot: watch::Receiver<Arc<PlcSnapshot>>,
    pub health: watch::Receiver<PlcHealth>,
    pub events: broadcast::Sender<PlcEvent>,
    cmd_tx: mpsc::Sender<PlcCommand>,
}

impl PlcHandle {
    pub fn name(&self) -> &str {
        &self.cfg.name
    }
    pub fn snap(&self) -> Arc<PlcSnapshot> {
        self.snapshot.borrow().clone()
    }
    pub fn health_now(&self) -> PlcHealth {
        self.health.borrow().clone()
    }
    pub fn layout(&self, db: &str) -> Option<&Layout> {
        self.layouts.get(db)
    }
    pub fn db_number(&self, db: &str) -> Option<u16> {
        self.contract.db_number(db)
    }
    pub async fn reconnect(&self) {
        let _ = self.cmd_tx.send(PlcCommand::Reconnect).await;
    }
    pub async fn reverify(&self) {
        let _ = self.cmd_tx.send(PlcCommand::Reverify).await;
    }
    /// On-demand read of a byte range of a DB (bypasses the tiers; used for history sync / verification).
    pub async fn read(&self, db: &str, start: u32, len: usize) -> Result<Vec<u8>, String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx.send(PlcCommand::Read { db: db.into(), start, len, reply: tx }).await.map_err(|_| "plc task gone".to_string())?;
        rx.await.map_err(|_| "plc task dropped request".to_string())?
    }
    pub async fn write(&self, db: &str, start: u32, data: Vec<u8>) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx.send(PlcCommand::Write { db: db.into(), start, data, reply: tx }).await.map_err(|_| "plc task gone".to_string())?;
        rx.await.map_err(|_| "plc task dropped request".to_string())?
    }
    /// 종료 절차: 큐에 남은 읽기/쓰기가 끝난 뒤 S7 연결을 닫는다. 한도를 넘기면 false(그래도 프로세스는 끝난다).
    pub async fn shutdown(&self, timeout: Duration) -> bool {
        let (tx, rx) = oneshot::channel();
        if self.cmd_tx.send(PlcCommand::Shutdown { done: tx }).await.is_err() {
            return true; // 폴링 태스크가 이미 없다
        }
        tokio::time::timeout(timeout, rx).await.is_ok()
    }

    /// Whether `db` is configured for this PLC (any tier) — the layouts map is built from exactly those.
    pub fn has_db(&self, db: &str) -> bool {
        self.layouts.contains_key(db)
    }
    /// Decodes a member path from the latest snapshot of `db` (PLC array indices).
    pub fn decode_path(&self, db: &str, path: &str) -> Option<Json> {
        let snap = self.snap();
        let d = snap.db(db)?;
        self.contract.decode_path(db, path, &d.raw).ok()
    }
}

/// A DB a route needs: not configured for this PLC → 400 with the reason (e.g. GR1 has no `LASERDIAG`), configured but
/// failed the layout check → `LayoutMismatch`.
pub fn ensure_db(h: &PlcHandle, db: &str) -> Result<(), crate::error::ApiError> {
    if !h.has_db(db) {
        return Err(crate::error::ApiError::BadRequest(format!("{} PLC 설정에 {db} DB 가 없습니다 (gr-console.toml [[plcs]] · 계약 확인)", h.name())));
    }
    let hl = h.health_now();
    if !hl.db_ok(db) {
        return Err(crate::error::ApiError::LayoutMismatch { plc: h.name().into(), db: db.into(), detail: hl.mismatch_detail(db) });
    }
    Ok(())
}

/// Spawns the poller task and returns its handle.
pub fn spawn(cfg: PlcCfg, contract: Arc<Contract>, poll: PollCfg) -> anyhow::Result<PlcHandle> {
    let mut layouts = HashMap::new();
    for db in cfg.all_dbs() {
        let l = contract.layout_db(&db).map_err(|e| anyhow::anyhow!("{}: layout of {db}: {e}", cfg.name))?;
        if contract.db_number(&db).is_none() {
            anyhow::bail!("{}: DB number of {db} unknown (missing .xml in contract)", cfg.name);
        }
        layouts.insert(db, l);
    }
    let layouts = Arc::new(layouts);
    let cfg = Arc::new(cfg);
    let (snap_tx, snap_rx) = watch::channel(Arc::new(PlcSnapshot { plc: cfg.name.clone(), ..Default::default() }));
    let (health_tx, health_rx) = watch::channel(PlcHealth { plc: cfg.name.clone(), host: cfg.host.clone(), ..Default::default() });
    let (ev_tx, _) = broadcast::channel(256);
    let (cmd_tx, cmd_rx) = mpsc::channel(64);
    let handle = PlcHandle { cfg: cfg.clone(), contract: contract.clone(), layouts: layouts.clone(), snapshot: snap_rx, health: health_rx, events: ev_tx.clone(), cmd_tx };
    tokio::spawn(run(cfg, contract, layouts, poll, snap_tx, health_tx, ev_tx, cmd_rx));
    Ok(handle)
}

#[allow(clippy::too_many_arguments)]
async fn run(
    cfg: Arc<PlcCfg>,
    contract: Arc<Contract>,
    layouts: Arc<HashMap<String, Layout>>,
    poll: PollCfg,
    snap_tx: watch::Sender<Arc<PlcSnapshot>>,
    health_tx: watch::Sender<PlcHealth>,
    ev_tx: broadcast::Sender<PlcEvent>,
    mut cmd_rx: mpsc::Receiver<PlcCommand>,
) {
    let s7cfg =
        S7Config { host: cfg.host.clone(), port: cfg.port, rack: cfg.rack, slot: cfg.slot, connection_type: cfg.connection_type, timeout: Duration::from_millis(cfg.timeout_ms), pdu_request: 960 };
    let fast_ms = if cfg.role == PlcRole::Grm { poll.grm_fast_ms } else { poll.fast_ms };
    let mut seq: u64 = 0;
    loop {
        health_tx.send_modify(|h| {
            h.connecting = true;
            h.connected = false;
        });
        let mut client = match S7Client::connect(&s7cfg).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(plc = %cfg.name, host = %cfg.host, error = %e, "S7 connect failed");
                health_tx.send_modify(|h| {
                    h.connecting = false;
                    h.last_error = Some(e.to_string());
                });
                // drain commands while down so callers get an error instead of hanging
                let deadline = Instant::now() + Duration::from_millis(poll.reconnect_ms);
                while let Ok(Some(cmd)) = tokio::time::timeout_at(deadline.into(), cmd_rx.recv()).await {
                    match cmd {
                        PlcCommand::Read { reply, .. } => {
                            let _ = reply.send(Err(format!("{}: not connected", cfg.name)));
                        }
                        PlcCommand::Write { reply, .. } => {
                            let _ = reply.send(Err(format!("{}: not connected", cfg.name)));
                        }
                        PlcCommand::Reconnect | PlcCommand::Reverify => break,
                        PlcCommand::Shutdown { done } => {
                            let _ = done.send(());
                            return;
                        }
                    }
                }
                continue;
            }
        };
        let pdu = client.pdu_size();
        let checks = verify::verify(&mut client, &cfg, &contract, &layouts).await;
        let all_ok = checks.iter().all(|c| matches!(c.result, CheckResult::Ok));
        for c in checks.iter().filter(|c| !matches!(c.result, CheckResult::Ok)) {
            tracing::warn!(plc = %cfg.name, db = %c.db, "layout check failed: {}", c.result.text());
        }
        health_tx.send_modify(|h| {
            // 한 번이라도 붙었다가 다시 붙은 것 = 재연결(첫 연결은 세지 않는다).
            if h.verified_at.is_some() {
                h.reconnect_count += 1;
            }
            h.cycle_max_ms.clear();
            h.connected = true;
            h.connecting = false;
            h.pdu = pdu;
            h.last_error = None;
            h.checks = checks.clone();
            h.verified_at = Some(now_str());
            h.layout_ok = Some(all_ok);
        });
        tracing::info!(plc = %cfg.name, pdu, layout_ok = all_ok, "S7 connected and verified");

        let mut fast = tokio::time::interval(Duration::from_millis(fast_ms.max(50)));
        let mut webmon = tokio::time::interval(Duration::from_millis(poll.webmon_ms.max(100)));
        let mut slow = tokio::time::interval(Duration::from_millis(poll.slow_ms.max(500)));
        fast.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        webmon.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        slow.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let ok_db = |db: &str| checks.iter().any(|c| c.db == db && matches!(c.result, CheckResult::Ok));

        // 느린 주기 작업(조각 읽기): 주기가 오면 DB 목록을 걸고, 한가할 때마다 SLICE_PDUS 씩 읽는다. DB 하나가 다 모이면
        // 바로 게시한다. 예전에는 느린 주기 ~60 PDU(≈0.7 s)를 한 번에 읽어 그동안 빠른 주기·쓰기가 섰다.
        let mut slow_queue: std::collections::VecDeque<String> = std::collections::VecDeque::new();
        let mut slow_cur: Option<(String, u16, usize, Vec<u8>)> = None;
        let mut slow_t0 = Instant::now();
        let mut hb: Option<(bool, Instant)> = None;
        let slice_bytes = client.max_read_chunk() * SLICE_PDUS;
        // 빠른 주기 부분 읽기 구간 — 이 DB 들은 느린 주기 작업에서 전체를 새로 읽는다(범위 밖을 채운다).
        let partial = resolve_fast_ranges(&cfg, &layouts);
        let result: Result<(), S7Error> = loop {
            let (tier, dbs): (Tier, Vec<String>) = tokio::select! {
                biased;
                _ = fast.tick() => (Tier::Fast, cfg.fast.clone()),
                _ = webmon.tick(), if !cfg.webmon.is_empty() => (Tier::Webmon, cfg.webmon.clone()),
                _ = slow.tick() => {
                    if slow_queue.is_empty() && slow_cur.is_none() {
                        slow_queue = cfg.slow.iter().chain(partial.keys().filter(|k| !cfg.slow.contains(k))).filter(|d| ok_db(d)).cloned().collect();
                        slow_t0 = Instant::now();
                    } else {
                        health_tx.send_modify(|h| h.slow_overruns += 1);
                    }
                    continue;
                }
                cmd = cmd_rx.recv() => {
                    match cmd {
                        None => return,
                        Some(PlcCommand::Reconnect) => break Ok(()),
                        Some(PlcCommand::Reverify) => break Ok(()),
                        Some(PlcCommand::Shutdown { done }) => {
                            // 앞선 읽기/쓰기는 이 채널에서 이미 처리됐다 — 여기서 연결만 닫는다.
                            drop(client);
                            health_tx.send_modify(|h| {
                                h.connected = false;
                                h.connecting = false;
                            });
                            tracing::info!(plc = %cfg.name, "S7 connection closed (shutdown)");
                            let _ = done.send(());
                            return;
                        }
                        Some(PlcCommand::Read { db, start, len, reply }) => {
                            let r = match contract.db_number(&db) {
                                Some(n) => client.read_db(n, start, len).await.map_err(|e| e.to_string()),
                                None => Err(format!("unknown DB {db}")),
                            };
                            let failed = matches!(r, Err(ref e) if e.contains("io") || e.contains("timeout"));
                            let _ = reply.send(r);
                            if failed { break Err(S7Error::Disconnected); }
                            continue;
                        }
                        Some(PlcCommand::Write { db, start, data, reply }) => {
                            let r = match contract.db_number(&db) {
                                Some(n) => client.write_db(n, start, &data).await.map_err(|e| e.to_string()),
                                None => Err(format!("unknown DB {db}")),
                            };
                            let failed = matches!(r, Err(ref e) if e.contains("io") || e.contains("timeout"));
                            let _ = reply.send(r);
                            if failed { break Err(S7Error::Disconnected); }
                            continue;
                        }
                    }
                }
                // 느린 조각 — 위 가지가 모두 대기 중일 때만(biased) 돈다.
                _ = std::future::ready(()), if slow_cur.is_some() || !slow_queue.is_empty() => {
                    if slow_cur.is_none() {
                        let Some(db) = slow_queue.pop_front() else { continue };
                        let (Some(n), Some(layout)) = (contract.db_number(&db), layouts.get(&db)) else { continue };
                        slow_cur = Some((db, n, layout.size as usize, Vec::with_capacity(layout.size as usize)));
                    }
                    let Some((_, n, size, buf)) = slow_cur.as_mut() else { continue };
                    let take = slice_bytes.min(*size - buf.len());
                    match client.read_db(*n, buf.len() as u32, take).await {
                        Ok(b) => buf.extend_from_slice(&b),
                        Err(e) => break Err(e),
                    }
                    if buf.len() >= *size {
                        let (db, _, _, raw) = slow_cur.take().expect("current slow DB");
                        if let Some(json) = decode_or_reuse(&cfg.name, &snap_tx, &contract, &db, &raw) {
                            let at = publish_dbs(&cfg.name, &snap_tx, &ev_tx, &mut seq, Tier::Slow, vec![(db, raw, json)]);
                            if cfg.fast.is_empty() {
                                health_tx.send_modify(|h| h.last_ok_at = Some(at));
                            }
                        }
                        if slow_queue.is_empty() {
                            let ms = slow_t0.elapsed().as_millis() as u64;
                            health_tx.send_modify(|h| {
                                h.cycle_ms.insert("slow".into(), ms);
                                let m = h.cycle_max_ms.entry("slow".into()).or_insert(0);
                                *m = (*m).max(ms);
                            });
                        }
                    }
                    continue;
                }
            };
            let dbs: Vec<String> = dbs.into_iter().filter(|d| ok_db(d)).collect();
            if dbs.is_empty() {
                continue;
            }
            let t0 = Instant::now();
            let mut updated = Vec::with_capacity(dbs.len());
            let mut err = None;
            for db in &dbs {
                let (Some(n), Some(layout)) = (contract.db_number(db), layouts.get(db)) else { continue };
                // 부분 읽기: 범위가 있고 전체 사본이 있으면 범위만 읽어 사본에 덧씌운다(첫 읽기는 전체).
                let prev = partial.get(db.as_str()).and_then(|r| snap_tx.borrow().db(db).filter(|p| p.raw.len() == layout.size as usize).map(|p| (r.clone(), p.raw.clone())));
                let read = match prev {
                    Some((ranges, prev)) => {
                        let reads: Vec<DbRead> = ranges.iter().map(|(a, b)| DbRead { db: n, start: *a, len: (b - a) as usize }).collect();
                        match client.read_many(&reads).await {
                            Ok(parts) => {
                                let mut buf = (*prev).clone();
                                let mut bad = None;
                                for ((a, _), part) in ranges.iter().zip(parts) {
                                    match part {
                                        Ok(bytes) => buf[*a as usize..*a as usize + bytes.len()].copy_from_slice(&bytes),
                                        Err(e) => bad = Some(e),
                                    }
                                }
                                match bad {
                                    Some(e) => Err(e),
                                    None => Ok(buf),
                                }
                            }
                            Err(e) => Err(e),
                        }
                    }
                    None => client.read_db(n, 0, layout.size as usize).await,
                };
                match read {
                    Ok(raw) => {
                        if let Some(json) = decode_or_reuse(&cfg.name, &snap_tx, &contract, db, &raw) {
                            updated.push((db.clone(), raw, json));
                        }
                    }
                    Err(e) => {
                        err = Some(e);
                        break;
                    }
                }
            }
            if let Some(e) = err {
                break Err(e);
            }
            let rtt = t0.elapsed().as_millis() as u64;
            // 하트비트 — 이 주기에 읽은 DB 에 경로가 있으면 값이 바뀐 시각을 잡는다.
            let beat = cfg.heartbeat.as_deref().and_then(|p| {
                let (db, rest) = p.split_once('.')?;
                let (_, _, json) = updated.iter().find(|(n, _, _)| n == db)?;
                json_bool(json, rest)
            });
            if let Some(v) = beat
                && hb.is_none_or(|(last, _)| last != v)
            {
                hb = Some((v, Instant::now()));
            }
            let at = publish_dbs(&cfg.name, &snap_tx, &ev_tx, &mut seq, tier, updated);
            let tn = tier_name(tier);
            health_tx.send_modify(|h| {
                if tier == Tier::Fast || cfg.fast.is_empty() {
                    h.rtt_ms = Some(rtt);
                    h.last_ok_at = Some(at.clone());
                }
                if let Some((_, t)) = hb {
                    h.heartbeat_age_ms = Some(t.elapsed().as_millis() as u64);
                }
                h.cycle_ms.insert(tn.into(), rtt);
                let m = h.cycle_max_ms.entry(tn.into()).or_insert(0);
                *m = (*m).max(rtt);
            });
        };
        match result {
            Ok(()) => tracing::info!(plc = %cfg.name, "reconnect / reverify requested"),
            Err(e) => {
                tracing::warn!(plc = %cfg.name, error = %e, "S7 poll failed; reconnecting");
                health_tx.send_modify(|h| {
                    h.error_count += 1;
                    h.connected = false;
                    h.last_error = Some(e.to_string());
                });
                tokio::time::sleep(Duration::from_millis(poll.reconnect_ms)).await;
            }
        }
        drop(client);
    }
}

#[cfg(test)]
mod range_tests {
    #[test]
    fn merge_ranges_joins_overlaps_and_neighbours() {
        assert_eq!(super::merge_ranges(vec![(10, 20), (0, 4), (20, 30), (25, 28), (40, 42)]), vec![(0, 4), (10, 30), (40, 42)]);
        assert!(super::merge_ranges(vec![]).is_empty());
    }
}
