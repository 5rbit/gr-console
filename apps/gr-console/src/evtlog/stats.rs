//! Alarm and step statistics over `events.db` — `/api/events/stats/*` and the daily report.
//!
//! SQL only narrows (ALARM by the `(cat, code)` index, STEP by the time index); pairing and percentiles run here.
//! STEP uses `+cat` so the planner takes the time range instead of the category index, whose range is the whole
//! retention.

use std::collections::{BTreeMap, HashMap};

use axum::Router;
use axum::extract::{Query, State};
use axum::routing::get;
use rusqlite::types::Value;
use serde::{Deserialize, Serialize};

use evt_catalog::errorlist::{self, CAT_INFO, CAT_OPERATOR, TRANS_MUL};
use evt_catalog::{Level, Trans, Ty};

use super::catalog::{Texts, now_ms};
use super::routes::{list, log, num, opt, types};
use super::store::Store;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

// catalog codes — the tests hold them against the embedded catalog
pub const CAT_SYS: u8 = 1;
pub const CAT_STEP: u8 = 3;
pub const CAT_TASK: u8 = 4;
pub const CAT_ALARM: u8 = 6;
pub const ALM_RAISED: u32 = 601;
pub const ALM_CLEARED: u32 = 602;
pub const ALM_RESET: u32 = 603;
pub const CON_EVT_EPOCH: u32 = 9021;
pub const TASK_ACCEPTED: u32 = 401;
pub const TASK_LOADED: u32 = 404;
const PROC_TASK: u32 = 20;
/// Outliers returned (worst first).
pub const MAX_OUTLIERS: usize = 300;
/// TASK_LOADED of a WorkId whose steps start in the window may lie a little before it.
const TYPE_LOOKBACK_MS: i64 = 6 * 3_600_000;

/// `AND plc IN (?, …)` + its arguments; empty = every PLC.
pub(super) fn plc_clause(plcs: &[String], args: &mut Vec<Value>) -> String {
    if plcs.is_empty() {
        return String::new();
    }
    args.extend(plcs.iter().map(|p| Value::Text(p.clone())));
    format!(" AND plc IN ({})", vec!["?"; plcs.len()].join(","))
}

// ---------------------------------------------------------------- alarms

/// Alarm row (bit alarm rows and ErrorList rows of any level), or the console's CON_EVT_EPOCH, which ends every
/// open alarm of that PLC.
#[derive(Clone, Debug)]
pub struct AlarmEv {
    pub plc: String,
    pub epoch: i64,
    pub ts: i64,
    pub cat: u8,
    pub code: u32,
    pub src: u32,
    pub a: i64,
    pub b: i64,
}

