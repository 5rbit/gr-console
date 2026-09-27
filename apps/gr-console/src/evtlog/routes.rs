//! `/api/events*` (PLC + console event log) and `/api/plc/{plc}/evtlog/cfg` (EVTLOG.Cfg over S7).

use std::sync::Arc;

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use serde::Deserialize;
use serde_json::{Value as Json, json};

use super::alerts::AlertMsg;
use super::store::{Cursor, Filter, Origin};
use super::{DB, EventRow, EvtLog};
use crate::error::{ApiError, ApiResult};
use crate::plc::{PlcHandle, ensure_db};
use crate::state::AppState;

const MAX_LIMIT: usize = 5000;
const CSV_MAX: usize = 200_000;
/// Rows a text search (`q`) renders before it returns a partial page with a cursor.
const SEARCH_SCAN: usize = 20_000;

pub(super) fn log(st: &AppState) -> Result<&Arc<EvtLog>, ApiError> {
    st.evtlog.as_ref().ok_or_else(|| ApiError::BadRequest("이벤트 로그가 꺼져 있습니다 ([evtlog] enabled)".into()))
}

/// Every value is a string so an empty `lvl=` from a form is "not set", not a 400.
#[derive(Deserialize, Default, Clone)]
pub struct Q {
    plc: Option<String>,
    cat: Option<String>,
    lvl: Option<String>,
    code: Option<String>,
    src: Option<String>,
    ctx: Option<String>,
    origin: Option<String>,
    from: Option<String>,
    to: Option<String>,
    q: Option<String>,
    limit: Option<String>,
    before: Option<String>,
    order: Option<String>,
}

