//! Task composition: `TaskRequest` (console, snake_case) → `TaskData` (PLC, PascalCase).
//!
//! Precedence of tunables: `Defaults.base` ← `Defaults.by[type][kind]` ← `request.params`.
//! Lookups (target cell / station, item) happen in `compose`; the arithmetic lives in `compose_from`
//! so it is unit-testable without an `AppState`. `warnings` are advisory — the PLC validates anyway.

use gr_proto::{CellInfo, StockItem, TaskData, TaskParams, TaskType};
use serde::Serialize;

use crate::error::ApiError;
use crate::ledger::TaskRequest;
use crate::registry::Defaults;
use crate::state::AppState;

#[derive(Clone, Debug, Serialize)]
pub struct Composed {
    pub task: TaskData,
    pub params: TaskParams,
    pub warnings: Vec<String>,
}

/// Gripper opening (G axis) used when nothing better is known.
const G_MIN: f32 = 200.0;
/// G = inner diameter minus this clearance.
const G_CLEARANCE: f32 = 30.0;
/// MOVE hovers this far above the cell floor.
const MOVE_CLEARANCE: f32 = 500.0;
/// Default G for UP / no-item tasks.
const G_DEFAULT: f32 = 300.0;

pub fn parse_task_type(s: &str) -> Result<TaskType, ApiError> {
    Ok(match s.trim().to_ascii_uppercase().as_str() {
        "UP" => TaskType::Up,
        "PICK" => TaskType::Pick,
        "DROP" => TaskType::Drop,
        "MOVE" => TaskType::Move,
        "MEASURE" => TaskType::Measure,
        other => return Err(ApiError::BadRequest(format!("unknown task type {other}"))),
    })
}

/// Full path: resolve target + item from the registry, then compose (stock from the console inventory).
pub fn compose(st: &AppState, req: &TaskRequest) -> Result<Composed, ApiError> {
    compose_with(st, req, None)
}

/// Like `compose`, but `stock_hint` (assumed tires in the target cell) replaces the inventory count —
/// used by the planning preview whose stock is a simulated chain of the steps before.
pub fn compose_with(st: &AppState, req: &TaskRequest, stock_hint: Option<u32>) -> Result<Composed, ApiError> {
    let cell = match &req.target {
        Some(t) => {
            let cell = match t.kind.as_str() {
                "cell" => st.registry.cell(t.id)?.map(|c| c.cell),
                "station" => st.registry.station(t.id)?.map(|s| s.para.info),
                k => return Err(ApiError::BadRequest(format!("unknown target kind {k}"))),
            };
            Some(cell.ok_or_else(|| ApiError::BadRequest(format!("{} {} not registered", t.kind, t.id)))?)
        }
        None => None,
    };
    // stock of the target cell (console-owned inventory) — Z stacking + item code fallback
    let stock = match &req.target {
        Some(t) if t.kind == "cell" => st.stock.get(t.id)?,
        _ => None,
    };
    let code = req.item_code.or_else(|| stock.as_ref().map(|s| s.item_code).filter(|c| *c != 0));
    let item = match code {
        Some(code) => Some(st.registry.item(code)?.ok_or_else(|| ApiError::BadRequest(format!("item {code} not registered")))?.item),
        None => None,
    };
    let stock_pair = match (stock_hint, stock) {
        (Some(n), Some(s)) => Some((s.item_code, n)),
        (Some(n), None) => Some((0, n)),
        (None, Some(s)) => Some((s.item_code, s.count)),
        (None, None) => None,
    };
    compose_from(&st.registry.defaults()?, req, cell, item, stock_pair)
}