/// Bit alarm rows name a bit of the area, ErrorList rows the ErrorList number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AlarmId {
    Bit(u32),
    Code(u32),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AlarmKey {
    pub plc: String,
    /// `alarm_area`: 1 FAULT, 2 WARN, 3 OPERATOR, 4 INFO.
    pub area: u32,
    pub id: AlarmId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndKind {
    Clear,
    Reset,
    Epoch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interval {
    pub key: AlarmKey,
    pub start: i64,
    /// `None` = still active at the end of the rows.
    pub end: Option<(i64, EndKind)>,
    epoch: i64,
    /// ErrorList clear row: B = active ms (raise to the last drop, without the PLC's off-delay).
    pub active_ms: Option<i64>,
    /// ErrorList clear row: A = drops absorbed by the off-delay while it was raised.
    pub flicker: u32,
}

/// Bit alarm rows, ErrorList raise / clear rows of every level, and epoch changes — each term on the
/// `(cat, code)` index.
pub fn alarm_rows(store: &Store, from: i64, to: i64, plcs: &[String]) -> rusqlite::Result<Vec<AlarmEv>> {
    let mut args = vec![Value::Integer(from), Value::Integer(to)];
    let el = [CAT_ALARM, CAT_OPERATOR, CAT_INFO].map(|c| format!("(cat = {c} AND code >= {} AND code < {})", TRANS_MUL, 3 * TRANS_MUL)).join(" OR ");
    let sql = format!(
        "SELECT plc, epoch, plc_ts, cat, code, src, a, b FROM events WHERE ((cat = {CAT_ALARM} AND code IN ({ALM_RAISED}, {ALM_CLEARED}, {ALM_RESET})) OR (cat = {CAT_SYS} AND code = {CON_EVT_EPOCH}) OR {el}) AND plc_ts >= ? AND plc_ts <= ?{} ORDER BY plc_ts, id",
        plc_clause(plcs, &mut args)
    );
    store.db().with(|c| {
        let mut st = c.prepare(&sql)?;
        let it = st.query_map(rusqlite::params_from_iter(args), |r| {
            Ok(AlarmEv { plc: r.get(0)?, epoch: r.get(1)?, ts: r.get(2)?, cat: r.get(3)?, code: r.get(4)?, src: r.get(5)?, a: r.get(6)?, b: r.get(7)? })
        })?;
        it.collect()
    })
}

/// Raise → clear per (plc, area, bit) for bit alarm rows and per (plc, level, ErrorList number) for ErrorList rows. A
/// reset of an area ends every open **bit** alarm of that area on that PLC (a FAULT reset clears all FAULT bits at once;
/// ErrorList rows log their own clear), a new epoch (PLC restart) every open alarm of the PLC. A second raise of an open alarm and a
/// clear without a raise in the rows are ignored. Rows must be oldest first.
pub fn pair_alarms(rows: &[AlarmEv]) -> Vec<Interval> {
    let mut out: Vec<Interval> = Vec::new();
    let mut open: HashMap<AlarmKey, usize> = HashMap::new();
    let close = |out: &mut Vec<Interval>, open: &mut HashMap<AlarmKey, usize>, ts: i64, kind: EndKind, hit: &dyn Fn(&Interval) -> bool| {
        open.retain(|_, i| {
            if hit(&out[*i]) {
                out[*i].end = Some((ts, kind));
                false
            } else {
                true
            }
        });
    };
    let raise = |out: &mut Vec<Interval>, open: &mut HashMap<AlarmKey, usize>, key: AlarmKey, r: &AlarmEv| {
        if !open.contains_key(&key) {
            open.insert(key.clone(), out.len());
            out.push(Interval { key, start: r.ts, end: None, epoch: r.epoch, active_ms: None, flicker: 0 });
        }
    };
    for r in rows {
        if r.cat == CAT_SYS {
            if r.code == CON_EVT_EPOCH {
                close(&mut out, &mut open, r.ts, EndKind::Epoch, &|iv| iv.key.plc == r.plc);
            }
            continue;
        }
        // a PLC row of a newer epoch without the console's epoch row (e.g. it was pruned) still ends the old ones
        if r.epoch > 0 {
            close(&mut out, &mut open, r.ts, EndKind::Epoch, &|iv| iv.key.plc == r.plc && iv.epoch > 0 && iv.epoch < r.epoch);
        }
        if let Some(v) = errorlist::decode(r.cat, r.src, r.code) {
            let key = AlarmKey { plc: r.plc.clone(), area: v.level.area_id(), id: AlarmId::Code(v.num) };
            match v.trans {
                Trans::Raise => raise(&mut out, &mut open, key, r),
                Trans::Clear => {
                    if let Some(i) = open.remove(&key) {
                        out[i].end = Some((r.ts, EndKind::Clear));
                        out[i].active_ms = Some(r.b.max(0));
                        out[i].flicker = u32::try_from(r.a).unwrap_or(0);
                    }
                }
                Trans::Momentary | Trans::Summary => {}
            }
            continue;
        }
        if r.cat != CAT_ALARM {
            continue;
        }
        match r.code {
            ALM_RESET => close(&mut out, &mut open, r.ts, EndKind::Reset, &|iv| iv.key.plc == r.plc && iv.key.area == r.src && matches!(iv.key.id, AlarmId::Bit(_))),
            ALM_RAISED | ALM_CLEARED => {
                let Ok(bit) = u32::try_from(r.a) else { continue };
                let key = AlarmKey { plc: r.plc.clone(), area: r.src, id: AlarmId::Bit(bit) };
                if r.code == ALM_RAISED {
                    raise(&mut out, &mut open, key, r);
                } else if let Some(i) = open.remove(&key) {
                    out[i].end = Some((r.ts, EndKind::Clear));
                }
            }
            _ => {}
        }
    }
    out
}

/// Per-alarm totals; an open interval counts up to `end` (the end of the range, or now when the range is still
/// running), an ErrorList clear by its own active time. Most frequent first.
pub fn aggregate(ivs: &[Interval], end: i64) -> Vec<(AlarmKey, AlarmAgg)> {
    let mut m: HashMap<&AlarmKey, AlarmAgg> = HashMap::new();
    for iv in ivs {
        let stop = iv.end.map(|(t, _)| t).unwrap_or(end).max(iv.start);
        let dur = iv.active_ms.unwrap_or(stop - iv.start);
        let a = m.entry(&iv.key).or_default();
        a.count += 1;
        a.total_ms += dur;
        a.max_ms = a.max_ms.max(dur);
        a.open += u32::from(iv.end.is_none());
        a.last_ts = a.last_ts.max(iv.start);
        a.flicker += iv.flicker;
    }
    let mut v: Vec<(AlarmKey, AlarmAgg)> = m.into_iter().map(|(k, a)| (k.clone(), a)).collect();
    v.sort_by(|(ka, a), (kb, b)| b.count.cmp(&a.count).then(b.total_ms.cmp(&a.total_ms)).then(ka.cmp(kb)));
    v
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AlarmAgg {
    pub count: u32,
    pub total_ms: i64,
    pub max_ms: i64,
    pub open: u32,
    pub last_ts: i64,
    pub flicker: u32,
}

fn level_of_area(area: u32) -> Level {
    match area {
        1 => Level::Alarm,
        2 => Level::Warn,
        3 => Level::Operator,
        _ => Level::Info,
    }
}

/// ErrorList type of a paired alarm (TASK-area bits are task records).
pub fn key_ty(texts: &Texts, k: &AlarmKey) -> Ty {
    match k.id {
        AlarmId::Bit(_) => match k.area {
            1 => Ty::Alarm,
            2 => Ty::Warn,
            3 => Ty::Operator,
            _ => Ty::Task,
        },
        AlarmId::Code(n) => {
            let level = level_of_area(k.area);
            texts.renderer(&k.plc).errorlist().get(level, n).map(|e| e.ty()).unwrap_or(match level {
                Level::Alarm => Ty::Alarm,
                Level::Warn => Ty::Warn,
                Level::Operator => Ty::Operator,
                Level::Info => Ty::Info,
            })
        }
    }
}

/// How an alarm reads in the event list: (area name, code, `F0501`, text, what selects its rows in the list).
/// Bits: the exact rendered `{alarm}` text (a text search); ErrorList codes: the code itself (a code filter).
pub fn alarm_label(texts: &Texts, plc: &str, area: u32, id: AlarmId) -> (String, u32, String, String, String) {
    let r = texts.renderer(plc);
    let area_name = r.enum_label("alarm_area", i64::from(area)).unwrap_or("area?").to_string();
    let bit = match id {
        AlarmId::Code(n) => {
            let level = level_of_area(area);
            let code = errorlist::label(level, n);
            let text = r.errorlist().get(level, n).map(|e| e.text(false).to_string()).unwrap_or_default();
            return (level.area().to_string(), n, code.clone(), text, code);
        }
        AlarmId::Bit(b) => b,
    };
    match r.alarm(area, bit) {
        Some(a) => {
            let text = if a.text_ko.is_empty() { a.text_en.clone() } else { a.text_ko.clone() };
            let head = if a.code == 0 { area_name.clone() } else { format!("{}{:04}", area_name.chars().next().unwrap_or('?'), a.code) };
            let full = format!("{head} {text}").trim_end().to_string();
            (area_name, a.code, head, text, full)
        }
        None => {
            let full = format!("{area_name} bit {bit} ({}.{})", bit / 8, bit % 8);
            (area_name.clone(), 0, format!("{area_name} bit {bit}"), String::new(), full)
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct AlarmStat {
    pub plc: String,
    #[serde(rename = "type")]
    pub ty: &'static str,
    pub area: u32,
    pub area_name: String,
    /// Bit alarm rows only (ErrorList rows name the code).
    pub bit: Option<u32>,
    pub code: u32,
    pub label: String,
    pub text: String,
    pub count: u32,
    pub total_ms: i64,
    pub mean_ms: i64,
    pub max_ms: i64,
    /// Occurrences still active at the end of the range.
    pub open: u32,
    /// Drops the PLC's off-delay absorbed (ErrorList clear rows).
    pub flicker: u32,
    pub last_ts: i64,
    pub last: String,
    /// Selects this alarm's rows in `/api/events`: a text search (`q`), or the ErrorList code (`code`) when `by_code`.
    pub search: String,
    pub by_code: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlcTotal {
    pub plc: String,
    pub alarms: u32,
    pub count: u32,
    pub total_ms: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct AlarmStats {
    pub from: i64,
    pub to: i64,
    pub count: u32,
    pub total_ms: i64,
    pub by_plc: Vec<PlcTotal>,
    pub rows: Vec<AlarmStat>,
}

/// Types counted when a request names none.
pub const DEFAULT_TYPES: [Ty; 2] = [Ty::Alarm, Ty::Warn];

pub fn alarm_stats(texts: &Texts, rows: &[AlarmEv], from: i64, to: i64, now: i64, types: &[Ty]) -> AlarmStats {
    let types = if types.is_empty() { &DEFAULT_TYPES[..] } else { types };
    let aggs = aggregate(&pair_alarms(rows), to.min(now));
    let mut by_plc: BTreeMap<String, PlcTotal> = BTreeMap::new();
    let rows: Vec<AlarmStat> = aggs
        .into_iter()
        .filter_map(|(k, a)| {
            let ty = key_ty(texts, &k);
            if !types.contains(&ty) {
                return None;
            }
            let p = by_plc.entry(k.plc.clone()).or_insert_with(|| PlcTotal { plc: k.plc.clone(), alarms: 0, count: 0, total_ms: 0 });
            p.alarms += 1;
            p.count += a.count;
            p.total_ms += a.total_ms;
            let (area_name, code, label, text, search) = alarm_label(texts, &k.plc, k.area, k.id);
            Some(AlarmStat {
                ty: ty.name(),
                area_name,
                code,
                label,
                text,
                search,
                by_code: matches!(k.id, AlarmId::Code(_)),
                count: a.count,
                total_ms: a.total_ms,
                mean_ms: a.total_ms / i64::from(a.count.max(1)),
                max_ms: a.max_ms,
                open: a.open,
                flicker: a.flicker,
                last_ts: a.last_ts,
                last: texts.fmt_ms(a.last_ts),
                plc: k.plc,
                area: k.area,
                bit: match k.id {
                    AlarmId::Bit(b) => Some(b),
                    AlarmId::Code(_) => None,
                },
            })
        })
        .collect();
    AlarmStats { from, to, count: rows.iter().map(|r| r.count).sum(), total_ms: rows.iter().map(|r| r.total_ms).sum(), by_plc: by_plc.into_values().collect(), rows }
}

// ---------------------------------------------------------------- steps

/// STEP row: `step` = the step left (B), `dwell` = ms spent in it (A), `proc` = Src, `ctx` = WorkId.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepSample {
    pub id: i64,
    pub plc: String,
    pub ts: i64,
    pub proc: u32,
    pub step: i64,
    pub dwell: i64,
    pub ctx: u32,
}

/// Step 0 (idle between runs) is not a step and is left out.
pub fn step_rows(store: &Store, from: i64, to: i64, plcs: &[String], proc: Option<u32>) -> rusqlite::Result<Vec<StepSample>> {
    let mut args = vec![Value::Integer(from), Value::Integer(to)];
    let mut sql = format!("SELECT id, plc, plc_ts, src, b, a, ctx FROM events WHERE +cat = {CAT_STEP} AND plc_ts >= ? AND plc_ts <= ? AND b <> 0 AND a >= 0{}", plc_clause(plcs, &mut args));
    if let Some(p) = proc {
        sql.push_str(" AND src = ?");
        args.push(Value::Integer(i64::from(p)));
    }
    store.db().with(|c| {
        let mut st = c.prepare(&sql)?;
        let it =
            st.query_map(rusqlite::params_from_iter(args), |r| Ok(StepSample { id: r.get(0)?, plc: r.get(1)?, ts: r.get(2)?, proc: r.get(3)?, step: r.get(4)?, dwell: r.get(5)?, ctx: r.get(6)? }))?;
        it.collect()
    })
}

/// ErrorList task rows with the TASK_ACCEPTED / TASK_LOADED payload (Src = task type, A = Cell, B = WorkId).
pub const EL_TASK_ACCEPTED: u32 = 3 * TRANS_MUL + 301;
pub const EL_TASK_STARTED: u32 = 3 * TRANS_MUL + 302;
pub const EL_TASK_COMPLETED: u32 = 3 * TRANS_MUL + 303;

/// (plc, WorkId) → task type, from TASK_ACCEPTED / TASK_LOADED (Src = task type, B = WorkId) and their ErrorList rows
/// I0301 / I0302 — `task` = `Texts::type_cond(&[Ty::Task])` keeps a PLC whose I0301 is something else (GRM) out.
pub fn task_types(store: &Store, from: i64, to: i64, plcs: &[String], task: Option<&(String, Vec<Value>)>) -> rusqlite::Result<HashMap<(String, u32), u32>> {
    let mut args = Vec::new();
    let el = match task {
        Some((cond, a)) => {
            args.extend(a.iter().cloned());
            format!(" OR (cat = {CAT_INFO} AND code IN ({EL_TASK_ACCEPTED}, {EL_TASK_STARTED}) AND {cond})")
        }
        None => String::new(),
    };
    args.extend([Value::Integer(from - TYPE_LOOKBACK_MS), Value::Integer(to)]);
    let sql = format!(
        "SELECT plc, src, b FROM events WHERE ((cat = {CAT_TASK} AND code IN ({TASK_ACCEPTED}, {TASK_LOADED})){el}) AND plc_ts >= ? AND plc_ts <= ?{} ORDER BY plc_ts",
        plc_clause(plcs, &mut args)
    );
    store.db().with(|c| {
        let mut st = c.prepare(&sql)?;
        let it = st.query_map(rusqlite::params_from_iter(args), |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?, r.get::<_, i64>(2)?)))?;
        let mut m = HashMap::new();
        for x in it {
            let (plc, ty, work) = x?;
            if let Ok(w) = u32::try_from(work) {
                m.insert((plc, w), ty);
            }
        }
        Ok(m)
    })
}

/// Nearest-rank percentile of an ascending slice (0 when empty).
pub fn percentile(sorted: &[i64], p: f64) -> i64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// A dwell above this is an outlier: 1.5 × p95, but at least 2 s over it (short steps jitter by more than 50 %).
pub fn outlier_limit(p95: i64) -> i64 {
    ((p95 as f64 * 1.5).ceil() as i64).max(p95 + 2000)
}

#[derive(Clone, Debug, Serialize)]
pub struct StepStat {
    pub plc: String,
    pub proc: u32,
    pub proc_name: String,
    pub step: i64,
    pub step_name: String,
    pub task_type: Option<u32>,
    pub task_type_name: Option<String>,
    pub count: u32,
    pub mean_ms: i64,
    pub p50_ms: i64,
    pub p95_ms: i64,
    pub max_ms: i64,
    pub outliers: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct StepOutlier {
    pub id: i64,
    pub ts: String,
    pub ts_ms: i64,
    pub plc: String,
    pub proc: u32,
    pub proc_name: String,
    pub step: i64,
    pub step_name: String,
    pub task_type: Option<u32>,
    pub dwell_ms: i64,
    pub p95_ms: i64,
    pub limit_ms: i64,
    /// WorkId.
    pub ctx: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct StepStats {
    pub from: i64,
    pub to: i64,
    pub samples: u32,
    pub rows: Vec<StepStat>,
    /// Worst first (dwell / p95), at most [`MAX_OUTLIERS`].
    pub outliers: Vec<StepOutlier>,
    pub outliers_total: u32,
}

type StepKey = (String, u32, i64, Option<u32>);

/// Groups by (plc, proc, step[, task type]) — `types` splits by the task type of the WorkId (unknown → `None`).
pub fn step_stats(texts: &Texts, samples: Vec<StepSample>, types: Option<&HashMap<(String, u32), u32>>, from: i64, to: i64) -> StepStats {
    let total = samples.len() as u32;
    let mut groups: BTreeMap<StepKey, Vec<StepSample>> = BTreeMap::new();
    for s in samples {
        let ty = types.map(|m| m.get(&(s.plc.clone(), s.ctx)).copied());
        groups.entry((s.plc.clone(), s.proc, s.step, ty.flatten())).or_default().push(s);
    }
    let mut rows = Vec::with_capacity(groups.len());
    let mut outliers = Vec::new();
    for ((plc, proc, step, ty), mut v) in groups {
        v.sort_by_key(|s| s.dwell);
        let d: Vec<i64> = v.iter().map(|s| s.dwell).collect();
        let (p50, p95) = (percentile(&d, 50.0), percentile(&d, 95.0));
        let limit = outlier_limit(p95);
        let r = texts.renderer(&plc);
        let proc_name = r.enum_label("proc", i64::from(proc)).map(str::to_string).unwrap_or_else(|| proc.to_string());
        let step_name = if proc == PROC_TASK { r.enum_label("task_step", step).map(str::to_string) } else { None }.unwrap_or_else(|| step.to_string());
        let task_type_name = ty.map(|t| r.enum_label("task_type", i64::from(t)).map(str::to_string).unwrap_or_else(|| t.to_string()));
        let mut n_out = 0;
        for s in v.iter().rev().take_while(|s| s.dwell > limit) {
            n_out += 1;
            outliers.push(StepOutlier {
                id: s.id,
                ts: texts.fmt_ms(s.ts),
                ts_ms: s.ts,
                plc: plc.clone(),
                proc,
                proc_name: proc_name.clone(),
                step,
                step_name: step_name.clone(),
                task_type: ty,
                dwell_ms: s.dwell,
                p95_ms: p95,
                limit_ms: limit,
                ctx: s.ctx,
            });
        }
        rows.push(StepStat {
            plc,
            proc,
            proc_name,
            step,
            step_name,
            task_type: ty,
            task_type_name,
            count: d.len() as u32,
            mean_ms: d.iter().sum::<i64>() / d.len().max(1) as i64,
            p50_ms: p50,
            p95_ms: p95,
            max_ms: d.last().copied().unwrap_or(0),
            outliers: n_out,
        });
    }
    let outliers_total = outliers.len() as u32;
    outliers.sort_by(|a, b| (b.dwell_ms as f64 / b.p95_ms.max(1) as f64).total_cmp(&(a.dwell_ms as f64 / a.p95_ms.max(1) as f64)).then(b.ts_ms.cmp(&a.ts_ms)));
    outliers.truncate(MAX_OUTLIERS);
    StepStats { from, to, samples: total, rows, outliers, outliers_total }
}

// ---------------------------------------------------------------- routes

#[derive(Deserialize, Default)]
pub struct StatsQ {
    pub from: Option<String>,
    pub to: Option<String>,
    pub plc: Option<String>,
    proc: Option<String>,
    split: Option<String>,
    /// Alarm statistics: ErrorList types (default Alarm, Warn).
    #[serde(rename = "type")]
    pub ty: Option<String>,
}

/// Local midnight of the day `ms` falls on.
pub fn local_midnight(texts: &Texts, ms: i64) -> i64 {
    let day = texts.fmt_ms(ms);
    texts.parse_time(&format!("{} 00:00", &day[..10.min(day.len())])).unwrap_or(ms)
}

/// `from` / `to` of a stats request: default today (local midnight → now).
pub fn range(texts: &Texts, from: &Option<String>, to: &Option<String>) -> Result<(i64, i64), ApiError> {
    let now = now_ms();
    let t = |s: &Option<String>, what: &str| -> Result<Option<i64>, ApiError> {
        opt(s).map(|v| texts.parse_time(v).ok_or_else(|| ApiError::BadRequest(format!("{what}: {v} 시각을 읽을 수 없습니다")))).transpose()
    };
    let to = t(to, "to")?.unwrap_or(now);
    let from = t(from, "from")?.unwrap_or_else(|| local_midnight(texts, now));
    if from > to {
        return Err(ApiError::BadRequest("from 이 to 보다 뒤입니다".into()));
    }
    Ok((from, to))
}

async fn alarms(State(st): State<AppState>, Query(q): Query<StatsQ>) -> ApiResult<AlarmStats> {
    let log = log(&st)?.clone();
    let (from, to) = range(&log.texts, &q.from, &q.to)?;
    let plcs: Vec<String> = list(&q.plc).into_iter().map(str::to_string).collect();
    let tys = types(&q.ty)?;
    let out = tokio::task::spawn_blocking(move || -> Result<AlarmStats, ApiError> {
        let rows = alarm_rows(&log.store, from, to, &plcs)?;
        Ok(alarm_stats(&log.texts, &rows, from, to, now_ms(), &tys))
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))??;
    Ok(axum::Json(out))
}

async fn steps(State(st): State<AppState>, Query(q): Query<StatsQ>) -> ApiResult<StepStats> {
    let log = log(&st)?.clone();
    let (from, to) = range(&log.texts, &q.from, &q.to)?;
    let plcs: Vec<String> = list(&q.plc).into_iter().map(str::to_string).collect();
    let proc: Option<u32> = num(&q.proc, "proc")?;
    let split = opt(&q.split).is_some_and(|s| s == "type" || s == "task_type");
    let out = tokio::task::spawn_blocking(move || -> Result<StepStats, ApiError> {
        let samples = step_rows(&log.store, from, to, &plcs, proc)?;
        let types = if split { Some(task_types(&log.store, from, to, &plcs, log.texts.type_cond(&[Ty::Task]).as_ref())?) } else { None };
        Ok(step_stats(&log.texts, samples, types.as_ref(), from, to))
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))??;
    Ok(axum::Json(out))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/events/stats/alarms", get(alarms)).route("/api/events/stats/steps", get(steps))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::evtlog::catalog::PlcTexts;
    use crate::evtlog::store::{Origin, Row};
    use evt_catalog::{AlarmEntry, AlarmTable, Catalog, ErrorList};
    use std::sync::Arc;

    pub(crate) fn texts() -> Texts {
        let cat = Arc::new(crate::evtlog::catalog::embedded());
        let alarms = AlarmTable {
            entries: vec![
                AlarmEntry { area: "FAULT".into(), bit: 593, code: 3118, class: "Fault".into(), text_en: "Z Axis - Station Interlock Complete Timeout".into(), text_ko: String::new() },
                AlarmEntry { area: "WARN".into(), bit: 320, code: 1101, class: "Warning".into(), text_en: "Station 01 - Measuring Error".into(), text_ko: "스테이션 01 - 측정 오류".into() },
            ],
        };
        let consts: HashMap<String, i64> = [("CMD_TASK_PICK".to_string(), 1), ("CMD_TASK_DROP".to_string(), 2)].into_iter().collect();
        let empty = HashMap::new();
        let mut gr2 = PlcTexts::new("GR2", &consts, alarms.clone());
        gr2.errorlist = errorlist_fixture(false);
        let mut grm = PlcTexts::new("GRM", &empty, alarms);
        grm.errorlist = errorlist_fixture(true);
        let mut t = Texts::new(cat, vec![gr2, grm], "[gr.I0301]\nsrc = \"enum:task_type\"\nsrc_label = \"Type\"\na_label = \"Cell\"\n");
        t.offset = time::UtcOffset::from_hms(9, 0, 0).unwrap();
        t
    }

    /// A few ErrorList rows: GR2 F3118 / W1101 / O0101 (CMD_JOG Src 1) / I0301 (TASK) / I5101 (STEP_CHANGED);
    /// GRM W1101 (Station 01) / I0301 (heartbeat, INFO class — the same number as GR's task row).
    pub(crate) fn errorlist_fixture(grm: bool) -> ErrorList {
        let e = |code: &str, class: &str, en: &str, ko: &str, a: &str, catalog: Option<(&str, Option<u32>)>| {
            let (level, num) = evt_catalog::errorlist::parse_label(code).unwrap();
            evt_catalog::ElEntry {
                level: Some(level),
                code: code.into(),
                num,
                hmi_class: class.into(),
                text_en: en.into(),
                text_ko: ko.into(),
                a_meaning: a.into(),
                catalog: catalog.map(|c| c.0.to_string()),
                catalog_src: catalog.and_then(|c| c.1),
                ..Default::default()
            }
        };
        if grm {
            return ErrorList::new(vec![
                e("W1101", "WARN", "Station 01 - Measuring Error", "스테이션 01 - 측정 오류", "", None),
                e("I0301", "INFO", "Robot Heartbeat Stopped", "로봇 heartbeat 멈춤", "멈춘 시간 ms", None),
            ]);
        }
        ErrorList::new(vec![
            e("F3118", "FAULT", "Z Axis - Station Interlock Complete Timeout", "Z축 - 스테이션 인터록 완료 시간 초과", "", None),
            e("W1101", "WARN", "X Axis - Lag Error", "X축 - Lag 오류", "", None),
            e("O0101", "OPERATOR", "X Axis - Jog Speed Forward", "X 축 Jog Speed 전진", "시작 위치 (mm×10)", Some(("CMD_JOG", Some(1)))),
            evt_catalog::ElEntry { b_meaning: "WorkId".into(), ..e("I0301", "TASK", "Task - Accepted to Buffer", "작업 수락 (Buff)", "Cell.Id (Src = task type)", Some(("TASK_ACCEPTED", None))) },
            e("I5101", "INFO", "Step - Changed", "Step 변경", "", Some(("STEP_CHANGED", None))),
        ])
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn raw(plc: &str, epoch: i64, ts: i64, cat: u8, lvl: u8, code: u32, src: u32, a: i64, b: i64, ctx: u32) -> Row {
        Row { plc: plc.into(), epoch, seq: None, plc_ts: ts, rx_ts: ts, cat, lvl, src, code, a, b, ctx, origin: if epoch > 0 { Origin::Plc } else { Origin::Console }, detail: None }
    }

    fn ev(plc: &str, epoch: i64, ts: i64, code: u32, src: u32, a: i64) -> AlarmEv {
        AlarmEv { plc: plc.into(), epoch, ts, cat: if code == CON_EVT_EPOCH { CAT_SYS } else { CAT_ALARM }, code, src, a, b: 0 }
    }

    /// ErrorList row: (level, trans, number) → (cat, src, code), A = flicker / value, B = active ms / value.
    fn el(plc: &str, ts: i64, level: Level, trans: Trans, num: u32, a: i64, b: i64) -> AlarmEv {
        let (cat, src, code) = evt_catalog::errorlist::encode(level, trans, num);
        AlarmEv { plc: plc.into(), epoch: 1, ts, cat, code, src: src.unwrap_or(0), a, b }
    }

    fn bit(k: &AlarmKey) -> u32 {
        match k.id {
            AlarmId::Bit(b) => b,
            AlarmId::Code(n) => 100_000 + n,
        }
    }

    #[test]
    fn codes_match_the_catalog() {
        let c: Catalog = crate::evtlog::catalog::embedded();
        for (name, cat, code) in [
            ("ALM_RAISED", CAT_ALARM, ALM_RAISED),
            ("ALM_CLEARED", CAT_ALARM, ALM_CLEARED),
            ("ALM_RESET", CAT_ALARM, ALM_RESET),
            ("CON_EVT_EPOCH", CAT_SYS, CON_EVT_EPOCH),
            ("TASK_ACCEPTED", CAT_TASK, TASK_ACCEPTED),
            ("TASK_LOADED", CAT_TASK, TASK_LOADED),
        ] {
            let e = c.event_by_name(name).unwrap();
            assert_eq!((e.cat, e.code.map(u32::from)), (cat, Some(code)), "{name}");
        }
        assert_eq!(c.cat_id("STEP"), Some(CAT_STEP));
    }

    #[test]
    fn pairs_raise_clear_reset_epoch_and_leaves_the_rest_open() {
        let rows = vec![
            ev("GR2", 1, 100, ALM_RAISED, 2, 320),
            ev("GR2", 1, 110, ALM_RAISED, 2, 320), // repeated raise: ignored
            ev("GR2", 1, 150, ALM_CLEARED, 2, 320),
            ev("GR2", 1, 160, ALM_CLEARED, 2, 320), // clear without raise: ignored
            ev("GR2", 1, 200, ALM_RAISED, 1, 593),
            ev("GR2", 1, 210, ALM_RAISED, 1, 594),
            ev("GR2", 1, 220, ALM_RAISED, 2, 320),
            ev("GRM", 1, 230, ALM_RAISED, 1, 593),
            ev("GR2", 1, 300, ALM_RESET, 1, 2),     // FAULT reset: both GR2 FAULT bits, not the WARN, not GRM
            ev("GR2", 0, 400, CON_EVT_EPOCH, 0, 5), // GR2 restarts: the WARN ends
            ev("GRM", 1, 500, ALM_RAISED, 2, 1),
            ev("GRM", 2, 600, ALM_CLEARED, 2, 1), // newer epoch without the console row: both GRM alarms end at 600
            ev("GRM", 2, 700, ALM_RAISED, 1, 8),
        ];
        let ivs = pair_alarms(&rows);
        type Got<'a> = (&'a str, u32, u32, i64, Option<(i64, EndKind)>);
        let got: Vec<Got> = ivs.iter().map(|i| (i.key.plc.as_str(), i.key.area, bit(&i.key), i.start, i.end)).collect();
        assert_eq!(
            got,
            vec![
                ("GR2", 2, 320, 100, Some((150, EndKind::Clear))),
                ("GR2", 1, 593, 200, Some((300, EndKind::Reset))),
                ("GR2", 1, 594, 210, Some((300, EndKind::Reset))),
                ("GR2", 2, 320, 220, Some((400, EndKind::Epoch))),
                ("GRM", 1, 593, 230, Some((600, EndKind::Epoch))),
                ("GRM", 2, 1, 500, Some((600, EndKind::Epoch))),
                ("GRM", 1, 8, 700, None),
            ]
        );
        let agg = aggregate(&ivs, 1000);
        let warn = agg.iter().find(|(k, _)| k.plc == "GR2" && bit(k) == 320).unwrap().1;
        assert_eq!(warn, AlarmAgg { count: 2, total_ms: 50 + 180, max_ms: 180, open: 0, last_ts: 220, flicker: 0 });
        let open = agg.iter().find(|(k, _)| k.plc == "GRM" && bit(k) == 8).unwrap().1;
        assert_eq!((open.count, open.total_ms, open.open), (1, 300, 1), "open until the end of the range");
        assert_eq!(bit(&agg[0].0), 320, "most frequent first");
    }

    #[test]
    fn errorlist_rows_pair_by_code_with_active_ms_and_flicker() {
        let rows = vec![
            el("GR2", 100, Level::Warn, Trans::Raise, 1101, 0, 0),
            el("GR2", 150, Level::Alarm, Trans::Raise, 1101, 0, 0),      // same number, other level: its own alarm
            el("GR2", 160, Level::Warn, Trans::Raise, 1101, 0, 0),       // repeated raise: ignored
            ev("GR2", 1, 200, ALM_RESET, 2, 1),                          // a reset does not end ErrorList alarms (they log a clear)
            el("GR2", 2_300, Level::Warn, Trans::Clear, 1101, 3, 1_700), // T_off 2 s after the last drop: B = active ms
            el("GR2", 2_400, Level::Warn, Trans::Clear, 1101, 0, 50),    // clear without raise: ignored
            el("GR2", 2_500, Level::Operator, Trans::Raise, 101, 5984, 0),
            el("GR2", 3_000, Level::Operator, Trans::Clear, 101, 0, 500),
            el("GR2", 3_100, Level::Info, Trans::Momentary, 301, 101, 7), // momentary: not an interval
            ev("GR2", 1, 3_200, ALM_RAISED, 2, 320),                      // a bit alarm row next to ErrorList rows
            el("GR2", 3_300, Level::Warn, Trans::Summary, 1101, 12, 0),
        ];
        let ivs = pair_alarms(&rows);
        type Got = (u32, AlarmId, i64, Option<i64>, Option<i64>, u32);
        let got: Vec<Got> = ivs.iter().map(|i| (i.key.area, i.key.id, i.start, i.end.map(|e| e.0), i.active_ms, i.flicker)).collect();
        assert_eq!(
            got,
            vec![
                (2, AlarmId::Code(1101), 100, Some(2_300), Some(1_700), 3),
                (1, AlarmId::Code(1101), 150, None, None, 0),
                (3, AlarmId::Code(101), 2_500, Some(3_000), Some(500), 0),
                (2, AlarmId::Bit(320), 3_200, None, None, 0),
            ]
        );
        let agg = aggregate(&ivs, 4_000);
        let w = agg.iter().find(|(k, _)| k.id == AlarmId::Code(1101) && k.area == 2).unwrap().1;
        assert_eq!((w.total_ms, w.flicker), (1_700, 3), "the clear row's active time, not raise → clear");
        // statistics: default Alarm + Warn; the Operator state item only when asked for; labels = ErrorList
        let t = texts();
        let s = alarm_stats(&t, &rows, 0, 4_000, 10_000, &[]);
        let labels: Vec<(&str, &str, bool, Option<u32>)> = s.rows.iter().map(|r| (r.label.as_str(), r.ty, r.by_code, r.bit)).collect();
        assert_eq!(labels, vec![("F1101", "Alarm", true, None), ("W1101", "Warn", true, None), ("W1101", "Warn", false, Some(320))], "the open F1101 is the longest");
        assert_eq!((s.rows[1].text.as_str(), s.rows[1].search.as_str()), ("X축 - Lag 오류", "W1101"), "ErrorList rows are found by code");
        let op = alarm_stats(&t, &rows, 0, 4_000, 10_000, &[Ty::Operator]);
        assert_eq!(op.rows.iter().map(|r| (r.label.as_str(), r.total_ms)).collect::<Vec<_>>(), vec![("O0101", 500)]);
    }

    #[test]
    fn alarm_stats_label_like_the_renderer_and_select_their_rows() {
        let t = texts();
        let rows = vec![ev("GR2", 1, 0, ALM_RAISED, 2, 320), ev("GR2", 1, 4_000, ALM_CLEARED, 2, 320), ev("GR2", 1, 5_000, ALM_RAISED, 1, 7), ev("GRM", 1, 6_000, ALM_RAISED, 1, 593)];
        let s = alarm_stats(&t, &rows, 0, 10_000, 8_000, &[]);
        assert_eq!((s.count, s.total_ms), (3, 4_000 + 3_000 + 2_000), "open ones count up to now inside a running range");
        let w = s.rows.iter().find(|r| r.bit == Some(320)).unwrap();
        assert_eq!((w.label.as_str(), w.code, w.text.as_str(), w.area_name.as_str()), ("W1101", 1101, "스테이션 01 - 측정 오류", "WARN"));
        // the search is exactly what the renderer writes for the ALARM rows
        let rendered = t.text(&raw("GR2", 1, 0, CAT_ALARM, 3, ALM_RAISED, 2, 320, 0, 0));
        assert!(rendered.contains(&w.search), "{rendered} / {}", w.search);
        let unknown = s.rows.iter().find(|r| r.bit == Some(7)).unwrap();
        assert!(t.text(&raw("GR2", 1, 0, CAT_ALARM, 4, ALM_RAISED, 1, 7, 0, 0)).contains(&unknown.search));
        assert_eq!(s.by_plc.iter().map(|p| (p.plc.as_str(), p.alarms, p.count)).collect::<Vec<_>>(), vec![("GR2", 2, 2), ("GRM", 1, 1)]);
    }

    #[test]
    fn percentiles_and_outliers() {
        let v: Vec<i64> = (1..=100).collect();
        assert_eq!((percentile(&v, 50.0), percentile(&v, 95.0), percentile(&v, 100.0)), (50, 95, 100));
        assert_eq!(percentile(&[7], 95.0), 7);
        assert_eq!(percentile(&[], 50.0), 0);
        assert_eq!(outlier_limit(1000), 3000, "short steps: p95 + 2 s");
        assert_eq!(outlier_limit(10_000), 15_000, "long steps: 1.5 × p95");

        let t = texts();
        let mut samples: Vec<StepSample> = (0..40).map(|i| StepSample { id: i, plc: "GR2".into(), ts: 1000 + i, proc: 20, step: 300, dwell: 900 + (i % 10) * 10, ctx: 500 + i as u32 }).collect();
        samples.push(StepSample { id: 99, plc: "GR2".into(), ts: 5000, proc: 20, step: 300, dwell: 9_000, ctx: 777 });
        samples.push(StepSample { id: 100, plc: "GR2".into(), ts: 6000, proc: 20, step: 400, dwell: 500, ctx: 777 });
        let types: HashMap<(String, u32), u32> = [(("GR2".to_string(), 777), 2)].into_iter().collect();
        let s = step_stats(&t, samples.clone(), None, 0, 10_000);
        assert_eq!(s.samples, 42);
        let travel = s.rows.iter().find(|r| r.step == 300).unwrap();
        assert_eq!((travel.count, travel.p50_ms, travel.p95_ms, travel.max_ms, travel.outliers), (41, 950, 990, 9_000, 1));
        assert_eq!((travel.step_name.as_str(), travel.proc_name.as_str()), ("Travel", "Task"));
        assert_eq!(s.outliers.len(), 1);
        assert_eq!((s.outliers[0].ctx, s.outliers[0].dwell_ms, s.outliers[0].limit_ms), (777, 9_000, 2_990));
        // split by the WorkId's task type
        let s = step_stats(&t, samples, Some(&types), 0, 10_000);
        let keys: Vec<(i64, Option<u32>, u32)> = s.rows.iter().map(|r| (r.step, r.task_type, r.count)).collect();
        assert_eq!(keys, vec![(300, None, 40), (300, Some(2), 1), (400, Some(2), 1)]);
        assert_eq!(s.rows[1].task_type_name.as_deref(), Some("DROP"));
    }

    #[test]
    fn step_and_alarm_rows_come_from_sql_with_indexes() {
        let store = Store::memory();
        let rows = vec![
            raw("GR2", 1, 100, CAT_TASK, 2, TASK_LOADED, 1, 101, 555, 555),
            raw("GR2", 1, 200, CAT_STEP, 2, 200, 20, 0, 0, 555),
            raw("GR2", 1, 900, CAT_STEP, 2, 300, 20, 700, 200, 555),
            raw("GRM", 1, 950, CAT_STEP, 2, 300, 140, 50, 10, 0),
            raw("GR2", 1, 1000, CAT_ALARM, 3, ALM_RAISED, 2, 320, 0, 0),
            raw("GR2", 0, 1100, CAT_SYS, 2, CON_EVT_EPOCH, 0, 2, 0, 0),
        ];
        store.write(rows, &[]).unwrap();
        let s = step_rows(&store, 0, 5000, &[], None).unwrap();
        assert_eq!(s.len(), 2, "step 0 (idle) left out");
        assert_eq!(step_rows(&store, 0, 5000, &["GR2".into()], Some(20)).unwrap().len(), 1);
        assert_eq!(task_types(&store, 0, 5000, &[], None).unwrap().get(&("GR2".to_string(), 555)), Some(&1));
        // ErrorList rows: I0301 of GR2 is a task row, GRM's I0301 (an Info item there) is not
        let t = texts();
        store.write(vec![raw("GR2", 1, 150, CAT_INFO, 2, EL_TASK_ACCEPTED, 2, 101, 556, 556), raw("GRM", 1, 160, CAT_INFO, 2, EL_TASK_ACCEPTED, 1, 3000, 9, 0)], &[]).unwrap();
        let m = task_types(&store, 0, 5000, &[], t.type_cond(&[Ty::Task]).as_ref()).unwrap();
        assert_eq!((m.get(&("GR2".to_string(), 556)), m.get(&("GRM".to_string(), 9))), (Some(&2), None));
        let a = alarm_rows(&store, 0, 5000, &[]).unwrap();
        assert_eq!(a.iter().map(|r| r.code).collect::<Vec<_>>(), vec![ALM_RAISED, CON_EVT_EPOCH]);
        // the planner uses an index for each of them (no full scan of events)
        let plan = |sql: &str| -> String {
            store
                .db()
                .with(|c| {
                    let mut st = c.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
                    let v: Vec<String> = st.query_map([], |r| r.get::<_, String>(3))?.collect::<Result<_, _>>()?;
                    Ok(v.join(" | "))
                })
                .unwrap()
        };
        for sql in [
            "SELECT plc FROM events WHERE ((cat = 6 AND code IN (601, 602, 603)) OR (cat = 1 AND code = 9021)) AND plc_ts >= 0 AND plc_ts <= 1",
            "SELECT plc FROM events WHERE +cat = 3 AND plc_ts >= 0 AND plc_ts <= 1 AND b <> 0 AND a >= 0",
            "SELECT plc FROM events WHERE +cat = 3 AND plc_ts >= 0 AND plc_ts <= 1 AND b <> 0 AND a >= 0 AND plc IN ('GR2')",
            "SELECT plc FROM events WHERE cat = 4 AND code IN (401, 404) AND plc_ts >= 0 AND plc_ts <= 1",
        ] {
            let p = plan(sql);
            assert!(p.contains("USING INDEX") || p.contains("USING COVERING INDEX"), "{sql}\n→ {p}");
        }
    }
}
