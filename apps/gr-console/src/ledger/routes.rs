//! `/api/tasks*` — task ledger REST + SSE.
//!
//! `GET /api/tasks?state=active|terminal|a,b&type=PICK&origin=console|scenario|external&since=<RFC3339>&q=&limit=&offset=`
//! `GET /api/tasks/stats` · `GET /api/tasks/gate` · `GET /api/tasks/plc-view` · `GET /api/tasks/stream` (SSE snapshot|upsert|remove)
//! `POST /api/tasks[?submit=false]` · `POST /api/tasks/pair[?submit=false]`(PICK 제출 + 짝 DROP 초안) · `GET|DELETE /api/tasks/{id}` · `POST /api/tasks/{id}/submit|cancel|complete|resubmit|mark-failed`
//! `POST /api/robots/{id}/command/start|stop|reset|buzzerstop|gripper-learn|complete|clear` (`robot_cmd`)

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
    // 로봇이 둘 이상이면 대상을 반드시 받는다 — 빠지면 첫 로봇으로 몰래 가던 사고(2026-09-21, GR2 선택 중 GR1 로 제출).
    let r = st.robot_required(req.robot, "작업 제출")?;
    // PICK 은 늘 DROP 과 짝이다 — 단독 PICK 은 받지 않는다(짝은 순차 계획/시나리오 실행기가 보낸다). DROP 단독은
    // Hand 에 든 것을 내려놓는 복구용으로 받고, 짝 검사(`enforce_hand`)가 품목·수량을 본다.
    if crate::issue::parse_task_type(&req.task_type)? == gr_proto::TaskType::Pick && req.source.is_none() {
        return Err(ApiError::Conflict(crate::ledger::ops::with_robot(&r.name, "PICK 단독 제출 불가 — PICK 은 다음 DROP 과 짝으로 보냅니다(POST /api/tasks/pair)")));
    }
    // 단독 DROP(Hand 복구)은 손에 든 화물의 이송 지시를 잇는다.
    let mut req = req;
    if req.transfer_order_id.is_none() && crate::issue::parse_task_type(&req.task_type)? == gr_proto::TaskType::Drop {
        req.transfer_order_id = st.stock.hand(&r.plc)?.transfer_order_id;
    }
    let composed = crate::issue::compose(&st, &req)?;
    let origin = if req.source.is_some() { Origin::Scenario } else { Origin::Console };
    let e = super::ops::create_and_submit(&st, r, origin, Some(req), Some(composed.params), composed.task, composed.pallet, q.submit.unwrap_or(true)).await?;
    Ok(axum::Json(e))
}

#[derive(Deserialize)]
struct PairBody {
    pick: TaskRequest,
    drop: TaskRequest,
}

