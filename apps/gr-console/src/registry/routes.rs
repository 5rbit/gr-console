//! `/api/items`, `/api/cells`, `/api/stations`, `/api/registry`, `/api/defaults`, `/api/issue/compose`.
//!
//! Frontend shapes are snake_case (`Item`, `Cell`, `Station` in `types.ts`); PLC-mirror payloads stay
//! PascalCase inside `issue::Composed.task`.

use axum::Router;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use gr_proto::{CellInfo, StationPara, StockItem};
use serde::Deserialize;
use serde_json::{Value as Json, json};

use super::diff::Diff;
use super::plc_io::{self, ImportSummary};
use super::xlsx::{self, ApplyCounts, ItemRow, Tables, Want};
use super::{CellEntry, Defaults, ItemEntry, StationEntry};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

// ---- items (frontend shape: snake_case Item)
fn item_view(e: &ItemEntry) -> Json {
    json!({ "code": e.code, "name": e.name, "count": e.item.count, "inner_diameter": e.item.inner_diameter, "outer_diameter": e.item.outer_diameter,
        "lower_bead_height": e.item.lower_bid_height, "upper_bead_height": e.item.upper_bid_height, "height": e.item.height, "deflection_factor": e.item.deflection_factor,
        "note": e.note, "updated_at": e.updated_at })
}

#[derive(Deserialize)]
#[serde(default)]
struct ItemBody {
    code: u32,
    name: String,
    count: u8,
    inner_diameter: f32,
    outer_diameter: f32,
    lower_bead_height: f32,
    upper_bead_height: f32,
    height: f32,
    deflection_factor: f32,
    note: String,
}
impl Default for ItemBody {
    fn default() -> Self {
        Self {
            code: 0,
            name: String::new(),
            count: 1,
            inner_diameter: 0.0,
            outer_diameter: 0.0,
            lower_bead_height: 0.0,
            upper_bead_height: 0.0,
            height: 0.0,
            deflection_factor: 0.0,
            note: String::new(),
        }
    }
}
impl ItemBody {
    fn stock(&self) -> StockItem {
        StockItem {
            code: self.code,
            count: self.count,
            inner_diameter: self.inner_diameter,
            outer_diameter: self.outer_diameter,
            lower_bid_height: self.lower_bead_height,
            upper_bid_height: self.upper_bead_height,
            height: self.height,
            deflection_factor: self.deflection_factor,
        }
    }
}

async fn items(State(st): State<AppState>) -> ApiResult<Vec<Json>> {
    Ok(axum::Json(st.registry.items()?.iter().map(item_view).collect()))
}
async fn item_create(State(st): State<AppState>, axum::Json(b): axum::Json<ItemBody>) -> ApiResult<Json> {
    if b.code == 0 {
        return Err(ApiError::BadRequest("code must be > 0".into()));
    }
    Ok(axum::Json(item_view(&st.registry.upsert_item(b.code, &b.name, &b.stock(), &b.note)?)))
}
async fn item_update(State(st): State<AppState>, Path(code): Path<u32>, axum::Json(mut b): axum::Json<ItemBody>) -> ApiResult<Json> {
    b.code = code;
    Ok(axum::Json(item_view(&st.registry.upsert_item(code, &b.name, &b.stock(), &b.note)?)))
}
async fn item_delete(State(st): State<AppState>, Path(code): Path<u32>) -> ApiResult<Json> {
    Ok(axum::Json(json!({ "deleted": st.registry.delete_item(code)? })))
}

// ---- cells (frontend shape: snake_case Cell)
pub fn cell_view(e: &CellEntry) -> Json {
    json!({ "id": e.id, "use": e.cell.use_, "blend_use": e.cell.blend_use, "section": e.cell.section, "row": e.cell.row, "col": e.cell.col,
        "length": e.cell.length, "width": e.cell.width, "position": e.cell.position, "source": e.source, "dirty": e.dirty, "updated_at": e.updated_at, "plc_seen_at": e.plc_seen_at })
}

