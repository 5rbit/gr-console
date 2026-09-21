//! `GET /api/taskgen` (설정 · 후보와 점수 근거 · 예정 큐 · 지표) · `PUT /api/taskgen/config` (버전 +1)
//! · `POST /api/taskgen/rules/{id}/request` (수동 요청 +1) · `DELETE /api/taskgen/queue/{id}` (안 보낸 예정 지우기)
//! · `GET/PUT /api/params` · `GET /api/params/history` · `POST /api/params/reset` (스케줄링 파라미터 한 곳)

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::routing::{delete, get, post, put};
use serde::Deserialize;
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
    let metrics = e.metrics.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let p = crate::params::current(&st.db);
    let robot_name = |id: u8| st.robots.iter().find(|r| r.id == id).map(|r| r.name.clone()).unwrap_or_else(|| format!("로봇 {id}"));
    let cand = |c: &super::Candidate, why: Option<&String>| {
        json!({ "rule_id": c.rule_id, "rule_name": c.rule_name, "robot": c.robot, "robot_name": robot_name(c.robot), "score": c.score,
            "breakdown": c.breakdown, "area": c.area, "first": c.first, "second": c.second, "age_min": c.age_min, "order": c.order, "reason": why })
    };
    let candidates: Vec<Json> = sel.generate.iter().map(|c| cand(c, None)).chain(sel.waiting.iter().map(|(c, w)| cand(c, Some(w)))).collect();
    let skipped: Vec<Json> = sel.skipped.iter().map(|(r, w)| json!({ "rule": r, "reason": w })).collect();
    Ok(axum::Json(json!({ "config": e.config(), "candidates": candidates, "skipped": skipped, "queue": queue, "note": note, "metrics": metrics, "separation_mm": p.anticol_separation_mm })))
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

async fn remove(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Json> {
    let e = eng()?;
    let order = e.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().find(|g| g.id == id).and_then(|g| g.transfer_order_id.clone());
    e.remove(&id)?;
    // 한 번도 안 보낸 짝이 이미 지시를 열었으면(제출 실패 재시도 중) 닫는다.
    if let Some(o) = order {
        st.stock.abort_order(&o, "예정 지우기 — 보내지 않음", false)?;
    }
    Ok(axum::Json(json!({ "removed": id })))
}

/// PLC 계산 간격(두 로봇 중 큰 값) — 콘솔 간격의 하한.
pub fn plc_max(st: &AppState) -> Option<f64> {
    crate::params::plc_anticol(st).iter().filter_map(|p| p.separation).fold(None, |a, x| Some(a.map_or(x, |a: f64| a.max(x))))
}

async fn params_get(State(st): State<AppState>) -> ApiResult<Json> {
    let p = crate::params::current(&st.db);
    Ok(axum::Json(json!({
        "version": crate::params::version(&st.db),
        "params": p,
        "defaults": crate::params::Params::default(),
        "spec": crate::params::spec(),
        "plc": crate::params::plc_anticol(&st),
        "plc_separation_max": plc_max(&st),
        "warnings": crate::params::warnings(&st.db, &p),
        "config": { "echo_timeout_ms": st.cfg.cmd.echo_timeout_ms },
        "robots": st.robots.iter().map(|r| json!({ "id": r.id, "name": r.name })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
struct PutParams {
    params: crate::params::Params,
    #[serde(default)]
    by: String,
}

async fn params_put(State(st): State<AppState>, axum::Json(b): axum::Json<PutParams>) -> ApiResult<Json> {
    let by = if b.by.trim().is_empty() { "console" } else { b.by.trim() };
    crate::params::check_plc_floor(b.params.anticol_separation_mm, plc_max(&st)).map_err(ApiError::BadRequest)?;
    let (v, p, ch) = crate::params::save(&st.db, b.params, by)?;
    Ok(axum::Json(json!({ "version": v, "params": p, "changes": ch.iter().map(|(k, a, n)| json!({ "key": k, "old": a, "new": n })).collect::<Vec<_>>() })))
}

#[derive(Deserialize)]
struct ResetBody {
    /// 되돌릴 키(비면 전부).
    #[serde(default)]
    keys: Vec<String>,
    #[serde(default)]
    by: String,
}

async fn params_reset(State(st): State<AppState>, axum::Json(b): axum::Json<ResetBody>) -> ApiResult<Json> {
    let cur = serde_json::to_value(crate::params::current(&st.db))?;
    let def = serde_json::to_value(crate::params::Params::default())?;
    let mut next = cur.clone();
    for sp in crate::params::spec() {
        if b.keys.is_empty() || b.keys.iter().any(|k| k == sp.key) {
            next[sp.key] = def[sp.key].clone();
        }
    }
    let p: crate::params::Params = serde_json::from_value(next)?;
    let by = if b.by.trim().is_empty() { "console (reset)" } else { b.by.trim() };
    let (v, p, ch) = crate::params::save(&st.db, p, by)?;
    Ok(axum::Json(json!({ "version": v, "params": p, "changes": ch.len() })))
}

#[derive(Deserialize)]
struct HistQ {
    limit: Option<u32>,
}

async fn params_history(State(st): State<AppState>, Query(q): Query<HistQ>) -> ApiResult<Json> {
    Ok(axum::Json(Json::Array(crate::params::history(&st.db, q.limit.unwrap_or(50).clamp(1, 500))?)))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/taskgen", get(get_all))
        .route("/api/taskgen/config", put(put_config))
        .route("/api/taskgen/rules/{id}/request", post(request))
        .route("/api/taskgen/queue/{id}", delete(remove))
        .route("/api/params", get(params_get).put(params_put))
        .route("/api/params/history", get(params_history))
        .route("/api/params/reset", post(params_reset))
}
