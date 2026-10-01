//! `/api/jobs/*` — 작업 대기열.
//!
//! - `GET  /api/jobs?robot=&ended=` — 끝나지 않은 작업 + 끝난 작업(최근 `ended` 건, 기본 50) + 로봇별 보내기 상태 + 대기 순번
//! - `POST /api/jobs` — 사람이 넣는 작업(`NewJob`, 할당 주체 = 사람)
//! - `PATCH /api/jobs/{id}` — `{priority}` 또는 `{front: true}`(대기 · 예정만)
//! - `POST /api/jobs/{id}/cancel` · `POST /api/jobs/{id}/resend`(유실 단계 다시 보내기)
//! - `PUT  /api/jobs/dispatch/{robot}` — `{enabled}` · `POST …/resume`(멈춤 풀기) · `POST …/next`(지금 한 건)

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::routing::{get, patch, post, put};
use serde::Deserialize;
use serde_json::{Value as Json, json};

use super::{NewJob, Stage, queue_order, store};
use crate::error::{ApiError, ApiResult};
use crate::ledger::Origin;
use crate::state::AppState;

fn s() -> Result<&'static std::sync::Arc<super::JobStore>, ApiError> {
    store().ok_or_else(|| ApiError::Internal("job store not ready".into()))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ListQuery {
    robot: Option<u8>,
    ended: Option<u32>,
}

async fn list(State(st): State<AppState>, Query(q): Query<ListQuery>) -> ApiResult<Json> {
    let s = s()?;
    let active: Vec<_> = s.list().into_iter().filter(|j| !j.stage.is_end() && (q.robot.is_none() || j.robot == q.robot)).collect();
    let ended = s.history(q.robot, q.ended.unwrap_or(50).min(500))?;
    let robots: Vec<Json> = st
        .robots
        .iter()
        .filter(|r| q.robot.is_none_or(|id| id == r.id))
        .map(|r| {
            let d = s.dispatch_state(r.id);
            json!({ "robot": r.id, "name": r.name, "enabled": d.enabled, "paused": d.paused, "order": queue_order(&active, r.id) })
        })
        .collect();
    Ok(axum::Json(json!({ "active": active, "ended": ended, "dispatch": robots, "planned": planned(&st, q.robot) })))
}

/// 예정(작업이 되기 전) — 자동 생성 엔진의 마지막 판정 중 아직 대기열에 안 든 후보: 막혀 기다리는 것과, 자동 생성이 꺼져
/// 있으면 만들 후보까지. 작업 표의 "예정" 행(WorkId 없음)이다. 모양은 작업과 같게 낸다(화면이 한 표로 그린다).
fn planned(st: &AppState, robot: Option<u8>) -> Vec<Json> {
    let Some(e) = crate::taskgen::run::engine() else { return vec![] };
    let sel = e.last.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let auto = e.config().auto;
    let now = crate::util::now_str();
    let step = |t: &crate::ledger::Target, op: &str, item: Option<u32>| json!({ "request": { "type": op, "target": t, "item_code": item, "count": 1, "params": {}, "position_override": null, "note": "", "source": null }, "task": null, "state": null });
    let row = |c: &crate::taskgen::Candidate, why: String| {
        let steps: Vec<Json> = match &c.second {
            Some(s) => vec![step(&c.first, "PICK", c.item), step(s, "DROP", c.item)],
            None => vec![step(&c.first, "MEASURE", c.item)],
        };
        json!({
            "id": format!("pre:{}:{}", c.rule_id, c.robot), "seq": 0, "robot": c.robot, "stage": "pre", "origin": "auto",
            "via": if c.rule_id.starts_with("req:") { c.rule_id.clone() } else { format!("gen:{}", c.rule_id) },
            "priority": c.score.round() as i64, "work_id": null, "transfer_order_id": null, "steps": steps,
            "wait_reason": why, "error": null, "note": c.rule_name, "created_at": now, "updated_at": now, "ended_at": null, "history": [],
        })
    };
    let mut out: Vec<Json> = sel.waiting.iter().map(|(c, w)| row(c, w.clone())).collect();
    if !auto {
        out.extend(sel.generate.iter().map(|c| row(c, "자동 생성 꺼짐 — 켜면 이 작업이 대기열에 들어감".into())));
    }
    out.retain(|j| robot.is_none_or(|r| j["robot"].as_u64() == Some(u64::from(r))));
    let _ = st;
    out
}