#[derive(Deserialize)]
#[serde(default)]
pub struct CellBody {
    pub id: u16,
    #[serde(rename = "use")]
    pub use_: bool,
    pub blend_use: bool,
    pub section: u16,
    pub row: u16,
    pub col: u16,
    pub length: f32,
    pub width: f32,
    pub position: [f32; 3],
}
impl Default for CellBody {
    fn default() -> Self {
        Self { id: 0, use_: true, blend_use: false, section: 0, row: 0, col: 0, length: 0.0, width: 0.0, position: [0.0; 3] }
    }
}
impl CellBody {
    pub fn info(&self) -> CellInfo {
        CellInfo { use_: self.use_, blend_use: self.blend_use, id: self.id, section: self.section, row: self.row, col: self.col, length: self.length, width: self.width, position: self.position }
    }
}

async fn cells(State(st): State<AppState>) -> ApiResult<Vec<Json>> {
    Ok(axum::Json(st.registry.cells()?.iter().map(cell_view).collect()))
}
async fn cell_create(State(st): State<AppState>, axum::Json(b): axum::Json<CellBody>) -> ApiResult<Json> {
    let info = b.info();
    xlsx::validate_cell(&info).map_err(ApiError::BadRequest)?;
    Ok(axum::Json(cell_view(&st.registry.upsert_cell(&info, "local", true, None)?)))
}
async fn cell_update(State(st): State<AppState>, Path(id): Path<u16>, axum::Json(mut b): axum::Json<CellBody>) -> ApiResult<Json> {
    b.id = id;
    let info = b.info();
    xlsx::validate_cell(&info).map_err(ApiError::BadRequest)?;
    Ok(axum::Json(cell_view(&st.registry.upsert_cell(&info, "local", true, None)?)))
}
async fn cell_delete(State(st): State<AppState>, Path(id): Path<u16>) -> ApiResult<Json> {
    Ok(axum::Json(json!({ "deleted": st.registry.delete_cell(id)? })))
}

// ---- query helpers

#[derive(Deserialize, Default)]
struct PlcQuery {
    plc: Option<String>,
    force: Option<String>,
}
impl PlcQuery {
    fn plc<'a>(&'a self, st: &'a AppState) -> &'a str {
        self.plc.as_deref().filter(|s| !s.trim().is_empty()).unwrap_or(&st.cfg.cmd.status_plc)
    }
    fn force(&self) -> bool {
        flag(self.force.as_deref())
    }
}

fn flag(v: Option<&str>) -> bool {
    matches!(v.map(str::trim).map(str::to_ascii_lowercase).as_deref(), Some("1" | "true" | "yes" | "on"))
}

fn summary_view(s: &ImportSummary) -> Json {
    json!({ "imported": s.imported, "updated": s.updated, "removed": s.removed, "skipped": s.skipped, "errors": s.errors })
}

fn diff_view<T>(rows: Vec<Diff<T>>, view: impl Fn(&T) -> Json) -> Vec<Json> {
    rows.into_iter().map(|d| json!({ "id": d.id, "status": d.status, "local": d.local.as_ref().map(&view), "plc": d.plc.as_ref().map(&view) })).collect()
}

/// PLC → local. `?plc=GR2|GRM|gr2_s7` (default: status PLC).
async fn cells_import(State(st): State<AppState>, Query(q): Query<PlcQuery>) -> ApiResult<Json> {
    let h = plc_io::resolve_plc(&st, q.plc(&st))?;
    Ok(axum::Json(summary_view(&plc_io::import_cells(&st, h)?)))
}

/// local → PLC. `?plc=GR2|GRM|both[&force=1]`.
async fn cells_push(State(st): State<AppState>, Query(q): Query<PlcQuery>) -> ApiResult<Json> {
    let mut results = Vec::new();
    for h in plc_io::plc_targets(&st, q.plc(&st))? {
        results.push(plc_io::push_cells(&st, h, q.force()).await?);
    }
    st.emit("registry", json!({ "kind": "cells_pushed", "results": results }));
    Ok(axum::Json(plc_io::aggregate(results)))
}

async fn cells_diff(State(st): State<AppState>, Query(q): Query<PlcQuery>) -> ApiResult<Vec<Json>> {
    let h = plc_io::resolve_plc(&st, q.plc(&st))?;
    Ok(axum::Json(diff_view(plc_io::diff_cells(&st, h)?, cell_view)))
}

