//! Excel (`rust_xlsxwriter` out / `calamine` in) and CSV codec for the registry tables.
//!
//! Sheets: `Cells`, `Stations`, `Items`, `ItemBeadProfile`. Import maps columns by **header name**
//! (case/space-insensitive, English or Korean aliases), validates each row and reports `{row, message}` per
//! rejected row. Parsing is pure (bytes → structs) so the round trip is unit-testable; applying to sqlite is
//! a separate step.
//!
//! Item spec (`spec.rs`): the `StackMax` / `PalletMax` / `WeightKg` / `PickBeadOffset` / `Compression`
//! columns and the `ItemBeadProfile` sheet are optional. A **missing column / sheet leaves that part of an
//! existing item's spec unchanged** (a new item gets the default); a present column with an empty cell means
//! 0 / none. When the `ItemBeadProfile` sheet is present it is authoritative for every code in the `Items`
//! sheet (a code with no rows = no profile); rows for codes not in the `Items` sheet update the profiles of
//! existing items only. A profile sheet with **only its header row counts as absent** — the blank template
//! (`template_workbook`) carries one, and filling only its `Items` sheet must not wipe measured beads.
//!
//! 정본은 **잰 스택 크기별 절대 프로파일**(`Stack` + `Level` 키, 셀 바닥 기준 mm)이다 — 하중(`Above`)
//! 곡선은 콘솔이 여기서 파생하므로 시트로 오가지 않는다.

use std::collections::{BTreeMap, HashSet};
use std::io::Cursor;

use calamine::{Data, Reader};
use gr_proto::{CellInfo, SensorSettings, StationPara, StockItem};
use serde::Serialize;

use super::spec::{self, BeadProfile, ItemSpec, ProfileRow, validate_spec, validate_spec_for};
use super::{ItemEntry, Registry};
use crate::error::ApiError;

pub const CELL_COLS: [&str; 11] = ["Id", "Use", "BlendUse", "Section", "Row", "Col", "Length", "Width", "X", "Y", "Z"];
pub const STATION_COLS: [&str; 26] = [
    "Id",
    "ConvNo",
    "TaskType",
    "RotateType",
    "Group",
    "GroupIndex",
    "ConnPrev",
    "ConnNext",
    "Use",
    "BlendUse",
    "Section",
    "Row",
    "Col",
    "Length",
    "Width",
    "X",
    "Y",
    "Z",
    "IOLinkModule",
    "IOLinkPortL",
    "IOLinkPortR",
    "DetectionFactor",
    "AllowRange",
    "LSensorOffset",
    "RSensorOffset",
    "IOBlockNo",
];
pub const ITEM_COLS: [&str; 15] = [
    "Code",
    "Name",
    "Count",
    "InnerDiameter",
    "OuterDiameter",
    "LowerBeadHeight",
    "UpperBeadHeight",
    "Height",
    "DeflectionFactor",
    "StackMax",
    "PalletMax",
    "WeightKg",
    "PickBeadOffset",
    "Compression",
    "Note",
];
/// 잰 스택 크기(`Stack`)별 **절대** 비드 프로파일 시트 — 정본이자 내보내기·가져오기의 유일한 비드 시트.
/// 비드·적재 높이는 셀 바닥 기준 mm 다. 하중(`Above`) 곡선은 콘솔이 여기서 파생한다.
pub const PROFILE_COLS: [&str; 12] = ["Code", "Stack", "Level", "LowerBead", "UpperBead", "StackHeight", "Source", "SamplePlc", "SampleSeq", "TotalHeight", "EachHeight", "At"];

/// 프로파일 시트 한 줄(= 한 단). 스택 단위 값(`total_height` 등)은 같은 스택의 모든 줄에 되풀이된다.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProfileSheetRow {
    pub code: u32,
    pub count: u8,
    pub row: ProfileRow,
    pub sample_plc: String,
    pub sample_seq: u32,
    pub total_height: Option<f32>,
    pub each_height: Option<f32>,
    pub at: String,
}

/// Which spec parts a sheet row carried (`None` = column/sheet absent → keep the stored value).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpecPatch {
    pub stack_max: Option<u8>,
    pub pallet_max: Option<u8>,
    pub weight_kg: Option<Option<f32>>,
    /// 잰 스택 크기별 절대 비드 프로파일(정본).
    pub profiles: Option<Vec<BeadProfile>>,
    pub pick_bead_offset: Option<Option<f32>>,
    pub compression: Option<Option<f32>>,
}

impl SpecPatch {
    /// Every part present (export side).
    pub fn full(s: &ItemSpec) -> Self {
        SpecPatch {
            stack_max: Some(s.stack_max),
            pallet_max: Some(s.pallet_max),
            weight_kg: Some(s.weight_kg),
            profiles: Some(s.profiles.clone()),
            pick_bead_offset: Some(s.pick_bead_offset),
            compression: Some(s.compression),
        }
    }
    pub fn apply(&self, base: &ItemSpec) -> ItemSpec {
        ItemSpec {
            stack_max: self.stack_max.unwrap_or(base.stack_max),
            pallet_max: self.pallet_max.unwrap_or(base.pallet_max),
            weight_kg: self.weight_kg.unwrap_or(base.weight_kg),
            profiles: self.profiles.clone().unwrap_or_else(|| base.profiles.clone()),
            bead_source: base.bead_source.clone(),
            pick_bead_offset: self.pick_bead_offset.unwrap_or(base.pick_bead_offset),
            compression: self.compression.unwrap_or(base.compression),
            compression_source: base.compression_source.clone(),
            auto_apply_measured: base.auto_apply_measured,
        }
    }
}

/// Item as it travels through a sheet (the registry keeps name/note/spec beside the PLC `StockItem`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemRow {
    pub code: u32,
    pub name: String,
    pub item: StockItem,
    pub note: String,
    pub spec: SpecPatch,
}

