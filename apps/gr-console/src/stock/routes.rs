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

async fn list(State(st): State<AppState>) -> ApiResult<Json> {
    Ok(axum::Json(serde_json::to_value(st.stock.list()?).unwrap_or_default()))
}

#[derive(Deserialize)]
struct SetBody {
    item_code: u32,
    count: u32,
    #[serde(default)]
    note: String,
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
    Ok(axum::Json(serde_json::to_value(st.stock.set(cell, b.item_code, b.count, &b.note, "manual")?).unwrap_or_default()))
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
    let stock = st.stock.get(q.cell)?;
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
        "compression": z.compression, "used": z.used, "warnings": z.warnings, "stack_max": spec.as_ref().map(|s| s.stack_max) }),
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
        .route("/api/stock/{cell}", put(set).delete(remove))
}