// ---- stations (frontend shape)
pub fn station_view(e: &StationEntry) -> Json {
    let p = &e.para;
    json!({ "id": e.id, "conv_no": p.conv_no, "task_type": p.task_type, "rotate_type": p.rotate_type, "group": p.group, "group_index": p.group_index,
        "connection_prev": p.connection_prev, "connection_next": p.connection_next,
        "info": { "id": p.info.id, "use": p.info.use_, "blend_use": p.info.blend_use, "section": p.info.section, "row": p.info.row, "col": p.info.col, "length": p.info.length, "width": p.info.width, "position": p.info.position },
        "sensor": { "io_link_master_module": p.sensor_settings.io_link_master_module, "io_link_master_port_l": p.sensor_settings.io_link_master_port_l, "io_link_master_port_r": p.sensor_settings.io_link_master_port_r,
            "detection_factor": p.sensor_settings.detection_factor, "allow_range": p.sensor_settings.allow_range, "l_sensor_offset": p.sensor_settings.l_sensor_offset, "r_sensor_offset": p.sensor_settings.r_sensor_offset },
        "io_block_no": p.io_block_no, "source": e.source, "dirty": e.dirty, "updated_at": e.updated_at, "plc_seen_at": e.plc_seen_at })
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct StationBody {
    pub id: u16,
    pub conv_no: u16,
    pub task_type: u8,
    pub rotate_type: u8,
    pub group: u8,
    pub group_index: u8,
    pub connection_prev: u8,
    pub connection_next: u8,
    pub info: CellBody,
    pub sensor: Option<Json>,
    pub io_block_no: u8,
}
impl StationBody {
    pub fn para(&self) -> StationPara {
        let mut info = self.info.info();
        info.id = self.id;
        let sensor = self.sensor.as_ref().map(|s| gr_proto::SensorSettings {
            io_link_master_module: s["io_link_master_module"].as_u64().unwrap_or(0) as u8,
            io_link_master_port_l: s["io_link_master_port_l"].as_u64().unwrap_or(0) as u8,
            io_link_master_port_r: s["io_link_master_port_r"].as_u64().unwrap_or(0) as u8,
            detection_factor: s["detection_factor"].as_f64().unwrap_or(0.0) as f32,
            allow_range: s["allow_range"].as_f64().unwrap_or(0.0) as f32,
            l_sensor_offset: s["l_sensor_offset"].as_f64().unwrap_or(0.0) as f32,
            r_sensor_offset: s["r_sensor_offset"].as_f64().unwrap_or(0.0) as f32,
        });
        StationPara {
            conv_no: self.conv_no,
            task_type: self.task_type,
            rotate_type: self.rotate_type,
            group: self.group,
            group_index: self.group_index,
            connection_prev: self.connection_prev,
            connection_next: self.connection_next,
            info,
            sensor_settings: sensor.unwrap_or_default(),
            io_block_no: self.io_block_no,
        }
    }
}

async fn stations(State(st): State<AppState>) -> ApiResult<Vec<Json>> {
    Ok(axum::Json(st.registry.stations()?.iter().map(station_view).collect()))
}
async fn station_create(State(st): State<AppState>, axum::Json(b): axum::Json<StationBody>) -> ApiResult<Json> {
    let para = b.para();
    xlsx::validate_station(&para).map_err(ApiError::BadRequest)?;
    Ok(axum::Json(station_view(&st.registry.upsert_station(&para, "local", true, None)?)))
}
async fn station_update(State(st): State<AppState>, Path(id): Path<u16>, axum::Json(mut b): axum::Json<StationBody>) -> ApiResult<Json> {
    b.id = id;
    let para = b.para();
    xlsx::validate_station(&para).map_err(ApiError::BadRequest)?;
    Ok(axum::Json(station_view(&st.registry.upsert_station(&para, "local", true, None)?)))
}
async fn station_delete(State(st): State<AppState>, Path(id): Path<u16>) -> ApiResult<Json> {
    Ok(axum::Json(json!({ "deleted": st.registry.delete_station(id)? })))
}

async fn stations_import(State(st): State<AppState>, Query(q): Query<PlcQuery>) -> ApiResult<Json> {
    let h = plc_io::resolve_plc(&st, q.plc(&st))?;
    Ok(axum::Json(summary_view(&plc_io::import_stations(&st, h)?)))
}

async fn stations_push(State(st): State<AppState>, Query(q): Query<PlcQuery>) -> ApiResult<Json> {
    let mut results = Vec::new();
    for h in plc_io::plc_targets(&st, q.plc(&st))? {
        results.push(plc_io::push_stations(&st, h, q.force()).await?);
    }
    st.emit("registry", json!({ "kind": "stations_pushed", "results": results }));
    Ok(axum::Json(plc_io::aggregate(results)))
}

async fn stations_diff(State(st): State<AppState>, Query(q): Query<PlcQuery>) -> ApiResult<Vec<Json>> {
    let h = plc_io::resolve_plc(&st, q.plc(&st))?;
    Ok(axum::Json(diff_view(plc_io::diff_stations(&st, h)?, station_view)))
}

// ---- Excel

const XLSX_MIME: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

fn xlsx_response(bytes: Vec<u8>, filename: &str) -> Response {
    ([(header::CONTENT_TYPE, XLSX_MIME.to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{filename}\""))], bytes).into_response()
}