impl From<&ItemEntry> for ItemRow {
    fn from(e: &ItemEntry) -> Self {
        ItemRow { code: e.code, name: e.name.clone(), item: e.item.clone(), note: e.note.clone(), spec: SpecPatch::full(&e.spec) }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct RowError {
    pub sheet: String,
    /// 1-based sheet row (`0` = 줄을 짚을 수 없는 오류).
    pub row: usize,
    pub message: String,
}

/// 오류 한 줄의 사람 문장 — **줄을 앞세운다**.
///
/// 운전자가 받은 것은 열다섯 열짜리 시트이고, 고칠 수 있는 단위는 줄이다. `count must be >= 1` 만
/// 오면 어느 줄인지 찾느라 파일을 훑어야 한다 — 표에 열이 따로 있어도 토스트 한 줄과 복사해 붙인
/// 메시지에는 줄이 같이 붙어 있어야 쓸모가 있다.
pub fn error_text(e: &RowError) -> String {
    let sheet = e.sheet.trim();
    match (sheet.is_empty() || sheet.eq_ignore_ascii_case("csv"), e.row) {
        (true, 0) => e.message.clone(),
        (true, n) => format!("row {n}: {}", e.message),
        (false, 0) => format!("{sheet}: {}", e.message),
        (false, n) => format!("{sheet} row {n}: {}", e.message),
    }
}

#[derive(Clone, Debug, Default)]
pub struct Tables {
    pub cells: Vec<CellInfo>,
    pub stations: Vec<StationPara>,
    pub items: Vec<ItemRow>,
    /// `items[i]` 가 온 시트 줄 — 저장된 값과 합쳐야 드러나는 오류도 줄을 짚을 수 있게.
    pub item_rows: Vec<usize>,
    /// 품목이 실린 시트 이름(csv 면 `csv`) — 오류 문장이 진짜 시트를 부르게.
    pub items_sheet: String,
    /// Parsed `ItemBeadProfile` rows — merged into `items[*].spec.profiles`.
    pub item_profiles: Vec<ProfileSheetRow>,
    /// 프로파일 시트에서 각 코드가 처음 나온 줄.
    pub profile_row_of: BTreeMap<u32, usize>,
    pub profile_sheet: String,
    pub orphan_profiles: Vec<(u32, Vec<BeadProfile>)>,
    pub errors: Vec<RowError>,
    /// Which tables the file actually carried (a missing sheet is not an error).
    pub has_cells: bool,
    pub has_stations: bool,
    pub has_items: bool,
    pub has_item_profiles: bool,
}

impl Tables {
    fn items_sheet_name(&self) -> String {
        if self.items_sheet.is_empty() { "Items".into() } else { self.items_sheet.clone() }
    }
    fn profile_sheet_name(&self) -> String {
        if self.profile_sheet.is_empty() { "ItemBeadProfile".into() } else { self.profile_sheet.clone() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    Cells,
    Stations,
    Items,
    All,
}

// ---- validation (shared with the CRUD routes)

pub fn validate_cell(c: &CellInfo) -> Result<(), String> {
    if !(gr_proto::CELL_ID_MIN..=gr_proto::CELL_ID_MAX).contains(&c.id) {
        return Err(format!("cell id {} must be 1..1000", c.id));
    }
    if !(1..=3).contains(&c.section) {
        return Err(format!("section {} must be 1..3", c.section));
    }
    // 셀 바닥 Z 는 **바닥 평탄도 보정**이라 0 이나 음수일 수 있다(현장 CELL 301..305 = -8.8 … -23.5).
    // 타이어 높이가 더해져 명령이 나가므로 데이터로도 작업으로도 정상이다: GR2 `isValidTaskData`(79–83행)의
    // INVALID_CELL_POSZ 검사는 `IF #Task.Cell.Id > 2000` 안에 있어 **스테이션 대상에만** 걸린다(셀 id 는 1..1000).
    // 그래서 셀에는 경고도 내지 않는다(`floor_z_warning` 은 스테이션 id 만 본다).
    // 숫자가 아닌 Z(NaN/inf)는 계산을 망가뜨리므로 여전히 오류다.
    // 셀 X/Y 는 0 이상이면 된다(태스크 위치는 RangeMin..Max 로 검사).
    let [x, y, z] = c.position;
    if let Some((axis, v)) = [("X", x), ("Y", y)].into_iter().find(|(_, v)| *v < 0.0 || v.is_nan()) {
        return Err(format!("position {axis} = {v} must be >= 0"));
    }
    if !z.is_finite() {
        return Err(format!("position Z = {z} must be a finite number"));
    }
    Ok(())
}

/// 바닥 Z ≤ 0 경고 문구 — 한 군데서만 만든다(레지스트리 CRUD · 파일/PLC 가져오기 · PLC 쓰기 · 작성 미리보기).
/// GR2 `isValidTaskData` 의 INVALID_CELL_POSZ 는 `Task.Cell.Id > 2000`(스테이션)일 때만 보므로,
/// 셀(id 1..1000)은 Z 가 음수여도 경고하지 않는다. 유한하지 않은 Z 는 `validate_cell` 이 막는다.
pub fn floor_z_warning(kind: &str, id: u16, z: f32) -> Option<String> {
    (id > 2000 && z <= 0.0).then(|| format!("{kind} {id} Z {z} ≤ 0 — GR2 가 INVALID_CELL_POSZ 로 작업을 거부합니다"))
}

/// 셀 한 줄의 비차단 경고(셀 바닥 Z 는 PLC 가 검사하지 않으므로 지금은 없다 — 채널만 유지).
pub fn cell_warnings(c: &CellInfo) -> Vec<String> {
    floor_z_warning("셀", c.id, c.position[2]).into_iter().collect()
}

/// 여러 셀의 경고를 한 줄씩 모은다(가져오기·쓰기 요약 채널).
pub fn cells_warnings<'a>(cells: impl IntoIterator<Item = &'a CellInfo>) -> Vec<String> {
    cells.into_iter().flat_map(cell_warnings).collect()
}

pub fn validate_station(p: &StationPara) -> Result<(), String> {
    let id = p.info.id;
    if !gr_proto::is_station_id(id) {
        return Err(format!("station id {id} must be 2001..2999 with id mod 100 in 1..32"));
    }
    if !(1..=3).contains(&p.info.section) {
        return Err(format!("section {} must be 1..3", p.info.section));
    }
    if let Some((axis, v)) = ["X", "Y", "Z"].iter().zip(p.info.position).find(|(_, v)| *v <= 0.0 || v.is_nan()) {
        return Err(format!("position {axis} = {v} must be > 0"));
    }
    Ok(())
}

pub fn validate_item(i: &ItemRow) -> Result<(), String> {
    if i.code == 0 {
        return Err("code must be > 0".into());
    }
    let s = &i.item;
    if s.count == 0 {
        return Err("count must be >= 1".into());
    }
    let dims = [("inner_diameter", s.inner_diameter), ("outer_diameter", s.outer_diameter), ("lower_bead_height", s.lower_bid_height), ("upper_bead_height", s.upper_bid_height), ("height", s.height)];
    if let Some((k, v)) = dims.iter().find(|(_, v)| !v.is_finite() || *v < 0.0) {
        return Err(format!("{k} = {v} must be a finite number >= 0"));
    }
    if !s.deflection_factor.is_finite() {
        return Err(format!("deflection_factor = {} must be a finite number", s.deflection_factor));
    }
    // 0 은 "미입력"이라 비교하지 않는다(치수를 나중에 채우는 품목이 있다).
    if s.inner_diameter > 0.0 && s.outer_diameter > 0.0 && s.inner_diameter >= s.outer_diameter {
        return Err(format!("inner_diameter {} must be < outer_diameter {}", s.inner_diameter, s.outer_diameter));
    }
    Ok(())
}

// ---- export

fn xerr(e: rust_xlsxwriter::XlsxError) -> ApiError {
    ApiError::Internal(format!("xlsx: {e}"))
}

fn write_sheet<T>(wb: &mut rust_xlsxwriter::Workbook, name: &str, cols: &[&str], rows: &[T], cell: impl Fn(&T, usize) -> Field) -> Result<(), ApiError> {
    let ws = wb.add_worksheet();
    ws.set_name(name).map_err(xerr)?;
    let bold = rust_xlsxwriter::Format::new().set_bold();
    for (c, h) in cols.iter().enumerate() {
        ws.write_with_format(0, c as u16, *h, &bold).map_err(xerr)?;
    }
    for (r, row) in rows.iter().enumerate() {
        for c in 0..cols.len() {
            let rr = (r + 1) as u32;
            match cell(row, c) {
                Field::Num(v) => ws.write(rr, c as u16, v).map_err(xerr)?,
                Field::Bool(b) => ws.write(rr, c as u16, b).map_err(xerr)?,
                Field::Text(s) => ws.write(rr, c as u16, s.as_str()).map_err(xerr)?,
            };
        }
    }
    ws.autofit();
    ws.set_freeze_panes(1, 0).map_err(xerr)?;
    Ok(())
}

enum Field {
    Num(f64),
    Bool(bool),
    Text(String),
}

fn cell_field(c: &CellInfo, i: usize) -> Field {
    match i {
        0 => Field::Num(c.id as f64),
        1 => Field::Bool(c.use_),
        2 => Field::Bool(c.blend_use),
        3 => Field::Num(c.section as f64),
        4 => Field::Num(c.row as f64),
        5 => Field::Num(c.col as f64),
        6 => Field::Num(c.length as f64),
        7 => Field::Num(c.width as f64),
        8 => Field::Num(c.position[0] as f64),
        9 => Field::Num(c.position[1] as f64),
        _ => Field::Num(c.position[2] as f64),
    }
}

fn station_field(p: &StationPara, i: usize) -> Field {
    let s = &p.sensor_settings;
    match i {
        0 => Field::Num(p.info.id as f64),
        1 => Field::Num(p.conv_no as f64),
        2 => Field::Num(p.task_type as f64),
        3 => Field::Num(p.rotate_type as f64),
        4 => Field::Num(p.group as f64),
        5 => Field::Num(p.group_index as f64),
        6 => Field::Num(p.connection_prev as f64),
        7 => Field::Num(p.connection_next as f64),
        8..=17 => cell_field(&p.info, i - 7),
        18 => Field::Num(s.io_link_master_module as f64),
        19 => Field::Num(s.io_link_master_port_l as f64),
        20 => Field::Num(s.io_link_master_port_r as f64),
        21 => Field::Num(s.detection_factor as f64),
        22 => Field::Num(s.allow_range as f64),
        23 => Field::Num(s.l_sensor_offset as f64),
        24 => Field::Num(s.r_sensor_offset as f64),
        _ => Field::Num(p.io_block_no as f64),
    }
}

fn item_field(it: &ItemRow, i: usize) -> Field {
    let s = &it.item;
    match i {
        0 => Field::Num(it.code as f64),
        1 => Field::Text(it.name.clone()),
        2 => Field::Num(s.count as f64),
        3 => Field::Num(s.inner_diameter as f64),
        4 => Field::Num(s.outer_diameter as f64),
        5 => Field::Num(s.lower_bid_height as f64),
        6 => Field::Num(s.upper_bid_height as f64),
        7 => Field::Num(s.height as f64),
        8 => Field::Num(s.deflection_factor as f64),
        9 => Field::Num(it.spec.stack_max.unwrap_or(0) as f64),
        10 => Field::Num(it.spec.pallet_max.unwrap_or(0) as f64),
        11 => opt_num(it.spec.weight_kg.flatten()),
        12 => opt_num(it.spec.pick_bead_offset.flatten()),
        13 => opt_num(it.spec.compression.flatten()),
        _ => Field::Text(it.note.clone()),
    }
}

fn opt_num(v: Option<f32>) -> Field {
    v.map(|x| Field::Num(x as f64)).unwrap_or(Field::Text(String::new()))
}

fn profile_field(r: &ProfileSheetRow, i: usize) -> Field {
    match i {
        0 => Field::Num(r.code as f64),
        1 => Field::Num(r.count as f64),
        2 => Field::Num(r.row.level as f64),
        3 => opt_num(r.row.lower_bead),
        4 => opt_num(r.row.upper_bead),
        5 => opt_num(r.row.stack_height),
        6 => Field::Text(r.row.source.clone()),
        7 => Field::Text(r.sample_plc.clone()),
        8 => Field::Num(r.sample_seq as f64),
        9 => opt_num(r.total_height),
        10 => opt_num(r.each_height),
        _ => Field::Text(r.at.clone()),
    }
}

/// 품목들의 프로파일을 시트 줄로 편다.
fn profile_rows(items: &[ItemRow]) -> Vec<ProfileSheetRow> {
    let mut out = Vec::new();
    for it in items {
        for p in it.spec.profiles.iter().flatten() {
            for row in &p.rows {
                out.push(ProfileSheetRow {
                    code: it.code,
                    count: p.count,
                    row: row.clone(),
                    sample_plc: p.sample_plc.clone(),
                    sample_seq: p.sample_seq,
                    total_height: p.total_height,
                    each_height: p.each_height,
                    at: p.at.clone(),
                });
            }
        }
    }
    out
}

/// Builds a workbook with the sheets given (`None` = omit the sheet).
pub fn export_workbook(cells: Option<&[CellInfo]>, stations: Option<&[StationPara]>, items: Option<&[ItemRow]>) -> Result<Vec<u8>, ApiError> {
    export_workbook_with_stock(cells, stations, items, None)
}

/// 같은 것 + 재고(`Stock` 시트) — 운영 데이터 한 파일(`GET /api/registry/export.xlsx`)·재고만(`GET /api/stock/export.xlsx`).
pub fn export_workbook_with_stock(cells: Option<&[CellInfo]>, stations: Option<&[StationPara]>, items: Option<&[ItemRow]>, stock: Option<&[StockExportRow]>) -> Result<Vec<u8>, ApiError> {
    let mut wb = rust_xlsxwriter::Workbook::new();
    if let Some(c) = cells {
        write_sheet(&mut wb, "Cells", &CELL_COLS, c, cell_field)?;
    }
    if let Some(s) = stations {
        write_sheet(&mut wb, "Stations", &STATION_COLS, s, station_field)?;
    }
    if let Some(i) = items {
        write_sheet(&mut wb, "Items", &ITEM_COLS, i, item_field)?;
        write_sheet(&mut wb, "ItemBeadProfile", &PROFILE_COLS, &profile_rows(i), profile_field)?;
    }
    if let Some(s) = stock {
        write_sheet(&mut wb, STOCK_SHEET, &STOCK_COLS, s, stock_field)?;
    }
    if cells.is_none() && stations.is_none() && items.is_none() && stock.is_none() {
        wb.add_worksheet();
    }
    wb.save_to_buffer().map_err(xerr)
}

// ---- stock sheet

pub const STOCK_SHEET: &str = "Stock";
/// `ItemName`·`UpdatedAt` 은 읽기 쉬우라고 싣는 열 — 가져올 때는 읽지 않는다.
const STOCK_COLS: [&str; 6] = ["CellId", "ItemCode", "ItemName", "Count", "Note", "UpdatedAt"];

/// 내보낼 재고 한 줄(셀·스테이션 모두 — 스테이션은 컨베이어 화물 코드).
#[derive(Clone, Debug, Default)]
pub struct StockExportRow {
    pub cell_id: u16,
    pub item_code: u32,
    pub item_name: String,
    pub count: u32,
    pub note: String,
    pub updated_at: String,
}

fn stock_field(r: &StockExportRow, i: usize) -> Field {
    match i {
        0 => Field::Num(r.cell_id as f64),
        1 => Field::Num(r.item_code as f64),
        2 => Field::Text(r.item_name.clone()),
        3 => Field::Num(r.count as f64),
        4 => Field::Text(r.note.clone()),
        _ => Field::Text(r.updated_at.clone()),
    }
}

/// 파일의 재고 한 줄. `count = 0` 은 "비움".
#[derive(Clone, Debug, PartialEq)]
pub struct StockRow {
    pub cell_id: u16,
    pub item_code: u32,
    pub count: u32,
    pub note: String,
}

#[derive(Debug, Default)]
pub struct StockSheet {
    /// 읽은 시트 이름(csv 면 `csv`). 재고 시트가 없으면 빈 문자열.
    pub sheet: String,
    /// (1-based 시트 줄, 값)
    pub rows: Vec<(usize, StockRow)>,
    pub errors: Vec<RowError>,
}

impl StockSheet {
    pub fn found(&self) -> bool {
        !self.sheet.is_empty()
    }
}

fn parse_stock_row(r: &Row) -> Result<StockRow, String> {
    let cell_id: u16 = r.int("Id")?;
    if cell_id == 0 {
        return Err("CellId 가 비었다".into());
    }
    let item_code: u32 = if r.get("ItemCode").is_some() { r.int("ItemCode")? } else { r.int("Code")? };
    let count: u32 = r.int("Count")?;
    if count > 0 && item_code == 0 {
        return Err(format!("Count {count} 인데 ItemCode 가 없다"));
    }
    Ok(StockRow { cell_id, item_code, count, note: r.text("Note") })
}

/// 재고 파일 읽기. 시트는 이름 `Stock` → (`any_sheet` 면) 첫 시트. `CellId`(= `Id`) · `Count` 머리글이 있어야 한다.
/// 통합 레지스트리 파일은 `any_sheet = false` 로 부른다(재고 시트가 없으면 재고는 건드리지 않는다).
pub fn parse_stock(name: &str, bytes: &[u8], any_sheet: bool) -> Result<StockSheet, ApiError> {
    let raw: Vec<(String, Grid)> = if is_csv(name) { vec![("csv".into(), grid_from_csv(bytes)?)] } else { sheets_from_xlsx(bytes)? };
    let sheets: Vec<Sheet> = raw.iter().filter_map(|(n, g)| Sheet::from_grid(n, g)).collect();
    let pick = sheets.iter().find(|s| s.is_stock()).or_else(|| if any_sheet { sheets.first() } else { None });
    let Some(sheet) = pick else { return Ok(StockSheet::default()) };
    let mut out = StockSheet { sheet: sheet.name.clone(), ..Default::default() };
    for (k, label) in [("Id", "CellId"), ("Count", "Count")] {
        if !sheet.has(k) {
            out.errors.push(RowError {
                sheet: sheet.name.clone(),
                row: 1,
                message: format!("머리글에 '{label}' 열이 없다 (있는 열: {})", sheet.cols.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(", ")),
            });
        }
    }
    if !out.errors.is_empty() {
        return Ok(out);
    }
    for (rn, cells) in &sheet.rows {
        match parse_stock_row(&Row { sheet, cells }) {
            Ok(r) => out.rows.push((*rn, r)),
            Err(m) => out.errors.push(RowError { sheet: sheet.name.clone(), row: *rn, message: m }),
        }
    }
    Ok(out)
}

/// 빈 품목 양식 — `Items` + `ItemBeadProfile` 머리글만(`GET /api/items/template.xlsx`).
///
/// **예시 줄은 넣지 않는다.** 예시를 지우지 않고 올린 파일은 있지도 않은 품목을 하나 만들거나
/// 오류 한 줄을 내고, 어느 쪽이든 "이 줄은 지우세요"를 어딘가에 또 적어야 한다. 줄이 없으면
/// 받은 그대로 다시 올려도 아무것도 바뀌지 않는다 — 처음 쓰는 사람이 왕복을 먼저 확인할 수 있다.
/// 채워야 하는 것은 `Code`(없으면 줄이 거부된다)와 `Name`(비면 이름 없는 품목이 된다)뿐이고,
/// 나머지 열은 비우면 기본값이다(`docs/item-spec-z.md`).
pub fn template_workbook() -> Result<Vec<u8>, ApiError> {
    export_workbook(None, None, Some(&[]))
}

// ---- import: bytes → grid → structs

#[derive(Clone, Debug, PartialEq)]
enum Cell {
    Empty,
    Num(f64),
    Text(String),
    Bool(bool),
}

impl Cell {
    fn is_empty(&self) -> bool {
        match self {
            Cell::Empty => true,
            Cell::Text(s) => s.trim().is_empty(),
            _ => false,
        }
    }
    fn text(&self) -> String {
        match self {
            Cell::Empty => String::new(),
            Cell::Num(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    format!("{}", *n as i64)
                } else {
                    n.to_string()
                }
            }
            Cell::Text(s) => s.trim().to_string(),
            Cell::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        }
    }
    fn num(&self) -> Option<f64> {
        match self {
            Cell::Num(n) => Some(*n),
            Cell::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Cell::Text(s) => s.trim().replace(',', "").parse::<f64>().ok(),
            Cell::Empty => None,
        }
    }
    fn boolean(&self) -> Option<bool> {
        match self {
            Cell::Bool(b) => Some(*b),
            Cell::Num(n) => Some(*n != 0.0),
            Cell::Text(s) => match s.trim().to_ascii_lowercase().as_str() {
                "true" | "1" | "y" | "yes" | "o" | "on" | "예" | "사용" | "true()" => Some(true),
                "false" | "0" | "n" | "no" | "x" | "off" | "아니오" | "미사용" | "" => Some(false),
                _ => None,
            },
            Cell::Empty => None,
        }
    }
}

impl From<&Data> for Cell {
    fn from(d: &Data) -> Self {
        match d {
            Data::Int(i) => Cell::Num(*i as f64),
            Data::Float(f) => Cell::Num(*f),
            Data::String(s) => Cell::Text(s.clone()),
            Data::Bool(b) => Cell::Bool(*b),
            Data::DateTime(dt) => Cell::Num(dt.as_f64()),
            Data::DateTimeIso(s) | Data::DurationIso(s) => Cell::Text(s.clone()),
            Data::Error(e) => Cell::Text(format!("{e:?}")),
            Data::Empty => Cell::Empty,
        }
    }
}

type Grid = Vec<Vec<Cell>>;

/// Header normalisation: lowercase, drop spaces / `_` / `-` / `.` / parentheses.
fn norm(h: &str) -> String {
    h.chars().filter(|c| !matches!(c, ' ' | '_' | '-' | '.' | '(' | ')' | '\t')).flat_map(char::to_lowercase).collect()
}

/// Canonical key of a header (English column name or a Korean alias) — `None` if unknown.
fn canonical(h: &str) -> Option<&'static str> {
    let n = norm(h);
    let k = match n.as_str() {
        "id" | "셀id" | "셀" | "스테이션id" | "스테이션" | "아이디" | "번호" | "cellid" | "stationid" => "Id",
        "use" | "사용" | "사용여부" | "enabled" => "Use",
        "blenduse" | "blend" | "블렌드" | "블렌드사용" => "BlendUse",
        "section" | "구역" | "섹션" => "Section",
        "row" | "행" => "Row",
        "col" | "column" | "열" => "Col",
        "length" | "lenth" | "len" | "길이" => "Length",
        "width" | "폭" | "너비" => "Width",
        "x" | "posx" | "positionx" | "position[1]" | "x좌표" => "X",
        "y" | "posy" | "positiony" | "position[2]" | "y좌표" => "Y",
        "z" | "posz" | "positionz" | "position[3]" | "z좌표" => "Z",
        "convno" | "conv" | "컨베이어" | "컨베이어번호" => "ConvNo",
        "tasktype" | "작업종류" | "작업유형" => "TaskType",
        "rotatetype" | "회전" | "회전종류" => "RotateType",
        "group" | "그룹" => "Group",
        "groupindex" | "그룹순번" | "그룹인덱스" => "GroupIndex",
        "connprev" | "connectionprev" | "prev" | "이전연결" | "이전" => "ConnPrev",
        "connnext" | "connectionnext" | "next" | "다음연결" | "다음" => "ConnNext",
        "iolinkmodule" | "iolinkmastermodule" | "io링크모듈" | "iolink모듈" => "IOLinkModule",
        "iolinkportl" | "iolinkmasterportl" | "portl" | "좌포트" | "io링크포트l" => "IOLinkPortL",
        "iolinkportr" | "iolinkmasterportr" | "portr" | "우포트" | "io링크포트r" => "IOLinkPortR",
        "detectionfactor" | "검출계수" => "DetectionFactor",
        "allowrange" | "허용범위" => "AllowRange",
        "lsensoroffset" | "좌센서오프셋" | "l센서오프셋" => "LSensorOffset",
        "rsensoroffset" | "우센서오프셋" | "r센서오프셋" => "RSensorOffset",
        "ioblockno" | "ioblock" | "io블록" | "io블록번호" => "IOBlockNo",
        "code" | "코드" | "품목코드" | "타이어코드" => "Code",
        "name" | "이름" | "품명" | "품목명" => "Name",
        "count" | "수량" | "적재수" | "개수" => "Count",
        "itemcode" => "ItemCode",
        "innerdiameter" | "내경" | "id(mm)" => "InnerDiameter",
        "outerdiameter" | "외경" => "OuterDiameter",
        "lowerbeadheight" | "lowerbidheight" | "하부비드" | "하부비드높이" => "LowerBeadHeight",
        "upperbeadheight" | "upperbidheight" | "상부비드" | "상부비드높이" => "UpperBeadHeight",
        "height" | "높이" => "Height",
        "deflectionfactor" | "처짐계수" => "DeflectionFactor",
        "stackmax" | "단수max" | "최대단수" | "적재단수max" => "StackMax",
        "palletmax" | "팔레트max" | "팔레트최대" => "PalletMax",
        "weightkg" | "weight" | "무게" | "무게kg" | "중량" => "WeightKg",
        "pickbeadoffset" | "픽비드오프셋" | "집는비드오프셋" => "PickBeadOffset",
        "compression" | "눌림" | "눌림양" => "Compression",
        "lowerbead" | "하단비드" => "LowerBead",
        "upperbead" | "상단비드" => "UpperBead",
        "stack" | "stackcount" | "스택" | "단수" | "잰단수" => "Stack",
        "level" | "단" | "단번호" => "Level",
        "stackheight" | "적재높이" => "StackHeight",
        "totalheight" | "전체높이" => "TotalHeight",
        "eachheight" | "한개높이" => "EachHeight",
        "at" | "시각" | "측정시각" => "At",
        "source" | "출처" => "Source",
        "sampleplc" | "표본plc" | "측정로봇" => "SamplePlc",
        "sampleseq" | "표본seq" | "측정seq" => "SampleSeq",
        "note" | "비고" | "메모" | "설명" => "Note",
        _ => return None,
    };
    Some(k)
}

struct Sheet {
    name: String,
    /// canonical key → column index
    cols: Vec<(&'static str, usize)>,
    /// (1-based sheet row, cells)
    rows: Vec<(usize, Vec<Cell>)>,
}

impl Sheet {
    fn from_grid(name: &str, grid: &Grid) -> Option<Sheet> {
        let (hi, header) = grid.iter().enumerate().find(|(_, r)| r.iter().any(|c| !c.is_empty()))?;
        let cols: Vec<(&'static str, usize)> = header.iter().enumerate().filter_map(|(i, c)| canonical(&c.text()).map(|k| (k, i))).collect();
        let rows = grid.iter().enumerate().skip(hi + 1).filter(|(_, r)| r.iter().any(|c| !c.is_empty())).map(|(i, r)| (i + 1, r.clone())).collect();
        Some(Sheet { name: name.into(), cols, rows })
    }
    fn has(&self, k: &str) -> bool {
        self.cols.iter().any(|(c, _)| *c == k)
    }
    /// 재고 시트(이름 `Stock`) — 레지스트리 표가 아니라 `parse_stock` 이 읽는다. 머리글 `CellId` 는 `Id` 로 읽힌다.
    fn is_stock(&self) -> bool {
        self.name.eq_ignore_ascii_case(STOCK_SHEET)
    }
    /// 절대 프로파일 시트(`ItemBeadProfile`, 또는 품목 치수 없이 Code + Stack + Level 머리글).
    fn is_profile(&self) -> bool {
        self.name.eq_ignore_ascii_case("ItemBeadProfile") || (self.has("Code") && self.has("Stack") && self.has("Level") && !self.has("InnerDiameter") && !self.has("Name"))
    }
    /// Which table this sheet looks like, from its headers (`None` for the profile sheet — handled apart).
    fn kind(&self) -> Option<Want> {
        if self.is_stock() || self.is_profile() {
            None
        } else if self.has("ConvNo") || self.has("IOBlockNo") || self.has("GroupIndex") {
            Some(Want::Stations)
        } else if self.has("Code") || self.has("InnerDiameter") {
            Some(Want::Items)
        } else if self.has("Id") {
            Some(Want::Cells)
        } else {
            None
        }
    }
}

struct Row<'a> {
    sheet: &'a Sheet,
    cells: &'a [Cell],
}

impl Row<'_> {
    fn get(&self, k: &str) -> Option<&Cell> {
        self.sheet.cols.iter().find(|(c, _)| *c == k).and_then(|(_, i)| self.cells.get(*i)).filter(|c| !c.is_empty())
    }
    fn num(&self, k: &str) -> Result<Option<f64>, String> {
        match self.get(k) {
            None => Ok(None),
            Some(c) => c.num().map(Some).ok_or_else(|| format!("{k}: '{}' is not a number", c.text())),
        }
    }
    fn f32(&self, k: &str) -> Result<f32, String> {
        Ok(self.num(k)?.unwrap_or(0.0) as f32)
    }
    fn int<T: TryFrom<i64>>(&self, k: &str) -> Result<T, String> {
        let v = self.num(k)?.unwrap_or(0.0);
        if v.fract() != 0.0 {
            return Err(format!("{k}: {v} is not an integer"));
        }
        T::try_from(v as i64).map_err(|_| format!("{k}: {v} out of range"))
    }
    fn boolean(&self, k: &str, default: bool) -> Result<bool, String> {
        match self.get(k) {
            None => Ok(default),
            Some(c) => c.boolean().ok_or_else(|| format!("{k}: '{}' is not a boolean", c.text())),
        }
    }
    fn text(&self, k: &str) -> String {
        self.get(k).map(Cell::text).unwrap_or_default()
    }
}

fn parse_cell(r: &Row) -> Result<CellInfo, String> {
    let c = CellInfo {
        id: r.int("Id")?,
        use_: r.boolean("Use", true)?,
        blend_use: r.boolean("BlendUse", false)?,
        section: r.int("Section")?,
        row: r.int("Row")?,
        col: r.int("Col")?,
        length: r.f32("Length")?,
        width: r.f32("Width")?,
        position: [r.f32("X")?, r.f32("Y")?, r.f32("Z")?],
    };
    validate_cell(&c)?;
    Ok(c)
}

fn parse_station(r: &Row) -> Result<StationPara, String> {
    let info = CellInfo {
        id: r.int("Id")?,
        use_: r.boolean("Use", true)?,
        blend_use: r.boolean("BlendUse", false)?,
        section: r.int("Section")?,
        row: r.int("Row")?,
        col: r.int("Col")?,
        length: r.f32("Length")?,
        width: r.f32("Width")?,
        position: [r.f32("X")?, r.f32("Y")?, r.f32("Z")?],
    };
    let p = StationPara {
        conv_no: r.int("ConvNo")?,
        task_type: r.int("TaskType")?,
        rotate_type: r.int("RotateType")?,
        group: r.int("Group")?,
        group_index: r.int("GroupIndex")?,
        connection_prev: r.int("ConnPrev")?,
        connection_next: r.int("ConnNext")?,
        info,
        sensor_settings: SensorSettings {
            io_link_master_module: r.int("IOLinkModule")?,
            io_link_master_port_l: r.int("IOLinkPortL")?,
            io_link_master_port_r: r.int("IOLinkPortR")?,
            detection_factor: r.f32("DetectionFactor")?,
            allow_range: r.f32("AllowRange")?,
            l_sensor_offset: r.f32("LSensorOffset")?,
            r_sensor_offset: r.f32("RSensorOffset")?,
        },
        io_block_no: r.int("IOBlockNo")?,
    };
    validate_station(&p)?;
    Ok(p)
}

fn parse_item(r: &Row) -> Result<ItemRow, String> {
    let code: u32 = r.int("Code")?;
    let it = ItemRow {
        code,
        name: r.text("Name"),
        note: r.text("Note"),
        item: StockItem {
            code,
            count: if r.get("Count").is_some() { r.int("Count")? } else { 1 },
            inner_diameter: r.f32("InnerDiameter")?,
            outer_diameter: r.f32("OuterDiameter")?,
            lower_bid_height: r.f32("LowerBeadHeight")?,
            upper_bid_height: r.f32("UpperBeadHeight")?,
            height: r.f32("Height")?,
            deflection_factor: r.f32("DeflectionFactor")?,
        },
        spec: SpecPatch {
            stack_max: if r.sheet.has("StackMax") { Some(r.int("StackMax")?) } else { None },
            pallet_max: if r.sheet.has("PalletMax") { Some(r.int("PalletMax")?) } else { None },
            weight_kg: if r.sheet.has("WeightKg") { Some(r.num("WeightKg")?.map(|v| v as f32)) } else { None },
            profiles: None,
            pick_bead_offset: if r.sheet.has("PickBeadOffset") { Some(r.num("PickBeadOffset")?.map(|v| v as f32)) } else { None },
            compression: if r.sheet.has("Compression") { Some(r.num("Compression")?.map(|v| v as f32)) } else { None },
        },
    };
    validate_item(&it)?;
    // the row alone (curve points are checked against stack_max when applied)
    validate_spec_for(&it.spec.apply(&ItemSpec::default()), &it.item)?;
    Ok(it)
}

fn parse_profile(r: &Row) -> Result<ProfileSheetRow, String> {
    let code: u32 = r.int("Code")?;
    if code == 0 {
        return Err("code must be > 0".into());
    }
    let opt = |k: &str| -> Result<Option<f32>, String> { Ok(r.num(k)?.map(|v| v as f32)) };
    let source = if r.sheet.has("Source") { r.text("Source") } else { String::new() };
    let source = match source.trim().to_ascii_lowercase().as_str() {
        spec::SOURCE_MEASURED => spec::SOURCE_MEASURED,
        spec::SOURCE_MANUAL => spec::SOURCE_MANUAL,
        "" => spec::SOURCE_MANUAL, // 사람이 쓴 시트에서 온 값은 손으로 넣은 것으로 본다
        other => return Err(format!("Source {other} must be measured | manual")),
    };
    let out = ProfileSheetRow {
        code,
        count: r.int("Stack")?,
        row: ProfileRow {
            level: r.int("Level")?,
            lower_bead: opt("LowerBead")?,
            upper_bead: opt("UpperBead")?,
            stack_height: if r.sheet.has("StackHeight") { opt("StackHeight")? } else { None },
            source: source.into(),
        },
        sample_plc: if r.sheet.has("SamplePlc") { r.text("SamplePlc") } else { String::new() },
        sample_seq: if r.sheet.has("SampleSeq") { r.num("SampleSeq")?.map(|v| v.round() as u32).unwrap_or(0) } else { 0 },
        total_height: if r.sheet.has("TotalHeight") { opt("TotalHeight")? } else { None },
        each_height: if r.sheet.has("EachHeight") { opt("EachHeight")? } else { None },
        at: if r.sheet.has("At") { r.text("At") } else { String::new() },
    };
    validate_spec(&ItemSpec { profiles: vec![BeadProfile { count: out.count, rows: vec![out.row.clone()], ..Default::default() }], ..Default::default() })?;
    Ok(out)
}

fn consume_profile(sheet: &Sheet, out: &mut Tables) {
    if let Some(k) = ["Code", "Stack", "Level"].into_iter().find(|k| !sheet.has(k)) {
        out.errors.push(RowError { sheet: sheet.name.clone(), row: 1, message: format!("header row has no '{k}' column") });
        return;
    }
    // 머리글만 있는 시트는 **없는 시트**로 본다. 양식(`template_workbook`)을 받아 Items 만 채워
    // 올리면 빈 비드 시트가 따라오는데, 그것을 "이 코드들은 프로파일 없음"으로 읽으면 기존 품목을
    // 고치려던 사람이 잰 비드를 조용히 잃는다. 프로파일을 비우는 일은 화면(Beads 탭)이 한다.
    if sheet.rows.is_empty() {
        return;
    }
    out.has_item_profiles = true;
    out.profile_sheet = sheet.name.clone();
    let mut seen = HashSet::new();
    for (rn, cells) in &sheet.rows {
        match parse_profile(&Row { sheet, cells }) {
            Ok(row) => {
                if seen.insert((row.code, row.count, row.row.level)) {
                    out.profile_row_of.entry(row.code).or_insert(*rn);
                    out.item_profiles.push(row);
                } else {
                    out.errors.push(RowError { sheet: sheet.name.clone(), row: *rn, message: format!("code {} stack {} level {} is duplicated", row.code, row.count, row.row.level) });
                }
            }
            Err(m) => out.errors.push(RowError { sheet: sheet.name.clone(), row: *rn, message: m }),
        }
    }
}

/// 시트 줄들을 품목별 프로파일로 모은다.
fn profiles_by_code(rows: &[ProfileSheetRow]) -> BTreeMap<u32, Vec<BeadProfile>> {
    let mut by: BTreeMap<(u32, u8), BeadProfile> = BTreeMap::new();
    for r in rows {
        let p = by.entry((r.code, r.count)).or_insert_with(|| BeadProfile { count: r.count, ..Default::default() });
        p.rows.push(r.row.clone());
        if p.sample_seq == 0 {
            p.sample_plc = r.sample_plc.clone();
            p.sample_seq = r.sample_seq;
        }
        p.total_height = p.total_height.or(r.total_height);
        p.each_height = p.each_height.or(r.each_height);
        if p.at.is_empty() {
            p.at = r.at.clone();
        }
    }
    let mut out: BTreeMap<u32, Vec<BeadProfile>> = BTreeMap::new();
    for ((code, _), mut p) in by {
        p.rows.sort_by_key(|r| r.level);
        out.entry(code).or_default().push(p);
    }
    out
}

/// `ItemBeadProfile` → `items[*].spec.profiles` (authoritative for listed codes); the rest become `orphan_profiles`.
fn merge_profiles(out: &mut Tables) {
    if !out.has_item_profiles {
        return;
    }
    let mut by = profiles_by_code(&out.item_profiles);
    for it in &mut out.items {
        it.spec.profiles = Some(by.get(&it.code).cloned().unwrap_or_default());
    }
    for it in &out.items {
        by.remove(&it.code);
    }
    out.orphan_profiles = by.into_iter().collect();
}

fn consume_sheet(sheet: &Sheet, as_kind: Want, out: &mut Tables) {
    let required = match as_kind {
        Want::Items => "Code",
        _ => "Id",
    };
    if !sheet.has(required) {
        out.errors.push(RowError {
            sheet: sheet.name.clone(),
            row: 1,
            message: format!("header row has no '{required}' column (found: {})", sheet.cols.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(", ")),
        });
        return;
    }
    match as_kind {
        Want::Cells => out.has_cells = true,
        Want::Stations => out.has_stations = true,
        Want::Items => {
            out.has_items = true;
            out.items_sheet = sheet.name.clone();
        }
        Want::All => {}
    }
    for (rn, cells) in &sheet.rows {
        let r = Row { sheet, cells };
        let res = match as_kind {
            Want::Cells => parse_cell(&r).map(|c| out.cells.push(c)),
            Want::Stations => parse_station(&r).map(|s| out.stations.push(s)),
            Want::Items => parse_item(&r).map(|i| {
                out.items.push(i);
                out.item_rows.push(*rn);
            }),
            Want::All => Ok(()),
        };
        if let Err(m) = res {
            out.errors.push(RowError { sheet: sheet.name.clone(), row: *rn, message: m });
        }
    }
}

fn grid_from_csv(bytes: &[u8]) -> Result<Grid, ApiError> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let mut rdr = csv::ReaderBuilder::new().has_headers(false).flexible(true).from_reader(text.as_bytes());
    let mut grid = Vec::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|e| ApiError::BadRequest(format!("csv: {e}")))?;
        grid.push(rec.iter().map(|s| if s.trim().is_empty() { Cell::Empty } else { Cell::Text(s.to_string()) }).collect());
    }
    Ok(grid)
}

