//! `GET /api/stock` · `PUT /api/stock/{cell}` · `DELETE /api/stock/{cell}` · `POST /api/stock/clear`
//! · `GET /api/stock/stream` (SSE snapshot|upsert|remove) · `GET /api/stock/z?type=&cell=&item=&count=` (Z preview)

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use serde::Deserialize;
use serde_json::{Value as Json, json};

use super::StockEvent;
use crate::error::{ApiError, ApiResult};
use crate::issue::parse_task_type;
use crate::registry::spec::stack_z_with;
use crate::sse::broadcast_sse;
use crate::state::AppState;

/// 로봇별 Hand(표 값 + 예상 값). 설정된 로봇은 기록이 없어도 빈 손으로 나온다.
async fn hands(State(st): State<AppState>) -> ApiResult<Json> {
    let mut out = Vec::new();
    for r in st.robots.iter() {
        let p = crate::issue::projected_hand(&st, Some(r.id))?;
        out.push(json!({ "robot": r.id, "robot_name": r.name, "plc": r.plc, "item_code": p.base.item_code, "count": p.base.count, "updated_at": p.base.updated_at, "transfer_order_id": p.base.transfer_order_id,
            "projected": { "item_code": p.projected.item_code, "count": p.projected.count }, "pending": p.pending.len(), "lost": p.lost }));
    }
    Ok(axum::Json(Json::Array(out)))
}

#[derive(Deserialize)]
struct HandBody {
    item_code: u32,
    count: u32,
    /// 이 정정이 속한 이송 지시(없으면 `manual` 로만 남는다).
    #[serde(default)]
    transfer_order_id: Option<String>,
}

/// Hand 손 정정(현장에서 그리퍼를 비웠거나 확인한 값) — 이 로봇에 진행 중 PICK/DROP 이 있으면 거부.
async fn set_hand(State(st): State<AppState>, Path(robot): Path<u8>, axum::Json(b): axum::Json<HandBody>) -> ApiResult<Json> {
    let r = st.robot(Some(robot))?;
    let p = crate::issue::projected_hand(&st, Some(robot))?;
    if !p.pending.is_empty() {
        return Err(ApiError::Conflict(format!("{}: 진행 중 PICK/DROP {} 건 — 끝난 뒤에 Hand 를 고치세요", r.name, p.pending.len())));
    }
    let cur = st.stock.hand(&r.plc)?;
    // 손을 비우면 손에 걸려 있던 지시는 사람이 끝낸 것(aborted) — 이력에 남는다.
    if b.count == 0
        && let Some(id) = &cur.transfer_order_id
    {
        st.stock.abort_order(id, "Hand 손 정정으로 비움", false)?;
    }
    let order = b.transfer_order_id.clone().or(cur.transfer_order_id.clone());
    Ok(axum::Json(serde_json::to_value(st.stock.set_hand(&r.plc, b.item_code, b.count, "manual", None, order.as_deref())?).unwrap_or_default()))
}

