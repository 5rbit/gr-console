//! `/api/tasks*` — task ledger REST + SSE.
//!
//! `GET /api/tasks?state=active|terminal|a,b&type=PICK&origin=console|scenario|external&since=<RFC3339>&q=&limit=&offset=`
//! `GET /api/tasks/stats` · `GET /api/tasks/gate` · `GET /api/tasks/plc-view` · `GET /api/tasks/stream` (SSE snapshot|upsert|remove)
//! `POST /api/tasks[?submit=false]` · `GET|DELETE /api/tasks/{id}` · `POST /api/tasks/{id}/submit|cancel|complete|resubmit|mark-failed`

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::{Value as Json, json};

use super::{LedgerEntry, Origin, QueryFilter, TaskRequest, TaskState};
use crate::error::{ApiError, ApiResult};
use crate::sse::broadcast_sse;
use crate::state::AppState;

#[derive(Deserialize)]
struct ListQuery {
    /// Robot id; absent = every robot.
    robot: Option<u8>,
    state: Option<String>,
    #[serde(rename = "type")]
    task_type: Option<String>,
    q: Option<String>,
    origin: Option<String>,
    /// RFC 3339 lower bound on `created_at`.
    since: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
}

async fn list(State(st): State<AppState>, Query(q): Query<ListQuery>) -> ApiResult<Json> {
    let states = match q.state.as_deref() {
        None | Some("") => None,
        Some("active") => Some(vec![TaskState::Submitted, TaskState::Accepted, TaskState::Queued, TaskState::Running, TaskState::Lost, TaskState::Draft]),
        Some("terminal") => Some(vec![TaskState::Completed, TaskState::Canceled, TaskState::Rejected, TaskState::Failed]),
        Some(list) => Some(list.split(',').filter_map(TaskState::parse).collect()),
    };
    let task_type = q.task_type.as_deref().and_then(|t| match t.to_ascii_uppercase().as_str() {
        "UP" => Some(0x40),
        "PICK" => Some(0x41),
        "DROP" => Some(0x42),
        "MOVE" => Some(0x43),
        "MEASURE" => Some(0x44),
        _ => None,
    });
    let origin = match q.origin.as_deref().filter(|s| !s.is_empty()) {
        None => None,
        Some(o) => Some(Origin::parse(o).ok_or_else(|| ApiError::BadRequest(format!("unknown origin '{o}' (console|scenario|external)")))?),
    };
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);
    let offset = q.offset.unwrap_or(0);
    let f = QueryFilter { states, task_type, origin, since: q.since, q: q.q, limit, offset };
    let (items, total) = match q.robot {
        Some(id) => st.robot(Some(id))?.ledger.query_filtered(f)?,
        None if st.robots.len() == 1 => st.ledger.query_filtered(f)?,
        None => {
            // union across robots: fetch offset+limit from each, merge newest first, then page
            let mut all = Vec::new();
            let mut total = 0;
            for r in st.robots.iter() {
                let (items, t) = r.ledger.query_filtered(QueryFilter { limit: offset + limit, offset: 0, ..f.clone() })?;
                all.extend(items);
                total += t;
            }
            all.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            (all.into_iter().skip(offset).take(limit).collect(), total)
        }
    };
    Ok(axum::Json(json!({ "items": items, "total": total, "limit": limit, "offset": offset })))
}

#[derive(Deserialize)]
struct RobotQuery {
    robot: Option<u8>,
}

async fn stats(State(st): State<AppState>, Query(q): Query<RobotQuery>) -> ApiResult<super::LedgerStats> {
    if let Some(id) = q.robot {
        return Ok(axum::Json(st.robot(Some(id))?.ledger.stats()?));
    }
    let mut out = super::LedgerStats::default();
    let mut sum = 0.0;
    for r in st.robots.iter() {
        let s = r.ledger.stats()?;
        out.total += s.total;
        out.active += s.active;
        out.completed_today += s.completed_today;
        out.rejected_today += s.rejected_today;
        for (k, v) in s.by_state {
            *out.by_state.entry(k).or_insert(0) += v;
        }
        if let Some(a) = s.avg_complete_secs {
            sum += a * s.completed_samples as f64;
            out.completed_samples += s.completed_samples;
        }
    }
    out.avg_complete_secs = (out.completed_samples > 0).then(|| sum / out.completed_samples as f64);
    Ok(axum::Json(out))
}

