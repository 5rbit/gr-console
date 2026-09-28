//! `/api/tasks*` — task ledger REST + SSE.
//!
//! `GET /api/tasks?state=active|terminal|a,b&type=PICK&origin=console|scenario|external&since=<RFC3339>&q=&limit=&offset=`
//! `GET /api/tasks/stats` · `GET /api/tasks/gate` · `GET /api/tasks/plc-view` · `GET /api/tasks/stream` (SSE snapshot|upsert|remove)
//! `POST /api/tasks[?submit=false]` · `POST /api/tasks/pair` (PICK 제출 + 짝 DROP 초안) · `GET|DELETE /api/tasks/{id}`
//! `POST /api/tasks/{id}/submit[?refresh=true]|cancel|complete|resubmit|mark-failed`
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
    // PICK 은 늘 DROP 과 짝이다 — 단독 PICK 은 받지 않는다(짝은 `POST /api/tasks/pair`·시나리오 실행기가 보낸다). DROP 단독은
    // Hand 에 든 것을 내려놓는 복구용으로 받고, 짝 검사(`enforce_hand`)가 품목·수량을 본다.
    if crate::issue::parse_task_type(&req.task_type)? == gr_proto::TaskType::Pick && req.source.is_none() {
        return Err(ApiError::Conflict(crate::ledger::ops::with_robot(&r.name, "PICK 단독 제출 불가 — PICK 은 짝 DROP 과 함께 보냅니다(POST /api/tasks/pair)")));
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

/// `POST /api/tasks/pair` — PICK/DROP 한 짝을 함께 만든다: 이송 지시를 열고 PICK 은 바로 제출, 짝 DROP 은 같은 지시로
/// 초안까지만(보내기는 차례가 되면 `POST /api/tasks/{id}/submit?refresh=true`). 순차 계획의 다음 1건·자동 제출이 쓴다 —
/// 시나리오로 저장하지 않고 로봇이 받을 수 있을 때 하나씩 보낸다.
async fn create_pair(State(st): State<AppState>, axum::Json(b): axum::Json<PairBody>) -> ApiResult<Json> {
    let PairBody { mut pick, mut drop } = b;
    let r = st.robot_required(pick.robot, "작업 제출")?;
    if drop.robot.is_some_and(|d| d != r.id) {
        return Err(ApiError::BadRequest("짝 DROP 은 PICK 과 같은 로봇이어야 합니다".into()));
    }
    use gr_proto::TaskType as T;
    if crate::issue::parse_task_type(&pick.task_type)? != T::Pick || crate::issue::parse_task_type(&drop.task_type)? != T::Drop {
        return Err(ApiError::BadRequest("짝 = PICK + DROP".into()));
    }
    if pick.count != drop.count {
        return Err(ApiError::BadRequest(format!("짝 수량 PICK {} ≠ DROP {}", pick.count, drop.count)));
    }
    if let (Some(a), Some(d)) = (pick.item_code, drop.item_code)
        && a != d
    {
        return Err(ApiError::BadRequest(format!("짝 품목 PICK {a} ≠ DROP {d}")));
    }
    if pick.source.is_some() || drop.source.is_some() {
        return Err(ApiError::BadRequest("짝 제출은 source 없이(시나리오 실행기는 제 경로로 보낸다)".into()));
    }
    pick.robot = Some(r.id);
    drop.robot = Some(r.id);
    // 잘못된 스텝은 이송 지시를 열기 전에 거른다(DROP 은 보낼 때 다시 작성).
    let pc = crate::issue::compose(&st, &pick)?;
    crate::issue::compose(&st, &drop)?;
    let order = st
        .stock
        .open_order(crate::stock::transfer::NewOrder {
            robot: Some(r.id),
            plc: r.plc.clone(),
            item_code: pc.task.item.code,
            count: u32::from(pick.count),
            from: pick.target.clone(),
            to: drop.target.clone(),
            source: "plan".into(),
            note: if pick.note.trim().is_empty() { "순차 계획".into() } else { pick.note.clone() },
        })?
        .id;
    pick.transfer_order_id = Some(order.clone());
    drop.transfer_order_id = Some(order.clone());
    let pick_e = match super::ops::create_and_submit(&st, r, Origin::Console, Some(pick), Some(pc.params), pc.task, pc.pallet, true).await {
        Ok(e) => e,
        Err(e) => {
            if let Err(x) = st.stock.abort_order(&order, "PICK 제출 실패", true) {
                tracing::warn!(%order, %x, "transfer order: abort failed");
            }
            return Err(e);
        }
    };
    // PICK 은 이미 나갔다 — 초안을 못 만들면 차례에 DROP 을 새로 작성해 보낸다(단독 DROP 은 Hand 의 지시를 잇는다).
    let draft = async {
        let c = crate::issue::compose(&st, &drop)?;
        super::ops::create_and_submit(&st, r, Origin::Console, Some(drop), Some(c.params), c.task, c.pallet, false).await
    };
    let (drop_e, warn) = match draft.await {
        Ok(e) => (Some(e), None),
        Err(e) => {
            tracing::warn!(robot = %r.name, %e, "pair DROP draft failed — will compose at its turn");
            (None, Some(format!("짝 DROP 초안 실패 — 차례에 새로 작성합니다: {e}")))
        }
    };
    Ok(axum::Json(json!({ "pick": pick_e, "drop": drop_e, "transfer_order_id": order, "warn": warn })))
}

#[derive(Deserialize)]
struct SubmitQuery {
    /// 보낼 때의 값으로 다시 작성(짝 초안) — 재고·팔렛 슬롯·스테이션 트래킹이 만든 뒤에 바뀌었을 수 있다.
    refresh: Option<bool>,
}

#[derive(Deserialize)]
struct SubmitBody {
    /// `refresh` 와 함께: 초안의 요청을 이것으로 바꿔 작성(계획에서 고친 대상·팔렛 등). 종류·로봇·이송 지시는 초안 것.
    request: Option<TaskRequest>,
}

async fn submit(State(st): State<AppState>, Path(id): Path<String>, Query(q): Query<SubmitQuery>, body: Option<axum::Json<SubmitBody>>) -> ApiResult<LedgerEntry> {
    let (r, mut e) = st.find_task(&id).ok_or_else(|| ApiError::NotFound(format!("task {id}")))?;
    // 이송 지시에 묶인 짝 초안은 어디서 보내든(Task 관리 화면 포함) 보낼 때의 값으로 다시 작성한다.
    let pair_draft = e.state == TaskState::Draft && e.transfer_order_id.is_some();
    if !q.refresh.unwrap_or(false) && !pair_draft {
        return Ok(axum::Json(super::ops::submit(&st, r, e).await?));
    }
    if let Some(mut req) = body.and_then(|b| b.0.request) {
        let tt = crate::issue::parse_task_type(&req.task_type)?;
        if Some(tt) != gr_proto::TaskType::from_code(e.plc_task.task_type) {
            return Err(ApiError::BadRequest(format!("task {id}: 종류는 바꿀 수 없습니다")));
        }
        let cur = e.request.as_ref();
        req.robot = Some(r.id);
        req.source = cur.and_then(|c| c.source.clone());
        req.transfer_order_id = cur.and_then(|c| c.transfer_order_id.clone()).or(e.transfer_order_id.clone());
        e.request = Some(req);
    }
    Ok(axum::Json(super::ops::submit_refreshed(&st, r, e).await?))
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