pub(super) fn opt(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

pub(super) fn list(s: &Option<String>) -> Vec<&str> {
    opt(s).map(|s| s.split(',').map(str::trim).filter(|x| !x.is_empty()).collect()).unwrap_or_default()
}

pub(super) fn num<T: std::str::FromStr>(s: &Option<String>, what: &str) -> Result<Option<T>, ApiError> {
    opt(s).map(|v| v.parse::<T>().map_err(|_| ApiError::BadRequest(format!("{what}: {v} 는 숫자가 아닙니다")))).transpose()
}

/// SQL filter, lower-cased search text, limit, cursor, ascending.
type Parsed = (Filter, Option<String>, usize, Option<Cursor>, bool);

/// Query string → SQL filter (+ search text, limit, cursor, ascending).
fn filter(log: &EvtLog, q: &Q, default_limit: usize, max_limit: usize) -> Result<Parsed, ApiError> {
    let cat = &log.texts.catalog;
    let mut f = Filter { plcs: list(&q.plc).into_iter().map(str::to_string).collect(), ..Default::default() };
    for c in list(&q.cat) {
        let id = c.parse::<u8>().ok().or_else(|| cat.cat_id(c)).ok_or_else(|| ApiError::BadRequest(format!("cat: {c} 를 모릅니다")))?;
        f.cats.push(id);
    }
    f.min_lvl = match opt(&q.lvl) {
        Some(l) => Some(l.parse::<u8>().ok().or_else(|| cat.level_id(l)).ok_or_else(|| ApiError::BadRequest(format!("lvl: {l} 를 모릅니다")))?),
        None => None,
    };
    for c in list(&q.code) {
        match c.parse::<u32>() {
            Ok(n) => f.codes.push((None, Some(n))),
            Err(_) => {
                let e = cat.event_by_name(c).ok_or_else(|| ApiError::BadRequest(format!("code: {c} 이벤트를 모릅니다")))?;
                f.codes.push((Some(e.cat), e.code.map(u32::from)));
            }
        }
    }
    f.src = num(&q.src, "src")?;
    f.ctx = num(&q.ctx, "ctx")?;
    f.origin = match opt(&q.origin) {
        Some("plc") => Some(Origin::Plc),
        Some("console") => Some(Origin::Console),
        Some(o) => return Err(ApiError::BadRequest(format!("origin: {o} (plc | console)"))),
        None => None,
    };
    let time = |s: &Option<String>, what: &str| -> Result<Option<i64>, ApiError> {
        opt(s).map(|v| log.texts.parse_time(v).ok_or_else(|| ApiError::BadRequest(format!("{what}: {v} 시각을 읽을 수 없습니다")))).transpose()
    };
    f.from = time(&q.from, "from")?;
    f.to = time(&q.to, "to")?;
    let limit = num::<usize>(&q.limit, "limit")?.unwrap_or(default_limit).clamp(1, max_limit);
    let cursor = match opt(&q.before) {
        Some(c) => Some(Cursor::parse(c).ok_or_else(|| ApiError::BadRequest(format!("before: {c}")))?),
        None => None,
    };
    let asc = opt(&q.order).is_some_and(|o| o.eq_ignore_ascii_case("asc"));
    Ok((f, opt(&q.q).map(str::to_lowercase), limit, cursor, asc))
}

fn matches(r: &EventRow, needle: &str) -> bool {
    [Some(r.text.as_str()), r.name.as_deref(), r.detail.as_deref(), Some(r.plc.as_str())].into_iter().flatten().any(|s| s.to_lowercase().contains(needle))
}

/// One page (newest first unless `order=asc`). Blocking — call from `spawn_blocking`.
fn page(log: &EvtLog, f: &Filter, needle: Option<&str>, limit: usize, mut cursor: Option<Cursor>, asc: bool) -> Result<(Vec<EventRow>, Option<String>, usize), ApiError> {
    let mut out = Vec::new();
    let mut scanned = 0usize;
    loop {
        let batch = if needle.is_some() { 1000 } else { limit + 1 - out.len() };
        let rows = log.store.query(f, cursor, batch, asc)?;
        let exhausted = rows.len() < batch;
        for (id, r) in rows {
            scanned += 1;
            if out.len() == limit {
                // one more matching row exists beyond the page
                let next = out.last().map(|e: &EventRow| Cursor { ts: e.ts_ms, id: e.id }.text());
                return Ok((out, next, scanned));
            }
            let row = log.texts.row(id, &r);
            cursor = Some(Cursor { ts: r.plc_ts, id });
            if needle.is_none_or(|n| matches(&row, n)) {
                out.push(row);
                if out.len() == limit && needle.is_some() {
                    return Ok((out, cursor.map(|c| c.text()), scanned));
                }
            }
        }
        if exhausted {
            return Ok((out, None, scanned));
        }
        if scanned >= SEARCH_SCAN {
            return Ok((out, cursor.map(|c| c.text()), scanned));
        }
    }
}

async fn events(State(st): State<AppState>, Query(q): Query<Q>) -> ApiResult<Json> {
    let log = log(&st)?.clone();
    let (f, needle, limit, cursor, asc) = filter(&log, &q, 200, MAX_LIMIT)?;
    let (rows, next, scanned) = tokio::task::spawn_blocking(move || page(&log, &f, needle.as_deref(), limit, cursor, asc)).await.map_err(|e| ApiError::Internal(e.to_string()))??;
    Ok(axum::Json(json!({ "rows": rows, "next_before": next, "scanned": scanned })))
}

#[derive(Deserialize, Default)]
struct StreamQ {
    /// `only` = alerts without the rows (the status bar badge).
    alerts: Option<String>,
}

/// Named events: `evt` (row), `alert` (new alert), `alert_ack` (acknowledged ids, `[]` = all), `lag`.
async fn stream(State(st): State<AppState>, Query(q): Query<StreamQ>) -> Result<impl IntoResponse, ApiError> {
    use axum::response::sse::{Event, KeepAlive, Sse};
    use futures::StreamExt;
    use tokio_stream::wrappers::BroadcastStream;
    use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

    let log = log(&st)?;
    let lag = |n: u64| Event::default().event("lag").data(n.to_string());
    let alerts = BroadcastStream::new(log.alerts_tx.subscribe()).map(move |m| {
        Ok::<_, std::convert::Infallible>(match m {
            Ok(AlertMsg::New(a)) => Event::default().event("alert").json_data(a).unwrap_or_default(),
            Ok(AlertMsg::Ack(ids)) => Event::default().event("alert_ack").json_data(ids).unwrap_or_default(),
            Err(BroadcastStreamRecvError::Lagged(n)) => lag(n),
        })
    });
    let only_alerts = opt(&q.alerts).is_some_and(|v| v == "only");
    let rows = if only_alerts { None } else { Some(broadcast_sse_stream(log.stream.subscribe())) };
    let merged = match rows {
        Some(r) => futures::stream::select(alerts, r).boxed(),
        None => alerts.boxed(),
    };
    Ok(Sse::new(merged).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)).text("ping")))
}

