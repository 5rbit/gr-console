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

use super::beads;
use super::bulk;
use super::diff::Diff;
use super::plc_io::{self, ImportSummary};
use super::spec::{self, ItemSpec, LevelValues};
use super::xlsx::{self, ApplyCounts, ItemRow, SpecPatch, Tables, Want};
use super::{CellEntry, Defaults, ItemEntry, StationEntry};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

// ---- items (frontend shape: snake_case Item)
fn item_view(e: &ItemEntry) -> Json {
    json!({ "code": e.code, "name": e.name, "count": e.item.count, "inner_diameter": e.item.inner_diameter, "outer_diameter": e.item.outer_diameter,
        "lower_bead_height": e.item.lower_bid_height, "upper_bead_height": e.item.upper_bid_height, "height": e.item.height, "deflection_factor": e.item.deflection_factor,
        "note": e.note, "spec": e.spec, "updated_at": e.updated_at })
}

/// Normalises + validates a spec coming from a client (400 on strict contradictions).
fn checked_spec(s: Option<ItemSpec>, item: &StockItem) -> Result<Option<ItemSpec>, ApiError> {
    let Some(s) = s.map(ItemSpec::normalized) else { return Ok(None) };
    spec::validate_spec_for(&s, item).map_err(|e| ApiError::BadRequest(format!("spec: {e}")))?;
    Ok(Some(s))
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
    /// 없으면(옛 클라이언트) 저장된 spec 을 그대로 둔다 — 새 품목은 기본값.
    spec: Option<ItemSpec>,
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
            spec: None,
        }
    }
}
impl ItemBody {
    /// The row the xlsx importer validates — CRUD and Excel share `validate_item`.
    fn row(&self) -> ItemRow {
        ItemRow { code: self.code, name: self.name.clone(), item: self.stock(), note: self.note.clone(), spec: SpecPatch::default() }
    }
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
    xlsx::validate_item(&b.row()).map_err(ApiError::BadRequest)?;
    let spec = checked_spec(b.spec.clone(), &b.stock())?;
    Ok(axum::Json(item_view(&st.registry.upsert_item_full(b.code, &b.name, &b.stock(), &b.note, spec.as_ref())?)))
}
async fn item_update(State(st): State<AppState>, Path(code): Path<u32>, axum::Json(mut b): axum::Json<ItemBody>) -> ApiResult<Json> {
    b.code = code;
    xlsx::validate_item(&b.row()).map_err(ApiError::BadRequest)?;
    // 손으로 고친 단을 표시해 둔다 — SKU 측정 자동 반영이 그 행을 덮지 않는다(`spec::mark_manual_edits`).
    if let (Some(s), Some(prev)) = (b.spec.as_mut(), st.registry.item(code)?) {
        spec::mark_manual_edits(s, &prev.spec);
    }
    let spec = checked_spec(b.spec.clone(), &b.stock())?;
    Ok(axum::Json(item_view(&st.registry.upsert_item_full(code, &b.name, &b.stock(), &b.note, spec.as_ref())?)))
}
async fn item_delete(State(st): State<AppState>, Path(code): Path<u32>) -> ApiResult<Json> {
    Ok(axum::Json(json!({ "deleted": st.registry.delete_item(code)? })))
}

#[derive(Deserialize, Default)]
struct LevelsQuery {
    robot: Option<u8>,
    /// `apply-measured` 만: `beads` · `compression`(쉼표로 여러 개, 기본 둘 다).
    fields: Option<String>,
    /// 단별 보기를 몇 개 쌓은 스택 기준으로 볼지(비면 StackMax). 표는 하중(`Above`)으로 저장되고 단 번호는
    /// 여기서 파생된다 — 5 개 스택의 3 단과 8 개 스택의 3 단은 하중이 달라 값도 다르다.
    preview: Option<u32>,
}

/// Latest SKU measurement of `code` on the robot's measure store (per level) + the MEASLOG by-code trends.
/// Both are best-effort: a robot without a store/snapshot yields nulls, never an error.
fn measured_levels(st: &AppState, robot: Option<u8>, code: u32) -> (Option<Vec<LevelValues>>, Json, Json) {
    let Ok(r) = st.robot(robot) else { return (None, Json::Null, Json::Null) };
    let latest = r.measure.entries(None, Some(gr_proto::MEAS_LOG_KIND_SKU), Some(code), 20).ok().and_then(|(rows, _)| rows.into_iter().find(|e| e["Status"].as_u64().unwrap_or(0) < 3));
    let (levels, source) = match latest {
        Some(e) => {
            let data: Vec<f32> = e["Data"].as_array().map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default();
            let at = |i: usize| data.get(i).copied().unwrap_or(0.0);
            let src = json!({ "plc": r.plc, "seq": e["Seq"], "at": e["TimeStamp"], "count": at(17), "each_height": at(18), "stack_height": at(19) });
            (Some(spec::measured_from_sku(&data)), src)
        }
        None => (None, Json::Null),
    };
    let summary = r
        .measure
        .snapshot()
        .ok()
        .and_then(|s| s["by_code"].as_array().and_then(|a| a.iter().find(|b| b["Code"].as_u64() == Some(code as u64)).cloned()))
        .map(|b| {
            let m = &b["Meas"];
            json!({ "plc": r.plc, "count": b["Count"], "last_time": b["LastTime"], "upper_bead": m["UpperBeadHeight"], "tire_height": m["TireHeight"],
                "each_height": m["SkuEachHeight"], "stack_height": m["SkuStackHeight"], "pick_bead_pos": m["PickBeadPos"] })
        })
        .unwrap_or(Json::Null);
    (levels, source, summary)
}