fn stamp() -> String {
    crate::util::now_str().chars().take(19).filter(|c| c.is_ascii_digit()).collect()
}

async fn cells_export(State(st): State<AppState>) -> Result<Response, ApiError> {
    let cells: Vec<CellInfo> = st.registry.cells()?.into_iter().map(|e| e.cell).collect();
    Ok(xlsx_response(xlsx::export_workbook(Some(&cells), None, None)?, &format!("cells_{}.xlsx", stamp())))
}

async fn stations_export(State(st): State<AppState>) -> Result<Response, ApiError> {
    let stations: Vec<StationPara> = st.registry.stations()?.into_iter().map(|e| e.para).collect();
    Ok(xlsx_response(xlsx::export_workbook(None, Some(&stations), None)?, &format!("stations_{}.xlsx", stamp())))
}

async fn registry_export(State(st): State<AppState>) -> Result<Response, ApiError> {
    let cells: Vec<CellInfo> = st.registry.cells()?.into_iter().map(|e| e.cell).collect();
    let stations: Vec<StationPara> = st.registry.stations()?.into_iter().map(|e| e.para).collect();
    let items: Vec<ItemRow> = st.registry.items()?.iter().map(ItemRow::from).collect();
    Ok(xlsx_response(xlsx::export_workbook(Some(&cells), Some(&stations), Some(&items))?, &format!("registry_{}.xlsx", stamp())))
}

#[derive(Deserialize, Default)]
struct FileQuery {
    dry_run: Option<String>,
}

async fn read_upload(mp: &mut Multipart) -> Result<(String, Vec<u8>), ApiError> {
    let merr = |e: axum::extract::multipart::MultipartError| ApiError::BadRequest(format!("multipart: {e}"));
    while let Some(field) = mp.next_field().await.map_err(merr)? {
        if field.name() == Some("file") || field.file_name().is_some() {
            let name = field.file_name().unwrap_or("upload.xlsx").to_string();
            let bytes = field.bytes().await.map_err(merr)?;
            if bytes.is_empty() {
                return Err(ApiError::BadRequest("empty file".into()));
            }
            return Ok((name, bytes.to_vec()));
        }
    }
    Err(ApiError::BadRequest("multipart field 'file' missing".into()))
}

/// Applies (or, for `dry_run`, only counts) the parsed tables; `errors` carry sheet-prefixed messages.
fn apply_tables(st: &AppState, t: &Tables, dry_run: bool) -> Result<Json, ApiError> {
    let mut c = ApplyCounts::default();
    if t.has_cells {
        c.add(&xlsx::apply_cells(&st.registry, &t.cells, dry_run)?);
    }
    if t.has_stations {
        c.add(&xlsx::apply_stations(&st.registry, &t.stations, dry_run)?);
    }
    if t.has_items {
        c.add(&xlsx::apply_items(&st.registry, &t.items, dry_run)?);
    }
    let errors: Vec<Json> =
        t.errors.iter().map(|e| json!({ "row": e.row, "sheet": e.sheet, "message": if e.sheet == "csv" { e.message.clone() } else { format!("{}: {}", e.sheet, e.message) } })).collect();
    Ok(json!({ "imported": c.imported, "updated": c.updated, "removed": 0, "skipped": c.skipped, "errors": errors, "dry_run": dry_run,
        "counts": { "cells": t.cells.len(), "stations": t.stations.len(), "items": t.items.len() } }))
}

