//! `/api/scenarios*` (M3-C slice implements run/pause/stop/import/export; M1 provides CRUD + stream).

use axum::Router;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use serde_json::{Value as Json, json};

use super::Scenario;
use crate::error::{ApiError, ApiResult};
use crate::sse::broadcast_sse;
use crate::state::AppState;

async fn list(State(st): State<AppState>) -> ApiResult<Vec<Scenario>> {
    Ok(axum::Json(st.scenario.list()?))
}
async fn one(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Scenario> {
    st.scenario.get(&id)?.map(axum::Json).ok_or_else(|| ApiError::NotFound(format!("scenario {id}")))
}
async fn create(State(st): State<AppState>, axum::Json(s): axum::Json<Scenario>) -> ApiResult<Scenario> {
    Ok(axum::Json(st.scenario.save(Scenario { id: String::new(), ..s })?))
}
async fn update(State(st): State<AppState>, Path(id): Path<String>, axum::Json(s): axum::Json<Scenario>) -> ApiResult<Scenario> {
    let existing = st.scenario.get(&id)?.ok_or_else(|| ApiError::NotFound(format!("scenario {id}")))?;
    Ok(axum::Json(st.scenario.save(Scenario { id, created_at: existing.created_at, ..s })?))
}
async fn delete(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Json> {
    Ok(axum::Json(json!({ "deleted": st.scenario.delete(&id)? })))
}
async fn run_now(State(st): State<AppState>) -> ApiResult<super::RunState> {
    Ok(axum::Json(st.scenario.current()))
}
async fn not_implemented() -> ApiError {
    ApiError::Conflict("scenario runner not implemented yet (M3-C)".into())
}
async fn runs_stream(State(st): State<AppState>) -> impl IntoResponse {
    broadcast_sse(st.scenario.events.subscribe(), "run", Some(st.scenario.current()))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/scenarios", get(list).post(create))
        .route("/api/scenarios/run", get(run_now))
        .route("/api/scenarios/run/pause", post(not_implemented))
        .route("/api/scenarios/run/resume", post(not_implemented))
        .route("/api/scenarios/run/stop", post(not_implemented))
        .route("/api/scenarios/runs/stream", get(runs_stream))
        .route("/api/scenarios/{id}", get(one).put(update).delete(delete))
        .route("/api/scenarios/{id}/run", post(not_implemented))
}