fn sheets_from_xlsx(bytes: &[u8]) -> Result<Vec<(String, Grid)>, ApiError> {
    let mut wb = calamine::open_workbook_auto_from_rs(Cursor::new(bytes.to_vec())).map_err(|e| ApiError::BadRequest(format!("workbook: {e}")))?;
    let names = wb.sheet_names();
    let mut out = Vec::new();
    for name in names {
        let range = wb.worksheet_range(&name).map_err(|e| ApiError::BadRequest(format!("sheet {name}: {e}")))?;
        let grid: Grid = range.rows().map(|r| r.iter().map(Cell::from).collect()).collect();
        out.push((name, grid));
    }
    Ok(out)
}

fn is_csv(name: &str) -> bool {
    name.rsplit('.').next().map(|e| e.eq_ignore_ascii_case("csv") || e.eq_ignore_ascii_case("txt")).unwrap_or(false)
}

/// Parses an uploaded file. `want` picks the sheet by name (`Cells`/`Stations`/`Items`) and falls back
/// to the first sheet; `All` takes every sheet it can classify.
pub fn parse_file(name: &str, bytes: &[u8], want: Want) -> Result<Tables, ApiError> {
    let raw: Vec<(String, Grid)> = if is_csv(name) { vec![("csv".into(), grid_from_csv(bytes)?)] } else { sheets_from_xlsx(bytes)? };
    let sheets: Vec<Sheet> = raw.iter().filter_map(|(n, g)| Sheet::from_grid(n, g)).collect();
    let mut out = Tables::default();
    if sheets.is_empty() {
        return Err(ApiError::BadRequest("file has no data rows".into()));
    }
    let by_name = |n: &str| sheets.iter().find(|s| s.name.eq_ignore_ascii_case(n));
    match want {
        Want::All => {
            let mut used = false;
            for (n, k) in [("Cells", Want::Cells), ("Stations", Want::Stations), ("Items", Want::Items)] {
                if let Some(s) = by_name(n) {
                    consume_sheet(s, k, &mut out);
                    used = true;
                }
            }
            if !used {
                for s in &sheets {
                    if let Some(k) = s.kind() {
                        consume_sheet(s, k, &mut out);
                    }
                }
            }
            if let Some(s) = sheets.iter().find(|s| s.is_profile()) {
                consume_profile(s, &mut out);
            }
            if !(out.has_cells || out.has_stations || out.has_items || out.has_item_profiles) && out.errors.is_empty() {
                return Err(ApiError::BadRequest("no recognisable Cells / Stations / Items sheet".into()));
            }
        }
        Want::Items => {
            if let Some(s) = sheets.iter().find(|s| s.is_profile()) {
                consume_profile(s, &mut out);
            }
            // a profile-only file carries no Items sheet — never parse that sheet as items
            if let Some(s) = by_name("Items").or_else(|| sheets.iter().find(|s| !s.is_profile())) {
                consume_sheet(s, Want::Items, &mut out);
            }
        }
        k => {
            let n = match k {
                Want::Cells => "Cells",
                Want::Stations => "Stations",
                _ => "Items",
            };
            let s = by_name(n).unwrap_or(&sheets[0]);
            consume_sheet(s, k, &mut out);
        }
    }
    merge_profiles(&mut out);
    Ok(out)
}

