//! 수동작업 목록(옛 시퀀스 시나리오) — 지금 대기열의 사람 작업을 이름 붙여 저장하고, 불러오면 그 순서대로 다시 넣는다.
//! 옛 시나리오는 실행기가 은퇴했다(송신자는 작업 대기열 하나) — 가져오기로 목록을 만든다(짝 = PICK 뒤 같은 로봇의 DROP).
//! 저장은 `settings` 의 `job_templates` 한 줄(JSON). 불러올 때 Z 는 그 시점 재고로 다시 작성된다(작업을 보낼 때 compose).

use axum::Router;
use axum::extract::{Path, State};
use axum::routing::{delete, get, post};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use super::{NewJob, Stage, queue_order, store};
use crate::error::{ApiError, ApiResult};
use crate::ledger::{Origin, TaskRequest};
use crate::state::AppState;
use crate::util::now_str;

const KEY: &str = "job_templates";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct JobTemplate {
    pub id: String,
    pub name: String,
    /// 작업마다 한 건 또는 PICK + DROP.
    pub jobs: Vec<Vec<TaskRequest>>,
    pub note: String,
    pub created_at: String,
}

fn load(st: &AppState) -> Vec<JobTemplate> {
    st.db.setting(KEY).ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save(st: &AppState, v: &[JobTemplate]) -> Result<(), ApiError> {
    st.db.set_setting(KEY, &serde_json::to_string(v)?)?;
    Ok(())
}

/// 평평한 스텝 목록 → 작업(짝은 PICK 과 그 뒤 같은 로봇의 첫 스텝이 DROP 일 때). 짝이 안 되는 PICK 은 버리고 알린다(순수).
pub fn pair_steps(steps: &[TaskRequest]) -> (Vec<Vec<TaskRequest>>, usize) {
    let mut used = vec![false; steps.len()];
    let mut out = Vec::new();
    let mut dropped = 0;
    for i in 0..steps.len() {
        if used[i] {
            continue;
        }
        let s = &steps[i];
        if s.task_type.eq_ignore_ascii_case("PICK") {
            let j = (i + 1..steps.len()).find(|&j| !used[j] && steps[j].robot == s.robot);
            match j {
                Some(j) if steps[j].task_type.eq_ignore_ascii_case("DROP") => {
                    used[i] = true;
                    used[j] = true;
                    out.push(vec![s.clone(), steps[j].clone()]);
                }
                _ => {
                    used[i] = true;
                    dropped += 1;
                }
            }
            continue;
        }
        used[i] = true;
        out.push(vec![s.clone()]);
    }
    (out, dropped)
}

async fn list(State(st): State<AppState>) -> ApiResult<Json> {
    Ok(axum::Json(serde_json::to_value(load(&st)).unwrap_or_default()))
}

#[derive(Deserialize)]
struct SaveBody {
    name: String,
    #[serde(default)]
    robot: Option<u8>,
    /// 바로 저장할 작업(팔렛 패턴 화면) — 없으면 `robot` 대기열의 사람 작업.
    #[serde(default)]
    jobs: Option<Vec<Vec<TaskRequest>>>,
}

/// 지금 이 로봇 대기열의 **사람 작업**(대기 중, 나갈 순서대로)을 목록으로 저장.
async fn save_current(State(st): State<AppState>, axum::Json(b): axum::Json<SaveBody>) -> ApiResult<Json> {
    let s = store().ok_or_else(|| ApiError::Internal("job store not ready".into()))?;
    let all = s.list();
    let jobs: Vec<Vec<TaskRequest>> = if let Some(j) = b.jobs.clone() {
        for (i, steps) in j.iter().enumerate() {
            super::check_shape(steps).map_err(|e| ApiError::BadRequest(format!("작업 {}: {e}", i + 1)))?;
        }
        j
    } else {
        let robot = b.robot.ok_or_else(|| ApiError::BadRequest("robot 또는 jobs 가 필요합니다".into()))?;
        queue_order(&all, robot)
            .into_iter()
            .filter_map(|id| all.iter().find(|j| j.id == id && j.stage == Stage::Wait && j.origin == Origin::Manual))
            .map(|j| {
                j.steps
                    .iter()
                    .map(|x| {
                        let mut r = x.request.clone();
                        r.transfer_order_id = None;
                        r.robot = None;
                        r
                    })
                    .collect()
            })
            .collect()
    };
    if jobs.is_empty() {
        return Err(ApiError::BadRequest("저장할 수동작업이 없습니다 — 대기 중인 사람 작업만 저장됩니다".into()));
    }
    let name = b.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("이름이 필요합니다".into()));
    }
    let mut v = load(&st);
    let t = JobTemplate { id: uuid::Uuid::new_v4().to_string(), name: name.into(), jobs, note: String::new(), created_at: now_str() };
    v.push(t.clone());
    save(&st, &v)?;
    Ok(axum::Json(serde_json::to_value(t).unwrap_or_default()))
}

