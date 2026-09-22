//! `/api/plcs`, `/api/plc/{plc}/...`, `/api/status`, `/api/status/stream`, `/api/health`, `/api/opcua/*`.

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use crate::error::{ApiError, ApiResult};
use crate::sse::broadcast_sse;
use crate::state::AppState;

#[derive(Serialize)]
pub struct LayoutCheckView {
    pub ok: Option<bool>,
    pub detail: Option<String>,
    pub checked_at: Option<String>,
    pub mismatches: Vec<MismatchView>,
}

#[derive(Serialize)]
pub struct MismatchView {
    pub db: String,
    pub kind: String,
    pub expected: String,
    pub actual: String,
}

#[derive(Serialize)]
pub struct PlcStatusView {
    pub id: String,
    /// Config name (`GR1`) — S7 only; the registry toolbar lists these as PLC targets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `gr` / `grm` — S7 only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<crate::config::PlcRole>,
    pub label: String,
    pub kind: &'static str,
    pub endpoint: String,
    pub connected: bool,
    pub rtt_ms: Option<u64>,
    pub last_ok_at: Option<String>,
    pub last_error: Option<String>,
    pub layout: LayoutCheckView,
    /// 통신 품질(S7) — 폴링 실패·재연결 횟수, 주기별 마지막/최대 한 바퀴(ms). OPC UA 줄에는 없다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comm: Option<serde_json::Value>,
}