/// Rows as `evt` events (the `broadcast_sse` shape, as a stream to merge).
fn broadcast_sse_stream(rx: tokio::sync::broadcast::Receiver<EventRow>) -> impl futures::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>> + Send {
    use axum::response::sse::Event;
    use futures::StreamExt;
    use tokio_stream::wrappers::BroadcastStream;
    use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
    BroadcastStream::new(rx).map(|r| {
        Ok(match r {
            Ok(v) => Event::default().event("evt").json_data(v).unwrap_or_default(),
            Err(BroadcastStreamRecvError::Lagged(n)) => Event::default().event("lag").data(n.to_string()),
        })
    })
}

async fn export_csv(State(st): State<AppState>, Query(q): Query<Q>) -> Result<impl IntoResponse, ApiError> {
    let log = log(&st)?.clone();
    let (f, needle, limit, cursor, asc) = filter(&log, &q, 50_000, CSV_MAX)?;
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, ApiError> {
        let (rows, _, _) = page(&log, &f, needle.as_deref(), limit, cursor, asc)?;
        let mut w = csv::Writer::from_writer(vec![0xEF, 0xBB, 0xBF]);
        let err = |e: csv::Error| ApiError::Internal(format!("csv: {e}"));
        w.write_record(["id", "time", "rx_time", "plc", "origin", "epoch", "seq", "cat", "cat_name", "lvl", "lvl_name", "src", "code", "name", "a", "b", "ctx", "detail", "text"]).map_err(err)?;
        for r in rows {
            w.write_record([
                r.id.to_string(),
                r.ts,
                r.rx_ts,
                r.plc,
                r.origin.to_string(),
                r.epoch.to_string(),
                r.seq.map(|s| s.to_string()).unwrap_or_default(),
                r.cat.to_string(),
                r.cat_name,
                r.lvl.to_string(),
                r.lvl_name,
                r.src.to_string(),
                r.code.to_string(),
                r.name.unwrap_or_default(),
                r.a.to_string(),
                r.b.to_string(),
                r.ctx.to_string(),
                r.detail.unwrap_or_default(),
                r.text,
            ])
            .map_err(err)?;
        }
        w.into_inner().map_err(|e| ApiError::Internal(format!("csv: {e}")))
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))??;
    let name = format!("events-{}.csv", crate::util::now_str().chars().filter(char::is_ascii_digit).take(14).collect::<String>());
    Ok(([(header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))], bytes))
}

#[derive(Deserialize)]
struct CatalogQ {
    plc: Option<String>,
}

async fn catalog(State(st): State<AppState>, Query(q): Query<CatalogQ>) -> ApiResult<Json> {
    let log = log(&st)?;
    let plc = q.plc.or_else(|| st.robots.first().map(|r| r.plc.clone()));
    Ok(axum::Json(log.texts.catalog_json(plc.as_deref())))
}

