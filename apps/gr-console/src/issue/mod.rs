//! Task composition (M3-A slice owns the full version; M1 ships a working baseline that resolves the
//! target from the registry, applies defaults and builds `TaskData`).

use gr_proto::{StockItem, TaskData, TaskParams, TaskType};
use serde::Serialize;

use crate::error::ApiError;
use crate::ledger::TaskRequest;
use crate::state::AppState;

#[derive(Clone, Debug, Serialize)]
pub struct Composed {
    pub task: TaskData,
    pub params: TaskParams,
    pub warnings: Vec<String>,
}

pub fn compose(st: &AppState, req: &TaskRequest) -> Result<Composed, ApiError> {
    let tt = match req.task_type.to_ascii_uppercase().as_str() {
        "UP" => TaskType::Up,
        "PICK" => TaskType::Pick,
        "DROP" => TaskType::Drop,
        "MOVE" => TaskType::Move,
        "MEASURE" => TaskType::Measure,
        other => return Err(ApiError::BadRequest(format!("unknown task type {other}"))),
    };
    let mut warnings = Vec::new();
    let mut task = TaskData { task_type: tt.code(), ..Default::default() };
    // target
    if let Some(t) = &req.target {
        let cell = match t.kind.as_str() {
            "cell" => st.registry.cell(t.id)?.map(|c| c.cell),
            "station" => st.registry.station(t.id)?.map(|s| s.para.info),
            k => return Err(ApiError::BadRequest(format!("unknown target kind {k}"))),
        };
        let cell = cell.ok_or_else(|| ApiError::BadRequest(format!("{} {} not registered", t.kind, t.id)))?;
        task.cell = cell.clone();
        task.position = [cell.position[0], cell.position[1], cell.position[2], 300.0];
    } else if tt != TaskType::Up {
        warnings.push("target not set".into());
    }
    // item
    if let Some(code) = req.item_code {
        let item = st.registry.item(code)?.ok_or_else(|| ApiError::BadRequest(format!("item {code} not registered")))?;
        task.item = StockItem { count: req.count.max(1), ..item.item };
        task.item.code = code;
    } else if matches!(tt, TaskType::Pick | TaskType::Drop | TaskType::Measure) {
        warnings.push("item not set".into());
    }
    // defaults + overrides
    let defaults = st.registry.defaults()?;
    let kind = req.target.as_ref().map(|t| t.kind.clone()).unwrap_or_else(|| "cell".into());
    let mut params = defaults.base.clone();
    if let Some(by) = defaults.by.get(tt.name()).and_then(|m| m.get(&kind)) {
        params = params.overlay(by)?;
    }
    params = params.overlay(&req.params)?;
    if tt == TaskType::Measure && !(params.measure_floor || params.measure_item || params.measure_sku) {
        params.measure_item = true;
        warnings.push("MEASURE without a measure flag: MeasureItem assumed".into());
    }
    params.apply(&mut task);
    // Z: stack top = cell floor + (count-1)*height for PICK, floor for DROP (heuristic; UI may override)
    if req.position_override.is_none() && task.cell.id != 0 {
        let stack = task.item.count.max(1) as f32;
        let h = task.item.height;
        task.position[2] = match tt {
            TaskType::Pick | TaskType::Measure => task.cell.position[2] + (stack - 1.0) * h,
            TaskType::Drop => task.cell.position[2] + (stack - 1.0).max(0.0) * h,
            TaskType::Move => task.cell.position[2] + 500.0,
            TaskType::Up => task.position[2],
        };
        task.position[3] = task.item.inner_diameter.max(200.0) - 30.0;
    }
    if let Some(p) = req.position_override {
        task.position = p;
    }
    Ok(Composed { task, params, warnings })
}