// ---- apply to sqlite

/// 줄마다 하나씩 붙는 결과 — 셀 일괄 API(`bulk::Outcome`)와 같은 말을 쓴다.
///
/// 예전에는 `skipped` 하나가 "저장된 값과 같아서 안 썼다"를 뜻했는데, 그건 **건너뜀이 아니라
/// 동일**이다. 둘을 갈라 두면 미리보기가 "이 파일에 바뀔 것이 정말 없다"와 "고쳐야 할 줄이 있다"를
/// 한눈에 가른다.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ApplyCounts {
    /// 새로 생긴 행.
    pub imported: usize,
    pub updated: usize,
    /// 파일 값이 저장된 것과 같아 쓰지 않은 행.
    pub unchanged: usize,
    /// 적용하지 못한 행 — `errors` 와 1:1 이다.
    pub skipped: usize,
    /// Rows rejected only when merged with the stored state (e.g. a level deeper than the stored stack_max).
    pub errors: Vec<RowError>,
}

impl ApplyCounts {
    pub fn add(&mut self, o: &ApplyCounts) {
        self.imported += o.imported;
        self.updated += o.updated;
        self.unchanged += o.unchanged;
        self.skipped += o.skipped;
        self.errors.extend(o.errors.iter().cloned());
    }
}