/// Logger state of one PLC: header / Stat / Cfg from the fast-tier snapshot + collector status.
fn source(log: &EvtLog, h: &PlcHandle) -> Json {
    let hl = h.health_now();
    let available = h.has_db(DB);
    let verified = hl.verified_at.is_some() && hl.checks.iter().any(|c| c.db == DB);
    let layout_ok = verified.then(|| hl.db_ok(DB));
    let detail = if !available {
        Some(format!("{} 계약에 {DB} 가 없습니다", h.cfg.contract))
    } else if !hl.connected {
        Some(hl.last_error.clone().unwrap_or_else(|| "연결 안 됨".into()))
    } else if layout_ok == Some(false) {
        Some(hl.mismatch_detail(DB))
    } else {
        None
    };
    let snap = h.snap();
    let j = snap.db(DB).filter(|_| layout_ok == Some(true)).map(|d| d.json.clone());
    let n = |v: &Json, p: &str| v.pointer(p).cloned().unwrap_or(Json::Null);
    let header = j.as_ref().map(|v| json!({ "layout_sig": n(v, "/LayoutSig"), "total": n(v, "/Total"), "head": n(v, "/Head"), "boot_id": n(v, "/BootId"), "dropped": n(v, "/Dropped") }));
    let stat = j.as_ref().map(|v| json!({ "run_us": n(v, "/Stat/RunUs"), "max_run_us": n(v, "/Stat/MaxRunUs"), "max_per_scan_hit": n(v, "/Stat/MaxPerScanHit") }));
    let cfg = j.as_ref().map(|v| json!({ "min_level": n(v, "/Cfg/MinLevel"), "cat_mask": n(v, "/Cfg/CatMask"), "max_per_scan": n(v, "/Cfg/MaxPerScan") }));
    json!({ "plc": h.name(), "available": available, "layout_ok": layout_ok, "detail": detail, "header": header, "stat": stat, "cfg": cfg, "collector": log.status(h.name()), "console_dropped": log.console_dropped() })
}

async fn sources(State(st): State<AppState>) -> ApiResult<Json> {
    let log = log(&st)?;
    let mut names: Vec<&String> = st.plcs.keys().collect();
    names.sort();
    Ok(axum::Json(Json::Array(names.into_iter().map(|n| source(log, &st.plcs[n])).collect())))
}

async fn cfg_get(State(st): State<AppState>, Path(plc): Path<String>) -> ApiResult<Json> {
    Ok(axum::Json(source(log(&st)?, st.plc(&plc)?)))
}

#[derive(Deserialize)]
struct CfgBody {
    min_level: Option<u8>,
    cat_mask: Option<u32>,
    max_per_scan: Option<i16>,
}

