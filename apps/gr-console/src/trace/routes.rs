//! `/api/trace/*`: channel catalogue, session control, stored sessions and the live chunk stream.

use axum::Router;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use serde::Deserialize;
use serde_json::{Value as Json, json};

use crate::error::{ApiError, ApiResult};
use crate::sse::broadcast_sse;
use crate::state::AppState;
use crate::trace::channels;
use crate::trace::store::{READ_DEFAULT, StartReq};

#[derive(Deserialize)]
struct CatalogQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct ReadQuery {
    from: Option<i64>,
    to: Option<i64>,
    max: Option<usize>,
}

#[derive(Deserialize)]
struct MarkReq {
    #[serde(default)]
    text: String,
}

/// Traceable variables, filtered by a substring of the path.
async fn catalog(State(s): State<AppState>, Query(q): Query<CatalogQuery>) -> ApiResult<Json> {
    let store = s.trace.as_ref().ok_or_else(|| ApiError::Internal("trace is not configured".into()))?;
    let limit = q.limit.unwrap_or(500).clamp(1, 5000);
    let (hits, total) = channels::search(store.contract(), &q.q, limit);
    Ok(axum::Json(json!({
        "plc": store.plc(),
        "databases": channels::databases(store.contract()),
        "channels": hits,
        "total": total,
        "limit": limit,
        "chan_max": channels::CHAN_MAX,
        "flush_min_ms": channels::FLUSH_MIN_MS,
    })))
}

/// Current session, link readiness and the stored sessions.
async fn overview(State(s): State<AppState>) -> ApiResult<Json> {
    let store = s.trace.as_ref().ok_or_else(|| ApiError::Internal("trace is not configured".into()))?;
    Ok(axum::Json(json!({
        "plc": store.plc(),
        "link_ready": store.link_ready().await,
        "current": store.current().await,
        "sessions": store.list(),
    })))
}

async fn start(State(s): State<AppState>, axum::Json(req): axum::Json<StartReq>) -> ApiResult<Json> {
    let store = s.trace.as_ref().ok_or_else(|| ApiError::Internal("trace is not configured".into()))?;
    let meta = store.start(req).await.map_err(classify)?;
    Ok(axum::Json(json!({ "current": meta })))
}

async fn stop(State(s): State<AppState>) -> ApiResult<Json> {
    let store = s.trace.as_ref().ok_or_else(|| ApiError::Internal("trace is not configured".into()))?;
    let meta = store.stop().await.map_err(classify)?;
    Ok(axum::Json(json!({ "session": meta })))
}

async fn mark(State(s): State<AppState>, axum::Json(req): axum::Json<MarkReq>) -> ApiResult<Json> {
    let store = s.trace.as_ref().ok_or_else(|| ApiError::Internal("trace is not configured".into()))?;
    let m = store.mark(&req.text).await.map_err(classify)?;
    Ok(axum::Json(json!({ "mark": m })))
}

async fn read(State(s): State<AppState>, UrlPath(id): UrlPath<String>, Query(q): Query<ReadQuery>) -> ApiResult<Json> {
    let store = s.trace.as_ref().ok_or_else(|| ApiError::Internal("trace is not configured".into()))?;
    let (meta, marks, rows) = store.read(&id, q.from, q.to, q.max.unwrap_or(READ_DEFAULT)).map_err(classify)?;
    Ok(axum::Json(json!({ "meta": meta, "marks": marks, "rows": rows, "returned": rows.len() })))
}

async fn export_csv(State(s): State<AppState>, UrlPath(id): UrlPath<String>) -> Result<impl IntoResponse, ApiError> {
    let store = s.trace.as_ref().ok_or_else(|| ApiError::Internal("trace is not configured".into()))?;
    let csv = store.csv(&id).map_err(classify)?;
    let headers = [(header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"trace-{id}.csv\""))];
    Ok((headers, csv))
}

async fn remove(State(s): State<AppState>, UrlPath(id): UrlPath<String>) -> ApiResult<Json> {
    let store = s.trace.as_ref().ok_or_else(|| ApiError::Internal("trace is not configured".into()))?;
    store.delete(&id).map_err(classify)?;
    Ok(axum::Json(json!({ "deleted": id })))
}

/// Live chunks: one event per received chunk, plus `mark` and `stopped` events.
async fn stream(State(s): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let store = s.trace.as_ref().ok_or_else(|| ApiError::Internal("trace is not configured".into()))?;
    Ok(broadcast_sse(store.subscribe(), "trace", None))
}

/// Maps a store error onto the API error the client should see.
fn classify(e: String) -> ApiError {
    if e.contains("already running") {
        ApiError::Conflict(e)
    } else if e.contains("no such session") || e.contains("not a session id") {
        ApiError::NotFound(e)
    } else if e.contains("no PLC link") || e.contains("link stopped") || e.contains("did not answer") || e.contains("did not acknowledge") || e.contains("link closed") {
        ApiError::PlcUnavailable(e)
    } else {
        ApiError::BadRequest(e)
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/trace", get(overview))
        .route("/api/trace/channels", get(catalog))
        .route("/api/trace/start", post(start))
        .route("/api/trace/stop", post(stop))
        .route("/api/trace/mark", post(mark))
        .route("/api/trace/stream", get(stream))
        .route("/api/trace/{id}", get(read))
        .route("/api/trace/{id}", delete(remove))
        .route("/api/trace/{id}/export.csv", get(export_csv))
}
