//! `GET /api/stock` · `PUT /api/stock/{cell}` · `DELETE /api/stock/{cell}` · `POST /api/stock/clear`
//! · `GET /api/stock/stream` (SSE snapshot|upsert|remove) · `GET /api/stock/z?type=&cell=&item=&count=` (Z preview)
//! · `GET /api/stock/export.xlsx` · `POST /api/stock/import-file?dry_run=&mode=merge|replace` (`stock::io`)
//! · `GET /api/stock/snapshots?limit=` · `GET /api/stock/snapshots/{id}` · `POST /api/stock/snapshots/{id}/restore` (`stock::snapshot`)

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

/// 재고만 한 파일(`Stock` 시트) — 셀·스테이션 모두.
async fn export_xlsx(State(st): State<AppState>) -> Result<axum::response::Response, ApiError> {
    use crate::registry::routes::{stamp, xlsx_response};
    let rows = super::io::export_rows(&st)?;
    let bytes = crate::registry::xlsx::export_workbook_with_stock(None, None, None, Some(&rows))?;
    Ok(xlsx_response(bytes, &format!("stock_{}.xlsx", stamp())))
}

#[derive(Deserialize, Default)]
struct ImportQuery {
    dry_run: Option<String>,
    mode: Option<String>,
}

/// 파일 → 재고. 응답은 레지스트리 가져오기와 같은 모양(`added|updated|unchanged|skipped|removed` + `errors`).
async fn import_file(State(st): State<AppState>, Query(q): Query<ImportQuery>, mut mp: axum::extract::Multipart) -> ApiResult<Json> {
    use crate::registry::routes::{flag, read_upload};
    let dry_run = flag(q.dry_run.as_deref());
    let mode = super::io::Mode::parse(q.mode.as_deref());
    let (name, bytes) = read_upload(&mut mp).await?;
    let s = crate::registry::xlsx::parse_stock(&name, &bytes, true)?;
    if !s.found() {
        return Err(ApiError::BadRequest("file has no data rows".into()));
    }
    let c = super::io::apply(&st, &s, mode, dry_run)?;
    let errors: Vec<Json> = c.errors.iter().map(|e| json!({ "row": e.row, "sheet": e.sheet, "message": crate::registry::xlsx::error_text(e) })).collect();
    Ok(axum::Json(json!({
        "imported": c.added, "added": c.added, "updated": c.updated, "unchanged": c.unchanged, "removed": c.removed, "skipped": c.skipped,
        "errors": errors, "dry_run": dry_run, "snapshot_id": c.snapshot_id, "mode": if mode == super::io::Mode::Replace { "replace" } else { "merge" },
        "counts": { "cells": 0, "stations": 0, "items": 0, "stock": s.rows.len() + s.errors.len() }
    })))
}

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

/// `GET /api/stock/sync` — 로봇별 동기화 경고(Hand · 이송 지시 vs PLC).
async fn sync_list(State(st): State<AppState>) -> ApiResult<Json> {
    let t = st.stock.sync.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let out: Vec<Json> = st.robots.iter().map(|r| json!({ "robot": r.id, "robot_name": r.name, "plc": r.plc, "issues": t.issues(&r.plc) })).collect();
    Ok(axum::Json(Json::Array(out)))
}

#[derive(Deserialize)]
struct ResolveBody {
    /// `clear_hand` | `adopt_plc`
    action: String,
    /// `adopt_plc` 의 품목·개수 — 없으면 PLC Completed 링의 마지막 PICK.
    #[serde(default)]
    item_code: Option<u32>,
    #[serde(default)]
    count: Option<u32>,
    /// `apply_task` | `ignore_task` 의 원장 Task id.
    #[serde(default)]
    task_id: Option<String>,
}