/// 측정 출처 json 에서 잰 스택의 단수와 한 개 높이.
fn measured_shape(source: &Json) -> (u32, Option<f32>) {
    let f = |k: &str| source[k].as_f64().map(|v| v as f32).filter(|v| v.is_finite() && *v > 0.0);
    (f("count").map(|v| v.round() as u32).unwrap_or(0), f("each_height"))
}

/// 아직 표본으로 안 읽은 SKU 측정을 뒤늦게 적재한다(화면이 품목을 열 때마다 한 번). 실패해도 조회는 계속.
fn backfill_samples(st: &AppState, robot: Option<u8>, code: u32) {
    let Ok(r) = st.robot(robot) else { return };
    if let Err(e) = r.measure.backfill_sku(Some(code), 50) {
        tracing::warn!(code, "bead sample backfill: {e}");
    }
}

/// `GET /api/items/{code}/levels?robot=&preview=n` — 스택 크기 `n` 의 단별 보기로
/// 펼친다: configured · effective(measured|interpolated|computed) · 절대 비드 · 공칭 · 측정 · 편차 · PickZ.
/// 행마다 어느 SKU 표본이 채웠는지(`sample`)와 그 하중에서 먹은 눌림(`compression_at`)도 나온다.
async fn item_levels(State(st): State<AppState>, Path(code): Path<u32>, Query(q): Query<LevelsQuery>) -> ApiResult<Json> {
    backfill_samples(&st, q.robot, code);
    let e = st.registry.item(code)?.ok_or_else(|| ApiError::NotFound(format!("item {code} not registered")))?;
    let (measured, source, summary) = measured_levels(&st, q.robot, code);
    let (count, each) = measured_shape(&source);
    let rows = spec::level_views(&e.item, &e.spec, measured.as_deref(), q.preview, count);
    let suggestion = spec::suggest_compression(&e.item, measured.as_deref().unwrap_or(&[]), count, each);
    // 한 번의 measureSKU = 스택 하나의 눌림 프로파일 — 단별 pitch·CompressionAt 을 그대로 보여 준다.
    let total_h = source["stack_height"].as_f64().map(|v| v as f32).filter(|v| v.is_finite() && *v > 0.0);
    let profile = spec::stack_profile(&e.item, measured.as_deref().unwrap_or(&[]), count, each, total_h);
    Ok(axum::Json(json!({
        "code": code, "name": e.name, "height": e.item.height, "eff_height": spec::eff_height(&e.item),
        "lower_bead_height": e.item.lower_bid_height, "upper_bead_height": e.item.upper_bid_height,
        "deflection_factor": e.item.deflection_factor, "deflection_applied": false,
        "spec": e.spec, "rows": rows, "warnings": spec::spec_warnings(&e.spec, Some(&e.item)),
        "pick_bead_offset": e.spec.pick_bead_offset(), "compression": e.spec.compression(), "reference_stack": e.spec.reference_stack(),
        "suggestion": suggestion,
        "measured": source, "measured_summary": summary,
        "auto_apply_measured": e.spec.auto_apply_measured(), "profile": profile,
        "preview": rows.len() as u32, "measured_aboves": e.spec.measured_aboves(), "measured_counts": e.spec.measured_counts(),
        "bead_samples": beads::samples_json(&st.registry.bead_samples(code, plc_of(&st, q.robot).as_deref(), beads::HISTORY_LIMIT)?),
    })))
}

/// 로봇의 상태 PLC 이름(표본 필터) — 로봇을 못 찾으면 `None`(= 모든 로봇).
fn plc_of(st: &AppState, robot: Option<u8>) -> Option<String> {
    st.robot(robot).ok().map(|r| r.plc.clone())
}

#[derive(Deserialize, Default)]
struct SamplesQuery {
    robot: Option<u8>,
    limit: Option<usize>,
    /// 모든 로봇의 표본을 보려면 `all=true`.
    all: Option<bool>,
}

/// `GET /api/items/{code}/bead-samples?robot=&limit=&all=` — SKU 측정 표본 이력(최신 순, 반영 여부·거부 사유 포함).
async fn item_bead_samples(State(st): State<AppState>, Path(code): Path<u32>, Query(q): Query<SamplesQuery>) -> ApiResult<Json> {
    backfill_samples(&st, q.robot, code);
    let plc = if q.all.unwrap_or(false) { None } else { plc_of(&st, q.robot) };
    let rows = st.registry.bead_samples(code, plc.as_deref(), q.limit.unwrap_or(beads::HISTORY_LIMIT).clamp(1, 200))?;
    let spec = st.registry.item(code)?.map(|e| e.spec).unwrap_or_default();
    Ok(axum::Json(json!({ "code": code, "plc": plc, "limit": beads::HISTORY_LIMIT, "auto_apply_measured": spec.auto_apply_measured(), "samples": beads::samples_json(&rows) })))
}