pub fn apply_cells(reg: &Registry, cells: &[CellInfo], dry_run: bool) -> Result<ApplyCounts, ApiError> {
    let existing = reg.cells()?;
    let mut c = ApplyCounts::default();
    for cell in cells {
        match existing.iter().find(|e| e.id == cell.id) {
            Some(e) if e.cell == *cell => {
                c.unchanged += 1;
                continue;
            }
            Some(_) => c.updated += 1,
            None => c.imported += 1,
        }
        if !dry_run {
            reg.upsert_cell(cell, "local", true, None)?;
        }
    }
    Ok(c)
}

pub fn apply_stations(reg: &Registry, stations: &[StationPara], dry_run: bool) -> Result<ApplyCounts, ApiError> {
    let existing = reg.stations()?;
    let mut c = ApplyCounts::default();
    for p in stations {
        match existing.iter().find(|e| e.id == p.info.id) {
            Some(e) if e.para == *p => {
                c.unchanged += 1;
                continue;
            }
            Some(_) => c.updated += 1,
            None => c.imported += 1,
        }
        if !dry_run {
            reg.upsert_station(p, "local", true, None)?;
        }
    }
    Ok(c)
}

/// `Items` + `ItemBeadProfile` 를 한 번에 적용한다 — 한 파일로 품목과 그 단별 비드를 같이 넣는 길.
///
/// `Tables` 를 통째로 받는 이유는 **줄 번호** 다: 저장된 값과 합쳐야 드러나는 오류(저장된
/// `StackMax` 보다 깊은 단 따위)도 파일의 몇 번째 줄인지 말할 수 있어야 고칠 수 있다.
pub fn apply_items(reg: &Registry, t: &Tables, dry_run: bool) -> Result<ApplyCounts, ApiError> {
    let existing = reg.items()?;
    let (items_sheet, profile_sheet) = (t.items_sheet_name(), t.profile_sheet_name());
    let mut c = ApplyCounts::default();
    for (i, it) in t.items.iter().enumerate() {
        let row = t.item_rows.get(i).copied().unwrap_or(0);
        let cur = existing.iter().find(|e| e.code == it.code);
        let spec = it.spec.apply(&cur.map(|e| e.spec.clone()).unwrap_or_default()).normalized();
        if let Err(m) = validate_spec_for(&spec, &it.item) {
            c.skipped += 1;
            c.errors.push(RowError { sheet: items_sheet.clone(), row, message: format!("Code {}: {m}", it.code) });
            continue;
        }
        match cur {
            Some(e) if e.item == it.item && e.name == it.name && e.note == it.note && e.spec == spec => {
                c.unchanged += 1;
                continue;
            }
            Some(_) => c.updated += 1,
            None => c.imported += 1,
        }
        if !dry_run {
            reg.upsert_item_full(it.code, &it.name, &it.item, &it.note, Some(&spec))?;
        }
    }
    for (code, profiles) in &t.orphan_profiles {
        let row = t.profile_row_of.get(code).copied().unwrap_or(0);
        let Some(e) = existing.iter().find(|e| e.code == *code) else {
            c.skipped += 1;
            c.errors.push(RowError { sheet: profile_sheet.clone(), row, message: format!("Code {code}: 등록된 품목이 없습니다(Items 시트에도 없음)") });
            continue;
        };
        let spec = ItemSpec { profiles: profiles.clone(), ..e.spec.clone() }.normalized();
        if let Err(m) = validate_spec_for(&spec, &e.item) {
            c.skipped += 1;
            c.errors.push(RowError { sheet: profile_sheet.clone(), row, message: format!("Code {code}: {m}") });
            continue;
        }
        if e.spec == spec {
            c.unchanged += 1;
            continue;
        }
        c.updated += 1;
        if !dry_run {
            reg.set_item_spec(*code, &spec)?;
        }
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(id: u16, i: usize) -> CellInfo {
        CellInfo {
            use_: i.is_multiple_of(2),
            blend_use: i.is_multiple_of(3),
            id,
            section: 2,
            row: (i / 4 + 1) as u16,
            col: (i % 4 + 1) as u16,
            length: 1200.5,
            width: 1100.0,
            position: [12000.0 + i as f32, 3000.25, 1500.0],
        }
    }
    fn station(id: u16, i: usize) -> StationPara {
        StationPara {
            conv_no: (i + 1) as u16,
            task_type: 1,
            rotate_type: 2,
            group: 1,
            group_index: (i + 1) as u8,
            connection_prev: 3,
            connection_next: 4,
            info: CellInfo { use_: true, blend_use: false, id, section: 3, row: 1, col: (i + 1) as u16, length: 1500.0, width: 1500.0, position: [20000.0 + i as f32, 1000.0, 1450.5] },
            sensor_settings: SensorSettings {
                io_link_master_module: 1,
                io_link_master_port_l: 2,
                io_link_master_port_r: 3,
                detection_factor: 0.5,
                allow_range: 12.5,
                l_sensor_offset: -1.25,
                r_sensor_offset: 2.5,
            },
            io_block_no: 7,
        }
    }
    fn item(code: u32) -> ItemRow {
        ItemRow {
            code,
            name: format!("225/45R{code}"),
            note: "비고 테스트".into(),
            item: StockItem { code, count: 4, inner_diameter: 381.0, outer_diameter: 780.5, lower_bid_height: 20.0, upper_bid_height: 220.0, height: 240.0, deflection_factor: 0.1 },
            spec: SpecPatch::full(&ItemSpec::default()),
        }
    }
    fn pr(level: u8, lo: f32, up: f32, sh: Option<f32>, source: &str) -> ProfileRow {
        ProfileRow { level, lower_bead: Some(lo), upper_bead: Some(up), stack_height: sh, source: source.into() }
    }
    /// 3 단 스택 하나를 잰 절대 프로파일(셀 바닥 기준).
    fn profile3() -> BeadProfile {
        BeadProfile {
            count: 3,
            rows: vec![pr(1, 20.0, 220.0, None, spec::SOURCE_MEASURED), pr(2, 255.0, 455.0, None, spec::SOURCE_MEASURED), pr(3, 492.0, 692.0, Some(711.0), spec::SOURCE_MANUAL)],
            total_height: Some(711.0),
            each_height: Some(237.0),
            sample_plc: "GR2".into(),
            sample_seq: 11,
            at: "2026-09-18 10:11:12".into(),
        }
    }

    #[test]
    fn xlsx_item_spec_and_profile_round_trip() {
        let spec = ItemSpec { stack_max: 4, pallet_max: 12, weight_kg: Some(9.5), profiles: vec![profile3()], ..Default::default() };
        let items = vec![ItemRow { spec: SpecPatch::full(&spec), ..item(1001) }, item(1002)];
        let bytes = export_workbook(None, None, Some(&items)).unwrap();
        for want in [Want::Items, Want::All] {
            let t = parse_file("items.xlsx", &bytes, want).unwrap();
            assert!(t.errors.is_empty(), "{:?}", t.errors);
            assert!(t.has_items && t.has_item_profiles);
            assert_eq!(t.items, items);
            assert!(t.orphan_profiles.is_empty());
        }
        // apply → stored spec equals; a second apply is a no-op
        let reg = Registry::new(crate::db::Db::open_memory().unwrap());
        let t = parse_file("items.xlsx", &bytes, Want::Items).unwrap();
        let c = apply_items(&reg, &t, false).unwrap();
        assert_eq!((c.imported, c.errors.len()), (2, 0));
        assert_eq!(reg.item(1001).unwrap().unwrap().spec, spec);
        let c = apply_items(&reg, &t, false).unwrap();
        assert_eq!((c.unchanged, c.skipped), (2, 0));
    }

    #[test]
    fn old_format_import_keeps_stored_spec() {
        let reg = Registry::new(crate::db::Db::open_memory().unwrap());
        let spec = ItemSpec { stack_max: 4, profiles: vec![profile3()], ..Default::default() };
        let base = item(1001);
        reg.upsert_item_full(1001, "A", &base.item, "", Some(&spec)).unwrap();
        let spec = reg.item(1001).unwrap().unwrap().spec;
        assert_eq!(spec.measured_counts(), vec![3]);
        // old header set: no spec columns, no ItemBeadProfile sheet
        let csv = "Code,Name,Count,InnerDiameter,OuterDiameter,Height\n1001,A2,2,381,780,240\n1005,B,1,381,780,240\n";
        let t = parse_file("items.csv", csv.as_bytes(), Want::Items).unwrap();
        assert!(t.errors.is_empty() && !t.has_item_profiles, "{:?}", t.errors);
        assert_eq!(t.items[0].spec, SpecPatch::default());
        let dry = apply_items(&reg, &t, true).unwrap();
        assert_eq!((dry.imported, dry.updated), (1, 1));
        assert!(reg.item(1005).unwrap().is_none(), "dry run must not write");
        apply_items(&reg, &t, false).unwrap();
        assert_eq!(reg.item(1001).unwrap().unwrap().spec, spec, "missing columns keep the stored spec");
        assert_eq!(reg.item(1001).unwrap().unwrap().name, "A2");
        assert_eq!(reg.item(1005).unwrap().unwrap().spec, ItemSpec::default());
        // plain CRUD upsert (no spec) keeps it too
        reg.upsert_item(1001, "A3", &base.item, "").unwrap();
        assert_eq!(reg.item(1001).unwrap().unwrap().spec, spec);
        // a StackMax column alone changes only stack_max
        let csv = "Code,StackMax\n1001,6\n";
        let t = parse_file("items.csv", csv.as_bytes(), Want::Items).unwrap();
        apply_items(&reg, &t, false).unwrap();
        let got = reg.item(1001).unwrap().unwrap().spec;
        assert_eq!((got.stack_max, got.profiles.len()), (6, 1));
    }

    #[test]
    fn a_profile_only_file_updates_existing_items() {
        let reg = Registry::new(crate::db::Db::open_memory().unwrap());
        reg.upsert_item_full(1001, "A", &item(1001).item, "", Some(&ItemSpec { stack_max: 3, ..Default::default() })).unwrap();
        let csv = "코드,단수,단,하단 비드,상단 비드,적재 높이\n1001,3,2,255,455,\n1001,3,1,20,220,\n1001,3,2,1,2,3\n9999,3,1,20,200,240\n";
        let t = parse_file("profile.csv", csv.as_bytes(), Want::Items).unwrap();
        assert!(!t.has_items && t.has_item_profiles);
        assert_eq!(t.errors.iter().map(|e| e.row).collect::<Vec<_>>(), vec![4], "{:?}", t.errors);
        assert!(t.errors[0].message.contains("duplicated"));
        let c = apply_items(&reg, &t, false).unwrap();
        assert_eq!((c.updated, c.skipped), (1, 1));
        assert_eq!(c.errors.len(), 1);
        assert!(c.errors[0].message.contains("9999"));
        // 적용 단계 오류도 줄을 짚는다 — 9999 가 처음 나온 시트 줄(머리글 + 네 번째 자료 줄).
        assert_eq!(c.errors[0].row, 5, "{:?}", c.errors);
        // 절대값 그대로 저장되고, 사람이 쓴 시트라 손입력(manual)으로 들어간다
        let got = reg.item(1001).unwrap().unwrap().spec;
        assert_eq!(got.measured_counts(), vec![3]);
        let p = got.profile(3).unwrap();
        assert_eq!((p.abs_upper_bead(1), p.abs_upper_bead(2)), (Some(220.0), Some(455.0)));
        assert!(p.at_level(2).unwrap().is_manual());
        // 스택 크기보다 깊은 단은 가져올 때 거부된다
        let t = parse_file("profile.csv", "Code,Stack,Level,UpperBead\n1001,3,5,900\n".as_bytes(), Want::Items).unwrap();
        assert!(t.errors[0].message.contains("level must be 1..=3"), "{:?}", t.errors);
    }

    /// 양식(`GET /api/items/template.xlsx`)은 **머리글만** 들고, 받은 그대로 다시 올리면
    /// 한 줄도 바뀌지 않는다 — 처음 쓰는 사람이 왕복을 먼저 확인할 수 있어야 한다.
    #[test]
    fn items_template_is_headers_only_and_imports_as_a_no_op() {
        let bytes = template_workbook().unwrap();
        let sheets = sheets_from_xlsx(&bytes).unwrap();
        assert_eq!(sheets.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), vec!["Items", "ItemBeadProfile"]);
        for (name, cols) in [("Items", &ITEM_COLS[..]), ("ItemBeadProfile", &PROFILE_COLS[..])] {
            let grid = &sheets.iter().find(|(n, _)| n == name).unwrap().1;
            assert_eq!(grid.len(), 1, "{name} 에 자료 줄이 있으면 안 된다");
            assert_eq!(grid[0].iter().map(Cell::text).collect::<Vec<_>>(), cols);
            // 머리글이 하나라도 안 읽히면 그 열은 조용히 버려진다 — 양식이 자기 파서를 통과해야 한다
            assert!(grid[0].iter().all(|c| canonical(&c.text()).is_some()), "{name} 머리글");
        }
        let reg = Registry::new(crate::db::Db::open_memory().unwrap());
        reg.upsert_item_full(1001, "A", &item(1001).item, "", None).unwrap();
        let t = parse_file("items_template.xlsx", &bytes, Want::Items).unwrap();
        assert!(t.errors.is_empty(), "{:?}", t.errors);
        // 비드 시트는 머리글뿐이라 없는 시트로 읽힌다(`consume_profile`)
        assert!(t.has_items && !t.has_item_profiles && t.items.is_empty());
        let c = apply_items(&reg, &t, false).unwrap();
        assert_eq!((c.imported, c.updated, c.unchanged, c.skipped), (0, 0, 0, 0));
        assert_eq!(reg.items().unwrap().len(), 1, "빈 양식이 등록된 품목을 지우면 안 된다");
    }

    /// 오류 문장은 **시트와 줄**을 앞세운다 — 토스트 한 줄만 봐도 고칠 자리를 찾는다.
    #[test]
    fn row_errors_name_the_sheet_and_the_row() {
        let e = |sheet: &str, row: usize| RowError { sheet: sheet.into(), row, message: "count must be >= 1".into() };
        assert_eq!(error_text(&e("Items", 3)), "Items row 3: count must be >= 1");
        assert_eq!(error_text(&e("ItemBeadProfile", 12)), "ItemBeadProfile row 12: count must be >= 1");
        assert_eq!(error_text(&e("Items", 0)), "Items: count must be >= 1");
        // 시트가 하나뿐인 csv 는 시트 이름을 말할 것이 없다
        assert_eq!(error_text(&e("csv", 3)), "row 3: count must be >= 1");
    }

    /// 한 줄이 틀려도 **나머지는 들어온다**(전부 아니면 전무가 아니다) — dry-run 과 실제 적용이 같은 수를 센다.
    #[test]
    fn a_bad_row_is_skipped_and_the_good_rows_still_apply() {
        let bad = ItemRow { item: StockItem { count: 0, ..item(1002).item }, ..item(1002) };
        let items = vec![item(1001), bad, item(1003)];
        let bytes = export_workbook(None, None, Some(&items)).unwrap();
        let t = parse_file("items.xlsx", &bytes, Want::Items).unwrap();
        assert_eq!(t.items.iter().map(|i| i.code).collect::<Vec<_>>(), vec![1001, 1003]);
        assert_eq!(t.errors.len(), 1, "{:?}", t.errors);
        assert_eq!((t.errors[0].sheet.as_str(), t.errors[0].row), ("Items", 3));
        assert_eq!(error_text(&t.errors[0]), "Items row 3: count must be >= 1");
        let reg = Registry::new(crate::db::Db::open_memory().unwrap());
        let dry = apply_items(&reg, &t, true).unwrap();
        assert_eq!((dry.imported, dry.updated, dry.unchanged, dry.skipped), (2, 0, 0, 0));
        assert!(reg.items().unwrap().is_empty(), "dry run must not write");
        let wet = apply_items(&reg, &t, false).unwrap();
        assert_eq!((wet.imported, wet.skipped), (2, 0));
        assert_eq!(reg.items().unwrap().len(), 2);
        assert!(reg.item(1002).unwrap().is_none(), "거부된 줄은 들어오지 않는다");
        let again = apply_items(&reg, &t, false).unwrap();
        assert_eq!((again.imported, again.updated, again.unchanged), (0, 0, 2));
    }

    /// `Items` 시트만 든 파일 — 비드 시트가 없으면 프로파일은 건드리지 않는다.
    #[test]
    fn an_items_only_file_adds_new_codes() {
        let csv = "Code,Name,Count,InnerDiameter,OuterDiameter,LowerBeadHeight,UpperBeadHeight,Height\n\
                   2001,205/55R16,4,381,650,20,200,220\n\
                   2002,225/45R17,4,432,700,22,210,230\n";
        let t = parse_file("items.csv", csv.as_bytes(), Want::Items).unwrap();
        assert!(t.has_items && !t.has_item_profiles && t.errors.is_empty(), "{:?}", t.errors);
        let reg = Registry::new(crate::db::Db::open_memory().unwrap());
        let c = apply_items(&reg, &t, false).unwrap();
        assert_eq!((c.imported, c.updated, c.unchanged, c.skipped), (2, 0, 0, 0));
        assert_eq!(reg.item(2001).unwrap().unwrap().name, "205/55R16");
        assert!(reg.item(2001).unwrap().unwrap().spec.profiles.is_empty());
    }

    /// 양식이 두 시트인 이유 — 품목 한 줄과 그 단별 비드를 **한 파일로** 같이 넣는다.
    #[test]
    fn one_file_adds_an_item_and_its_bead_rows() {
        let spec = ItemSpec { stack_max: 5, profiles: vec![profile3()], ..Default::default() };
        let rows = vec![ItemRow { spec: SpecPatch::full(&spec), ..item(3001) }];
        let bytes = export_workbook(None, None, Some(&rows)).unwrap();
        let reg = Registry::new(crate::db::Db::open_memory().unwrap());
        let t = parse_file("items.xlsx", &bytes, Want::Items).unwrap();
        assert!(t.has_items && t.has_item_profiles);
        let c = apply_items(&reg, &t, false).unwrap();
        assert_eq!((c.imported, c.skipped), (1, 0));
        let got = reg.item(3001).unwrap().unwrap().spec;
        assert_eq!((got.stack_max, got.measured_counts()), (5, vec![3]));
    }

    #[test]
    fn xlsx_round_trip_all_sheets() {
        let cells: Vec<CellInfo> = (0..6).map(|i| cell(101 + i as u16, i)).collect();
        let stations: Vec<StationPara> = (0..2).map(|i| station(2101 + i as u16, i)).collect();
        let items = vec![item(1001), item(1002)];
        let bytes = export_workbook(Some(&cells), Some(&stations), Some(&items)).unwrap();
        let t = parse_file("registry.xlsx", &bytes, Want::All).unwrap();
        assert!(t.errors.is_empty(), "{:?}", t.errors);
        assert!(t.has_cells && t.has_stations && t.has_items);
        assert_eq!(t.cells, cells);
        assert_eq!(t.stations, stations);
        assert_eq!(t.items, without_bead_sheet(&items));
    }

    #[test]
    fn xlsx_single_sheet_by_name_or_first() {
        let cells = vec![cell(5, 0)];
        let bytes = export_workbook(Some(&cells), None, None).unwrap();
        let t = parse_file("cells.xlsx", &bytes, Want::Cells).unwrap();
        assert_eq!(t.cells, cells);
        // asking for stations on a cells-only workbook falls back to the first sheet and rejects the rows
        let t = parse_file("cells.xlsx", &bytes, Want::Stations).unwrap();
        assert!(t.stations.is_empty());
        assert_eq!(t.errors.len(), 1);
        assert!(t.errors[0].message.contains("station id 5"));
    }

    #[test]
    fn csv_with_korean_headers_and_validation_errors() {
        let csv = "\u{feff}셀 Id,사용,블렌드,구역,행,열,길이,폭,X,Y,Z\n\
                   7,예,아니오,1,2,3,10,20,100,200,300\n\
                   ,,,,,,,,,,\n\
                   0,TRUE,FALSE,1,1,1,1,1,1,1,1\n\
                   8,TRUE,FALSE,5,1,1,1,1,1,1,1\n\
                   9,TRUE,FALSE,2,1,1,1,1,1,-1,1\n\
                   abc,TRUE,FALSE,2,1,1,1,1,1,1,1\n";
        let t = parse_file("cells.csv", csv.as_bytes(), Want::Cells).unwrap();
        assert_eq!(t.cells.len(), 1);
        assert_eq!(t.cells[0].id, 7);
        assert!(t.cells[0].use_ && !t.cells[0].blend_use);
        assert_eq!(t.cells[0].position, [100.0, 200.0, 300.0]);
        let rows: Vec<usize> = t.errors.iter().map(|e| e.row).collect();
        assert_eq!(rows, vec![4, 5, 6, 7]);
        assert!(t.errors[0].message.contains("1..1000"));
        assert!(t.errors[1].message.contains("section"));
        assert!(t.errors[2].message.contains("position Y"));
        assert!(t.errors[3].message.contains("not a number"));
    }

    #[test]
    fn csv_kind_detection_for_registry_import() {
        let csv = "Id,ConvNo,TaskType,RotateType,Group,GroupIndex,ConnPrev,ConnNext,Use,BlendUse,Section,Row,Col,Length,Width,X,Y,Z,IOLinkModule,IOLinkPortL,IOLinkPortR,DetectionFactor,AllowRange,LSensorOffset,RSensorOffset,IOBlockNo\n\
                   2105,1,1,1,1,5,0,0,1,0,3,1,5,1500,1500,20000,1000,1450,0,0,0,0,0,0,0,0\n";
        let t = parse_file("stations.csv", csv.as_bytes(), Want::All).unwrap();
        assert!(t.has_stations && !t.has_cells);
        assert_eq!(t.stations.len(), 1);
        assert_eq!(t.stations[0].info.id, 2105);
        assert_eq!(t.stations[0].group_index, 5);
    }

    #[test]
    fn missing_required_header_is_reported() {
        let csv = "Use,Section\nTRUE,1\n";
        let t = parse_file("x.csv", csv.as_bytes(), Want::Cells).unwrap();
        assert!(t.cells.is_empty());
        assert_eq!(t.errors[0].row, 1);
        assert!(t.errors[0].message.contains("'Id'"));
    }

    #[test]
    fn validate_item_rules() {
        assert!(validate_item(&item(1)).is_ok());
        let with = |f: &dyn Fn(&mut ItemRow)| {
            let mut i = item(1);
            f(&mut i);
            validate_item(&i)
        };
        assert!(with(&|i| i.code = 0).unwrap_err().contains("code"));
        assert!(with(&|i| i.item.count = 0).unwrap_err().contains("count"));
        assert!(with(&|i| i.item.height = -1.0).unwrap_err().contains("height"));
        assert!(with(&|i| i.item.lower_bid_height = f32::NAN).unwrap_err().contains("lower_bead_height"));
        assert!(with(&|i| i.item.outer_diameter = f32::INFINITY).unwrap_err().contains("outer_diameter"));
        assert!(with(&|i| i.item.deflection_factor = f32::NAN).unwrap_err().contains("deflection_factor"));
        // negative deflection factor is allowed (a factor, not a dimension)
        assert!(with(&|i| i.item.deflection_factor = -0.2).is_ok());
        // inner must be below outer when both are given; 0 means "not entered"
        assert!(with(&|i| i.item.inner_diameter = 780.5).unwrap_err().contains("inner_diameter"));
        assert!(with(&|i| i.item.inner_diameter = 900.0).is_err());
        assert!(with(&|i| i.item.outer_diameter = 0.0).is_ok());
        assert!(with(&|i| i.item.inner_diameter = 0.0).is_ok());
    }

    #[test]
    fn xlsx_items_only_round_trip() {
        let items = vec![item(1001), item(1002), ItemRow { name: String::new(), note: String::new(), ..item(7) }];
        let bytes = export_workbook(None, None, Some(&items)).unwrap();
        let t = parse_file("items.xlsx", &bytes, Want::Items).unwrap();
        assert!(t.errors.is_empty(), "{:?}", t.errors);
        assert!(t.has_items && !t.has_cells && !t.has_stations);
        assert_eq!(t.items, without_bead_sheet(&items));
        // a whole-registry workbook: the Items sheet is picked by name, other sheets are ignored
        let bytes = export_workbook(Some(&[cell(5, 0)]), None, Some(&items)).unwrap();
        let t = parse_file("registry.xlsx", &bytes, Want::Items).unwrap();
        assert_eq!(t.items, without_bead_sheet(&items));
        assert!(t.cells.is_empty());
    }

    /// 비드 줄이 하나도 없는 파일은 비드 시트가 **없는** 것으로 읽힌다(`consume_profile`) —
    /// 그 파일의 품목은 프로파일을 싣지 않고, 적용할 때 저장된 프로파일이 그대로 남는다.
    fn without_bead_sheet(items: &[ItemRow]) -> Vec<ItemRow> {
        items.iter().cloned().map(|i| ItemRow { spec: SpecPatch { profiles: None, ..i.spec.clone() }, ..i }).collect()
    }

    /// 양식을 받아 Items 만 채워 올려도(빈 비드 시트가 따라온다) **이미 잰 비드는 지워지지 않는다**.
    #[test]
    fn an_empty_bead_sheet_keeps_stored_profiles() {
        let reg = Registry::new(crate::db::Db::open_memory().unwrap());
        let spec = ItemSpec { stack_max: 4, profiles: vec![profile3()], ..Default::default() };
        reg.upsert_item_full(1001, "A", &item(1001).item, "", Some(&spec)).unwrap();
        let stored = reg.item(1001).unwrap().unwrap().spec;
        // Items 한 줄(이름만 바꿈) + 머리글만 있는 ItemBeadProfile — 양식을 채운 모양
        let rows = vec![ItemRow { name: "A2".into(), note: String::new(), spec: SpecPatch { profiles: Some(Vec::new()), ..SpecPatch::full(&stored) }, ..item(1001) }];
        let bytes = export_workbook(None, None, Some(&rows)).unwrap();
        let t = parse_file("items.xlsx", &bytes, Want::Items).unwrap();
        assert!(t.has_items && !t.has_item_profiles, "머리글만 있는 비드 시트는 없는 시트");
        let c = apply_items(&reg, &t, false).unwrap();
        assert_eq!((c.updated, c.skipped), (1, 0));
        let got = reg.item(1001).unwrap().unwrap();
        assert_eq!(got.name, "A2");
        assert_eq!(got.spec.profiles, stored.profiles, "잰 비드가 남아야 한다");
    }

    #[test]
    fn items_import_reports_invalid_rows() {
        let csv = "코드,품명,수량,내경,외경,높이\n\
                   11,A,2,381,780,240\n\
                   12,B,0,381,780,240\n\
                   13,C,1,800,780,240\n\
                   14,D,1,381,780,-5\n";
        let t = parse_file("items.csv", csv.as_bytes(), Want::Items).unwrap();
        assert_eq!(t.items.iter().map(|i| i.code).collect::<Vec<_>>(), vec![11]);
        assert_eq!(t.errors.iter().map(|e| e.row).collect::<Vec<_>>(), vec![3, 4, 5]);
        assert!(t.errors[0].message.contains("count"));
        assert!(t.errors[1].message.contains("inner_diameter"));
        assert!(t.errors[2].message.contains("height"));
    }

    #[test]
    fn apply_counts_and_dry_run() {
        let db = crate::db::Db::open_memory().unwrap();
        let reg = Registry::new(db);
        reg.upsert_cell(&cell(1, 0), "plc", false, None).unwrap();
        let incoming = vec![cell(1, 0), cell(2, 1), CellInfo { row: 9, ..cell(1, 0) }];
        // last wins for duplicate ids in the sheet, but counting is per row
        let dry = apply_cells(&reg, &incoming, true).unwrap();
        assert_eq!((dry.imported, dry.updated, dry.unchanged), (1, 1, 1));
        assert_eq!(reg.cells().unwrap().len(), 1, "dry run must not write");
        let wet = apply_cells(&reg, &incoming, false).unwrap();
        assert_eq!((wet.imported, wet.updated, wet.unchanged), (1, 1, 1));
        let cells = reg.cells().unwrap();
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].cell.row, 9);
        assert!(cells[0].dirty && cells[0].source == "local");
    }

    /// 셀 바닥 Z 는 바닥 평탄도 보정이라 음수·0 도 맞다(현장 CELL 301..305). PLC 의 INVALID_CELL_POSZ 는
    /// 스테이션(Cell.Id > 2000)만 보므로 셀에는 경고도 없다. 숫자가 아닌 Z 는 여전히 오류.
    #[test]
    fn cell_floor_z_may_be_negative_or_zero() {
        let at = |z: f32| CellInfo { position: [12000.0, 3000.0, z], ..cell(301, 0) };
        for z in [-8.8f32, -23.5, 0.0, 1500.0] {
            assert!(validate_cell(&at(z)).is_ok(), "z {z} must be accepted");
            assert!(cell_warnings(&at(z)).is_empty(), "cells never warn: z {z}");
        }
        // 경고는 스테이션 id 에만
        assert!(floor_z_warning("스테이션", 2021, 0.0).is_some_and(|w| w.contains("INVALID_CELL_POSZ")));
        assert!(floor_z_warning("셀", 301, -8.8).is_none());
        assert!(floor_z_warning("스테이션", 2021, 10.0).is_none());
        for z in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(validate_cell(&at(z)).unwrap_err().contains("finite"), "z {z} must be rejected");
        }
        // X/Y 규칙과 id·구역 규칙은 그대로 오류다
        assert!(validate_cell(&CellInfo { position: [-1.0, 0.0, 1500.0], ..cell(301, 0) }).is_err());
        assert!(validate_cell(&CellInfo { section: 4, ..cell(301, 0) }).is_err());
        assert!(validate_cell(&cell(0, 0)).is_err());
    }

    /// 음수 바닥 Z 가 든 시트는 오류도 경고도 없이 들어온다.
    #[test]
    fn import_sheet_with_negative_floor_z_applies() {
        let csv = "Id,Section,Row,Col,X,Y,Z,Length,Width\n301,2,1,1,12000,3000,-8.8\n302,2,1,2,13000,3000,1500\n";
        let t = parse_file("cells.csv", csv.as_bytes(), Want::Cells).unwrap();
        assert!(t.errors.is_empty(), "{:?}", t.errors);
        assert_eq!(t.cells.len(), 2);
        assert_eq!(t.cells[0].position[2], -8.8);
        assert!(cells_warnings(&t.cells).is_empty());
        let reg = Registry::new(crate::db::Db::open_memory().unwrap());
        let c = apply_cells(&reg, &t.cells, false).unwrap();
        assert_eq!((c.imported, c.errors.len()), (2, 0));
        assert_eq!(reg.cell(301).unwrap().unwrap().cell.position[2], -8.8);
    }
}