async fn create(State(st): State<AppState>, axum::Json(n): axum::Json<NewJob>) -> ApiResult<Json> {
    let job = super::enqueue(&st, n, Origin::Manual, None)?;
    Ok(axum::Json(serde_json::to_value(job).unwrap_or_default()))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct PatchBody {
    priority: Option<i32>,
    front: bool,
}

async fn patch_job(Path(id): Path<String>, axum::Json(b): axum::Json<PatchBody>) -> ApiResult<Json> {
    let s = s()?;
    let all = s.list();
    let job = s.update(&id, |j| {
        if !matches!(j.stage, Stage::Wait | Stage::Pre) {
            return Err(ApiError::Conflict(format!("{} 작업은 순서를 바꿀 수 없습니다", j.stage.label())));
        }
        if b.front {
            let top = all.iter().filter(|o| o.robot == j.robot && o.stage == Stage::Wait && o.id != j.id).map(|o| o.priority).max().unwrap_or(j.priority);
            j.priority = j.priority.max(top + 1);
        }
        if let Some(p) = b.priority {
            j.priority = p.clamp(0, 999);
        }
        Ok(())
    })?;
    Ok(axum::Json(serde_json::to_value(job).unwrap_or_default()))
}

async fn resend(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Json> {
    let job = super::dispatch::resend(&st, &id)?;
    Ok(axum::Json(serde_json::to_value(job).unwrap_or_default()))
}

async fn cancel(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Json> {
    let job = super::dispatch::cancel(&st, &id).await?;
    Ok(axum::Json(serde_json::to_value(job).unwrap_or_default()))
}

#[derive(Deserialize)]
struct EnableBody {
    enabled: bool,
}

async fn set_enabled(State(st): State<AppState>, Path(robot): Path<u8>, axum::Json(b): axum::Json<EnableBody>) -> ApiResult<Json> {
    st.robot(Some(robot))?;
    let s = s()?;
    let mut d = s.dispatch_state(robot);
    d.enabled = b.enabled;
    s.set_dispatch(&d)?;
    Ok(axum::Json(serde_json::to_value(d).unwrap_or_default()))
}

async fn resume(State(st): State<AppState>, Path(robot): Path<u8>) -> ApiResult<Json> {
    st.robot(Some(robot))?;
    let s = s()?;
    let mut d = s.dispatch_state(robot);
    d.paused = None;
    s.set_dispatch(&d)?;
    Ok(axum::Json(serde_json::to_value(d).unwrap_or_default()))
}

/// 지금 한 건 — 보내기 스위치와 상관없이 판단 한 번(멈춤 상태면 거부).
async fn next(State(st): State<AppState>, Path(robot): Path<u8>) -> ApiResult<Json> {
    let r = st.robot(Some(robot))?;
    if let Some(p) = s()?.dispatch_state(robot).paused {
        return Err(ApiError::Conflict(format!("{} 보내기 멈춤 — {p}", r.name)));
    }
    super::dispatch::refresh(&st);
    Ok(axum::Json(match super::dispatch::step(&st, r).await? {
        Ok(e) => json!({ "sent": e }),
        Err(why) => json!({ "wait": why }),
    }))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/jobs", get(list).post(create))
        .route("/api/jobs/{id}", patch(patch_job))
        .route("/api/jobs/{id}/cancel", post(cancel))
        .route("/api/jobs/{id}/resend", post(resend))
        .route("/api/jobs/dispatch/{robot}", put(set_enabled))
        .route("/api/jobs/dispatch/{robot}/resume", post(resume))
        .route("/api/jobs/dispatch/{robot}/next", post(next))
}