#[derive(Deserialize)]
struct LoadBody {
    robot: u8,
}

/// 목록을 이 로봇 대기열 끝에 차례대로 넣는다. 넣지 못한 작업은 사유와 함께 돌려준다(나머지는 넣는다).
async fn load_into(State(st): State<AppState>, Path(id): Path<String>, axum::Json(b): axum::Json<LoadBody>) -> ApiResult<Json> {
    let t = load(&st).into_iter().find(|t| t.id == id).ok_or_else(|| ApiError::NotFound(format!("수동작업 목록 {id}")))?;
    let mut added = Vec::new();
    let mut failed = Vec::new();
    for (i, steps) in t.jobs.iter().enumerate() {
        let n = NewJob { robot: Some(b.robot), steps: steps.clone(), priority: None, note: format!("{} #{}", t.name, i + 1) };
        match super::enqueue(&st, n, Origin::Manual, None) {
            Ok(j) => added.push(j.work_id),
            Err(e) => failed.push(json!({ "index": i + 1, "error": e.to_string() })),
        }
    }
    Ok(axum::Json(json!({ "added": added, "failed": failed })))
}

async fn remove(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Json> {
    let mut v = load(&st);
    let n = v.len();
    v.retain(|t| t.id != id);
    if v.len() == n {
        return Err(ApiError::NotFound(format!("수동작업 목록 {id}")));
    }
    save(&st, &v)?;
    Ok(axum::Json(json!({ "removed": id })))
}

/// 옛 시나리오 → 수동작업 목록(실행기 은퇴, 2026-10-01). 스텝의 대기 조건 · 실패 처리 · 반복은 옮기지 않는다.
async fn from_scenario(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Json> {
    let sc = st.scenario.get(&id)?.ok_or_else(|| ApiError::NotFound(format!("scenario {id}")))?;
    let flat: Vec<TaskRequest> = sc
        .steps
        .iter()
        .map(|s| TaskRequest {
            task_type: format!("{:?}", s.task_type).to_ascii_uppercase(),
            target: s.target.clone(),
            item_code: s.item_code,
            count: s.count.clamp(1, 255) as u8,
            params: s.params.clone(),
            note: if s.label.trim().is_empty() { s.note.clone() } else { s.label.clone() },
            robot: s.robot,
            pallet: s.pallet.clone(),
            ..Default::default()
        })
        .collect();
    let (jobs, dropped) = pair_steps(&flat);
    let jobs: Vec<Vec<TaskRequest>> = jobs
        .into_iter()
        .map(|j| {
            j.into_iter()
                .map(|mut r| {
                    r.robot = None;
                    r
                })
                .collect()
        })
        .collect();
    let mut v = load(&st);
    let t = JobTemplate {
        id: uuid::Uuid::new_v4().to_string(),
        name: sc.name.clone(),
        jobs,
        note: if dropped > 0 { format!("시나리오에서 가져옴 — 짝 없는 PICK {dropped}건 뺌") } else { "시나리오에서 가져옴".into() },
        created_at: now_str(),
    };
    v.push(t.clone());
    save(&st, &v)?;
    Ok(axum::Json(serde_json::to_value(t).unwrap_or_default()))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/jobs/templates", get(list).post(save_current))
        .route("/api/jobs/templates/{id}", delete(remove))
        .route("/api/jobs/templates/{id}/load", post(load_into))
        .route("/api/jobs/templates/from-scenario/{id}", post(from_scenario))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(op: &str, robot: Option<u8>) -> TaskRequest {
        TaskRequest { task_type: op.into(), robot, count: 1, ..Default::default() }
    }

    #[test]
    fn pairs_pick_with_next_same_robot_drop() {
        let (jobs, dropped) = pair_steps(&[r("PICK", Some(2)), r("MOVE", Some(1)), r("DROP", Some(2)), r("MEASURE", None), r("PICK", None)]);
        let shape: Vec<Vec<String>> = jobs.iter().map(|j| j.iter().map(|x| x.task_type.clone()).collect()).collect();
        assert_eq!(shape, vec![vec!["PICK".to_string(), "DROP".to_string()], vec!["MOVE".to_string()], vec!["MEASURE".to_string()]]);
        assert_eq!(dropped, 1, "짝 없는 PICK 은 뺀다");
    }
}