#[derive(Deserialize, Default)]
struct ApplySampleQuery {
    robot: Option<u8>,
    /// 자동 반영 스위치가 꺼져 있어도 넣는다(화면의 Apply 버튼은 언제나 `true`).
    force: Option<bool>,
}

/// `POST /api/items/{code}/bead-samples/{seq}/apply?robot=&force=` — 옛 표본을 손으로 규격에 넣는다.
async fn item_bead_sample_apply(State(st): State<AppState>, Path((code, seq)): Path<(u32, u32)>, Query(q): Query<ApplySampleQuery>) -> ApiResult<Json> {
    let plc = plc_of(&st, q.robot);
    let got = st.registry.apply_bead_sample(code, plc.as_deref(), seq, q.force.unwrap_or(true))?;
    st.emit("registry", json!({ "kind": "item_bead_sample_applied", "code": code, "plc": got.plc, "seq": seq }));
    let e = st.registry.item(code)?.ok_or_else(|| ApiError::NotFound(format!("item {code} not registered")))?;
    Ok(axum::Json(json!({ "code": code, "plc": got.plc, "seq": seq, "applied": got.applied, "reason": got.reason, "detail": got.detail,
        "spec": e.spec, "rows": spec::level_views(&e.item, &e.spec, None, None, 0), "warnings": spec::spec_warnings(&e.spec, Some(&e.item)),
        "samples": beads::samples_json(&st.registry.bead_samples(code, plc.as_deref(), beads::HISTORY_LIMIT)?) })))
}

/// `POST /api/items/{code}/levels/apply-measured?robot=&fields=beads,compression` — 서버가 최신 SKU 측정을
/// 규격에 넣는다(검증 뒤 저장). `beads` = 그 크기의 절대 프로파일, `compression` = 되짚은 눌림양.
/// 필드를 고르지 않으면 표본 경로(`bead-samples/.../apply`)와 같은 코드가 돌아 손댄 행을 지켜 준다.
async fn item_levels_apply_measured(State(st): State<AppState>, Path(code): Path<u32>, Query(q): Query<LevelsQuery>) -> ApiResult<Json> {
    backfill_samples(&st, q.robot, code);
    // 필드를 안 고른(= 둘 다) 부름은 표본 경로로 넘긴다 — 검증·손댄 행 보호·표본 기록이 한 곳에서 돈다.
    if q.fields.is_none()
        && let Some(latest) = st.registry.bead_samples(code, plc_of(&st, q.robot).as_deref(), 1)?.first()
    {
        let seq = latest.sample.seq;
        return item_bead_sample_apply(State(st), Path((code, seq)), Query(ApplySampleQuery { robot: q.robot, force: Some(true) })).await;
    }
    let e = st.registry.item(code)?.ok_or_else(|| ApiError::NotFound(format!("item {code} not registered")))?;
    let asked = q.fields.as_deref().unwrap_or("beads,compression");
    let fields: Vec<&str> = asked.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    if let Some(bad) = fields.iter().find(|f| !matches!(**f, "beads" | "compression")) {
        return Err(ApiError::BadRequest(format!("fields={bad} — beads · compression 만 됩니다")));
    }
    let (measured, source, _) = measured_levels(&st, q.robot, code);
    let Some(measured) = measured.filter(|m| !m.is_empty()) else {
        return Err(ApiError::BadRequest(format!("품목 {code} 의 SKU 측정 기록이 없습니다")));
    };
    let (count, each) = measured_shape(&source);
    let suggestion = spec::suggest_compression(&e.item, &measured, count, each);
    let total_h = source["stack_height"].as_f64().map(|v| v as f32).filter(|v| v.is_finite() && *v > 0.0);
    let profile = spec::stack_profile(&e.item, &measured, count, each, total_h);
    let stamp = format!("측정 {} Seq {} {}", source["plc"].as_str().unwrap_or("?"), source["seq"], source["at"].as_str().unwrap_or("?"));
    let src = spec::SampleRef { plc: source["plc"].as_str().unwrap_or_default().into(), seq: source["seq"].as_u64().unwrap_or(0) as u32 };
    let mut s = e.spec.clone();
    let mut applied = Vec::new();
    if fields.contains(&"beads") {
        let stack = spec::MeasuredStack { count, levels: &measured, each_height: each, total_height: total_h, sample: Some(&src), at: &crate::util::now_str() };
        let hit = spec::put_profile(&mut s, &stack, true);
        s.bead_source = stamp.clone();
        applied.push(json!({ "field": "beads", "levels": hit.levels, "skipped": hit.skipped }));
    }
    if fields.contains(&"compression") {
        let Some(v) = suggestion.value else {
            return Err(ApiError::BadRequest(format!("품목 {code}: 측정에서 눌림양을 되짚을 수 없습니다(단별 비드·EachHeight 없음)")));
        };
        s.compression = Some(v);
        s.compression_source = format!("{stamp} — {} n={}", suggestion.method.unwrap_or("?"), suggestion.samples);
        applied.push(json!({ "field": "compression", "value": v, "method": suggestion.method }));
    }
    let s = s.normalized();
    spec::validate_spec_for(&s, &e.item).map_err(|m| ApiError::BadRequest(format!("품목 {code} spec: {m}")))?;
    st.registry.set_item_spec(code, &s)?;
    st.emit("registry", json!({ "kind": "item_spec_measured", "code": code, "fields": fields }));
    let e = st.registry.item(code)?.ok_or_else(|| ApiError::NotFound(format!("item {code} not registered")))?;
    Ok(axum::Json(json!({ "code": code, "applied": applied, "spec": e.spec, "suggestion": suggestion, "profile": profile,
        "rows": spec::level_views(&e.item, &e.spec, Some(&measured), q.preview, count), "warnings": spec::spec_warnings(&e.spec, Some(&e.item)), "measured": source })))
}

