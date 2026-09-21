//! `GET /api/taskgen` (설정 · 후보와 점수 근거 · 예정 큐) · `PUT /api/taskgen/config` (버전 +1)
//! · `POST /api/taskgen/rules/{id}/request` (수동 요청 +1) · `DELETE /api/taskgen/queue/{id}` (안 보낸 예정 지우기)

use axum::Router;
use axum::extract::{Path, State};
use axum::routing::{delete, get, post, put};
use serde_json::{Value as Json, json};

use super::GenConfig;
use super::run::engine;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

fn eng() -> Result<std::sync::Arc<super::run::Engine>, ApiError> {
    engine().ok_or_else(|| ApiError::Internal("task generator not started".into()))
}

async fn get_all(State(st): State<AppState>) -> ApiResult<Json> {
    let e = eng()?;
    let sel = e.last.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let queue = e.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let note = e.note.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let area = crate::area::load(&st.db);
    let robot_name = |id: u8| st.robots.iter().find(|r| r.id == id).map(|r| r.name.clone()).unwrap_or_else(|| format!("로봇 {id}"));
    let cand = |c: &super::Candidate, why: Option<&String>| {
        json!({ "rule_id": c.rule_id, "rule_name": c.rule_name, "robot": c.robot, "robot_name": robot_name(c.robot), "score": c.score,
            "breakdown": c.breakdown, "area": c.area, "age_min": c.age_min, "order": c.order, "reason": why })
    };
    let candidates: Vec<Json> = sel.generate.iter().map(|c| cand(c, None)).chain(sel.waiting.iter().map(|(c, w)| cand(c, Some(w)))).collect();
    Ok(axum::Json(json!({ "config": e.config(), "candidates": candidates, "queue": queue, "note": note, "separation_mm": area.separation_mm })))
}

async fn put_config(axum::Json(c): axum::Json<GenConfig>) -> ApiResult<GenConfig> {
    let mut ids = std::collections::HashSet::new();
    for r in &c.rules {
        if r.id.trim().is_empty() || !ids.insert(r.id.clone()) {
            return Err(ApiError::BadRequest(format!("rule id '{}' 가 비었거나 겹침", r.id)));
        }
    }
    Ok(axum::Json(eng()?.save(c)?))
}

async fn request(Path(id): Path<String>) -> ApiResult<Json> {
    let e = eng()?;
    if !e.config().rules.iter().any(|r| r.id == id) {
        return Err(ApiError::NotFound(format!("rule {id}")));
    }
    Ok(axum::Json(json!({ "rule": id, "requests": e.request(&id) })))
}

async fn remove(Path(id): Path<String>) -> ApiResult<Json> {
    eng()?.remove(&id)?;
    Ok(axum::Json(json!({ "removed": id })))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/taskgen", get(get_all))
        .route("/api/taskgen/config", put(put_config))
        .route("/api/taskgen/rules/{id}/request", post(request))
        .route("/api/taskgen/queue/{id}", delete(remove))
}
