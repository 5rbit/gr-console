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
    pub label: String,
    pub kind: &'static str,
    pub endpoint: String,
    pub connected: bool,
    pub rtt_ms: Option<u64>,
    pub last_ok_at: Option<String>,
    pub last_error: Option<String>,
    pub layout: LayoutCheckView,
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
            label: format!("{name} S7"),
            kind: "s7",
            endpoint: format!("{}:{}", h.cfg.host, h.cfg.port),
            connected: hl.connected,
            rtt_ms: hl.rtt_ms,
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
    let cs = st.cmd.status();
    out.push(PlcStatusView {
        id: "grm_opcua".into(),
        label: "GRM OPC UA".into(),
        kind: "opcua",
        endpoint: cs.endpoint,
        connected: cs.ready,
        rtt_ms: None,
        last_ok_at: cs.last_ok_at,
        last_error: cs.error,
        layout: LayoutCheckView { ok: cs.ready.then_some(true), detail: cs.detail, checked_at: None, mismatches: vec![] },
    });
    out
}

async fn plcs(State(st): State<AppState>) -> ApiResult<Vec<PlcStatusView>> {
    Ok(axum::Json(plc_status_views(&st)))
}

async fn plc_check(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Vec<PlcStatusView>> {
    if id == "grm_opcua" {
        st.cmd.rebrowse().await;
    } else if let Some(h) = st.plc_by_id(&id) {
        h.reverify().await;
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    } else {
        return Err(ApiError::NotFound(format!("plc {id}")));
    }
    Ok(axum::Json(plc_status_views(&st)))
}

async fn plc_reconnect(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Vec<PlcStatusView>> {
    if id == "grm_opcua" {
        st.cmd.rebrowse().await;
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
    Ok(axum::Json(json!({ "plcs": plcs, "opcua": st.cmd.status(), "demo": st.cfg.demo, "at": crate::util::now_str() })))
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

async fn status_now(State(st): State<AppState>) -> ApiResult<Json> {
    st.status.latest().map(axum::Json).ok_or_else(|| ApiError::PlcUnavailable("no WEBMON snapshot yet".into()))
}

async fn status_stream(State(st): State<AppState>) -> impl IntoResponse {
    broadcast_sse(st.status.tx.subscribe(), "status", st.status.latest())
}

async fn opcua_state(State(st): State<AppState>) -> ApiResult<Json> {
    Ok(axum::Json(serde_json::to_value(st.cmd.status())?))
}

async fn opcua_nodes(State(st): State<AppState>) -> ApiResult<Json> {
    Ok(axum::Json(st.cmd.nodes()))
}

async fn opcua_rebrowse(State(st): State<AppState>) -> ApiResult<Json> {
    st.cmd.rebrowse().await;
    Ok(axum::Json(serde_json::to_value(st.cmd.status())?))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/plcs", get(plcs))
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