/// `POST /api/stock/sync/{robot}/resolve` — 동기화 경고를 한 번에 고친다(콘솔 DB 만, PLC 에는 쓰지 않는다).
/// - `clear_hand`: Hand 를 비우고 걸려 있던 이송 지시를 `aborted`(사유: PLC 빈 그리퍼).
/// - `adopt_plc`: PLC 가 들고 있는 것으로 Hand 를 채우고 `in_hand` 지시를 연다(`source = plc-sync`).
async fn sync_resolve(State(st): State<AppState>, Path(robot): Path<u8>, axum::Json(b): axum::Json<ResolveBody>) -> ApiResult<Json> {
    let r = st.robot(Some(robot))?;
    // 손 정정 뒤에 끝난 Task — 반영/무시(정확히 한 번 기록된다).
    if matches!(b.action.as_str(), "apply_task" | "ignore_task") {
        let id = b.task_id.as_deref().ok_or_else(|| ApiError::BadRequest("task_id 가 필요합니다".into()))?;
        let (_, e) = st.find_task(id).ok_or_else(|| ApiError::NotFound(format!("task {id}")))?;
        st.stock.decide_held(&e, b.action == "apply_task")?;
        st.stock.sync.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear(&r.plc);
        let _ = st.stock.events.send(super::StockEvent::Sync { plc: r.plc.clone(), issues: vec![] });
        return Ok(axum::Json(json!({ "task_id": id, "action": b.action })));
    }
    let p = crate::issue::projected_hand(&st, Some(robot))?;
    if !p.pending.is_empty() {
        return Err(ApiError::Conflict(format!("{}: 진행 중 PICK/DROP {} 건 — 끝난 뒤에 고치세요", r.name, p.pending.len())));
    }
    let hand = st.stock.hand(&r.plc)?;
    let out = match b.action.as_str() {
        "clear_hand" => {
            if let Some(id) = &hand.transfer_order_id {
                st.stock.abort_order(id, "동기화: PLC 빈 그리퍼 — Hand 비움", false)?;
            }
            st.stock.set_hand(&r.plc, 0, 0, "sync-resolve: Hand 비움 (PLC 빈 그리퍼)", None, hand.transfer_order_id.as_deref())?
        }
        "adopt_plc" => {
            let last = st.robot_plc(r).ok().and_then(|h| h.decode_path("OPCUA", "STAT")).and_then(|v| gr_proto::StatusView::from_json(&v).ok()).map(|v| super::sync::PlcView::from_status(&v));
            if let Some(v) = &last
                && !v.hold_item
            {
                return Err(ApiError::Conflict(format!("{}: PLC HoldItem = 0 — 들고 있지 않아 맞출 것이 없음", r.name)));
            }
            let cand = last.and_then(|v| v.last_pick);
            let (item, count) = match (b.item_code, b.count, cand) {
                (Some(i), Some(n), _) if n > 0 => (i, n),
                (_, _, Some((i, n))) => (b.item_code.unwrap_or(i), b.count.filter(|n| *n > 0).unwrap_or(n)),
                _ => return Err(ApiError::BadRequest("PLC 에 마지막 PICK 이 없음 — item_code 와 count 를 주세요".into())),
            };
            let order = match hand.transfer_order_id.clone() {
                Some(id) => id,
                None => {
                    let n = super::transfer::NewOrder {
                        robot: Some(r.id),
                        plc: r.plc.clone(),
                        item_code: item,
                        count,
                        source: "plc-sync".into(),
                        note: "PLC HoldItem 기준 Hand 맞춤".into(),
                        ..Default::default()
                    };
                    st.stock.open_in_hand_order(n, "동기화: PLC 가 들고 있음 — Hand 맞춤")?.id
                }
            };
            st.stock.set_hand(&r.plc, item, count, "sync-resolve: PLC 기준 Hand 맞춤", None, Some(&order))?
        }
        a => return Err(ApiError::BadRequest(format!("unknown action {a} (clear_hand | adopt_plc | apply_task | ignore_task)"))),
    };
    st.stock.sync.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear(&r.plc);
    let _ = st.stock.events.send(super::StockEvent::Sync { plc: r.plc.clone(), issues: vec![] });
    Ok(axum::Json(serde_json::to_value(out).unwrap_or_default()))
}

