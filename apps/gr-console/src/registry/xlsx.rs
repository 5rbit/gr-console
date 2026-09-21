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
//! sheet (no rows = no profile); rows for codes not in the `Items` sheet update the profiles of existing
//! items only.
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
    /// 1-based sheet row.
    pub row: usize,
    pub message: String,
}

#[derive(Clone, Debug, Default)]
pub struct Tables {
    pub cells: Vec<CellInfo>,
    pub stations: Vec<StationPara>,
    pub items: Vec<ItemRow>,
    /// Parsed `ItemBeadProfile` rows — merged into `items[*].spec.profiles`.
    pub item_profiles: Vec<ProfileSheetRow>,
    pub orphan_profiles: Vec<(u32, Vec<BeadProfile>)>,
    pub errors: Vec<RowError>,
    /// Which tables the file actually carried (a missing sheet is not an error).
    pub has_cells: bool,
    pub has_stations: bool,
    pub has_items: bool,
    pub has_item_profiles: bool,
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
    // PLC: 셀 바닥 Z <= 0 이면 isValidTaskData 가 거부한다. 셀 X/Y 는 0 이상이면 된다(태스크 위치는 RangeMin..Max 로 검사).
    let [x, y, z] = c.position;
    if let Some((axis, v)) = [("X", x), ("Y", y)].into_iter().find(|(_, v)| *v < 0.0 || v.is_nan()) {
        return Err(format!("position {axis} = {v} must be >= 0"));
    }
    if z <= 0.0 || z.is_nan() {
        return Err(format!("position Z = {z} must be > 0 (PLC rejects a cell floor Z <= 0)"));
    }
    Ok(())
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
    if cells.is_none() && stations.is_none() && items.is_none() {
        wb.add_worksheet();
    }
    wb.save_to_buffer().map_err(xerr)
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
    /// 절대 프로파일 시트(`ItemBeadProfile`, 또는 품목 치수 없이 Code + Stack + Level 머리글).
    fn is_profile(&self) -> bool {
        self.name.eq_ignore_ascii_case("ItemBeadProfile") || (self.has("Code") && self.has("Stack") && self.has("Level") && !self.has("InnerDiameter") && !self.has("Name"))
    }
    /// Which table this sheet looks like, from its headers (`None` for the profile sheet — handled apart).
    fn kind(&self) -> Option<Want> {
        if self.is_profile() {
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
    out.has_item_profiles = true;
    let mut seen = HashSet::new();
    for (rn, cells) in &sheet.rows {
        match parse_profile(&Row { sheet, cells }) {
            Ok(row) => {
                if seen.insert((row.code, row.count, row.row.level)) {
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
        Want::Items => out.has_items = true,
        Want::All => {}
    }
    for (rn, cells) in &sheet.rows {
        let r = Row { sheet, cells };
        let res = match as_kind {
            Want::Cells => parse_cell(&r).map(|c| out.cells.push(c)),
            Want::Stations => parse_station(&r).map(|s| out.stations.push(s)),
            Want::Items => parse_item(&r).map(|i| out.items.push(i)),
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

#[derive(Clone, Debug, Default, Serialize)]
pub struct ApplyCounts {
    pub imported: usize,
    pub updated: usize,
    pub skipped: usize,
    /// Rows rejected only when merged with the stored state (e.g. a level deeper than the stored stack_max).
    pub errors: Vec<RowError>,
}

impl ApplyCounts {
    pub fn add(&mut self, o: &ApplyCounts) {
        self.imported += o.imported;
        self.updated += o.updated;
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
                c.skipped += 1;
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
                c.skipped += 1;
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

pub fn apply_items(reg: &Registry, items: &[ItemRow], orphan_profiles: &[(u32, Vec<BeadProfile>)], dry_run: bool) -> Result<ApplyCounts, ApiError> {
    let existing = reg.items()?;
    let mut c = ApplyCounts::default();
    for it in items {
        let cur = existing.iter().find(|e| e.code == it.code);
        let spec = it.spec.apply(&cur.map(|e| e.spec.clone()).unwrap_or_default()).normalized();
        if let Err(m) = validate_spec_for(&spec, &it.item) {
            c.errors.push(RowError { sheet: "Items".into(), row: 0, message: format!("code {}: {m}", it.code) });
            continue;
        }
        match cur {
            Some(e) if e.item == it.item && e.name == it.name && e.note == it.note && e.spec == spec => {
                c.skipped += 1;
                continue;
            }
            Some(_) => c.updated += 1,
            None => c.imported += 1,
        }
        if !dry_run {
            reg.upsert_item_full(it.code, &it.name, &it.item, &it.note, Some(&spec))?;
        }
    }
    for (code, profiles) in orphan_profiles {
        let Some(e) = existing.iter().find(|e| e.code == *code) else {
            c.errors.push(RowError { sheet: "ItemBeadProfile".into(), row: 0, message: format!("code {code}: 등록된 품목이 없습니다(Items 시트에도 없음)") });
            continue;
        };
        let spec = ItemSpec { profiles: profiles.clone(), ..e.spec.clone() }.normalized();
        if let Err(m) = validate_spec_for(&spec, &e.item) {
            c.errors.push(RowError { sheet: "ItemBeadProfile".into(), row: 0, message: format!("code {code}: {m}") });
            continue;
        }
        if e.spec == spec {
            c.skipped += 1;
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
        let c = apply_items(&reg, &t.items, &t.orphan_profiles, false).unwrap();
        assert_eq!((c.imported, c.errors.len()), (2, 0));
        assert_eq!(reg.item(1001).unwrap().unwrap().spec, spec);
        let c = apply_items(&reg, &t.items, &t.orphan_profiles, false).unwrap();
        assert_eq!(c.skipped, 2);
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
        let dry = apply_items(&reg, &t.items, &t.orphan_profiles, true).unwrap();
        assert_eq!((dry.imported, dry.updated), (1, 1));
        assert!(reg.item(1005).unwrap().is_none(), "dry run must not write");
        apply_items(&reg, &t.items, &t.orphan_profiles, false).unwrap();
        assert_eq!(reg.item(1001).unwrap().unwrap().spec, spec, "missing columns keep the stored spec");
        assert_eq!(reg.item(1001).unwrap().unwrap().name, "A2");
        assert_eq!(reg.item(1005).unwrap().unwrap().spec, ItemSpec::default());
        // plain CRUD upsert (no spec) keeps it too
        reg.upsert_item(1001, "A3", &base.item, "").unwrap();
        assert_eq!(reg.item(1001).unwrap().unwrap().spec, spec);
        // a StackMax column alone changes only stack_max
        let csv = "Code,StackMax\n1001,6\n";
        let t = parse_file("items.csv", csv.as_bytes(), Want::Items).unwrap();
        apply_items(&reg, &t.items, &t.orphan_profiles, false).unwrap();
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
        let c = apply_items(&reg, &t.items, &t.orphan_profiles, false).unwrap();
        assert_eq!(c.updated, 1);
        assert_eq!(c.errors.len(), 1);
        assert!(c.errors[0].message.contains("9999"));
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
        assert_eq!(t.items, items);
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
        assert_eq!(t.items, items);
        // a whole-registry workbook: the Items sheet is picked by name, other sheets are ignored
        let bytes = export_workbook(Some(&[cell(5, 0)]), None, Some(&items)).unwrap();
        let t = parse_file("registry.xlsx", &bytes, Want::Items).unwrap();
        assert_eq!(t.items, items);
        assert!(t.cells.is_empty());
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
        assert_eq!((dry.imported, dry.updated, dry.skipped), (1, 1, 1));
        assert_eq!(reg.cells().unwrap().len(), 1, "dry run must not write");
        let wet = apply_cells(&reg, &incoming, false).unwrap();
        assert_eq!((wet.imported, wet.updated, wet.skipped), (1, 1, 1));
        let cells = reg.cells().unwrap();
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].cell.row, 9);
        assert!(cells[0].dirty && cells[0].source == "local");
    }
}
