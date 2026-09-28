//! Station interlock swimlane (`/api/events/swimlane`): the robots' PI / PO while their ILK_TARGET is the station,
//! the GRM conveyor in / out bits of its slot, and point markers (CVNO reason, measurement, tracking, interlock
//! timeout, station alarms).
//!
//! A lane starts with the state at `from`, taken from the last row before it, so the window does not open blank.
//! `v = null` means "not this station" (robot lanes) or "no row yet".

use axum::Router;
use axum::extract::{Query, State};
use axum::routing::get;
use serde::{Deserialize, Serialize};

use super::EvtLog;
use super::catalog::{EventRow, Texts, now_ms};
use super::routes::{log, num, opt};
use super::stats::{ALM_CLEARED, ALM_RAISED, CAT_ALARM};
use super::store::{Cursor, Filter, Row};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use evt_catalog::Trans;
use evt_catalog::errorlist::{TRANS_MUL, decode_v2};

pub const CAT_ILOCK: u8 = 12;
pub const ILK_TARGET: u32 = 1201;
pub const ILK_PI: u32 = 1202;
pub const ILK_PO: u32 = 1203;
pub const ILK_TIMEOUT: u32 = 1204;
pub const CAT_STATION: u8 = 16;
pub const ST_CV_IN: u32 = 1603;
pub const ST_CV_OUT: u32 = 1604;
pub const ST_CVNO_REASON: u32 = 1605;
pub const ST_MEAS: u32 = 1606;
pub const ST_TRACKING: u32 = 1609;

/// `enum.pi_bits` (robot PI and GRM CV→GRM).
const PI_BITS: [(&str, u32); 4] = [("CVOK", 0), ("Req", 1), ("MeasReq", 2), ("ItemExist", 3)];
/// `enum.po_bits`.
const PO_BITS: [(&str, u32); 2] = [("CVNO", 0), ("Comp", 1)];
/// `enum.cv_out` (GRM→CV).
const CV_OUT_BITS: [(&str, u32); 4] = [("CVNO", 0), ("Comp", 1), ("MeasComp", 2), ("MeasErr", 3)];
const MAX_ROWS: usize = 20_000;
const MAX_WINDOW_MS: i64 = 24 * 3_600_000;
const DEFAULT_WINDOW_MS: i64 = 10 * 60_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Point {
    pub t: i64,
    pub v: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Lane {
    pub key: String,
    /// Robot name or the GRM PLC name.
    pub group: String,
    pub label: String,
    pub points: Vec<Point>,
}

impl Lane {
    fn new(key: String, group: &str, label: String) -> Lane {
        Lane { key, group: group.to_string(), label, points: Vec::new() }
    }
    fn push(&mut self, t: i64, v: Option<u8>) {
        if self.points.last().is_none_or(|p| p.v != v) {
            self.points.push(Point { t, v });
        }
    }
}

/// While a robot's ILK_TARGET is this station.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Span {
    pub group: String,
    pub from: i64,
    pub to: i64,
}

pub struct RobotIn {
    pub name: String,
    pub plc: String,
    /// State at `from` (last ILK_TARGET / ILK_PI / ILK_PO before it).
    pub target: Option<i64>,
    pub pi: Option<i64>,
    pub po: Option<i64>,
    /// ILOCK + ALARM rows in the window, oldest first.
    pub rows: Vec<(i64, Row)>,
}

pub struct GrmIn {
    pub plc: String,
    pub cv_in: Option<i64>,
    pub cv_out: Option<i64>,
    /// STATION rows of the slot + ALARM rows in the window, oldest first.
    pub rows: Vec<(i64, Row)>,
}