/// `POST /api/tasks/pair[?submit=false]` — PICK/DROP 을 **짝으로 만든다**: 이송 지시를 열고 PICK 을 제출(기본)하고,
/// DROP 은 같은 지시의 **초안**으로 둔다. 초안은 로봇이 받을 수 있을 때 `POST /api/tasks/{id}/submit` 으로 보낸다
/// (보낼 때 그때의 재고로 다시 작성). 순차 계획이 쓴다 — 명령은 짝으로 만들고 보내기는 하나씩.
async fn create_pair(State(st): State<AppState>, Query(q): Query<CreateQuery>, axum::Json(b): axum::Json<PairBody>) -> ApiResult<Json> {
    let (mut pick, mut drop) = (b.pick, b.drop);
    let r = st.robot_required(pick.robot.or(drop.robot), "작업 제출")?;
    if drop.robot.is_some_and(|x| x != r.id) {
        return Err(ApiError::BadRequest(crate::ledger::ops::with_robot(&r.name, "PICK 과 DROP 은 같은 로봇이어야 짝이 됩니다")));
    }
    if crate::issue::parse_task_type(&pick.task_type)? != gr_proto::TaskType::Pick || crate::issue::parse_task_type(&drop.task_type)? != gr_proto::TaskType::Drop {
        return Err(ApiError::BadRequest("짝은 PICK 다음 DROP 이어야 합니다".into()));
    }
    pick.robot = Some(r.id);
    drop.robot = Some(r.id);
    if drop.item_code.is_none() {
        drop.item_code = pick.item_code;
    }
    // PICK 을 먼저 작성해 본다 — 안 되면 지시를 열지 않는다.
    let composed = crate::issue::compose(&st, &pick)?;
    let order = st.stock.open_order(crate::stock::transfer::NewOrder {
        robot: Some(r.id),
        plc: r.plc.clone(),
        item_code: pick.item_code.unwrap_or(composed.task.item.code),
        count: u32::from(pick.count.max(1)),
        from: pick.target.clone(),
        to: drop.target.clone(),
        source: "console:plan".into(),
        note: "순차 계획".into(),
    })?;
    pick.transfer_order_id = Some(order.id.clone());
    drop.transfer_order_id = Some(order.id.clone());
    let pick_e = match super::ops::create_and_submit(&st, r, Origin::Console, Some(pick), Some(composed.params), composed.task, composed.pallet, q.submit.unwrap_or(true)).await {
        Ok(e) => e,
        Err(e) => {
            let _ = st.stock.abort_order(&order.id, &format!("짝 PICK 제출 실패: {e}"), false);
            return Err(e);
        }
    };
    // DROP 은 PICK 이 예상 Hand 에 들어간 뒤에 작성해야 짝 검사를 통과한다.
    let drop_res = match crate::issue::compose(&st, &drop) {
        Ok(c) => super::ops::create_and_submit(&st, r, Origin::Console, Some(drop), Some(c.params), c.task, c.pallet, false).await,
        Err(e) => Err(e),
    };
    let (drop_e, warning) = match drop_res {
        Ok(e) => (Some(e), None),
        Err(e) => (None, Some(format!("PICK 은 보냈지만 짝 DROP 초안을 못 만듦: {e} — DROP 을 다시 보내세요(Hand 복구)"))),
    };
    Ok(axum::Json(json!({ "order": order.id, "pick": pick_e, "drop": drop_e, "warning": warning })))
}

/// 초안 제출. 이송 지시에 묶인 짝 초안은 **지금 재고로 다시 작성**해 보낸다(그 사이 바뀐 재고 · 팔렛 슬롯).
async fn submit(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<LedgerEntry> {
    let (r, e) = st.find_task(&id).ok_or_else(|| ApiError::NotFound(format!("task {id}")))?;
    if e.state == TaskState::Draft && e.transfer_order_id.is_some() {
        return Ok(axum::Json(super::ops::submit_refreshed(&st, r, e).await?));
    }
    Ok(axum::Json(super::ops::submit(&st, r, e).await?))
}

async fn cancel(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<LedgerEntry> {
    Ok(axum::Json(super::ops::cancel(&st, &id).await?))
}

async fn complete(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<LedgerEntry> {
    Ok(axum::Json(super::ops::force_complete(&st, &id).await?))
}

/// `POST /api/robots/{id}/command/{action}` — start | stop | reset | buzzerstop | complete | clear (사이드바 로봇 우클릭).
async fn robot_command(State(st): State<AppState>, Path((robot, action)): Path<(u8, String)>) -> ApiResult<Json> {
    let a =
        super::robot_cmd::RobotAction::parse(&action).ok_or_else(|| ApiError::BadRequest(format!("unknown robot command '{action}' (start|stop|reset|buzzerstop|gripper-learn|complete|clear)")))?;
    let r = st.robot(Some(robot))?;
    Ok(axum::Json(super::robot_cmd::run(&st, r, a).await?))
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
    // `robot`/`plc` 를 같이 낸다 — 화면이 "이 게이트가 누구 것인가"를 선택 상태로 짐작하지 않는다.
    Ok(axum::Json(json!({ "can_submit": g.can_submit, "reasons": g.reasons, "robot": g.robot, "plc": g.plc })))
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
        .route("/api/tasks/pair", post(create_pair))
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
        .route("/api/robots/{id}/command/{action}", post(robot_command))
}
