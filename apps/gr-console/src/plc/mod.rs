//! One `PlcSource` per S7 PLC: connect → verify layout → tiered polling → snapshots + events.
//! Also serves on-demand reads/writes (registry push, history sync) through a command channel.

pub mod routes;
pub mod verify;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use plc_layout::{Contract, Layout};
use s7::{S7Client, S7Config, S7Error};
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
    Read { db: String, start: u32, len: usize, reply: oneshot::Sender<Result<Vec<u8>, String>> },
    Write { db: String, start: u32, data: Vec<u8>, reply: oneshot::Sender<Result<(), String>> },
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
    /// Decodes a member path from the latest snapshot of `db` (PLC array indices).
    pub fn decode_path(&self, db: &str, path: &str) -> Option<Json> {
        let snap = self.snap();
        let d = snap.db(db)?;
        self.contract.decode_path(db, path, &d.raw).ok()
    }
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

        let result: Result<(), S7Error> = loop {
            let (tier, dbs): (Tier, Vec<String>) = tokio::select! {
                _ = fast.tick() => (Tier::Fast, cfg.fast.clone()),
                _ = webmon.tick(), if !cfg.webmon.is_empty() => (Tier::Webmon, cfg.webmon.clone()),
                _ = slow.tick() => (Tier::Slow, cfg.slow.clone()),
                cmd = cmd_rx.recv() => {
                    match cmd {
                        None => return,
                        Some(PlcCommand::Reconnect) => break Ok(()),
                        Some(PlcCommand::Reverify) => break Ok(()),
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
                match client.read_db(n, 0, layout.size as usize).await {
                    Ok(raw) => match contract.decode_db(db, &raw) {
                        Ok(json) => updated.push((db.clone(), raw, json)),
                        Err(e) => tracing::error!(plc = %cfg.name, db, "decode failed: {e}"),
                    },
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
            seq += 1;
            let at = now_str();
            let names: Vec<String> = updated.iter().map(|(n, _, _)| n.clone()).collect();
            snap_tx.send_modify(|s| {
                let mut next = (**s).clone();
                next.seq = seq;
                next.at = at.clone();
                for (name, raw, json) in updated {
                    next.dbs.insert(name, DbSnap { raw: Arc::new(raw), json: Arc::new(json), at: at.clone(), seq, tier });
                }
                *s = Arc::new(next);
            });
            if tier == Tier::Fast || cfg.fast.is_empty() {
                health_tx.send_modify(|h| {
                    h.rtt_ms = Some(rtt);
                    h.last_ok_at = Some(at.clone());
                });
            }
            let _ = ev_tx.send(PlcEvent { plc: cfg.name.clone(), tier, seq, dbs: names, at });
        };
        match result {
            Ok(()) => tracing::info!(plc = %cfg.name, "reconnect / reverify requested"),
            Err(e) => {
                tracing::warn!(plc = %cfg.name, error = %e, "S7 poll failed; reconnecting");
                health_tx.send_modify(|h| {
                    h.connected = false;
                    h.last_error = Some(e.to_string());
                });
                tokio::time::sleep(Duration::from_millis(poll.reconnect_ms)).await;
            }
        }
        drop(client);
    }
}