/// Pure composition from already-resolved inputs.
/// `stock` = (item_code, count) currently in the target cell, if known.
pub fn compose_from(defaults: &Defaults, req: &TaskRequest, cell: Option<CellInfo>, item: Option<StockItem>, stock: Option<(u32, u32)>) -> Result<Composed, ApiError> {
    let tt = parse_task_type(&req.task_type)?;
    let mut warnings = Vec::new();
    let mut task = TaskData { task_type: tt.code(), ..Default::default() };
    let kind = req.target.as_ref().map(|t| t.kind.clone()).unwrap_or_else(|| "cell".into());

    // target
    match cell {
        Some(c) => {
            if !c.use_ {
                warnings.push(format!("{kind} {} is marked unused (Use=false)", c.id));
            }
            task.position = [c.position[0], c.position[1], c.position[2], G_DEFAULT];
            task.cell = c;
        }
        None if tt != TaskType::Up => warnings.push("target not set".into()),
        None => {}
    }

    // item
    match item {
        Some(mut it) => {
            it.count = req.count.max(1);
            if matches!(tt, TaskType::Pick | TaskType::Drop) && it.height <= 0.0 {
                warnings.push(format!("item {} has no height — Z cannot be stacked", it.code));
            }
            task.item = it;
        }
        None if matches!(tt, TaskType::Pick | TaskType::Drop | TaskType::Measure) => warnings.push("item not set".into()),
        None => {}
    }

    // tunables: base ← by[type][kind] ← request.params
    let mut params = defaults.base.clone();
    if let Some(by) = defaults.by.get(tt.name()).and_then(|m| m.get(&kind)) {
        params = params.overlay(by)?;
    }
    params = params.overlay(&req.params)?;
    if tt == TaskType::Measure && !(params.measure_floor || params.measure_item || params.measure_sku) {
        params.measure_item = true;
        warnings.push("MEASURE without a measure flag: MeasureItem assumed".into());
    }
    if tt != TaskType::Measure && (params.measure_floor || params.measure_item || params.measure_sku) {
        warnings.push("measure flags set on a non-MEASURE task".into());
    }
    if params.use_drag_out && params.drag_out_dist == 0 {
        warnings.push("UseDragOut with DragOutDist = 0".into());
    }
    if params.use_drag_in && params.drag_in_dist == 0 {
        warnings.push("UseDragIn with DragInDist = 0".into());
    }
    params.apply(&mut task);

    // Z from the cell stock (n tires already there), G from the inner diameter. The UI can override the whole position.
    if req.position_override.is_none() && task.cell.id != 0 {
        let c = task.item.count.max(1) as u32;
        let h = task.item.height.max(0.0);
        let floor = task.cell.position[2];
        let n = stock.map(|(_, n)| n).unwrap_or(match tt {
            TaskType::Pick | TaskType::Measure => c,
            _ => 0,
        });
        if let Some((code, n)) = stock {
            let taking = matches!(tt, TaskType::Pick | TaskType::Measure);
            if taking && n == 0 {
                warnings.push(format!("cell {} stock is empty", task.cell.id));
            } else if tt == TaskType::Pick && n < c {
                warnings.push(format!("cell {} stock {n} < count {c}", task.cell.id));
            }
            if (taking || tt == TaskType::Drop) && n > 0 && code != 0 && task.item.code != 0 && code != task.item.code {
                warnings.push(format!("cell {} holds item {code}, task item is {}", task.cell.id, task.item.code));
            }
        } else if matches!(tt, TaskType::Pick | TaskType::Drop | TaskType::Measure) {
            warnings.push(format!("cell {} stock unknown — Z assumes {}", task.cell.id, if tt == TaskType::Drop { "an empty cell" } else { "the task count on the floor" }));
        }
        task.position[2] = match tt {
            TaskType::Pick | TaskType::Measure | TaskType::Drop => crate::stock::stack_z(tt, floor, h, n, c),
            TaskType::Move => floor + MOVE_CLEARANCE,
            TaskType::Up => task.position[2],
        };
        if task.item.inner_diameter > 0.0 {
            task.position[3] = (task.item.inner_diameter - G_CLEARANCE).max(G_MIN);
        }
    }
    if let Some(p) = req.position_override {
        task.position = p;
    }
    if let Some((axis, v)) = ["X", "Y", "Z", "G"].iter().zip(task.position).find(|(_, v)| !v.is_finite() || *v < 0.0) {
        warnings.push(format!("position {axis} = {v} is not a valid coordinate"));
    }
    Ok(Composed { task, params, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::Target;
    use serde_json::json;

    fn defaults() -> Defaults {
        let mut d = Defaults::default();
        d.base.grip_height = 40;
        d.base.lift_up_height = 2500;
        d.by.get_mut("PICK").unwrap().insert("cell".into(), json!({ "grip_height": 50, "avoid": true }));
        d.by.get_mut("PICK").unwrap().insert("station".into(), json!({ "grip_height": 55, "find_station_item": true }));
        d
    }
    fn cell() -> CellInfo {
        CellInfo { use_: true, id: 101, section: 2, row: 1, col: 1, position: [12000.0, 3000.0, 1500.0], ..Default::default() }
    }
    fn item() -> StockItem {
        StockItem { code: 1001, count: 1, inner_diameter: 381.0, outer_diameter: 780.0, height: 240.0, ..Default::default() }
    }
    fn req(tt: &str, kind: &str) -> TaskRequest {
        TaskRequest { task_type: tt.into(), target: Some(Target { kind: kind.into(), id: 101 }), item_code: Some(1001), count: 3, params: json!({}), ..Default::default() }
    }

    #[test]
    fn precedence_base_by_request() {
        let d = defaults();
        // base only (DROP has no cell override for grip_height)
        let c = compose_from(&d, &req("DROP", "cell"), Some(cell()), Some(item()), None).unwrap();
        assert_eq!(c.params.grip_height, 40);
        assert!(!c.params.avoid);
        // by[PICK][cell]
        let c = compose_from(&d, &req("PICK", "cell"), Some(cell()), Some(item()), None).unwrap();
        assert_eq!(c.params.grip_height, 50);
        assert!(c.params.avoid);
        assert_eq!(c.task.grip_height, 50, "params are applied onto the PLC task");
        // by[PICK][station]
        let c = compose_from(&d, &req("PICK", "station"), Some(cell()), Some(item()), None).unwrap();
        assert_eq!(c.params.grip_height, 55);
        assert!(c.params.find_station_item);
        // request overrides everything; null keys are ignored
        let mut r = req("PICK", "cell");
        r.params = json!({ "grip_height": 60, "avoid": null, "lift_up_height": 1000 });
        let c = compose_from(&d, &r, Some(cell()), Some(item()), None).unwrap();
        assert_eq!(c.params.grip_height, 60);
        assert!(c.params.avoid);
        assert_eq!(c.task.lift_up_height, 1000);
    }

    #[test]
    fn position_and_item_count() {
        let d = defaults();
        let c = compose_from(&d, &req("PICK", "cell"), Some(cell()), Some(item()), None).unwrap();
        assert_eq!(c.task.item.count, 3);
        assert_eq!(c.task.position[0], 12000.0);
        // stock unknown: PICK assumes the 3 tires sit on the floor → grip the bottom one (mid-tire)
        assert_eq!(c.task.position[2], 1500.0 + 120.0);
        assert_eq!(c.task.position[3], 381.0 - 30.0);
        assert_eq!(c.task.cell.id, 101);
        assert_eq!(c.task.task_type, gr_proto::CMD_TASK_PICK);
        assert!(c.warnings.iter().any(|w| w.contains("stock unknown")), "{:?}", c.warnings);
        // stock known: 5 tires, taking 3 → grip the 3rd from the bottom; DROP lands on top of 5
        let c = compose_from(&d, &req("PICK", "cell"), Some(cell()), Some(item()), Some((1001, 5))).unwrap();
        assert_eq!(c.task.position[2], 1500.0 + 2.0 * 240.0 + 120.0);
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        let c = compose_from(&d, &req("DROP", "cell"), Some(cell()), Some(item()), Some((1001, 5))).unwrap();
        assert_eq!(c.task.position[2], 1500.0 + 5.0 * 240.0 + 120.0);
        // stock too small / different item → warnings
        let c = compose_from(&d, &req("PICK", "cell"), Some(cell()), Some(item()), Some((1002, 2))).unwrap();
        assert!(c.warnings.iter().any(|w| w.contains("stock 2 < count 3")));
        assert!(c.warnings.iter().any(|w| w.contains("holds item 1002")));
        let mut r = req("MOVE", "cell");
        r.position_override = Some([1.0, 2.0, 3.0, 4.0]);
        let c = compose_from(&d, &r, Some(cell()), None, None).unwrap();
        assert_eq!(c.task.position, [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn warnings_and_measure_flag() {
        let d = defaults();
        let mut r = req("MEASURE", "cell");
        r.item_code = None;
        r.target = None;
        let c = compose_from(&d, &r, None, None, None).unwrap();
        assert!(c.params.measure_item);
        assert!(c.warnings.iter().any(|w| w.contains("target not set")));
        assert!(c.warnings.iter().any(|w| w.contains("item not set")));
        assert!(c.warnings.iter().any(|w| w.contains("MeasureItem assumed")));
        let c = compose_from(&d, &req("UP", "cell"), None, None, None).unwrap();
        assert!(c.warnings.is_empty());
        assert!(compose_from(&d, &req("FLY", "cell"), None, None, None).is_err());
    }
}