#[cfg(test)]
mod stock_sheet_tests {
    use super::*;

    #[test]
    fn stock_sheet_round_trips_and_stays_out_of_registry_tables() {
        let rows = vec![
            StockExportRow { cell_id: 101, item_code: 1001, item_name: "225/45R17".into(), count: 3, note: "a".into(), updated_at: "t".into() },
            StockExportRow { cell_id: 2003, item_code: 1002, count: 1, ..Default::default() },
        ];
        let items: Vec<ItemRow> = vec![];
        let bytes = export_workbook_with_stock(None, None, Some(&items), Some(&rows)).unwrap();
        let s = parse_stock("r.xlsx", &bytes, false).unwrap();
        assert!(s.found() && s.errors.is_empty(), "{:?}", s.errors);
        assert_eq!(s.rows.iter().map(|(_, r)| (r.cell_id, r.item_code, r.count, r.note.clone())).collect::<Vec<_>>(), vec![(101, 1001, 3, "a".to_string()), (2003, 1002, 1, String::new())]);
        // 통합 가져오기의 레지스트리 표에는 Stock 줄이 섞이지 않는다
        let t = parse_file("r.xlsx", &bytes, Want::All).unwrap();
        assert!(t.cells.is_empty() && t.items.is_empty() && t.errors.is_empty(), "{:?}", t.errors);
    }

    #[test]
    fn stock_csv_needs_cellid_and_count_and_an_item_for_nonzero_count() {
        let ok = parse_stock("s.csv", "CellId,ItemCode,Count\n101,1001,2\n102,,0\n".as_bytes(), true).unwrap();
        assert_eq!(ok.rows.len(), 2);
        let bad = parse_stock("s.csv", "CellId,Count\n101,2\n".as_bytes(), true).unwrap();
        assert_eq!(bad.errors.len(), 1, "count without item code");
        let no_header = parse_stock("s.csv", "Code,Qty\n1,2\n".as_bytes(), true).unwrap();
        assert!(no_header.rows.is_empty() && !no_header.errors.is_empty());
        let absent = parse_stock("r.xlsx", &export_workbook(Some(&[]), None, None).unwrap(), false).unwrap();
        assert!(!absent.found(), "registry file without a Stock sheet leaves stock alone");
    }
}