#[derive(Deserialize)]
struct BulkSpecBody {
    codes: Vec<u32>,
    stack_max: Option<u8>,
    pallet_max: Option<u8>,
}

/// `POST /api/items/bulk-spec` — set stack_max / pallet_max on many items. All rows are
/// validated first (a curve point deeper than the new stack_max rejects the whole request).
async fn items_bulk_spec(State(st): State<AppState>, axum::Json(b): axum::Json<BulkSpecBody>) -> ApiResult<Json> {
    if b.codes.is_empty() {
        return Err(ApiError::BadRequest("codes is empty".into()));
    }
    if b.stack_max.is_none() && b.pallet_max.is_none() {
        return Err(ApiError::BadRequest("nothing to set (stack_max / pallet_max)".into()));
    }
    let items = st.registry.items()?;
    let mut next = Vec::with_capacity(b.codes.len());
    for code in &b.codes {
        let e = items.iter().find(|e| e.code == *code).ok_or_else(|| ApiError::BadRequest(format!("item {code} not registered")))?;
        let mut s = e.spec.clone();
        if let Some(v) = b.stack_max {
            s.stack_max = v;
        }
        if let Some(v) = b.pallet_max {
            s.pallet_max = v;
        }
        spec::validate_spec_for(&s, &e.item).map_err(|m| ApiError::BadRequest(format!("품목 {code}: {m}")))?;
        next.push((*code, s));
    }
    for (code, s) in &next {
        st.registry.set_item_spec(*code, s)?;
    }
    st.emit("registry", json!({ "kind": "items_bulk_spec", "codes": b.codes }));
    Ok(axum::Json(json!({ "updated": next.len() })))
}

// ---- cells (frontend shape: snake_case Cell)
pub fn cell_view(e: &CellEntry) -> Json {
    json!({ "id": e.id, "use": e.cell.use_, "blend_use": e.cell.blend_use, "section": e.cell.section, "row": e.cell.row, "col": e.cell.col,
        "length": e.cell.length, "width": e.cell.width, "position": e.cell.position, "source": e.source, "dirty": e.dirty, "updated_at": e.updated_at, "plc_seen_at": e.plc_seen_at,
        "warnings": xlsx::cell_warnings(&e.cell) })
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
        self.plc.as_deref().filter(|s| !s.trim().is_empty()).unwrap_or(st.default_plc_name())
    }
    /// 쓰기(push)·로컬 갱신(import)용 — 로봇이 둘 이상이면 `plc` 를 반드시 받고, `both`(옛 "상태 PLC + GRM")는
    /// 어느 GR 인지 모호해 거부한다. 빠지면 첫 로봇(GR1) PLC 로 쓰던 경로.
    fn plc_for_write<'a>(&'a self, st: &'a AppState) -> Result<&'a str, ApiError> {
        let p = self.plc.as_deref().map(str::trim).filter(|s| !s.is_empty());
        if st.robots.len() > 1 {
            match p {
                None => return Err(ApiError::BadRequest(format!("대상 PLC 를 지정해야 합니다 (plc: GR 이름·GRM·all; 로봇 {})", st.robot_choices()))),
                Some(v) if v.eq_ignore_ascii_case("both") => {
                    return Err(ApiError::BadRequest("plc=both 는 로봇이 둘일 때 어느 GR 인지 모호합니다 — GR 이름 또는 all 로 지정하세요".into()));
                }
                _ => {}
            }
        }
        Ok(p.unwrap_or(st.default_plc_name()))
    }
    fn force(&self) -> bool {
        flag(self.force.as_deref())
    }
}

pub(crate) fn flag(v: Option<&str>) -> bool {
    matches!(v.map(str::trim).map(str::to_ascii_lowercase).as_deref(), Some("1" | "true" | "yes" | "on"))
}

fn summary_view(s: &ImportSummary) -> Json {
    json!({ "imported": s.imported, "updated": s.updated, "removed": s.removed, "skipped": s.skipped, "errors": s.errors, "warnings": s.warnings })
}

fn diff_view<T>(rows: Vec<Diff<T>>, view: impl Fn(&T) -> Json) -> Vec<Json> {
    rows.into_iter().map(|d| json!({ "id": d.id, "status": d.status, "local": d.local.as_ref().map(&view), "plc": d.plc.as_ref().map(&view) })).collect()
}