/// 예상 재고 — 셀 표 + 모든 로봇의 진행 중 PICK/DROP(Hand 포함). 계획 표의 출발점.
async fn projected(State(st): State<AppState>) -> ApiResult<Json> {
    let (cells, hands) = st.stock.projected_all(|| st.robots.iter().flat_map(|r| r.ledger.list()).collect())?;
    let hands: Vec<Json> = hands
        .into_iter()
        .map(|h| {
            let robot = st.robots.iter().find(|r| r.plc == h.plc).map(|r| r.id);
            json!({ "plc": h.plc, "robot": robot, "item_code": h.item_code, "count": h.count })
        })
        .collect();
    Ok(axum::Json(json!({ "cells": cells, "hands": hands })))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct OrderListQuery {
    robot: Option<u8>,
    state: Option<String>,
    cell: Option<u16>,
    since: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
}

/// `GET /api/transfer-orders?robot=&state=a,b&cell=&since=&limit=&offset=` — 최신 먼저.
async fn orders(State(st): State<AppState>, Query(q): Query<OrderListQuery>) -> ApiResult<Json> {
    let plc = match q.robot {
        Some(id) => Some(st.robot(Some(id))?.plc.clone()),
        None => None,
    };
    let query = super::transfer::OrderQuery { plc, state: q.state, cell: q.cell, since: q.since, limit: q.limit, offset: q.offset };
    let (items, total) = st.stock.orders(&query)?;
    Ok(axum::Json(json!({ "items": items, "total": total })))
}

/// `GET /api/transfer-orders/{id}` — 지시 + 두 Task + 재고 변경 기록.
async fn order(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Json> {
    let o = st.stock.order(&id)?.ok_or_else(|| ApiError::NotFound(format!("transfer order {id}")))?;
    let task = |t: &Option<String>| t.as_deref().and_then(|t| st.find_task(t)).map(|(_, e)| e);
    let tasks: Vec<_> = [task(&o.pick_task), task(&o.drop_task)].into_iter().flatten().collect();
    let changes = st.stock.order_changes(&id)?;
    Ok(axum::Json(json!({ "order": o, "tasks": tasks, "stock_changes": changes })))
}

async fn list(State(st): State<AppState>) -> ApiResult<Json> {
    Ok(axum::Json(serde_json::to_value(st.stock.list()?).unwrap_or_default()))
}

#[derive(Deserialize)]
struct SetBody {
    item_code: u32,
    count: u32,
    #[serde(default)]
    note: String,
    /// 이 정정이 속한 이송 지시(없으면 `manual` 로만 남는다).
    #[serde(default)]
    transfer_order_id: Option<String>,
}

async fn set(State(st): State<AppState>, Path(cell): Path<u16>, axum::Json(b): axum::Json<SetBody>) -> ApiResult<Json> {
    if st.registry.cell(cell)?.is_none() {
        return Err(ApiError::NotFound(format!("cell {cell} not registered")));
    }
    if b.count > 0 && b.item_code != 0 {
        let item = st.registry.item(b.item_code)?.ok_or_else(|| ApiError::BadRequest(format!("item {} not registered", b.item_code)))?;
        if item.spec.exceeds(b.count) {
            return Err(ApiError::BadRequest(format!(
                "셀 {cell} 재고 {}개는 품목 {} 의 StackMax {} 를 넘습니다 — 화물 규격에서 StackMax 를 고치거나 개수를 줄이세요",
                b.count, b.item_code, item.spec.stack_max
            )));
        }
    }
    Ok(axum::Json(serde_json::to_value(st.stock.set_logged(cell, b.item_code, b.count, &b.note, "manual", None, b.transfer_order_id.as_deref())?).unwrap_or_default()))
}

async fn remove(State(st): State<AppState>, Path(cell): Path<u16>) -> ApiResult<Json> {
    Ok(axum::Json(json!({ "removed": st.stock.remove(cell)? })))
}

async fn clear(State(st): State<AppState>) -> ApiResult<Json> {
    Ok(axum::Json(json!({ "removed": st.stock.clear()? })))
}

async fn stream(State(st): State<AppState>) -> impl IntoResponse {
    let first = st.stock.list().map(|stock| StockEvent::Snapshot { stock }).ok();
    broadcast_sse(st.stock.events.subscribe(), "stock", first)
}

#[derive(Deserialize)]
struct ZQuery {
    #[serde(rename = "type")]
    task_type: String,
    cell: u16,
    item: Option<u32>,
    #[serde(default = "one")]
    count: u32,
    /// `mid` | `pick_bead` — default `Defaults.grip_ref`. 옛 `bead` 는 `pick_bead` 로 읽힌다.
    grip: Option<String>,
}
fn one() -> u32 {
    1
}

/// Z the composer would use right now for `type` on `cell` (stock-aware) — for the planning table.
async fn z_preview(State(st): State<AppState>, Query(q): Query<ZQuery>) -> ApiResult<Json> {
    let tt = parse_task_type(&q.task_type)?;
    let cell = st.registry.cell(q.cell)?.ok_or_else(|| ApiError::NotFound(format!("cell {}", q.cell)))?.cell;
    // compose 와 같은 예상 재고(진행 중 PICK/DROP 반영).
    let proj = crate::issue::projected_stock(&st, q.cell)?;
    let stock = proj.projected.clone();
    let code = q.item.or(stock.as_ref().map(|s| s.item_code)).filter(|c| *c != 0);
    let entry = match code {
        Some(c) => st.registry.item(c)?,
        None => None,
    };
    let item = entry.as_ref().map(|e| e.item.clone()).unwrap_or_default();
    let spec = entry.map(|e| e.spec);
    let grip_ref = q.grip.unwrap_or(st.registry.defaults()?.grip_ref);
    let n = stock.as_ref().map(|s| s.count).unwrap_or(0);
    // compose 와 같은 식(프로파일 → 곡선 → mid) — 계획 표의 Z 가 제출 Z 와 달라지지 않게.
    let z = stack_z_with(tt, cell.position[2], &item, spec.as_ref(), &grip_ref, n, q.count);
    Ok(axum::Json(
        json!({ "cell": q.cell, "type": tt.name(), "item_code": code, "height": item.height, "grip_ref": z.grip_ref, "grip_ref_asked": grip_ref, "grip": z.grip, "stock": n, "floor": cell.position[2],
        "z": z.z, "z_source": z.z_source, "level": z.level, "below": z.below, "above": z.above, "base": z.base, "upper_bead": z.upper_bead, "pick_bead_offset": z.pick_bead_offset,
        "compression": z.compression, "used": z.used, "warnings": z.warnings, "stack_max": spec.as_ref().map(|s| s.stack_max), "stock_base": proj.base_count(), "pending": proj.pending.len() }),
    ))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/stock", get(list))
        .route("/api/stock/clear", post(clear))
        .route("/api/stock/stream", get(stream))
        .route("/api/stock/z", get(z_preview))
        .route("/api/stock/hands", get(hands))
        .route("/api/stock/hand/{robot}", put(set_hand))
        .route("/api/stock/projected", get(projected))
        .route("/api/transfer-orders", get(orders))
        .route("/api/transfer-orders/{id}", get(order))
        .route("/api/stock/{cell}", put(set).delete(remove))
}
