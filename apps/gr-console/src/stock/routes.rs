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
    Ok(axum::Json(serde_json::to_value(st.stock.set(cell, b.item_code, b.count, &b.note, "manual")?).unwrap_or_default()))
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
        .route("/api/stock/{cell}", put(set).delete(remove))
}