async fn remove(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Json> {
    super::ops::remove(&st, &id)?;
    Ok(axum::Json(json!({ "removed": id })))
}

async fn one(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<LedgerEntry> {
    st.find_task(&id).map(|(_, e)| axum::Json(e)).ok_or_else(|| ApiError::NotFound(format!("task {id}")))
}

#[derive(Deserialize)]
struct CreateQuery {
    submit: Option<bool>,
}

/// `POST /api/tasks` — composes via the issue slice, then submits (default) or leaves a Draft.
async fn create(State(st): State<AppState>, Query(q): Query<CreateQuery>, axum::Json(req): axum::Json<TaskRequest>) -> ApiResult<LedgerEntry> {
    let r = st.robot(req.robot)?;
    let composed = crate::issue::compose(&st, &req)?;
    let origin = if req.source.is_some() { Origin::Scenario } else { Origin::Console };
    let e = super::ops::create_and_submit(&st, r, origin, Some(req), Some(composed.params), composed.task, composed.pallet, q.submit.unwrap_or(true)).await?;
    Ok(axum::Json(e))
}

async fn submit(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<LedgerEntry> {
    let (r, e) = st.find_task(&id).ok_or_else(|| ApiError::NotFound(format!("task {id}")))?;
    Ok(axum::Json(super::ops::submit(&st, r, e).await?))
}

async fn cancel(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<LedgerEntry> {
    Ok(axum::Json(super::ops::cancel(&st, &id).await?))
}

async fn complete(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<LedgerEntry> {
    Ok(axum::Json(super::ops::force_complete(&st, &id).await?))
}

async fn resubmit(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<LedgerEntry> {
    Ok(axum::Json(super::ops::resubmit(&st, &id).await?))
}

#[derive(Deserialize)]
struct FailBody {
    note: Option<String>,
}

async fn mark_failed(State(st): State<AppState>, Path(id): Path<String>, body: Option<axum::Json<FailBody>>) -> ApiResult<LedgerEntry> {
    Ok(axum::Json(super::ops::mark_failed(&st, &id, body.and_then(|b| b.0.note))?))
}

async fn gate(State(st): State<AppState>, Query(q): Query<RobotQuery>) -> ApiResult<Json> {
    let g = super::ops::gate(&st, st.robot(q.robot)?);
    Ok(axum::Json(json!({ "can_submit": g.can_submit, "reasons": g.reasons })))
}

async fn plc_view(State(st): State<AppState>, Query(q): Query<RobotQuery>) -> ApiResult<Json> {
    let h = st.robot_plc(st.robot(q.robot)?)?;
    let stat = h.decode_path("OPCUA", "STAT").ok_or_else(|| ApiError::PlcUnavailable("no OPCUA snapshot".into()))?;
    let v = gr_proto::StatusView::from_json(&stat)?;
    Ok(axum::Json(
        json!({ "status": v.task.status, "now": v.task.now, "queue": v.task.queue, "completed": v.task.completed, "canceled": v.task.canceled, "rejected": v.task.rejected, "res": v.res, "reject": v.reject_info() }),
    ))
}

async fn stream(State(st): State<AppState>) -> impl IntoResponse {
    let first = super::LedgerEvent::Snapshot { tasks: st.robots.iter().flat_map(|r| r.ledger.list()).collect() };
    broadcast_sse(st.task_events.subscribe(), "tasks", Some(first))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/tasks", get(list).post(create))
        .route("/api/tasks/gate", get(gate))
        .route("/api/tasks/stats", get(stats))
        .route("/api/tasks/plc-view", get(plc_view))
        .route("/api/tasks/stream", get(stream))
        .route("/api/tasks/{id}", get(one).delete(remove))
        .route("/api/tasks/{id}/submit", post(submit))
        .route("/api/tasks/{id}/cancel", post(cancel))
        .route("/api/tasks/{id}/complete", post(complete))
        .route("/api/tasks/{id}/resubmit", post(resubmit))
        .route("/api/tasks/{id}/mark-failed", post(mark_failed))
}