#[derive(Debug, Default)]
pub struct Built {
    pub lanes: Vec<Lane>,
    pub spans: Vec<Span>,
    /// (event id, kind).
    pub markers: Vec<(i64, &'static str)>,
}

/// Station id (21xx) whose slot is `slot`.
pub fn on_station(target: Option<i64>, slot: u32) -> bool {
    target.and_then(|t| u16::try_from(t).ok()).is_some_and(|t| gr_proto::is_station_id(t) && u32::from(t % 100) == slot)
}

fn bit(v: Option<i64>, n: u32) -> Option<u8> {
    v.map(|v| u8::from(v >> n & 1 == 1))
}

/// Old ALM_RAISED / ALM_CLEARED rows and v2 ALARM raise / clear rows.
fn alarm_edge(row: &Row) -> bool {
    row.cat == CAT_ALARM && (matches!(row.code, ALM_RAISED | ALM_CLEARED) || decode_v2(row.cat, row.src, row.code).is_some_and(|v| matches!(v.trans, Trans::Raise | Trans::Clear)))
}

/// `station_alarm(plc, is_robot, row)` decides which ALARM rows are markers.
pub fn build(slot: u32, from: i64, to: i64, robots: &[RobotIn], grm: Option<&GrmIn>, station_alarm: &dyn Fn(&str, bool, &Row) -> bool) -> Built {
    let mut out = Built::default();
    for r in robots {
        let mut lanes: Vec<Lane> = PO_BITS.iter().map(|(n, _)| Lane::new(format!("{}.PO.{n}", r.plc), &r.name, format!("PO.{n}"))).collect();
        lanes.extend(PI_BITS.iter().map(|(n, _)| Lane::new(format!("{}.PI.{n}", r.plc), &r.name, format!("PI.{n}"))));
        let (mut target, mut pi, mut po) = (r.target, r.pi, r.po);
        let mut span: Option<i64> = None;
        let apply = |t: i64, target: Option<i64>, pi: Option<i64>, po: Option<i64>, lanes: &mut Vec<Lane>, span: &mut Option<i64>, spans: &mut Vec<Span>| {
            let on = on_station(target, slot);
            let bits = PO_BITS.iter().map(|(_, b)| (po, *b)).chain(PI_BITS.iter().map(|(_, b)| (pi, *b)));
            for (lane, (v, b)) in lanes.iter_mut().zip(bits) {
                lane.push(t, if on { bit(v, b) } else { None });
            }
            match (on, *span) {
                (true, None) => *span = Some(t),
                (false, Some(s)) => {
                    spans.push(Span { group: r.name.clone(), from: s, to: t });
                    *span = None;
                }
                _ => {}
            }
        };
        apply(from, target, pi, po, &mut lanes, &mut span, &mut out.spans);
        for (id, row) in &r.rows {
            let on_before = on_station(target, slot);
            match (row.cat, row.code) {
                (CAT_ILOCK, ILK_TARGET) => target = Some(row.a),
                (CAT_ILOCK, ILK_PI) => pi = Some(row.a),
                (CAT_ILOCK, ILK_PO) => po = Some(row.a),
                (CAT_ILOCK, ILK_TIMEOUT) if on_before => out.markers.push((*id, "ilk_timeout")),
                (CAT_ALARM, _) if on_before && alarm_edge(row) && station_alarm(&r.plc, true, row) => out.markers.push((*id, "alarm")),
                _ => continue,
            }
            apply(row.plc_ts, target, pi, po, &mut lanes, &mut span, &mut out.spans);
        }
        if let Some(s) = span {
            out.spans.push(Span { group: r.name.clone(), from: s, to });
        }
        out.lanes.extend(lanes);
    }
    if let Some(g) = grm {
        let mut lanes: Vec<Lane> = PI_BITS.iter().map(|(n, _)| Lane::new(format!("{}.In.{n}", g.plc), &g.plc, format!("In.{n}"))).collect();
        lanes.extend(CV_OUT_BITS.iter().map(|(n, _)| Lane::new(format!("{}.Out.{n}", g.plc), &g.plc, format!("Out.{n}"))));
        let (mut cv_in, mut cv_out) = (g.cv_in, g.cv_out);
        let apply = |t: i64, cv_in: Option<i64>, cv_out: Option<i64>, lanes: &mut Vec<Lane>| {
            let bits = PI_BITS.iter().map(|(_, b)| (cv_in, *b)).chain(CV_OUT_BITS.iter().map(|(_, b)| (cv_out, *b)));
            for (lane, (v, b)) in lanes.iter_mut().zip(bits) {
                lane.push(t, bit(v, b));
            }
        };
        apply(from, cv_in, cv_out, &mut lanes);
        for (id, row) in &g.rows {
            match (row.cat, row.code) {
                (CAT_STATION, ST_CV_IN) if row.src == slot => cv_in = Some(row.a),
                (CAT_STATION, ST_CV_OUT) if row.src == slot => cv_out = Some(row.a),
                (CAT_STATION, ST_CVNO_REASON) if row.src == slot => out.markers.push((*id, "cvno_reason")),
                (CAT_STATION, ST_MEAS) if row.src == slot => out.markers.push((*id, "meas")),
                (CAT_STATION, ST_TRACKING) if row.src == slot => out.markers.push((*id, "tracking")),
                (CAT_ALARM, _) if alarm_edge(row) && station_alarm(&g.plc, false, row) => out.markers.push((*id, "alarm")),
                _ => {}
            }
            apply(row.plc_ts, cv_in, cv_out, &mut lanes);
        }
        out.lanes.extend(lanes);
    }
    out
}

/// `Station 01 - …` → 1 (lower-cased English alarm text).
fn station_no(text: &str) -> Option<u32> {
    let rest = text.strip_prefix("station")?.trim_start();
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// GRM alarms named after the slot's station, robot alarms about the station interlock (English text: alarms.json
/// for old rows, the ErrorList for v2 rows).
pub fn station_alarm_of(texts: &Texts, slot: u32) -> impl Fn(&str, bool, &Row) -> bool + '_ {
    move |plc, robot, row| {
        let r = texts.renderer(plc);
        let text = match decode_v2(row.cat, row.src, row.code) {
            Some(v) => r.errorlist().get(v.level, v.num).map(|e| e.text_en.clone()),
            None => u32::try_from(row.a).ok().and_then(|bit| r.alarm(row.src, bit)).map(|a| a.text_en.clone()),
        };
        text.is_some_and(|t| {
            let t = t.to_ascii_lowercase();
            if robot { t.contains("station interlock") } else { station_no(&t) == Some(slot) }
        })
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Marker {
    pub kind: &'static str,
    #[serde(flatten)]
    pub event: EventRow,
}

#[derive(Clone, Debug, Serialize)]
pub struct Swimlane {
    pub station: Option<u16>,
    pub slot: u32,
    pub from: i64,
    pub to: i64,
    pub lanes: Vec<Lane>,
    pub spans: Vec<Span>,
    pub markers: Vec<Marker>,
}

fn last_before(log: &EvtLog, plc: &str, cat: u8, code: u32, src: Option<u32>, before: i64) -> rusqlite::Result<Option<Row>> {
    let f = Filter { plcs: vec![plc.to_string()], codes: vec![(Some(cat), Some(code))], src, to: Some(before - 1), ..Default::default() };
    Ok(log.store.query(&f, None, 1, false)?.into_iter().next().map(|(_, r)| r))
}

fn window(log: &EvtLog, plc: &str, codes: Vec<(Option<u8>, Option<u32>)>, src: Option<u32>, from: i64, to: i64) -> rusqlite::Result<Vec<(i64, Row)>> {
    // ALARM windows also take the v2 raise / clear rows
    let code_or = codes.contains(&(Some(CAT_ALARM), Some(ALM_RAISED))).then(|| (format!("(cat = {CAT_ALARM} AND code >= {TRANS_MUL} AND code < {})", 3 * TRANS_MUL), Vec::new()));
    let f = Filter { plcs: vec![plc.to_string()], codes, src, from: Some(from), to: Some(to), code_or, ..Default::default() };
    log.store.query(&f, None, MAX_ROWS, true)
}

/// Blocking: gathers the rows and builds the chart.
pub fn load(log: &EvtLog, robots: &[(String, String)], grm_plc: Option<&str>, slot: u32, station: Option<u16>, from: i64, to: i64) -> rusqlite::Result<Swimlane> {
    let alarm_codes = || vec![(Some(CAT_ALARM), Some(ALM_RAISED)), (Some(CAT_ALARM), Some(ALM_CLEARED))];
    let mut ins = Vec::new();
    for (name, plc) in robots {
        let mut codes = alarm_codes();
        codes.push((Some(CAT_ILOCK), None));
        ins.push(RobotIn {
            name: name.clone(),
            plc: plc.clone(),
            target: last_before(log, plc, CAT_ILOCK, ILK_TARGET, None, from)?.map(|r| r.a),
            pi: last_before(log, plc, CAT_ILOCK, ILK_PI, None, from)?.map(|r| r.a),
            po: last_before(log, plc, CAT_ILOCK, ILK_PO, None, from)?.map(|r| r.a),
            rows: window(log, plc, codes, None, from, to)?,
        });
    }
    let grm = match grm_plc {
        Some(plc) => {
            let mut rows = window(log, plc, vec![(Some(CAT_STATION), None)], Some(slot), from, to)?;
            rows.extend(window(log, plc, alarm_codes(), None, from, to)?);
            rows.sort_by_key(|(id, r)| (r.plc_ts, *id));
            Some(GrmIn {
                plc: plc.to_string(),
                cv_in: last_before(log, plc, CAT_STATION, ST_CV_IN, Some(slot), from)?.map(|r| r.a),
                cv_out: last_before(log, plc, CAT_STATION, ST_CV_OUT, Some(slot), from)?.map(|r| r.a),
                rows,
            })
        }
        None => None,
    };
    let pred = station_alarm_of(&log.texts, slot);
    let b = build(slot, from, to, &ins, grm.as_ref(), &pred);
    let rows: std::collections::HashMap<i64, &Row> = ins.iter().flat_map(|r| r.rows.iter()).chain(grm.iter().flat_map(|g| g.rows.iter())).map(|(id, r)| (*id, r)).collect();
    let markers = b.markers.iter().filter_map(|(id, kind)| rows.get(id).map(|r| Marker { kind, event: log.texts.row(*id, r) })).collect();
    Ok(Swimlane { station, slot, from, to, lanes: b.lanes, spans: b.spans, markers })
}

/// Station slot an event row belongs to: a STATION row's Src, an ILK_TARGET's station, else the robot's last
/// ILK_TARGET **before that row in log order** (ts, id) — the robot logs the target and its PI/PO in the same ms.
pub fn slot_of_event(log: &EvtLog, id: i64) -> rusqlite::Result<Option<u32>> {
    let Some(r) = log.store.get(id)? else { return Ok(None) };
    let station = |a: i64| u16::try_from(a).ok().filter(|t| gr_proto::is_station_id(*t)).map(|t| u32::from(t % 100));
    Ok(match (r.cat, r.code) {
        (CAT_STATION, _) => Some(r.src).filter(|s| (1..=32).contains(s)),
        (CAT_ILOCK, ILK_TARGET) if station(r.a).is_some() => station(r.a),
        (CAT_ILOCK, _) => {
            let f = Filter { plcs: vec![r.plc.clone()], codes: vec![(Some(CAT_ILOCK), Some(ILK_TARGET))], ..Default::default() };
            log.store.query(&f, Some(Cursor { ts: r.plc_ts, id }), 1, false)?.first().and_then(|(_, t)| station(t.a))
        }
        _ => None,
    })
}

#[derive(Deserialize, Default)]
struct SwimQ {
    station: Option<String>,
    slot: Option<String>,
    /// An ILOCK / STATION event id: its station (entry from the ±30 s window).
    event: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

async fn swimlane(State(st): State<AppState>, Query(q): Query<SwimQ>) -> ApiResult<Swimlane> {
    let log = log(&st)?.clone();
    let time = |s: &Option<String>, what: &str| -> Result<Option<i64>, ApiError> {
        opt(s).map(|v| log.texts.parse_time(v).ok_or_else(|| ApiError::BadRequest(format!("{what}: {v} 시각을 읽을 수 없습니다")))).transpose()
    };
    let to = time(&q.to, "to")?.unwrap_or_else(now_ms);
    let from = time(&q.from, "from")?.unwrap_or(to - DEFAULT_WINDOW_MS);
    if from >= to || to - from > MAX_WINDOW_MS {
        return Err(ApiError::BadRequest("from < to, 창은 24 시간까지".into()));
    }
    let station: Option<u16> = num(&q.station, "station")?;
    let slot = match (station, num::<u32>(&q.slot, "slot")?, num::<i64>(&q.event, "event")?) {
        (Some(id), _, _) => {
            if !gr_proto::is_station_id(id) {
                return Err(ApiError::BadRequest(format!("station: {id} 는 스테이션 Id 가 아닙니다")));
            }
            u32::from(id % 100)
        }
        (None, Some(s), _) => s,
        (None, None, Some(ev)) => {
            let l = log.clone();
            match tokio::task::spawn_blocking(move || slot_of_event(&l, ev)).await.map_err(|e| ApiError::Internal(e.to_string()))?? {
                Some(s) => s,
                None => return Err(ApiError::NotFound(format!("이벤트 {ev} 의 스테이션을 찾지 못했습니다 (그때 ILK_TARGET 이 스테이션이 아님)"))),
            }
        }
        _ => return Err(ApiError::BadRequest("station, slot, event 중 하나가 필요합니다".into())),
    };
    let station = station.or_else(|| st.registry.stations().ok()?.iter().map(|s| s.id).find(|id| u32::from(id % 100) == slot));
    let robots: Vec<(String, String)> = st.robots.iter().map(|r| (r.name.clone(), r.plc.clone())).collect();
    let grm = st.grm_plc().map(|h| h.name().to_string());
    let out = tokio::task::spawn_blocking(move || load(&log, &robots, grm.as_deref(), slot, station, from, to)).await.map_err(|e| ApiError::Internal(e.to_string()))??;
    Ok(axum::Json(out))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/events/swimlane", get(swimlane))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evtlog::stats::tests::{raw, texts};

    fn pts(l: &Lane) -> Vec<(i64, Option<u8>)> {
        l.points.iter().map(|p| (p.t, p.v)).collect()
    }

    #[test]
    fn robot_lanes_follow_the_target_and_start_from_the_state_before() {
        let robot = RobotIn {
            name: "GR2".into(),
            plc: "GR2".into(),
            // before the window: working at 2101 with PO.CVNO up
            target: Some(2101),
            pi: Some(0b1001),
            po: Some(0b01),
            rows: vec![
                (1, raw("GR2", 1, 1_100, CAT_ILOCK, 2, ILK_PO, 0, 0b10, 0b01, 7)),
                (2, raw("GR2", 1, 1_200, CAT_ILOCK, 4, ILK_TIMEOUT, 600, 3118, 0, 7)),
                (3, raw("GR2", 1, 1_300, CAT_ILOCK, 2, ILK_TARGET, 0, 105, 2, 8)),     // leaves for a cell
                (4, raw("GR2", 1, 1_400, CAT_ILOCK, 2, ILK_PO, 0, 0b01, 0b10, 8)),     // another place: not shown
                (5, raw("GR2", 1, 1_450, CAT_ILOCK, 4, ILK_TIMEOUT, 400, 3118, 0, 8)), // not at the station: no marker
                (6, raw("GR2", 1, 1_500, CAT_ILOCK, 2, ILK_TARGET, 0, 2101, 1, 9)),
                (7, raw("GR2", 1, 1_600, CAT_ALARM, 4, ALM_RAISED, 1, 593, 0, 9)),
                (8, raw("GR2", 1, 1_650, CAT_ALARM, 4, ALM_RAISED, 1, 7, 0, 9)), // not an interlock alarm
            ],
        };
        let t = texts();
        let pred = station_alarm_of(&t, 1);
        let b = build(1, 1_000, 2_000, &[robot], None, &pred);
        let lane = |k: &str| b.lanes.iter().find(|l| l.key == k).unwrap();
        assert_eq!(pts(lane("GR2.PO.CVNO")), vec![(1_000, Some(1)), (1_100, Some(0)), (1_300, None), (1_500, Some(1))]);
        assert_eq!(pts(lane("GR2.PO.Comp")), vec![(1_000, Some(0)), (1_100, Some(1)), (1_300, None), (1_500, Some(0))]);
        assert_eq!(pts(lane("GR2.PI.ItemExist")), vec![(1_000, Some(1)), (1_300, None), (1_500, Some(1))]);
        assert_eq!(lane("GR2.PI.Req").group, "GR2");
        assert_eq!(b.spans, vec![Span { group: "GR2".into(), from: 1_000, to: 1_300 }, Span { group: "GR2".into(), from: 1_500, to: 2_000 }]);
        assert_eq!(b.markers, vec![(2, "ilk_timeout"), (7, "alarm")]);
        assert_eq!(b.lanes.len(), 6);
    }

    #[test]
    fn grm_lanes_take_only_their_slot_and_unknown_start_is_null() {
        let grm = GrmIn {
            plc: "GRM".into(),
            cv_in: None,
            cv_out: Some(0),
            rows: vec![
                (1, raw("GRM", 1, 1_100, CAT_STATION, 2, ST_CV_IN, 1, 0b1001, 0, 0)),
                (2, raw("GRM", 1, 1_150, CAT_STATION, 2, ST_CV_IN, 2, 0b1111, 0, 0)), // slot 2
                (3, raw("GRM", 1, 1_200, CAT_STATION, 2, ST_CV_OUT, 1, 0b0001, 0, 0)),
                (4, raw("GRM", 1, 1_250, CAT_STATION, 2, ST_CVNO_REASON, 1, 0b10, 0, 0)),
                (5, raw("GRM", 1, 1_300, CAT_STATION, 2, ST_MEAS, 1, 2, 7800, 0)),
                (6, raw("GRM", 1, 1_350, CAT_STATION, 2, ST_TRACKING, 1, 3, 2, 0)),
                (7, raw("GRM", 1, 1_400, CAT_ALARM, 3, ALM_RAISED, 2, 320, 0, 0)), // W1101 Station 01
                (8, raw("GRM", 1, 1_450, CAT_STATION, 2, ST_MEAS, 2, 1, 0, 0)),    // slot 2
            ],
        };
        let t = texts();
        let pred = station_alarm_of(&t, 1);
        let b = build(1, 1_000, 2_000, &[], Some(&grm), &pred);
        let lane = |k: &str| b.lanes.iter().find(|l| l.key == k).unwrap();
        assert_eq!(pts(lane("GRM.In.CVOK")), vec![(1_000, None), (1_100, Some(1))]);
        assert_eq!(pts(lane("GRM.In.Req")), vec![(1_000, None), (1_100, Some(0))]);
        assert_eq!(pts(lane("GRM.Out.CVNO")), vec![(1_000, Some(0)), (1_200, Some(1))]);
        assert_eq!(lane("GRM.Out.MeasErr").label, "Out.MeasErr");
        assert_eq!(b.markers, vec![(4, "cvno_reason"), (5, "meas"), (6, "tracking"), (7, "alarm")]);
        assert_eq!(b.lanes.len(), 8);
        assert_eq!(station_no("station 01 - measuring error"), Some(1));
        assert_eq!(station_no("stationary"), None);
        assert!(!on_station(Some(101), 1) && on_station(Some(2101), 1) && !on_station(Some(2101), 2) && !on_station(None, 1));
    }

    #[tokio::test]
    async fn load_reads_initial_state_and_window_from_the_store() {
        let cat = std::sync::Arc::new(crate::evtlog::catalog::embedded());
        let log = EvtLog::memory(cat);
        let rows = vec![
            raw("GR2", 1, 500, CAT_ILOCK, 2, ILK_TARGET, 0, 2101, 1, 5),
            raw("GR2", 1, 600, CAT_ILOCK, 2, ILK_PI, 0, 0b1001, 0, 5),
            raw("GR2", 1, 1_200, CAT_ILOCK, 2, ILK_PO, 0, 0b01, 0, 5),
            raw("GRM", 1, 700, CAT_STATION, 2, ST_CV_IN, 1, 0b0001, 0, 0),
            raw("GRM", 1, 1_300, CAT_STATION, 2, ST_CV_IN, 1, 0b1001, 0b0001, 0),
            raw("GRM", 1, 1_350, CAT_STATION, 2, ST_MEAS, 1, 1, 0, 0),
        ];
        log.store.write(rows, &[]).unwrap();
        let s = load(&log, &[("GR2".into(), "GR2".into())], Some("GRM"), 1, Some(2101), 1_000, 2_000).unwrap();
        let lane = |k: &str| s.lanes.iter().find(|l| l.key == k).unwrap();
        assert_eq!(pts(lane("GR2.PI.CVOK")), vec![(1_000, Some(1))], "PI from before the window");
        assert_eq!(pts(lane("GR2.PO.CVNO")), vec![(1_000, None), (1_200, Some(1))]);
        assert_eq!(pts(lane("GRM.In.ItemExist")), vec![(1_000, Some(0)), (1_300, Some(1))]);
        assert_eq!(s.markers.len(), 1);
        assert_eq!(s.markers[0].kind, "meas");
        assert_eq!(s.markers[0].event.name.as_deref(), Some("ST_MEAS"));
        let j = serde_json::to_value(&s).unwrap();
        assert_eq!(j["markers"][0]["kind"], "meas");
        assert!(j["markers"][0]["text"].is_string(), "the marker carries the rendered row");
    }

    #[tokio::test]
    async fn an_event_row_leads_to_its_station() {
        let cat = std::sync::Arc::new(crate::evtlog::catalog::embedded());
        let log = EvtLog::memory(cat);
        // same ms: the load logs TARGET then PI, the end logs PO then TARGET 0
        let rows = vec![
            raw("GR2", 1, 1_000, CAT_ILOCK, 2, ILK_TARGET, 0, 2102, 1, 5),
            raw("GR2", 1, 1_000, CAT_ILOCK, 2, ILK_PI, 0, 1, 0, 5),
            raw("GR2", 1, 2_000, CAT_ILOCK, 2, ILK_PO, 0, 0, 2, 5),
            raw("GR2", 1, 2_000, CAT_ILOCK, 2, ILK_TARGET, 0, 0, 0, 5),
            raw("GR2", 1, 3_000, CAT_ILOCK, 2, ILK_PO, 0, 1, 0, 6),
            raw("GRM", 1, 3_000, CAT_STATION, 2, ST_MEAS, 7, 1, 0, 0),
            raw("GR2", 1, 3_000, 3, 2, 300, 20, 5, 200, 6),
        ];
        let ids: Vec<i64> = log.store.write(rows, &[]).unwrap().into_iter().map(|(id, _)| id).collect();
        let slot = |i: usize| slot_of_event(&log, ids[i]).unwrap();
        assert_eq!(slot(0), Some(2), "ILK_TARGET names the station");
        assert_eq!(slot(1), Some(2), "PI right after its TARGET in the same ms");
        assert_eq!(slot(2), Some(2), "PO right before TARGET 0 in the same ms");
        assert_eq!(slot(4), None, "target cleared: not at a station");
        assert_eq!(slot(5), Some(7), "STATION row: Src is the slot");
        assert_eq!(slot(6), None);
        assert_eq!(slot_of_event(&log, 9_999).unwrap(), None);
    }
}