/// PLC → local. `?plc=GR2|GRM|gr2_s7` (default: status PLC).
async fn cells_import(State(st): State<AppState>, Query(q): Query<PlcQuery>) -> ApiResult<Json> {
    let h = plc_io::resolve_plc(&st, q.plc_for_write(&st)?)?;
    Ok(axum::Json(summary_view(&plc_io::import_cells(&st, h)?)))
}

/// local → PLC. `?plc=GR1|GR2|GRM|all|both[&force=1]` (`all` = every configured PLC, GR first then GRM).
async fn cells_push(State(st): State<AppState>, Query(q): Query<PlcQuery>) -> ApiResult<Json> {
    let mut results = Vec::new();
    for h in plc_io::plc_targets(&st, q.plc_for_write(&st)?)? {
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
/// 스테이션마다 지금 GRM 트래킹으로 계산한 보정(작업 명령 레일의 "스테이션 보정" 표).
async fn station_offsets(State(st): State<AppState>) -> ApiResult<Vec<crate::issue::StationOffsetRow>> {
    Ok(axum::Json(crate::issue::station_offset_rows(&st)?))
}
/// 스테이션마다 GRM 실시간 상태(Status·Tracking.Now·Interlock) — 레이아웃 맵 색·테두리.
async fn station_live(State(st): State<AppState>) -> ApiResult<Vec<crate::issue::station_live::StationLive>> {
    Ok(axum::Json(crate::issue::station_live::station_live_rows(&st)?))
}
/// 같은 것을 바뀔 때만 — 첫 메시지는 지금 값(`issue::station_live::publish`).
async fn station_live_stream() -> impl axum::response::IntoResponse {
    crate::sse::broadcast_sse(crate::issue::station_live::subscribe(), "stations", Some(crate::issue::station_live::snapshot()))
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
    let h = plc_io::resolve_plc(&st, q.plc_for_write(&st)?)?;
    Ok(axum::Json(summary_view(&plc_io::import_stations(&st, h)?)))
}

async fn stations_push(State(st): State<AppState>, Query(q): Query<PlcQuery>) -> ApiResult<Json> {
    let mut results = Vec::new();
    for h in plc_io::plc_targets(&st, q.plc_for_write(&st)?)? {
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

pub(crate) fn xlsx_response(bytes: Vec<u8>, filename: &str) -> Response {
    ([(header::CONTENT_TYPE, XLSX_MIME.to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{filename}\""))], bytes).into_response()
}

pub(crate) fn stamp() -> String {
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

async fn items_export(State(st): State<AppState>) -> Result<Response, ApiError> {
    let items: Vec<ItemRow> = st.registry.items()?.iter().map(ItemRow::from).collect();
    Ok(xlsx_response(xlsx::export_workbook(None, None, Some(&items))?, &format!("items_{}.xlsx", stamp())))
}

/// `GET /api/items/template.xlsx` — 빈 양식(`Items` + `ItemBeadProfile` 머리글, 자료 줄 없음).
///
/// 내보내기로 양식을 얻으려면 이미 품목이 있어야 한다 — 처음 채우는 현장에는 그 품목이 없다.
/// 파일 이름에 시각을 붙이지 않는 것은 양식이 **늘 같은 파일**이라서다(받은 것을 다시 받아도 같다).
async fn items_template() -> Result<Response, ApiError> {
    Ok(xlsx_response(xlsx::template_workbook()?, "items_template.xlsx"))
}

async fn registry_export(State(st): State<AppState>) -> Result<Response, ApiError> {
    let cells: Vec<CellInfo> = st.registry.cells()?.into_iter().map(|e| e.cell).collect();
    let stations: Vec<StationPara> = st.registry.stations()?.into_iter().map(|e| e.para).collect();
    let items: Vec<ItemRow> = st.registry.items()?.iter().map(ItemRow::from).collect();
    // 운영 데이터 한 파일 — 셀·스테이션·품목 + 재고(`Stock`). 같은 파일을 가져오면 재고도 merge 로 돌아온다.
    let stock = crate::stock::io::export_rows(&st)?;
    Ok(xlsx_response(xlsx::export_workbook_with_stock(Some(&cells), Some(&stations), Some(&items), Some(&stock))?, &format!("registry_{}.xlsx", stamp())))
}

#[derive(Deserialize, Default)]
struct FileQuery {
    dry_run: Option<String>,
}

pub(crate) async fn read_upload(mp: &mut Multipart) -> Result<(String, Vec<u8>), ApiError> {
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
    if t.has_items || t.has_item_profiles {
        c.add(&xlsx::apply_items(&st.registry, t, dry_run)?);
    }
    let errors: Vec<Json> = t.errors.iter().chain(c.errors.iter()).map(|e| json!({ "row": e.row, "sheet": e.sheet, "message": xlsx::error_text(e) })).collect();
    // 비차단 경고 — 바닥 Z ≤ 0 셀은 그대로 들어오고(바닥 평탄도 보정) 운전자에게 PLC 가 작업을 거부한다고만 알린다.
    let warnings = if t.has_cells { xlsx::cells_warnings(&t.cells) } else { Vec::new() };
    // 줄마다 하나씩 나오는 결과(`added|updated|unchanged|skipped`) — 셀 일괄 API 와 같은 말이다.
    // 읽는 중 거부된 줄(`t.errors`)도 적용되지 않았으므로 `skipped` 에 든다: `skipped == errors.len()`.
    // 옛 이름 `imported` 는 그대로 둔다(PLC 읽기 응답과 짝이 맞아야 한다).
    let skipped = c.skipped + t.errors.len();
    Ok(json!({ "imported": c.imported, "added": c.imported, "updated": c.updated, "unchanged": c.unchanged, "removed": 0, "skipped": skipped,
        "errors": errors, "warnings": warnings, "dry_run": dry_run,
        "counts": { "cells": t.cells.len(), "stations": t.stations.len(), "items": t.items.len(), "item_profiles": t.item_profiles.len() } }))
}

async fn import_file(st: &AppState, mp: &mut Multipart, want: Want, dry_run: bool) -> ApiResult<Json> {
    let (name, bytes) = read_upload(mp).await?;
    let t = xlsx::parse_file(&name, &bytes, want)?;
    let mut out = apply_tables(st, &t, dry_run)?;
    // 통합 파일에 `Stock` 시트가 있으면 재고도 같이(merge) — 셀·품목을 먼저 넣은 뒤라 새 자리·새 품목도 받는다.
    // dry_run 에서는 셀·품목이 아직 저장 전이라 새 자리의 재고 줄은 미리보기에서 오류로 보일 수 있다.
    if matches!(want, Want::All) {
        let s = xlsx::parse_stock(&name, &bytes, false)?;
        if s.found() {
            let c = crate::stock::io::apply(st, &s, crate::stock::io::Mode::Merge, dry_run)?;
            merge_stock_counts(&mut out, &c);
        }
    }
    if !dry_run {
        st.emit("registry", json!({ "kind": "file_imported", "file": name, "result": out }));
    }
    Ok(axum::Json(out))
}

/// 재고 결과를 레지스트리 결과 JSON 에 더한다(줄 결과 넷 + 오류 + `counts.stock`).
fn merge_stock_counts(out: &mut Json, c: &crate::stock::io::StockCounts) {
    let add = |out: &mut Json, k: &str, n: usize| out[k] = json!(out[k].as_u64().unwrap_or(0) + n as u64);
    add(out, "imported", c.added);
    add(out, "added", c.added);
    add(out, "updated", c.updated);
    add(out, "unchanged", c.unchanged);
    add(out, "skipped", c.skipped);
    add(out, "removed", c.removed);
    if let Some(errs) = out["errors"].as_array_mut() {
        errs.extend(c.errors.iter().map(|e| json!({ "row": e.row, "sheet": e.sheet, "message": xlsx::error_text(e) })));
    }
    out["counts"]["stock"] = json!(c.added + c.updated + c.unchanged + c.skipped);
}

async fn cells_import_file(State(st): State<AppState>, Query(q): Query<FileQuery>, mut mp: Multipart) -> ApiResult<Json> {
    import_file(&st, &mut mp, Want::Cells, flag(q.dry_run.as_deref())).await
}
async fn stations_import_file(State(st): State<AppState>, Query(q): Query<FileQuery>, mut mp: Multipart) -> ApiResult<Json> {
    import_file(&st, &mut mp, Want::Stations, flag(q.dry_run.as_deref())).await
}
/// Items sheet only — a whole-registry workbook works too (the `Items` sheet is picked by name).
async fn items_import_file(State(st): State<AppState>, Query(q): Query<FileQuery>, mut mp: Multipart) -> ApiResult<Json> {
    import_file(&st, &mut mp, Want::Items, flag(q.dry_run.as_deref())).await
}
async fn registry_import_file(State(st): State<AppState>, Query(q): Query<FileQuery>, mut mp: Multipart) -> ApiResult<Json> {
    import_file(&st, &mut mp, Want::All, flag(q.dry_run.as_deref())).await
}

// ---- defaults
async fn defaults(State(st): State<AppState>) -> ApiResult<Defaults> {
    Ok(axum::Json(st.registry.defaults()?))
}
/// 저장 — 보낸 `version` 이 저장된 것과 다르면 409(다른 곳에서 먼저 저장).
async fn defaults_save(State(st): State<AppState>, axum::Json(d): axum::Json<Defaults>) -> ApiResult<Defaults> {
    Ok(axum::Json(st.registry.save_defaults_checked(d, "")?))
}

#[derive(Deserialize)]
struct GripRefBody {
    grip_ref: String,
}

/// 그립 기준만(전체를 보내지 않는다 — 옛 스냅샷으로 남의 변경을 덮지 않게).
async fn defaults_grip_ref(State(st): State<AppState>, axum::Json(b): axum::Json<GripRefBody>) -> ApiResult<Defaults> {
    Ok(axum::Json(st.registry.set_grip_ref(&b.grip_ref)?))
}

#[derive(Deserialize)]
struct HistoryQuery {
    limit: Option<usize>,
}

async fn defaults_history(State(st): State<AppState>, Query(q): Query<HistoryQuery>) -> ApiResult<Vec<super::DefaultsHistoryRow>> {
    Ok(axum::Json(st.registry.defaults_history(q.limit.unwrap_or(30).clamp(1, 200))?))
}

async fn defaults_restore(State(st): State<AppState>, Path(id): Path<i64>) -> ApiResult<Defaults> {
    Ok(axum::Json(st.registry.restore_defaults(id)?))
}

/// 내보내기 — 다른 PC·백업용 한 파일(`kind` · `schema` 로 알아본다).
async fn defaults_export(State(st): State<AppState>) -> Result<axum::response::Response, ApiError> {
    use axum::http::{HeaderValue, header};
    use axum::response::IntoResponse;
    let d = st.registry.defaults()?;
    let body = serde_json::to_string_pretty(&json!({ "kind": "gr-console.defaults", "schema": 1, "exported_at": crate::util::now_str(), "defaults": d }))?;
    let mut resp = body.into_response();
    let h = resp.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"));
    h.insert(header::CONTENT_DISPOSITION, HeaderValue::from_static("attachment; filename=\"gr-console-defaults.json\""));
    Ok(resp)
}

#[derive(Deserialize)]
struct ImportQuery {
    dry_run: Option<String>,
}

/// 가져오기 — 내보낸 파일(또는 `Defaults` 그 자체). `dry_run=1` 이면 검사·차이만. 저장은 이력에 "import" 로 남는다.
async fn defaults_import(State(st): State<AppState>, Query(q): Query<ImportQuery>, axum::Json(body): axum::Json<Json>) -> ApiResult<Json> {
    let inner = match body.get("kind").and_then(Json::as_str) {
        Some("gr-console.defaults") => body.get("defaults").cloned().unwrap_or(Json::Null),
        Some(other) => return Err(ApiError::BadRequest(format!("기본값 파일이 아닙니다(kind = {other})"))),
        None => body,
    };
    let (mut incoming, dropped) = super::defaults_io::load_lenient(&inner.to_string());
    let problems = super::defaults_io::validate(&incoming);
    let cur = st.registry.defaults()?;
    let changes = super::defaults_io::diff(&cur, &incoming);
    let dry = flag(q.dry_run.as_deref());
    if dry || !problems.is_empty() || !dropped.is_empty() {
        // 버린 키가 있으면 저장하지 않는다 — 파일을 고쳐 다시 가져오게(몰래 일부만 들어가지 않게).
        return Ok(axum::Json(json!({ "saved": false, "changes": changes, "problems": problems, "dropped": dropped })));
    }
    incoming.version = cur.version;
    let saved = st.registry.save_defaults_checked(incoming, "import")?;
    Ok(axum::Json(json!({ "saved": true, "changes": changes, "problems": [], "dropped": [], "version": saved.version })))
}

// ---- compose preview (issue slice; registered here because `routes.rs` is lead-owned)
#[derive(Deserialize)]
pub struct ComposeQuery {
    /// 미리보기용 가정 재고(순차 계획 시뮬레이션 체인) — 있으면 실제 재고 대신 이 개수로 Z 를 계산한다.
    pub stock: Option<u32>,
}

async fn compose_preview(State(st): State<AppState>, Query(q): Query<ComposeQuery>, axum::Json(req): axum::Json<crate::ledger::TaskRequest>) -> ApiResult<crate::issue::Composed> {
    Ok(axum::Json(crate::issue::compose_with(&st, &req, q.stock)?))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/items", get(items).post(item_create))
        .route("/api/items/export.xlsx", get(items_export))
        .route("/api/items/template.xlsx", get(items_template))
        .route("/api/items/import-file", post(items_import_file))
        .route("/api/items/bulk-spec", post(items_bulk_spec))
        .route("/api/items/{code}", axum::routing::put(item_update).delete(item_delete))
        .route("/api/items/{code}/levels", get(item_levels))
        .route("/api/items/{code}/levels/apply-measured", post(item_levels_apply_measured))
        .route("/api/items/{code}/bead-samples", get(item_bead_samples))
        .route("/api/items/{code}/bead-samples/{seq}/apply", post(item_bead_sample_apply))
        .route("/api/cells", get(cells).post(cell_create))
        .route("/api/cells/bulk", post(cells_bulk))
        .route("/api/cells/import", post(cells_import))
        .route("/api/cells/push", post(cells_push))
        .route("/api/cells/diff", get(cells_diff))
        .route("/api/cells/export.xlsx", get(cells_export))
        .route("/api/cells/import-file", post(cells_import_file))
        .route("/api/cells/{id}", axum::routing::put(cell_update).delete(cell_delete))
        .route("/api/stations", get(stations).post(station_create))
        .route("/api/stations/import", post(stations_import))
        .route("/api/stations/push", post(stations_push))
        .route("/api/stations/offsets", get(station_offsets))
        .route("/api/stations/live", get(station_live))
        .route("/api/stations/live/stream", get(station_live_stream))
        .route("/api/stations/diff", get(stations_diff))
        .route("/api/stations/export.xlsx", get(stations_export))
        .route("/api/stations/import-file", post(stations_import_file))
        .route("/api/stations/{id}", axum::routing::put(station_update).delete(station_delete))
        .route("/api/registry/export.xlsx", get(registry_export))
        .route("/api/registry/import-file", post(registry_import_file))
        .route("/api/registry/diff", get(registry_diff))
        .route("/api/defaults", get(defaults).put(defaults_save))
        .route("/api/defaults/grip-ref", axum::routing::put(defaults_grip_ref))
        .route("/api/defaults/history", get(defaults_history))
        .route("/api/defaults/history/{id}/restore", post(defaults_restore))
        .route("/api/defaults/export.json", get(defaults_export))
        .route("/api/defaults/import", post(defaults_import))
        .route("/api/issue/compose", post(compose_preview))
}

/// Both tables at once (plan: `GET /api/registry/diff?plc=`).
async fn registry_diff(State(st): State<AppState>, Query(q): Query<PlcQuery>) -> ApiResult<Json> {
    let h = plc_io::resolve_plc(&st, q.plc(&st))?;
    Ok(axum::Json(json!({ "plc": h.name(), "cells": diff_view(plc_io::diff_cells(&st, h)?, cell_view), "stations": diff_view(plc_io::diff_stations(&st, h)?, station_view) })))
}

#[derive(Deserialize, Default)]
pub struct BulkQuery {
    /// `append`(기본, 아무것도 지우지 않고 충돌만 건너뜀) · `replace_section` · `overwrite`.
    /// 비어 있으면 옛 클라이언트 규약대로 `replace_section` 값이 있을 때만 구역 교체다.
    pub mode: Option<String>,
    /// Delete every existing cell of this section first (layout regeneration).
    pub replace_section: Option<u16>,
    /// id 가 겹치는 생성 셀을 그 구간의 빈 id 로 옮긴다(`append` 에서만 뜻이 있다).
    pub auto_id: Option<String>,
    /// 겹침 판정 지름(mm). 없으면 셀의 `max(length, width)`.
    pub diameter: Option<f32>,
    /// 겹침 허용 오차(mm, 기본 `bulk::OVERLAP_TOLERANCE`).
    pub tolerance: Option<f32>,
}

impl BulkQuery {
    fn mode(&self) -> Result<bulk::Mode, ApiError> {
        let sec = || self.replace_section.map(bulk::Mode::ReplaceSection).ok_or_else(|| ApiError::BadRequest("mode=replace_section needs replace_section=<1..3>".into()));
        match self.mode.as_deref().map(str::trim).unwrap_or("") {
            "" => Ok(self.replace_section.map(bulk::Mode::ReplaceSection).unwrap_or(bulk::Mode::Append)),
            "append" => Ok(bulk::Mode::Append),
            "overwrite" => Ok(bulk::Mode::Overwrite),
            "replace_section" | "replace" => sec(),
            m => Err(ApiError::BadRequest(format!("unknown mode {m} (append | replace_section | overwrite)"))),
        }
    }
}

/// `POST /api/cells/bulk?mode=&replace_section=&auto_id=&diameter=` — 여러 로컬 셀을 한 번에(레이아웃 편집기).
/// 모든 줄을 먼저 검증하고(하나라도 틀리면 아무것도 쓰지 않는다), 병합 규칙은 `bulk::plan` 이 정한다.
/// 결과에는 줄마다 `added|updated|unchanged|skipped|remapped` 와 사유가 실린다.
async fn cells_bulk(State(st): State<AppState>, Query(q): Query<BulkQuery>, axum::Json(rows): axum::Json<Vec<CellBody>>) -> ApiResult<Json> {
    let infos: Vec<gr_proto::CellInfo> = rows.iter().map(|b| b.info()).collect();
    let mut seen = std::collections::HashSet::new();
    for (i, info) in infos.iter().enumerate() {
        xlsx::validate_cell(info).map_err(|e| ApiError::BadRequest(format!("row {}: {e}", i + 1)))?;
        if !seen.insert(info.id) {
            return Err(ApiError::BadRequest(format!("row {}: duplicate id {}", i + 1, info.id)));
        }
    }
    let opts = bulk::Options { mode: q.mode()?, auto_id: flag(q.auto_id.as_deref()), diameter: q.diameter, tolerance: q.tolerance.unwrap_or(bulk::OVERLAP_TOLERANCE) };
    let before: Vec<gr_proto::CellInfo> = st.registry.cells()?.into_iter().map(|c| c.cell).collect();
    let plan = bulk::plan(&infos, &before, &opts);
    let mut removed = 0usize;
    for id in &plan.deletes {
        if st.registry.delete_cell(*id)? {
            removed += 1;
        }
    }
    let written: Vec<gr_proto::CellInfo> = plan.writes().cloned().collect();
    for info in &written {
        st.registry.upsert_cell(info, "local", true, None)?;
    }
    let mut warnings = xlsx::cells_warnings(&written);
    warnings.extend(plan.warnings.iter().cloned());
    Ok(axum::Json(json!({
        "mode": opts.mode.name(), "auto_id": opts.auto_id,
        "created": plan.count(bulk::Outcome::Added), "updated": plan.count(bulk::Outcome::Updated), "unchanged": plan.count(bulk::Outcome::Unchanged),
        "skipped": plan.count(bulk::Outcome::Skipped), "remapped": plan.count(bulk::Outcome::Remapped),
        "removed": removed, "total": st.registry.cells()?.len(), "rows": plan.rows_view(), "warnings": warnings
    })))
}