async fn import_file(st: &AppState, mp: &mut Multipart, want: Want, dry_run: bool) -> ApiResult<Json> {
    let (name, bytes) = read_upload(mp).await?;
    let t = xlsx::parse_file(&name, &bytes, want)?;
    let out = apply_tables(st, &t, dry_run)?;
    if !dry_run {
        st.emit("registry", json!({ "kind": "file_imported", "file": name, "result": out }));
    }
    Ok(axum::Json(out))
}

async fn cells_import_file(State(st): State<AppState>, Query(q): Query<FileQuery>, mut mp: Multipart) -> ApiResult<Json> {
    import_file(&st, &mut mp, Want::Cells, flag(q.dry_run.as_deref())).await
}
async fn stations_import_file(State(st): State<AppState>, Query(q): Query<FileQuery>, mut mp: Multipart) -> ApiResult<Json> {
    import_file(&st, &mut mp, Want::Stations, flag(q.dry_run.as_deref())).await
}
async fn registry_import_file(State(st): State<AppState>, Query(q): Query<FileQuery>, mut mp: Multipart) -> ApiResult<Json> {
    import_file(&st, &mut mp, Want::All, flag(q.dry_run.as_deref())).await
}

// ---- defaults
async fn defaults(State(st): State<AppState>) -> ApiResult<Defaults> {
    Ok(axum::Json(st.registry.defaults()?))
}
async fn defaults_save(State(st): State<AppState>, axum::Json(d): axum::Json<Defaults>) -> ApiResult<Defaults> {
    Ok(axum::Json(st.registry.save_defaults(d)?))
}

// ---- compose preview (issue slice; registered here because `routes.rs` is lead-owned)
async fn compose_preview(State(st): State<AppState>, axum::Json(req): axum::Json<crate::ledger::TaskRequest>) -> ApiResult<crate::issue::Composed> {
    Ok(axum::Json(crate::issue::compose(&st, &req)?))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/items", get(items).post(item_create))
        .route("/api/items/{code}", axum::routing::put(item_update).delete(item_delete))
        .route("/api/cells", get(cells).post(cell_create))
        .route("/api/cells/import", post(cells_import))
        .route("/api/cells/push", post(cells_push))
        .route("/api/cells/diff", get(cells_diff))
        .route("/api/cells/export.xlsx", get(cells_export))
        .route("/api/cells/import-file", post(cells_import_file))
        .route("/api/cells/{id}", axum::routing::put(cell_update).delete(cell_delete))
        .route("/api/stations", get(stations).post(station_create))
        .route("/api/stations/import", post(stations_import))
        .route("/api/stations/push", post(stations_push))
        .route("/api/stations/diff", get(stations_diff))
        .route("/api/stations/export.xlsx", get(stations_export))
        .route("/api/stations/import-file", post(stations_import_file))
        .route("/api/stations/{id}", axum::routing::put(station_update).delete(station_delete))
        .route("/api/registry/export.xlsx", get(registry_export))
        .route("/api/registry/import-file", post(registry_import_file))
        .route("/api/registry/diff", get(registry_diff))
        .route("/api/defaults", get(defaults).put(defaults_save))
        .route("/api/issue/compose", post(compose_preview))
}

/// Both tables at once (plan: `GET /api/registry/diff?plc=`).
async fn registry_diff(State(st): State<AppState>, Query(q): Query<PlcQuery>) -> ApiResult<Json> {
    let h = plc_io::resolve_plc(&st, q.plc(&st))?;
    Ok(axum::Json(json!({ "plc": h.name(), "cells": diff_view(plc_io::diff_cells(&st, h)?, cell_view), "stations": diff_view(plc_io::diff_stations(&st, h)?, station_view) })))
}
