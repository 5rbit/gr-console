//! `GET /api/stock` · `PUT /api/stock/{cell}` · `DELETE /api/stock/{cell}` · `POST /api/stock/clear`
//! · `GET /api/stock/stream` (SSE snapshot|upsert|remove) · `GET /api/stock/z?type=&cell=&item=&count=` (Z preview)

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use serde::Deserialize;
use serde_json::{Value as Json, json};

use super::{StockEvent, grip_offset, stack_z};
use crate::error::{ApiError, ApiResult};
use crate::issue::parse_task_type;
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
    if b.count > 0 && b.item_code != 0 && st.registry.item(b.item_code)?.is_none() {
        return Err(ApiError::BadRequest(format!("item {} not registered", b.item_code)));
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
    /// `mid` | `bead` — default `Defaults.grip_ref`.
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
    let item = match code {
        Some(c) => st.registry.item(c)?.map(|i| i.item).unwrap_or_default(),
        None => Default::default(),
    };
    let grip_ref = q.grip.unwrap_or(st.registry.defaults()?.grip_ref);
    let grip = grip_offset(&grip_ref, &item);
    let n = stock.as_ref().map(|s| s.count).unwrap_or(0);
    Ok(axum::Json(
        json!({ "cell": q.cell, "type": tt.name(), "item_code": code, "height": item.height, "grip_ref": grip_ref, "grip": grip, "stock": n, "floor": cell.position[2], "z": stack_z(tt, cell.position[2], item.height, grip, n, q.count) }),
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
