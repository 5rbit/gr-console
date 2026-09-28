//! PLC event log: EVTLOG (DB950) ring collector per PLC + console-derived events → `events.db`, rendered with
//! `plc/evtlog/catalog.toml` when read. Catalog and constant rules: `crates/evt-catalog`.
//!
//! Every write goes through one writer thread (mpsc): [`console`] never blocks its caller on sqlite, the collectors
//! only wait for channel space.

pub mod alerts;
pub mod catalog;
pub mod collect;
pub mod demo;
pub mod report;
pub mod routes;
pub mod stats;
pub mod store;
pub mod swimlane;
pub mod views;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use evt_catalog::Catalog;
use serde::Serialize;
use tokio::sync::{broadcast, mpsc};

use crate::config::{EvtLogCfg, PlcCfg};
use crate::plc::{PlcHandle, Tier};
pub use catalog::{EventRow, PlcTexts, Texts, now_ms};
use collect::{Header, RingGeo};
use store::{CollState, Origin, Row, Store};

pub const DB: &str = "EVTLOG";
/// Members the fast tier reads (header + Stat + Cfg, ~36 B); the entries are read by the collector only.
pub const HEADER_PATHS: [&str; 7] = ["LayoutSig", "Total", "Head", "BootId", "Dropped", "Stat", "Cfg"];
const QUEUE: usize = 4096;
/// Entries per S7 read — kept small so the poller's other tiers and commands interleave (a GR fast tick is 100 ms).
const MAX_ENTRY_BYTES_PER_READ: u32 = 900;

enum Msg {
    Rows(Vec<Row>, Option<(String, CollState)>),
}

struct Sink {
    tx: mpsc::Sender<Msg>,
    catalog: Arc<Catalog>,
    dropped: AtomicU64,
}

static SINK: OnceLock<Sink> = OnceLock::new();