pub fn plc_status_views(st: &AppState) -> Vec<PlcStatusView> {
    let mut out = Vec::new();
    for (name, h) in st.plcs.iter() {
        let hl = h.health_now();
        let mismatches: Vec<MismatchView> = hl
            .checks
            .iter()
            .filter(|c| !matches!(c.result, super::CheckResult::Ok))
            .map(|c| MismatchView {
                db: c.db.clone(),
                kind: match &c.result {
                    super::CheckResult::Missing => "missing",
                    super::CheckResult::SizeMismatch { .. } => "size",
                    super::CheckResult::SigMismatch { .. } => "signature",
                    super::CheckResult::Semantic { .. } => "semantic",
                    _ => "error",
                }
                .into(),
                expected: format!("{} B{}", c.expected_size, c.expected_sig.map(|s| format!(" / 16#{s:08X}")).unwrap_or_default()),
                actual: c.result.text(),
            })
            .collect();
        out.push(PlcStatusView {
            id: format!("{}_s7", name.to_ascii_lowercase()),
            name: Some(name.clone()),
            role: Some(h.cfg.role),
            label: format!("{name} S7"),
            kind: "s7",
            endpoint: format!("{}:{}", h.cfg.host, h.cfg.port),
            connected: hl.connected,
            rtt_ms: hl.rtt_ms,
            comm: Some(serde_json::json!({ "errors": hl.error_count, "reconnects": hl.reconnect_count, "cycle_ms": hl.cycle_ms, "cycle_max_ms": hl.cycle_max_ms, "heartbeat_age_ms": hl.heartbeat_age_ms, "slow_overruns": hl.slow_overruns })),
            last_ok_at: hl.last_ok_at.clone(),
            last_error: hl.last_error.clone(),
            layout: LayoutCheckView {
                ok: hl.layout_ok,
                detail: if mismatches.is_empty() { None } else { Some(mismatches.iter().map(|m| format!("{}: {}", m.db, m.actual)).collect::<Vec<_>>().join("; ")) },
                checked_at: hl.verified_at.clone(),
                mismatches,
            },
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    for (i, r) in st.robots.iter().enumerate() {
        let cs = r.cmd.status();
        out.push(PlcStatusView {
            id: if i == 0 { "grm_opcua".into() } else { format!("grm_opcua_{}", r.name.to_ascii_lowercase()) },
            name: None,
            role: None,
            label: format!("GRM OPC UA → {} ({})", r.name, r.opcua_root),
            kind: "opcua",
            endpoint: cs.endpoint,
            connected: cs.ready,
            rtt_ms: cs.io.as_ref().map(|io| io.write_last_ms),
            // OPC UA 줄: 쓰기 시간(마지막/최대)·실패·재연결 + 등록 노드 수·서버 한도.
            comm: cs.io.as_ref().map(|io| serde_json::json!({ "errors": io.write_failures, "reconnects": io.reconnects, "cycle_ms": { "write": io.write_last_ms }, "cycle_max_ms": { "write": io.write_max_ms }, "write_calls": io.write_calls, "write_nodes": io.write_nodes, "registered": io.registered, "max_nodes_per_write": io.max_nodes_per_write, "max_nodes_per_read": io.max_nodes_per_read, "struct_verified": io.struct_verified, "struct_active": io.struct_active, "struct_note": io.struct_note, "struct_writes": io.struct_writes })),
            last_ok_at: cs.last_ok_at,
            last_error: cs.error,
            layout: LayoutCheckView { ok: cs.ready.then_some(true), detail: cs.detail, checked_at: None, mismatches: vec![] },
        });
    }
    out
}

/// `grm_opcua` = default robot, `grm_opcua_<name>` = the others.
fn opcua_robot<'a>(st: &'a AppState, id: &str) -> Option<&'a crate::state::RobotCtx> {
    if id == "grm_opcua" {
        return st.robots.first();
    }
    let name = id.strip_prefix("grm_opcua_")?;
    st.robots.iter().find(|r| r.name.eq_ignore_ascii_case(name))
}

///  — robots behind GRM with command readiness, status PLC health and the submission gate.
async fn robots(State(st): State<AppState>) -> ApiResult<Json> {
    let mut out = Vec::new();
    for (i, r) in st.robots.iter().enumerate() {
        let cs = r.cmd.status();
        let plc = st.robot_plc(r).ok().map(|h| h.health_now());
        let g = crate::ledger::ops::gate(&st, r);
        let active = r.ledger.list().iter().filter(|e| e.state.is_active()).count();
        out.push(json!({
            "id": r.id, "name": r.name, "plc": r.plc, "opcua_root": r.opcua_root, "dst": r.dst, "default": i == 0,
            "cmd_ready": cs.ready, "cmd_error": cs.error,
            "plc_connected": plc.as_ref().map(|h| h.connected).unwrap_or(false), "layout_ok": plc.as_ref().and_then(|h| h.layout_ok),
            "gate": { "can_submit": g.can_submit, "reasons": g.reasons, "robot": g.robot, "plc": g.plc }, "active_tasks": active,
        }));
    }
    Ok(axum::Json(Json::Array(out)))
}

async fn plcs(State(st): State<AppState>) -> ApiResult<Vec<PlcStatusView>> {
    Ok(axum::Json(plc_status_views(&st)))
}

async fn plc_check(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Vec<PlcStatusView>> {
    if let Some(r) = opcua_robot(&st, &id) {
        r.cmd.rebrowse().await;
    } else if let Some(h) = st.plc_by_id(&id) {
        h.reverify().await;
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    } else {
        return Err(ApiError::NotFound(format!("plc {id}")));
    }
    Ok(axum::Json(plc_status_views(&st)))
}

async fn plc_reconnect(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Vec<PlcStatusView>> {
    if let Some(r) = opcua_robot(&st, &id) {
        r.cmd.rebrowse().await;
    } else if let Some(h) = st.plc_by_id(&id) {
        h.reconnect().await;
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    } else {
        return Err(ApiError::NotFound(format!("plc {id}")));
    }
    Ok(axum::Json(plc_status_views(&st)))
}

async fn health(State(st): State<AppState>) -> ApiResult<Json> {
    let plcs: serde_json::Map<String, Json> = st.plcs.iter().map(|(n, h)| (n.clone(), serde_json::to_value(h.health_now()).unwrap_or(Json::Null))).collect();
    Ok(axum::Json(
        json!({ "plcs": plcs, "opcua": st.cmd.status(), "robots": st.robots.iter().map(|r| json!({ "id": r.id, "name": r.name, "opcua": r.cmd.status() })).collect::<Vec<_>>(), "demo": st.cfg.demo, "at": crate::util::now_str() }),
    ))
}

#[derive(Deserialize)]
struct PathQuery {
    path: Option<String>,
}

async fn db_json(State(st): State<AppState>, Path((plc, db)): Path<(String, String)>, Query(q): Query<PathQuery>) -> ApiResult<Json> {
    let h = st.plc(&plc)?;
    let hl = h.health_now();
    if !hl.db_ok(&db) {
        return Err(ApiError::LayoutMismatch { plc: plc.clone(), db: db.clone(), detail: hl.mismatch_detail(&db) });
    }
    let snap = h.snap();
    let Some(d) = snap.db(&db) else { return Err(ApiError::PlcUnavailable(format!("{plc}.{db} not read yet"))) };
    match q.path {
        Some(p) if !p.is_empty() => Ok(axum::Json(h.contract.decode_path(&db, &p, &d.raw)?)),
        _ => Ok(axum::Json(json!({ "at": d.at, "seq": d.seq, "tier": d.tier, "size": d.raw.len(), "data": *d.json }))),
    }
}

async fn db_layout(State(st): State<AppState>, Path((plc, db)): Path<(String, String)>, Query(q): Query<PathQuery>) -> ApiResult<Json> {
    let h = st.plc(&plc)?;
    let l = h.layout(&db).ok_or_else(|| ApiError::NotFound(format!("{plc}.{db} not in contract")))?;
    let members: Vec<&plc_layout::Member> = l.members.iter().filter(|m| q.path.as_ref().is_none_or(|p| m.path.starts_with(p))).collect();
    Ok(axum::Json(json!({ "db": db, "number": h.db_number(&db), "size": l.size, "sig": h.contract.layout_sig(&db).ok(), "members": members })))
}

async fn plc_events(State(st): State<AppState>, Path(plc): Path<String>) -> Result<impl IntoResponse, ApiError> {
    let h = st.plc(&plc)?;
    Ok(broadcast_sse(h.events.subscribe(), "plc", None))
}

/// `?robot=<id>` — absent = default robot (`robots[0]`).
#[derive(Deserialize)]
struct RobotQuery {
    robot: Option<u8>,
}

/// `GET /api/status?robot=` — the robot's latest WEBMON event.
async fn status_now(State(st): State<AppState>, Query(q): Query<RobotQuery>) -> ApiResult<Json> {
    let r = st.robot(q.robot)?;
    r.status.latest().map(axum::Json).ok_or_else(|| ApiError::PlcUnavailable(format!("{} ({}) WEBMON snapshot not read yet", r.name, r.plc)))
}

/// `GET /api/status/stream?robot=` — SSE `event: status` of that robot only.
async fn status_stream(State(st): State<AppState>, Query(q): Query<RobotQuery>) -> Result<impl IntoResponse, ApiError> {
    let r = st.robot(q.robot)?;
    Ok(broadcast_sse(r.status.tx.subscribe(), "status", r.status.latest()))
}

async fn opcua_state(State(st): State<AppState>, Query(q): Query<RobotQuery>) -> ApiResult<Json> {
    Ok(axum::Json(serde_json::to_value(st.robot(q.robot)?.cmd.status())?))
}

async fn opcua_nodes(State(st): State<AppState>, Query(q): Query<RobotQuery>) -> ApiResult<Json> {
    Ok(axum::Json(st.robot(q.robot)?.cmd.nodes()))
}

async fn opcua_rebrowse(State(st): State<AppState>, Query(q): Query<RobotQuery>) -> ApiResult<Json> {
    let r = st.robot(q.robot)?;
    r.cmd.rebrowse().await;
    Ok(axum::Json(serde_json::to_value(r.cmd.status())?))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/plcs", get(plcs))
        .route("/api/robots", get(robots))
        .route("/api/plcs/{id}/check", post(plc_check))
        .route("/api/plcs/{id}/reconnect", post(plc_reconnect))
        .route("/api/health", get(health))
        .route("/api/plc/{plc}/db/{db}", get(db_json))
        .route("/api/plc/{plc}/layout/{db}", get(db_layout))
        .route("/api/plc/{plc}/events", get(plc_events))
        .route("/api/status", get(status_now))
        .route("/api/status/stream", get(status_stream))
        .route("/api/opcua/state", get(opcua_state))
        .route("/api/opcua/nodes", get(opcua_nodes))
        .route("/api/opcua/rebrowse", post(opcua_rebrowse))
}