/// `GET /api/anticol` — 두 로봇 영역 간격(기본 안전값 5000 mm, 하한 = PLC PARA p11 + p16 + p13).
async fn anticol_get(State(st): State<AppState>) -> ApiResult<Json> {
    Ok(axum::Json(serde_json::to_value(crate::area::load(&st.db)).unwrap_or_default()))
}

/// `PUT /api/anticol` — 간격·사용 여부 바꾸기.
async fn anticol_put(State(st): State<AppState>, axum::Json(b): axum::Json<crate::area::AreaConfig>) -> ApiResult<Json> {
    // 파라미터 한 곳(`params`)에 쓴다 — 범위 검사·이력도 거기서.
    crate::params::check_plc_floor(b.separation_mm, crate::taskgen::routes::plc_max(&st)).map_err(ApiError::BadRequest)?;
    let mut p = crate::params::current(&st.db);
    p.anticol_separation_mm = b.separation_mm;
    p.anticol_enabled = b.enabled;
    crate::params::save(&st.db, p, "api:/api/anticol")?;
    Ok(axum::Json(serde_json::to_value(crate::area::load(&st.db)).unwrap_or_default()))
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
    // 스테이션도 받는다 — 손으로 컨베이어에 올린 화물의 품목 코드를 지정하면 `stock::conveyor` 가 따라간다.
    if st.registry.cell(cell)?.is_none() && st.registry.station(cell)?.is_none() {
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

/// 전체 비우기 — 직전 재고를 스냅샷으로 떠 두고 비운다(`snapshot_id` 로 되돌린다).
async fn clear(State(st): State<AppState>) -> ApiResult<Json> {
    let (removed, snapshot_id) = st.stock.clear()?;
    Ok(axum::Json(json!({ "removed": removed, "snapshot_id": snapshot_id })))
}

#[derive(Deserialize)]
struct SnapshotsQuery {
    limit: Option<usize>,
}

async fn snapshots(State(st): State<AppState>, Query(q): Query<SnapshotsQuery>) -> ApiResult<Json> {
    Ok(axum::Json(serde_json::to_value(st.stock.snapshots(q.limit.unwrap_or(20))?).unwrap_or_default()))
}

async fn snapshot(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json> {
    let s = st.stock.snapshot(id)?.ok_or_else(|| ApiError::NotFound(format!("stock snapshot {id}")))?;
    Ok(axum::Json(serde_json::to_value(s).unwrap_or_default()))
}

/// 스냅샷으로 되돌린다 — 지금 재고는 먼저 `restore-before` 스냅샷이 된다(응답 `snapshot_id`).
async fn restore(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json> {
    Ok(axum::Json(serde_json::to_value(st.stock.restore(id)?).unwrap_or_default()))
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
        .route("/api/stock/export.xlsx", get(export_xlsx))
        .route("/api/stock/import-file", post(import_file))
        .route("/api/stock/snapshots", get(snapshots))
        .route("/api/stock/snapshots/{id}", get(snapshot))
        .route("/api/stock/snapshots/{id}/restore", post(restore))
        .route("/api/stock/stream", get(stream))
        .route("/api/stock/z", get(z_preview))
        .route("/api/stock/hands", get(hands))
        .route("/api/stock/hand/{robot}", put(set_hand))
        .route("/api/stock/projected", get(projected))
        .route("/api/anticol", get(anticol_get).put(anticol_put))
        .route("/api/stock/sync", get(sync_list))
        .route("/api/stock/sync/{robot}/resolve", post(sync_resolve))
        .route("/api/transfer-orders", get(orders))
        .route("/api/transfer-orders/{id}", get(order))
        .route("/api/stock/{cell}", put(set).delete(remove))
}