async fn cfg_put(State(st): State<AppState>, Path(plc): Path<String>, axum::Json(b): axum::Json<CfgBody>) -> ApiResult<Json> {
    let log = log(&st)?;
    let h = st.plc(&plc)?;
    ensure_db(h, DB)?;
    let top = log.texts.catalog.levels.iter().map(|l| l.id).max().unwrap_or(4);
    let mut writes: Vec<(&str, Json)> = Vec::new();
    if let Some(v) = b.min_level {
        if v > top {
            return Err(ApiError::BadRequest(format!("min_level 0..{top}")));
        }
        writes.push(("Cfg.MinLevel", json!(v)));
    }
    if let Some(v) = b.cat_mask {
        writes.push(("Cfg.CatMask", json!(v)));
    }
    if let Some(v) = b.max_per_scan {
        if !(1..=200).contains(&v) {
            return Err(ApiError::BadRequest("max_per_scan 1..200".into()));
        }
        writes.push(("Cfg.MaxPerScan", json!(v)));
    }
    if writes.is_empty() {
        return Err(ApiError::BadRequest("바꿀 값이 없습니다 (min_level, cat_mask, max_per_scan)".into()));
    }
    let layout = h.layout(DB).ok_or_else(|| ApiError::Internal("EVTLOG layout".into()))?;
    let _guard = st.shutdown.enter(format!("{} EVTLOG.Cfg 쓰기", h.name()))?;
    let mut scratch = vec![0u8; layout.size as usize];
    let mut done = Vec::new();
    for (path, v) in writes {
        let m = layout.find(path).ok_or_else(|| ApiError::Internal(format!("{path} not in layout")))?;
        h.contract.encode_path(DB, path, &v, &mut scratch)?;
        let (lo, hi) = (m.offset as usize, (m.offset + m.size.max(1)) as usize);
        h.write(DB, m.offset, scratch[lo..hi].to_vec()).await.map_err(ApiError::PlcUnavailable)?;
        done.push(format!("{path}={v}"));
    }
    crate::evtlog::console("CON_SETTINGS", h.name(), 0, 0, 0, format!("{} EVTLOG {}", h.name(), done.join(", ")));
    // the next fast tick carries the new Cfg; answer with what the PLC now holds
    let (lo, hi) = layout.range_of("Cfg").ok_or_else(|| ApiError::Internal("Cfg not in layout".into()))?;
    let bytes = h.read(DB, lo, (hi - lo) as usize).await.map_err(ApiError::PlcUnavailable)?;
    let mut buf = vec![0u8; hi as usize];
    buf[lo as usize..].copy_from_slice(&bytes);
    let cfg = h.contract.decode_path(DB, "Cfg", &buf)?;
    let mut out = source(log, h);
    out["cfg"] = json!({ "min_level": cfg["MinLevel"], "cat_mask": cfg["CatMask"], "max_per_scan": cfg["MaxPerScan"] });
    Ok(axum::Json(out))
}