/// Records a console-derived event (`[[console]]` of the catalog). Never blocks: a full queue drops the row and counts it.
/// `plc` = the PLC it concerns (config name) or `"console"`.
pub fn console(name: &str, plc: &str, a: i64, b: i64, ctx: u32, detail: impl Into<String>) {
    let Some(s) = SINK.get() else { return };
    let Some(def) = s.catalog.event_by_name(name).filter(|e| e.console) else {
        tracing::warn!(name, "evtlog: unknown console event");
        return;
    };
    let now = now_ms();
    let detail: String = detail.into();
    let row = Row {
        plc: plc.to_string(),
        epoch: 0,
        seq: None,
        plc_ts: now,
        rx_ts: now,
        cat: def.cat,
        lvl: def.lvl,
        src: 0,
        code: def.code.map(u32::from).unwrap_or(0),
        a,
        b,
        ctx,
        origin: Origin::Console,
        detail: (!detail.is_empty()).then_some(detail),
    };
    if s.tx.try_send(Msg::Rows(vec![row], None)).is_err() {
        s.dropped.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CollectorStatus {
    pub epoch: i64,
    pub last_seq: u32,
    /// Rows stored by this collector since start.
    pub stored: u64,
    pub gaps: u64,
    pub last_read_at: Option<String>,
    pub error: Option<String>,
    /// EVTLOG.Dropped of the last header read.
    pub dropped: Option<u32>,
}

pub struct EvtLog {
    pub cfg: EvtLogCfg,
    pub texts: Arc<Texts>,
    /// Reader connection (API); the writer thread has its own.
    pub store: Store,
    pub stream: broadcast::Sender<EventRow>,
    /// New / acknowledged alerts (SSE `alert` / `alert_ack`).
    pub alerts_tx: broadcast::Sender<alerts::AlertMsg>,
    alerts: Arc<alerts::Engine>,
    tx: mpsc::Sender<Msg>,
    status: Mutex<HashMap<String, CollectorStatus>>,
}

impl EvtLog {
    /// Opens `events.db`, starts the writer thread and installs the global console sink (first call only).
    /// `texts` = catalog + per-PLC renderers (contract constants, alarms.json, errorlist.json).
    pub fn start(cfg: EvtLogCfg, path: &Path, texts: Texts) -> anyhow::Result<Arc<EvtLog>> {
        let writer = Store::open(path)?;
        let store = Store::open(path)?;
        let catalog = texts.catalog.clone();
        let texts = Arc::new(texts);
        let (tx, rx) = mpsc::channel(QUEUE);
        let _ = SINK.set(Sink { tx: tx.clone(), catalog, dropped: AtomicU64::new(0) });
        let me = EvtLog::assemble(cfg, texts, store, writer, tx, rx);
        me.spawn_retention();
        if me.cfg.daily_report {
            let dir: PathBuf = path.parent().map(|p| p.join("reports")).unwrap_or_else(|| PathBuf::from("reports"));
            report::spawn_daily(me.clone(), dir);
        }
        Ok(me)
    }

    fn assemble(cfg: EvtLogCfg, texts: Arc<Texts>, store: Store, writer: Store, tx: mpsc::Sender<Msg>, rx: mpsc::Receiver<Msg>) -> Arc<EvtLog> {
        let (stream, _) = broadcast::channel(1024);
        let (alerts_tx, _) = broadcast::channel(256);
        let me = Arc::new(EvtLog { cfg, texts, store, stream, alerts_tx, alerts: Arc::new(alerts::Engine::default()), tx, status: Mutex::new(HashMap::new()) });
        me.reload_rules();
        match alerts::last_fired(&me.store) {
            Ok(m) => me.alerts.set_last(m),
            Err(e) => tracing::warn!("evtlog alerts: {e}"),
        }
        spawn_writer(writer, rx, me.texts.clone(), me.stream.clone(), me.alerts.clone(), me.alerts_tx.clone());
        me
    }

    #[cfg(test)]
    pub fn memory(catalog: Arc<Catalog>) -> Arc<EvtLog> {
        EvtLog::memory_with(Texts::new(catalog, vec![], ""))
    }

    #[cfg(test)]
    pub fn memory_with(texts: Texts) -> Arc<EvtLog> {
        let texts = Arc::new(texts);
        let (tx, rx) = mpsc::channel(QUEUE);
        let store = Store::memory();
        EvtLog::assemble(EvtLogCfg::default(), texts, store.clone(), store, tx, rx)
    }

    /// Enabled rules from the store into the writer's engine (after every rule change). A rule the catalog no
    /// longer knows is skipped with a warning, not fatal.
    pub fn reload_rules(&self) {
        match alerts::rules(&self.store) {
            Ok(rules) => {
                let compiled = rules
                    .iter()
                    .filter(|r| r.enabled)
                    .filter_map(|r| match alerts::compile(r, &self.texts.catalog) {
                        Ok(c) => Some(c),
                        Err(e) => {
                            tracing::warn!(rule = %r.name, "evtlog alert rule skipped: {e}");
                            None
                        }
                    })
                    .collect();
                self.alerts.set_rules(compiled);
            }
            Err(e) => tracing::warn!("evtlog alert rules: {e}"),
        }
    }

    /// Every collector's status, by PLC name.
    pub fn statuses(&self) -> Vec<(String, CollectorStatus)> {
        let mut v: Vec<(String, CollectorStatus)> = self.status.lock().unwrap_or_else(PoisonError::into_inner).iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    pub fn status(&self, plc: &str) -> CollectorStatus {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).get(plc).cloned().unwrap_or_default()
    }

    fn set_status(&self, plc: &str, f: impl FnOnce(&mut CollectorStatus)) {
        f(self.status.lock().unwrap_or_else(PoisonError::into_inner).entry(plc.to_string()).or_default());
    }

    /// Rows dropped because the writer queue was full (console events only).
    pub fn console_dropped(&self) -> u64 {
        SINK.get().map(|s| s.dropped.load(Ordering::Relaxed)).unwrap_or(0)
    }

    fn console_row(&self, name: &str, plc: &str, a: i64, b: i64) -> Option<Row> {
        let def = self.texts.catalog.event_by_name(name)?;
        let now = now_ms();
        Some(Row {
            plc: plc.into(),
            epoch: 0,
            seq: None,
            plc_ts: now,
            rx_ts: now,
            cat: def.cat,
            lvl: def.lvl,
            src: 0,
            code: def.code.map(u32::from).unwrap_or(0),
            a,
            b,
            ctx: 0,
            origin: Origin::Console,
            detail: None,
        })
    }

    /// Follows one PLC's EVTLOG: every fast-tier snapshot carries the header; the body is read when Total advances.
    pub fn attach(self: &Arc<Self>, plc: PlcHandle) {
        let Some(geo) = plc.layout(DB).and_then(RingGeo::from_layout) else {
            tracing::warn!(plc = %plc.name(), "evtlog: EVTLOG layout has no ring (Total/Head/BootId/Entry)");
            return;
        };
        let me = self.clone();
        tokio::spawn(async move {
            let name = plc.name().to_string();
            let mut st = match me.store.state(&name) {
                Ok(s) => s.unwrap_or(CollState { epoch: 0, boot_id: None, last_seq: 0 }),
                Err(e) => {
                    tracing::error!(plc = %name, "evtlog state: {e}");
                    CollState { epoch: 0, boot_id: None, last_seq: 0 }
                }
            };
            me.set_status(&name, |s| {
                s.epoch = st.epoch;
                s.last_seq = st.last_seq;
            });
            tracing::info!(plc = %name, epoch = st.epoch, last_seq = st.last_seq, capacity = geo.capacity, "evtlog collector");
            let mut rx = plc.events.subscribe();
            loop {
                match rx.recv().await {
                    Ok(ev) if ev.tier == Tier::Fast && ev.dbs.iter().any(|d| d == DB) => {}
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return,
                }
                let Some(h) = plc.snap().db(DB).and_then(|d| geo.header(&d.raw)) else { continue };
                if let Err(e) = me.collect(&plc, &geo, &mut st, &h).await {
                    tracing::warn!(plc = %name, "evtlog read: {e}");
                    me.set_status(&name, |s| s.error = Some(e));
                }
            }
        });
    }

    async fn collect(&self, plc: &PlcHandle, geo: &RingGeo, st: &mut CollState, h: &Header) -> Result<(), String> {
        let name = plc.name();
        // a failed read leaves the position where it was — the next header retries the same range
        let saved = st.clone();
        let d = collect::decide(st, h, geo.capacity);
        let dropped = h.dropped;
        self.set_status(name, |s| s.dropped = Some(dropped));
        let mut rows = Vec::new();
        if d.new_epoch {
            tracing::info!(plc = %name, epoch = st.epoch, boot_id = h.boot_id, total = h.total, "evtlog: new epoch");
            rows.extend(self.console_row("CON_EVT_EPOCH", name, i64::from(h.boot_id), i64::from(h.total)));
        }
        let mut gaps = 0u64;
        if let Some((lost, after)) = d.gap {
            rows.extend(self.console_row("CON_EVT_GAP", name, i64::from(lost), i64::from(after)));
            gaps += 1;
        }
        let Some((first, last)) = d.range else {
            if !rows.is_empty() {
                self.send(rows, Some((name.to_string(), st.clone()))).await;
            }
            return Ok(());
        };
        let max_n = (MAX_ENTRY_BYTES_PER_READ / geo.stride).max(1);
        let rx_ts = now_ms();
        let mut lost = 0u32;
        let mut n_entries = 0u64;
        for span in collect::plan(first, last, h.total, h.head, geo.capacity, max_n) {
            let start = geo.entry_off + span.idx * geo.stride;
            let bytes = match plc.read(DB, start, (span.n * geo.stride) as usize).await {
                Ok(b) => b,
                Err(e) => {
                    *st = saved;
                    return Err(e);
                }
            };
            for k in 0..span.n {
                let off = (k * geo.stride) as usize;
                let Some(b) = bytes.get(off..off + geo.stride as usize) else { break };
                let e = geo.entry(b);
                // overwritten while we read (the ring passed us) or never written
                if e.seq != span.seq + k {
                    lost += 1;
                    continue;
                }
                let plc_ts = e.time.and_then(|t| self.texts.local_ms(t)).unwrap_or(rx_ts);
                rows.push(Row {
                    plc: name.to_string(),
                    epoch: st.epoch,
                    seq: Some(i64::from(e.seq)),
                    plc_ts,
                    rx_ts,
                    cat: e.cat,
                    lvl: e.lvl,
                    src: u32::from(e.src),
                    code: u32::from(e.code),
                    a: i64::from(e.a),
                    b: i64::from(e.b),
                    ctx: e.ctx,
                    origin: Origin::Plc,
                    detail: None,
                });
                n_entries += 1;
            }
        }
        if lost > 0 {
            rows.extend(self.console_row("CON_EVT_GAP", name, i64::from(lost), i64::from(first.saturating_sub(1))));
            gaps += 1;
        }
        self.send(rows, Some((name.to_string(), st.clone()))).await;
        let (epoch, last_seq) = (st.epoch, st.last_seq);
        self.set_status(name, |s| {
            s.epoch = epoch;
            s.last_seq = last_seq;
            s.stored += n_entries;
            s.gaps += gaps;
            s.last_read_at = Some(crate::util::now_str());
            s.error = None;
        });
        Ok(())
    }

    async fn send(&self, rows: Vec<Row>, state: Option<(String, CollState)>) {
        if self.tx.send(Msg::Rows(rows, state)).await.is_err() {
            tracing::error!("evtlog writer gone");
        }
    }

    /// Hourly: rows older than `keep_days`, then oldest-first down to `max_mb`.
    fn spawn_retention(self: &Arc<Self>) {
        let me = self.clone();
        tokio::spawn(async move {
            let mut iv = tokio::time::interval(Duration::from_secs(3600));
            loop {
                iv.tick().await;
                let store = me.store.clone();
                let (days, max) = (me.cfg.keep_days, (me.cfg.max_mb as i64).saturating_mul(1024 * 1024));
                match tokio::task::spawn_blocking(move || store.prune(now_ms(), days, max)).await {
                    Ok(Ok((a, s))) if a + s > 0 => tracing::info!(by_age = a, by_size = s, "evtlog retention"),
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => tracing::warn!("evtlog retention: {e}"),
                    Err(e) => tracing::warn!("evtlog retention task: {e}"),
                }
            }
        });
    }
}

fn spawn_writer(store: Store, mut rx: mpsc::Receiver<Msg>, texts: Arc<Texts>, stream: broadcast::Sender<EventRow>, engine: Arc<alerts::Engine>, alerts_tx: broadcast::Sender<alerts::AlertMsg>) {
    let run = move || {
        while let Some(first) = rx.blocking_recv() {
            let mut rows = Vec::new();
            let mut states: Vec<(String, CollState)> = Vec::new();
            let mut next = Some(first);
            while let Some(Msg::Rows(r, s)) = next {
                rows.extend(r);
                if let Some(s) = s {
                    states.retain(|(p, _)| *p != s.0);
                    states.push(s);
                }
                next = if rows.len() < 2000 { rx.try_recv().ok() } else { None };
            }
            match store.write(rows, &states) {
                Ok(done) => {
                    if stream.receiver_count() > 0 {
                        for (id, r) in &done {
                            let _ = stream.send(texts.row(*id, r));
                        }
                    }
                    raise_alerts(&store, &texts, &engine, &alerts_tx, &done);
                }
                Err(e) => tracing::error!("evtlog write: {e}"),
            }
        }
    };
    if let Err(e) = std::thread::Builder::new().name("evtlog-writer".into()).spawn(run) {
        tracing::error!("evtlog writer thread: {e}");
    }
}

/// Rules over the rows just stored → `alerts` rows + SSE. Runs on the writer thread, after the rows are committed.
fn raise_alerts(store: &Store, texts: &Texts, engine: &alerts::Engine, tx: &broadcast::Sender<alerts::AlertMsg>, done: &[(i64, Row)]) {
    let render = |id: i64, r: &Row| {
        let e = texts.row(id, r);
        format!(
            "{}
{}
{}",
            e.text,
            e.name.unwrap_or_default(),
            e.detail.unwrap_or_default()
        )
    };
    let fires = engine.evaluate(done, &render, &|r| texts.classify(r));
    if fires.is_empty() {
        return;
    }
    match alerts::insert(store, &fires) {
        Ok(ids) => {
            for (aid, f) in ids.into_iter().zip(&fires) {
                let event = done.iter().find(|(id, _)| *id == f.event_id).map(|(id, r)| texts.row(*id, r));
                let rec = alerts::AlertRec { id: aid, ts: texts.fmt_ms(f.ts), ts_ms: f.ts, rule_id: f.rule_id, rule_name: f.rule_name.clone(), acked: false, event };
                let _ = tx.send(alerts::AlertMsg::New(Box::new(rec)));
            }
        }
        Err(e) => tracing::error!("evtlog alerts write: {e}"),
    }
}

/// Adds EVTLOG to a PLC's polling when its contract has it and the config does not mention it: header ranges in the
/// fast tier, never a whole-DB read, and a missing / mismatching DB does not fail the PLC's layout check.
pub fn auto_configure(p: &mut PlcCfg, has_evtlog: bool) {
    if !has_evtlog || p.all_dbs().iter().any(|d| d == DB) {
        return;
    }
    p.fast.push(DB.into());
    p.fast_ranges.insert(DB.into(), HEADER_PATHS.iter().map(|s| s.to_string()).collect());
    p.ranges_only.push(DB.into());
    p.optional.push(DB.into());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_configure_only_when_absent() {
        let mut p = PlcCfg::default();
        auto_configure(&mut p, true);
        assert!(p.fast.contains(&DB.to_string()) && p.ranges_only.contains(&DB.to_string()) && p.optional.contains(&DB.to_string()));
        assert_eq!(p.fast_ranges[DB].len(), HEADER_PATHS.len());
        let before = p.fast.len();
        auto_configure(&mut p, true);
        assert_eq!(p.fast.len(), before, "idempotent");
        let mut q = PlcCfg::default();
        auto_configure(&mut q, false);
        assert!(!q.all_dbs().contains(&DB.to_string()));
    }

    /// Fake S7 PLC → poller (header ranges only) → collector → store: backlog, ring wrap with a gap, restart epoch,
    /// read in PDU-sized spans.
    #[tokio::test]
    async fn collector_follows_the_ring_over_s7() {
        use crate::evtlog::store::Filter;
        let contract = Arc::new(plc_layout::Contract::load_dir(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plc/contract/GR2_PLC")).unwrap());
        let size = contract.size_of_db(DB).unwrap() as usize;
        let cat = Arc::new(catalog::embedded());
        let mut model = contract.decode_db(DB, &vec![0u8; size]).unwrap();
        demo::init(&mut model, contract.layout_sig(DB).unwrap(), 1, &cat);
        let server = s7::testing::FakeS7Server::spawn(s7::testing::FakeS7Server::store(), 480).await.unwrap();
        let publish = |m: &serde_json::Value| {
            let mut raw = vec![0u8; size];
            contract.encode_db(DB, m, &mut raw).unwrap();
            server.set_db(950, raw);
        };
        let push_n = |m: &mut serde_json::Value, n: i32| {
            for i in 0..n {
                if i % 32 == 0 {
                    demo::begin_scan(m, 10);
                }
                assert!(demo::push(m, "2026-09-27 10:00:00.000", 3, 2, 20, 300, i, 200, 7));
            }
        };
        push_n(&mut model, 40);
        publish(&model);

        let mut cfg = PlcCfg {
            name: "GR2".into(),
            host: "127.0.0.1".into(),
            port: server.addr.port(),
            fast: vec![],
            webmon: vec![],
            slow: vec![],
            on_demand: vec![],
            checks: vec![],
            heartbeat: None,
            ..PlcCfg::default()
        };
        auto_configure(&mut cfg, true);
        let poll = crate::config::PollCfg { fast_ms: 50, webmon_ms: 1000, slow_ms: 60_000, grm_fast_ms: 50, reconnect_ms: 200 };
        let h = crate::plc::spawn(cfg, contract.clone(), poll).unwrap();
        let log = EvtLog::memory(cat);
        log.attach(h.clone());
        let count = |f: &Filter| log.store.query(f, None, 5000, false).unwrap().len();
        let plc_rows = Filter { origin: Some(Origin::Plc), ..Default::default() };
        let wait = |want: usize, f: Filter| {
            let log = log.clone();
            async move {
                for _ in 0..200 {
                    if log.store.query(&f, None, 5000, false).unwrap().len() >= want {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                panic!("timed out waiting for {want} rows");
            }
        };
        wait(40, plc_rows.clone()).await;
        assert_eq!(log.status("GR2").last_seq, 40);

        // 1500 more: the ring (1000) wrapped past what we read → gap of 500, newest 1000 stored
        push_n(&mut model, 1500);
        publish(&model);
        wait(1040, plc_rows.clone()).await;
        let gap = Filter { codes: vec![(None, Some(9020))], ..Default::default() };
        wait(1, gap.clone()).await;
        let g = log.store.query(&gap, None, 10, false).unwrap();
        assert_eq!((g[0].1.a, g[0].1.b), (500, 40));
        let newest = log.store.query(&plc_rows, None, 1, false).unwrap();
        assert_eq!(newest[0].1.seq, Some(1540));

        // PLC restart: new BootId, ring reinitialized
        let mut fresh = contract.decode_db(DB, &vec![0u8; size]).unwrap();
        demo::init(&mut fresh, contract.layout_sig(DB).unwrap(), 2, &catalog::embedded());
        push_n(&mut fresh, 3);
        publish(&fresh);
        let epoch = Filter { codes: vec![(None, Some(9021))], ..Default::default() };
        wait(1, epoch).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        let e2: Vec<_> = log.store.query(&plc_rows, None, 5000, false).unwrap().into_iter().filter(|(_, r)| r.epoch == 2).collect();
        assert_eq!(e2.len(), 3);
        assert_eq!(count(&plc_rows), 1043);
        assert_eq!(log.status("GR2").epoch, 2);
    }

    #[tokio::test]
    async fn console_rows_reach_the_store_and_the_stream() {
        let cat = catalog::load_catalog(Path::new("does-not-exist.toml")).unwrap();
        let log = EvtLog::memory(cat);
        let mut rx = log.stream.subscribe();
        let row = log.console_row("CON_SUBMIT", "GR2", 0, 0).map(|mut r| {
            r.detail = Some("PICK 101".into());
            r
        });
        log.send(row.into_iter().collect(), None).await;
        let ev = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.unwrap().unwrap();
        assert_eq!(ev.text, "콘솔 제출 PICK 101");
        assert_eq!(ev.origin, "console");
        let got = log.store.query(&store::Filter::default(), None, 10, false).unwrap();
        assert_eq!(got.len(), 1);
    }
}