/// Events of one ledger task: robot PLC rows whose Ctx is the task's WorkId (the robots fill Ctx with the current
/// WorkId), inside the task's time window.
async fn task_events(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Json> {
    let log = log(&st)?.clone();
    let (robot, entry) = st.find_task(&id).ok_or_else(|| ApiError::NotFound(format!("task {id}")))?;
    let texts = log.texts.clone();
    let start = entry.submitted_at.as_deref().unwrap_or(&entry.created_at);
    let from = texts.parse_time(start).map(|t| t - 5_000);
    let to = entry.ended_at.as_deref().and_then(|t| texts.parse_time(t)).map(|t| t + 30_000);
    let f = Filter { plcs: vec![robot.plc.clone()], ctx: Some(entry.work_id), origin: Some(Origin::Plc), from, to, ..Default::default() };
    let rows = tokio::task::spawn_blocking(move || -> Result<Vec<EventRow>, ApiError> { Ok(log.store.query(&f, None, 5000, true)?.iter().map(|(id, r)| log.texts.row(*id, r)).collect()) })
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))??;
    Ok(axum::Json(json!({ "task_id": id, "plc": robot.plc, "ctx": [entry.work_id], "from": from.map(|t| texts.fmt_ms(t)), "to": to.map(|t| texts.fmt_ms(t)), "rows": rows })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/events", get(events))
        .route("/api/events/stream", get(stream))
        .route("/api/events/export.csv", get(export_csv))
        .route("/api/events/catalog", get(catalog))
        .route("/api/events/sources", get(sources))
        .route("/api/events/task/{id}", get(task_events))
        .route("/api/plc/{plc}/evtlog/cfg", get(cfg_get).put(cfg_put))
        .merge(super::stats::router())
        .merge(super::swimlane::router())
        .merge(super::alerts::router())
        .merge(super::views::router())
        .merge(super::report::router())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evtlog::store::Row;

    fn log_with_rows() -> Arc<EvtLog> {
        let cat = crate::evtlog::catalog::load_catalog(std::path::Path::new("-")).unwrap();
        let log = EvtLog::memory(cat);
        let rows: Vec<Row> = (0..30)
            .map(|i| Row {
                plc: if i % 3 == 0 { "GRM".into() } else { "GR2".into() },
                epoch: 1,
                seq: Some(i),
                plc_ts: 1_000 + i,
                rx_ts: 1_000 + i,
                cat: if i % 2 == 0 { 7 } else { 3 },
                lvl: (i % 5) as u8,
                src: 20,
                code: if i % 2 == 0 { 704 } else { 300 },
                a: i,
                b: 200,
                ctx: 5,
                origin: Origin::Plc,
                detail: None,
            })
            .collect();
        log.store.write(rows, &[]).unwrap();
        log
    }

    fn q(pairs: &[(&str, &str)]) -> Q {
        let s: String = pairs.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");
        serde_urlencoded_like(&s)
    }

    fn serde_urlencoded_like(s: &str) -> Q {
        let uri: axum::http::Uri = format!("/x?{s}").parse().unwrap();
        Query::<Q>::try_from_uri(&uri).unwrap().0
    }

    #[test]
    fn pages_filters_and_searches() {
        let log = log_with_rows();
        let (f, n, limit, cur, asc) = filter(&log, &q(&[("limit", "7"), ("lvl", "")]), 200, MAX_LIMIT).unwrap();
        let (rows, next, _) = page(&log, &f, n.as_deref(), limit, cur, asc).unwrap();
        assert_eq!(rows.len(), 7);
        assert_eq!(rows[0].seq, Some(29));
        let (f, n, limit, _, asc) = filter(&log, &q(&[("limit", "7"), ("before", next.as_deref().unwrap())]), 200, MAX_LIMIT).unwrap();
        let cur = Cursor::parse(next.as_deref().unwrap());
        let (rows2, _, _) = page(&log, &f, n.as_deref(), limit, cur, asc).unwrap();
        assert_eq!(rows2[0].seq, Some(22));
        // by event name (GRIP_ERROR = cat 7 code 704), by STEP wildcard, by level name, by plc
        let (f, ..) = filter(&log, &q(&[("code", "GRIP_ERROR"), ("plc", "GR2")]), 200, MAX_LIMIT).unwrap();
        assert_eq!(page(&log, &f, None, 100, None, false).unwrap().0.len(), 10);
        let (f, ..) = filter(&log, &q(&[("code", "STEP_CHANGED")]), 200, MAX_LIMIT).unwrap();
        assert_eq!(page(&log, &f, None, 100, None, false).unwrap().0.len(), 15);
        let (f, ..) = filter(&log, &q(&[("lvl", "ERROR"), ("cat", "GRIP,STEP")]), 200, MAX_LIMIT).unwrap();
        assert_eq!(page(&log, &f, None, 100, None, false).unwrap().0.len(), 6);
        // text search over rendered text, paged
        let (rows, next, _) = page(&log, &Filter::default(), Some("stoppos"), 4, None, false).unwrap();
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|r| r.text.contains("StopPos")));
        let (more, _, _) = page(&log, &Filter::default(), Some("stoppos"), 100, next.as_deref().and_then(Cursor::parse), false).unwrap();
        assert_eq!(rows.len() + more.len(), 15);
        // bad input is a 400, not a panic
        assert!(filter(&log, &q(&[("cat", "NOPE")]), 200, MAX_LIMIT).is_err());
        assert!(filter(&log, &q(&[("from", "yesterday")]), 200, MAX_LIMIT).is_err());
        // ascending window
        let (f, n, limit, cur, asc) = filter(&log, &q(&[("from", "1005"), ("to", "1008"), ("order", "asc")]), 200, MAX_LIMIT).unwrap();
        let rows = page(&log, &f, n.as_deref(), limit, cur, asc).unwrap().0;
        assert_eq!(rows.iter().map(|r| r.ts_ms).collect::<Vec<_>>(), vec![1005, 1006, 1007, 1008]);
    }
}
