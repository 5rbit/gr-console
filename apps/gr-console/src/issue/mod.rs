//! Task composition: `TaskRequest` (console, snake_case) → `TaskData` (PLC, PascalCase).
//!
//! Precedence of tunables: `Defaults.base` ← `Defaults.by[type][kind]` ← `request.params`.
//! Lookups (target cell / station, item) happen in `compose`; the arithmetic lives in `compose_from`
//! so it is unit-testable without an `AppState`. `warnings` are advisory — the PLC validates anyway.

pub mod station_offset;

use gr_proto::{CellInfo, StationPara, StockItem, TaskData, TaskParams, TaskType};
use serde::Serialize;
use serde_json::Value as Json;

use crate::error::ApiError;
use crate::ledger::TaskRequest;
use crate::registry::Defaults;
use crate::registry::spec::{ItemSpec, StackZ, stack_z_with};
use crate::state::AppState;
use station_offset::{Gr2Station, LiveStation, OffsetInput, StationOffsetAudit, Tracking};

#[derive(Clone, Debug, Serialize)]
pub struct Composed {
    pub task: TaskData,
    pub params: TaskParams,
    pub warnings: Vec<String>,
    /// 이 작성이 겨냥한 로봇과 그 상태 PLC — 미리보기·확인 창이 대상을 짐작하지 않게 응답에 싣는다.
    /// 순수 경로(`compose_from`)에서는 비어 있고 `compose_with` 가 채운다.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub robot: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub plc: String,
    /// 스테이션 대상 PICK/DROP/MEASURE 일 때 GRM 트래킹 보정 결과(`station_offset.rs`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub station_offset: Option<StationOffsetAudit>,
    /// 스택 Z 계산 근거 — `z_source` = `profile` | `curve` | `computed` 와 쓴 값들(PICK/DROP/MEASURE, 위치 덮어쓰기 없을 때).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_z: Option<StackZ>,
    /// 셀 단수 Max 검사(품목 `spec.stack_max > 0` 인 셀 대상 PICK/DROP/MEASURE).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_limit: Option<StackLimit>,
    /// 켜진 팔렛 프로파일 스테이션 + `pallet` 요청일 때 슬롯·드래그 근거(`pallet::compose`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pallet: Option<crate::pallet::compose::PalletAudit>,
    /// MOVE 의 모드·Z 근거(`params.move_mode`).
    #[serde(rename = "move", skip_serializing_if = "Option::is_none")]
    pub move_audit: Option<MoveAudit>,
}

/// 셀 단수 Max 검사 결과.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StackLimit {
    pub stack_max: u8,
    /// 셀 재고(모르면 `None` — 빈 셀로 본다).
    pub stock: Option<u32>,
    /// DROP: 놓은 뒤 개수 · PICK/MEASURE: 지금 개수.
    pub count_after: u32,
    pub over_limit: bool,
    /// DROP 초과를 `ignore_stack_max` 로 넘겼다.
    pub ignored: bool,
    /// 제출이 거부될 사유(DROP 초과, 무시 플래그 없음).
    pub blocked: Option<String>,
}

/// 순수 판정. `kind` 가 `cell` 이 아니거나 `stack_max == 0` 이면 `None`.
pub fn stack_limit(tt: TaskType, kind: &str, spec: Option<&ItemSpec>, stock: Option<u32>, count: u32, ignore: bool) -> Option<StackLimit> {
    let spec = spec.filter(|s| s.stack_max > 0)?;
    if kind != "cell" {
        return None;
    }
    let n = stock.unwrap_or(0);
    let c = count.max(1);
    let after = match tt {
        TaskType::Drop => n + c,
        TaskType::Pick | TaskType::Measure => n,
        _ => return None,
    };
    let over = spec.exceeds(after);
    let drop_over = over && tt == TaskType::Drop;
    let blocked = (drop_over && !ignore).then(|| format!("놓은 뒤 {after}개 > StackMax {} (재고 {n} + 놓기 {c})", spec.stack_max));
    Some(StackLimit { stack_max: spec.stack_max, stock, count_after: after, over_limit: over, ignored: drop_over && ignore, blocked })
}

/// 요청이 겨냥한 로봇 이름(모르면 `로봇`) — 거부 문구 앞에 붙는다.
fn robot_name(st: &AppState, req: &TaskRequest) -> String {
    st.robot(req.robot).map(|r| r.name.clone()).unwrap_or_else(|_| "로봇".into())
}

/// 제출 직전 단수 Max 검사 — 셀 대상 DROP 이 지금 재고 기준으로 `stack_max` 를 넘으면 409 (`ignore_stack_max` 면 통과).
pub fn enforce_stack_limit(st: &AppState, req: &TaskRequest, task: &TaskData) -> Result<(), ApiError> {
    if req.ignore_stack_max || TaskType::from_code(task.task_type) != Some(TaskType::Drop) || task.item.code == 0 {
        return Ok(());
    }
    let Some(t) = req.target.as_ref().filter(|t| t.kind == "cell") else { return Ok(()) };
    let Some(entry) = st.registry.item(task.item.code)? else { return Ok(()) };
    // 미리 넣은(아직 안 끝난) 작업까지 친 예상 재고로 본다.
    let n = projected_stock(st, t.id)?.projected.map(|s| s.count);
    match stack_limit(TaskType::Drop, "cell", Some(&entry.spec), n, task.item.count as u32, false).and_then(|l| l.blocked) {
        Some(b) => Err(ApiError::Conflict(crate::ledger::ops::with_robot(&robot_name(st, req), &format!("셀 {} StackMax: {b} — 품목 {} (무시하려면 ignore_stack_max)", t.id, task.item.code)))),
        None => Ok(()),
    }
}

/// Gripper opening (G axis) used when nothing better is known.
const G_MIN: f32 = 200.0;
/// G = inner diameter minus this clearance.
const G_CLEARANCE: f32 = 30.0;
/// MOVE `stack` 모드의 기본 여유 — 스택 윗면(빈 셀이면 바닥) 위 이만큼(옛 "바닥 + 500" 과 같은 값).
pub const MOVE_CLEARANCE: f32 = 500.0;
/// GR2 가 "하강 없는 이동" 으로 읽는 Z — `isValidTaskData`(Z 범위 검사 면제) · `isValidTaskArea`(영역 검사
/// 면제) · `PL_Task_V2` `isTaskMove`(Z HomePos 유지 → XY 이동 → 999, 하강·그립 없음).
pub const MOVE_TOP_Z: f32 = 9999.0;
/// Default G for UP / no-item tasks.
const G_DEFAULT: f32 = 300.0;

/// MOVE 작성 방식 — 요청 `params.move_mode`(없으면 `stack`, 예전 동작).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveMode {
    /// 대상 위로 내려간다: Z = 바닥 + 스택 높이(재고 × 품목) + `move_clearance`. 내려갔다 올라와 끝난다.
    Stack,
    /// Z = 9999 — Z 상단(HomePos)을 유지한 채 대상 XY 로만 이동. 품목·수량 불필요.
    Top,
    /// `Avoid` 플래그 + Z = 9999 — 상단에서 **X 만** 대상으로(Y 는 지금 위치 유지). 회피용.
    Avoid,
}

impl MoveMode {
    pub fn parse(s: &str) -> Result<MoveMode, ApiError> {
        Ok(match s.trim().to_ascii_lowercase().as_str() {
            "" | "stack" | "hover" => MoveMode::Stack,
            "top" | "position" | "9999" => MoveMode::Top,
            "avoid" => MoveMode::Avoid,
            other => return Err(ApiError::BadRequest(format!("unknown move_mode {other} (stack | top | avoid)"))),
        })
    }
}

/// MOVE 작성 근거 — 미리보기가 Z 가 어디서 왔는지 보인다.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MoveAudit {
    pub mode: MoveMode,
    /// 요청에 모드가 없어 기본(`stack`)을 썼다.
    pub defaulted: bool,
    /// `stack` 의 여유(mm).
    pub clearance: f32,
    /// `stack` 에서 쓴 스택 높이(바닥 위, mm)와 개수 — 모르면 `None`.
    pub stack_height: Option<f32>,
    pub stack_count: u32,
    pub z: f32,
}

/// `params.move_mode` / `params.move_clearance` — 요청이 먼저, 없으면 기본값 `by[MOVE][kind]`.
/// `TaskParams` 는 모르는 키를 버리므로 이 둘은 원본 JSON 에서 읽는다(시나리오 스텝 `params` 로도 실린다).
fn move_options(defaults: &Defaults, kind: &str, req: &Json) -> Result<(MoveMode, bool, f32), ApiError> {
    let by = defaults.by.get(TaskType::Move.name()).and_then(|m| m.get(kind));
    let pick = |k: &str| req.get(k).filter(|v| !v.is_null()).or_else(|| by.and_then(|b| b.get(k)).filter(|v| !v.is_null()));
    let (mode, defaulted) = match pick("move_mode") {
        Some(v) => (MoveMode::parse(v.as_str().ok_or_else(|| ApiError::BadRequest("move_mode 는 문자열(stack | top | avoid)".into()))?)?, false),
        None => (MoveMode::Stack, true),
    };
    let clearance = match pick("move_clearance") {
        Some(v) => v.as_f64().filter(|c| c.is_finite() && (0.0..=5000.0).contains(c)).ok_or_else(|| ApiError::BadRequest(format!("move_clearance {v} — 0..5000 mm")))? as f32,
        None => MOVE_CLEARANCE,
    };
    Ok((mode, defaulted, clearance))
}

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

/// 셀·스테이션 `id` 의 예상 재고 — 재고 표 + 모든 로봇 원장의 진행 중 PICK/DROP(`stock::Stock::projected`).
pub fn projected_stock(st: &AppState, id: u16) -> Result<crate::stock::Projection, ApiError> {
    st.stock.projected(id, || st.robots.iter().flat_map(|r| r.ledger.list()).collect())
}

/// Full path: resolve target + item from the registry, then compose (stock from the console inventory
/// plus tasks still in flight — see `projected_stock`).
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
    // stock of the target cell (console-owned inventory) — Z stacking + item code fallback.
    // 가정 재고(`stock_hint`, 계획 미리보기)가 없으면 진행 중 작업을 반영한 예상 재고를 쓴다(미리 넣기).
    let mut projection_note = None;
    let stock = match &req.target {
        Some(t) if t.kind == "cell" && stock_hint.is_none() => {
            let p = projected_stock(st, t.id)?;
            projection_note = p.note("셀");
            p.projected
        }
        Some(t) if t.kind == "cell" => st.stock.get(t.id)?,
        _ => None,
    };
    let code = req.item_code.or_else(|| stock.as_ref().map(|s| s.item_code).filter(|c| *c != 0));
    let entry = match code {
        Some(code) => Some(st.registry.item(code)?.ok_or_else(|| ApiError::BadRequest(format!("item {code} not registered")))?),
        None => None,
    };
    let item = entry.as_ref().map(|e| e.item.clone());
    let spec = entry.map(|e| e.spec);
    // 팔렛 — 켜진 프로파일이 있는 스테이션에만. 프로파일이 없으면 기존 스테이션 동작 그대로다.
    let station_id = req.target.as_ref().filter(|t| t.kind == "station").map(|t| t.id);
    let pallet_profile = match station_id {
        Some(id) => crate::pallet::compose::enabled_profile(st, id)?,
        None => None,
    };
    let prepared = match (&req.pallet, station_id, &pallet_profile) {
        (None, _, _) => None,
        (Some(_), None, _) => return Err(ApiError::BadRequest("pallet 은 스테이션 대상에만 씁니다".into())),
        (Some(_), Some(id), None) => {
            return Err(ApiError::BadRequest(format!("스테이션 {id} 에 켜진 팔렛 프로파일이 없습니다 — 팔렛 패턴 화면에서 만들고 Enabled 로 저장하세요")));
        }
        (Some(pref), Some(id), Some(profile)) => {
            let tt = parse_task_type(&req.task_type)?;
            let it = item.as_ref().ok_or_else(|| ApiError::BadRequest("pallet 작업에는 item_code 가 필요합니다".into()))?;
            let center = cell.as_ref().map(|c| [c.position[0], c.position[1]]).unwrap_or_default();
            // auto: 미리보기 체인의 가정 재고가 있으면 그것, 없으면 스테이션 재고(완료된 PICK/DROP 이 접힌 값)
            let station_stock = if pref.auto {
                match stock_hint {
                    Some(n) => Some(n),
                    None => {
                        let p = projected_stock(st, id)?;
                        projection_note = p.note("스테이션");
                        p.projected.map(|s| s.count)
                    }
                }
            } else {
                None
            };
            // 패턴은 편집 저장소에서 — 운전자가 고친 패턴이 바로 작업에 쓰인다(감사 기록에 편집 시각이 남는다).
            let lib = crate::pallet::store::PalletStore::new(st.db.clone()).library()?;
            let mut p = crate::pallet::compose::prepare_pure(&lib, profile, pref, tt, it, req.count.max(1) as u32, center, station_stock).map_err(ApiError::BadRequest)?;
            p.audit.station_id = id;
            Some(p)
        }
    };
    let stock_pair = match (&prepared, stock_hint, stock) {
        // 팔렛: 단에서 거꾸로 만든 스택 개수로 기존 Z 규칙을 그대로 쓴다
        (Some(p), _, _) => Some((0, p.n_for_z)),
        (None, Some(n), Some(s)) => Some((s.item_code, n)),
        (None, Some(n), None) => Some((0, n)),
        (None, None, Some(s)) => Some((s.item_code, s.count)),
        (None, None, None) => None,
    };
    let mut c = compose_from(&st.registry.defaults()?, req, cell, item, stock_pair, spec.as_ref())?;
    c.warnings.extend(projection_note);
    // 대상 로봇을 응답에 박는다 — 화면이 `robots.selected` 로 되짚으면 선택이 바뀐 뒤의 미리보기가
    // 엉뚱한 호기 이름을 달게 된다.
    if let Ok(r) = st.robot(req.robot) {
        c.robot = r.name.clone();
        c.plc = r.plc.clone();
    }
    if let Some(p) = prepared {
        crate::pallet::compose::apply_to(&mut c, p.audit, req.position_override.is_some());
        if let Some(id) = station_id
            && let Some(s) = st.registry.station(id)?
        {
            crate::pallet::compose::attach_area(st, req.robot, id, &s.para, &mut c);
        }
    } else if let (Some(id), Some(_)) = (station_id, &pallet_profile) {
        c.warnings.push(format!("스테이션 {id} 는 팔렛 프로파일이 켜져 있습니다 — pallet {{seq, level}} 없이 Info.Position 중심으로 작성했습니다(트래킹 보정 안 함)"));
    }
    // 켜진 팔렛 프로파일 스테이션은 `station_offset_for` 가 None 을 돌려준다(팔렛은 컨베이어가 아니다).
    if let Some((audit, pos)) = station_offset_for(st, req, c.task.position, c.task.item.outer_diameter)? {
        c.task.position = pos;
        // 자세한 경고는 감사 기록(`station_offset.warnings`)에 — 여기엔 다른 화면(계획 등)도 알아채도록 요약만.
        if let Some(b) = &audit.blocked {
            c.warnings.push(format!("제출 거부 예정 — {b}"));
        }
        if !audit.warnings.is_empty() {
            c.warnings.push(format!("스테이션 보정 경고 {}건 — {}", audit.warnings.len(), audit.warnings[0]));
        }
        c.station_offset = Some(audit);
    }
    Ok(c)
}

/// 제출 직전 보정 재계산 — 트래킹은 컨베이어가 돌면서 바뀌고 픽 뒤에 지워지므로 작성 때 값을 믿지 않는다.
/// `task.position` 의 X·Y 를 레지스트리 Info + 새 보정으로 다시 쓴다(Z·G 는 그대로). `enforce` 면 거부 사유가
/// 있을 때 409. 스테이션 대상 PICK/DROP/MEASURE 가 아니면 `None`.
pub fn refresh_station_offset(st: &AppState, req: &TaskRequest, task: &mut TaskData, enforce: bool) -> Result<Option<StationOffsetAudit>, ApiError> {
    let Some((audit, pos)) = station_offset_for(st, req, task.position, task.item.outer_diameter)? else {
        return Ok(None);
    };
    if enforce && let Some(b) = &audit.blocked {
        return Err(ApiError::Conflict(crate::ledger::ops::with_robot(&robot_name(st, req), &format!("스테이션 보정: {b}"))));
    }
    task.position = pos;
    Ok(Some(audit))
}

/// 스냅샷을 읽어 `station_offset::evaluate` 에 넘긴다. `position` 은 콘솔 규칙으로 정한 위치(Z·G 가 여기서 온다).
fn station_offset_for(st: &AppState, req: &TaskRequest, position: [f32; 4], item_od: f32) -> Result<Option<(StationOffsetAudit, [f32; 4])>, ApiError> {
    let Some(t) = req.target.as_ref().filter(|t| t.kind == "station") else {
        return Ok(None);
    };
    let tt = parse_task_type(&req.task_type)?;
    if !station_offset::applies_to(tt) {
        return Ok(None);
    }
    // 켜진 팔렛 프로파일 스테이션은 팔렛이지 컨베이어가 아니다 — 트래킹 보정을 쓰지 않는다(작성·제출 모두).
    if crate::pallet::compose::enabled_profile(st, t.id)?.is_some() {
        return Ok(None);
    }
    let para = st.registry.station(t.id)?.ok_or_else(|| ApiError::BadRequest(format!("station {} not registered", t.id)))?.para;
    let override_position = req.position_override.is_some();
    let base = if override_position { position } else { [para.info.position[0], para.info.position[1], position[2], position[3]] };
    let (live, live_missing) = match grm_live(st, t.id) {
        Ok(l) => (Some(l), None),
        Err(why) => (None, Some(why)),
    };
    let (gr2, gr2_note) = gr2_station(st, req.robot, t.id, &para);
    let (mut audit, pos) =
        station_offset::evaluate(&OffsetInput { station_id: t.id, task_type: tt, switch: req.station_offset, override_position, para: &para, live, live_missing, gr2, item_od, position: base });
    audit.warnings.extend(gr2_note);
    Ok(Some((audit, pos)))
}

fn age_ms(at: &str) -> Option<i64> {
    let t = crate::ledger::parse_rfc3339(at)?;
    Some((time::OffsetDateTime::now_utc() - t).whole_milliseconds() as i64)
}

fn f32_at(j: &Json) -> f32 {
    j.as_f64().unwrap_or(0.0) as f32
}

fn tracking_of(j: &Json) -> Tracking {
    Tracking { od: f32_at(&j["OutterDiameter"]), tx: f32_at(&j["TaskOffset"][0]), ty: f32_at(&j["TaskOffset"][1]) }
}

/// GRM 슬롯 `id MOD 100` 의 트래킹 — fast `OPCUA.STATION[n]`(Comm_CV 가 매 스캔 복사) 우선, 그 슬롯 Id 가
/// 다르거나 없으면 slow `STATION.Station[n]`. JSON 배열은 0 부터라 PLC 인덱스 n 은 `[n-1]`.
fn grm_live(st: &AppState, id: u16) -> Result<LiveStation, String> {
    let slot = station_offset::slot_of(id).ok_or_else(|| format!("슬롯 {}(id MOD 100)이 1..32 밖", id % 100))?;
    let h = st.grm_plc().ok_or_else(|| "GRM PLC 미설정".to_string())?;
    let snap = h.snap();
    let mut seen = Vec::new();
    for (db, arr) in [("OPCUA", "STATION"), ("STATION", "Station")] {
        let Some(d) = snap.db(db) else { continue };
        let el = &d.json[arr][slot - 1];
        if el.is_null() {
            continue;
        }
        let got = el["Para"]["Info"]["Id"].as_u64().unwrap_or(0);
        if got != id as u64 {
            seen.push(format!("{db}[{slot}].Id = {got}"));
            continue;
        }
        let mismatch = el["Status"]["DataMissMatch"].as_array().map(|a| a.iter().any(|b| b.as_bool() == Some(true))).unwrap_or(false);
        return Ok(LiveStation {
            rotate_type: el["Para"]["RotateType"].as_u64().unwrap_or(0) as u8,
            now: tracking_of(&el["Tracking"]["Now"]),
            staged: tracking_of(&el["Tracking"]["Staged"]),
            measuring_error: el["Status"]["MeasuringError"].as_bool().unwrap_or(false),
            data_mismatch: mismatch,
            source: db.to_string(),
            snapshot_at: d.at.clone(),
            age_ms: age_ms(&d.at),
            disconnected: !h.health_now().connected,
        });
    }
    Err(if seen.is_empty() { "GRM 스냅샷 없음".into() } else { format!("GRM 슬롯 Id 불일치: {} — 스테이션 푸시 필요", seen.join(", ")) })
}

/// GR2 `isValidTaskArea` 입력 — TaskType 은 슬롯 `id MOD 100`, RotateType·Info 는 Id 로 찾은 원소.
/// 스냅샷이 없으면 레지스트리 값. 두 번째 값은 GR2 가 스테이션을 못 찾을 때의 경고.
pub(crate) fn gr2_station(st: &AppState, robot: Option<u8>, id: u16, para: &StationPara) -> (Option<Gr2Station>, Option<String>) {
    let fallback = Gr2Station { task_type: para.task_type, rotate_type: para.rotate_type, info_position: para.info.position, from_registry: true };
    let Some(slot) = station_offset::slot_of(id) else { return (Some(fallback), None) };
    let Some(h) = st.robot(robot).ok().and_then(|r| st.robot_plc(r).ok()) else { return (Some(fallback), None) };
    let snap = h.snap();
    let Some(d) = snap.db("STATION") else { return (Some(fallback), None) };
    let Some(list) = d.json["Station"].as_array() else { return (Some(fallback), None) };
    let elem = |v: &'_ Json| -> Json { if v.get("Para").is_some() { v["Para"].clone() } else { v.clone() } };
    let task_type = list.get(slot - 1).map(|v| elem(v)["TaskType"].as_u64().unwrap_or(0) as u8).unwrap_or(0);
    match list.iter().map(elem).find(|v| v["Info"]["Id"].as_u64() == Some(id as u64)) {
        Some(v) => {
            let p = &v["Info"]["Position"];
            (Some(Gr2Station { task_type, rotate_type: v["RotateType"].as_u64().unwrap_or(0) as u8, info_position: [f32_at(&p[0]), f32_at(&p[1]), f32_at(&p[2])], from_registry: false }), None)
        }
        None => (Some(fallback), Some(format!("{} STATION 에 스테이션 {id} 없음 → GR2 거부 419 예상", h.name()))),
    }
}

/// Pure composition from already-resolved inputs.
/// `stock` = (item_code, count) currently in the target cell, if known. `spec` = the item's console spec
/// (단수 Max · 비드 프로파일).
pub fn compose_from(defaults: &Defaults, req: &TaskRequest, cell: Option<CellInfo>, item: Option<StockItem>, stock: Option<(u32, u32)>, spec: Option<&ItemSpec>) -> Result<Composed, ApiError> {
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
            // 바닥 Z ≤ 0 은 레지스트리에서 허용한다(바닥 평탄도 보정). 막지는 않고, 이 작업이 PLC 에서
            // 어떻게 될지만 미리 말한다 — `isValidTaskData` 가 INVALID_CELL_POSZ 로 돌려보낸다.
            if let Some(w) = crate::registry::xlsx::floor_z_warning(if kind == "station" { "스테이션" } else { "셀" }, c.id, c.position[2]) {
                warnings.push(w);
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
    // 드래그 거리를 아무도 안 정했으면 기본 150 mm(운전자 결정 2026-09-18) — 0 으로 내보내지 않는다.
    if params.use_drag_out && params.drag_out_dist == 0 {
        params.drag_out_dist = gr_proto::DEFAULT_DRAG_DIST;
    }
    if params.use_drag_in && params.drag_in_dist == 0 {
        params.drag_in_dist = gr_proto::DEFAULT_DRAG_DIST;
    }
    // MOVE 모드 — 모드를 정했으면 Avoid 플래그는 모드가 정한다(`avoid` 만 켜고, `stack`·`top` 은 끈다:
    // Avoid 가 켜져 있으면 PLC 가 하강도 Y 이동도 하지 않는다).
    let move_opts = if tt == TaskType::Move { Some(move_options(defaults, &kind, &req.params)?) } else { None };
    if let Some((mode, false, _)) = move_opts {
        params.avoid = mode == MoveMode::Avoid;
    }
    params.apply(&mut task);

    // Z from the cell stock (n tires already there), G from the inner diameter. The UI can override the whole position.
    let mut stack_z = None;
    let mut move_audit = None;
    if req.position_override.is_none() && task.cell.id != 0 {
        let c = task.item.count.max(1) as u32;
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
        let grip_ref = req.grip_ref.as_deref().unwrap_or(&defaults.grip_ref);
        task.position[2] = match tt {
            TaskType::Pick | TaskType::Measure | TaskType::Drop => {
                let z = stack_z_with(tt, floor, &task.item, spec, grip_ref, n, c);
                let v = z.z;
                warnings.extend(z.warnings.iter().map(|w| format!("Z: {w}")));
                stack_z = Some(z);
                v
            }
            TaskType::Move => {
                let (mode, defaulted, clearance) = move_opts.unwrap_or((MoveMode::Stack, true, MOVE_CLEARANCE));
                let (z, stack_height) = match mode {
                    MoveMode::Top | MoveMode::Avoid => (MOVE_TOP_Z, None),
                    MoveMode::Stack => {
                        // 스택 윗면 = 아래 n 개(눌림 반영)의 높이 — DROP 이 새 타이어를 얹을 자리와 같다.
                        let sh = if n == 0 {
                            Some(0.0)
                        } else if task.item.height > 0.0 {
                            Some(stack_z_with(TaskType::Drop, 0.0, &task.item, spec, "mid", n, 1).base)
                        } else {
                            warnings.push(format!("cell {} 재고 {n}개인데 품목 높이를 몰라 스택 높이를 뺀 Z 입니다 — top 모드(Z 9999)를 쓰세요", task.cell.id));
                            None
                        };
                        (floor + sh.unwrap_or(0.0) + clearance, sh)
                    }
                };
                move_audit = Some(MoveAudit { mode, defaulted, clearance, stack_height, stack_count: n, z });
                z
            }
            TaskType::Up => task.position[2],
        };
        if task.item.inner_diameter > 0.0 {
            task.position[3] = (task.item.inner_diameter - G_CLEARANCE).max(G_MIN);
        }
    }
    if let Some(p) = req.position_override {
        task.position = p;
    }
    // 단수 Max — 위치 덮어쓰기와 무관하게 본다(쌓이는 개수는 같다).
    let limit = if task.cell.id != 0 { stack_limit(tt, &kind, spec, stock.map(|(_, n)| n), task.item.count as u32, req.ignore_stack_max) } else { None };
    if let Some(l) = &limit {
        let id = task.cell.id;
        if let Some(b) = &l.blocked {
            warnings.push(format!("제출 거부 예정 — 셀 {id} StackMax: {b}"));
        } else if l.ignored {
            warnings.push(format!("셀 {id} StackMax {} 초과({}개) — ignore_stack_max 로 무시", l.stack_max, l.count_after));
        } else if l.over_limit {
            warnings.push(format!("셀 {id} 재고 {}개가 StackMax {} 초과", l.count_after, l.stack_max));
        }
    }
    if let Some((axis, v)) = ["X", "Y", "Z", "G"].iter().zip(task.position).find(|(_, v)| !v.is_finite() || *v < 0.0) {
        warnings.push(format!("position {axis} = {v} is not a valid coordinate"));
    }
    Ok(Composed { task, params, warnings, robot: String::new(), plc: String::new(), station_offset: None, stack_z, stack_limit: limit, pallet: None, move_audit })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` 단 스택을 통째로 잰 절대 프로파일(눌림 `comp` mm/개, 타이어 바닥 기준 상부 비드 220 도 같이 눌린다).
    fn measured_spec(n: u32, comp: f32, stack_max: u8) -> ItemSpec {
        use crate::registry::spec::{LevelValues, MeasuredStack, SampleRef, put_profile};
        let pressed = |above: u32| (240.0f32 - comp * above as f32).max(1.0);
        let mut rows: Vec<LevelValues> = Vec::new();
        let mut bottom = 0.0f32;
        for k in 1..=n {
            let above = n - k;
            rows.push(LevelValues { lower_bead: Some(bottom + 20.0), upper_bead: Some(bottom + 220.0 - comp * above as f32), stack_height: None });
            bottom += pressed(above);
        }
        if let Some(l) = rows.last_mut() {
            l.stack_height = Some(bottom);
        }
        let mut s = ItemSpec { stack_max, compression: Some(comp), ..Default::default() };
        let sample = SampleRef { plc: "GR2".into(), seq: 7 };
        put_profile(&mut s, &MeasuredStack { count: n, levels: &rows, each_height: Some(bottom / n as f32), total_height: Some(bottom), sample: Some(&sample), at: "t" }, true);
        s
    }
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

    /// 드래그를 켰는데 거리를 안 정했으면 기본 150 mm 를 넣는다(결정 2026-09-18) — 경고 대신 값.
    #[test]
    fn drag_distance_falls_back_to_150() {
        let mut d = defaults();
        d.base.drag_in_dist = 0;
        d.base.drag_out_dist = 0;
        let mut r = req("DROP", "cell");
        r.params = json!({ "use_drag_in": true, "use_drag_out": true });
        let c = compose_from(&d, &r, Some(cell()), Some(item()), None, None).unwrap();
        assert_eq!((c.params.drag_in_dist, c.params.drag_out_dist), (150, 150));
        assert_eq!((c.task.drag_in_dist, c.task.drag_out_dist), (150, 150));
        assert!(!c.warnings.iter().any(|w| w.contains("Dist")), "{:?}", c.warnings);
        // 운전자가 정한 값은 그대로 둔다
        r.params = json!({ "use_drag_in": true, "drag_in_dist": 80 });
        let c = compose_from(&d, &r, Some(cell()), Some(item()), None, None).unwrap();
        assert_eq!(c.task.drag_in_dist, 80);
        // 드래그를 안 켜면 건드리지 않는다
        let c = compose_from(&d, &req("DROP", "cell"), Some(cell()), Some(item()), None, None).unwrap();
        assert_eq!((c.task.drag_in_dist, c.task.drag_out_dist), (0, 0));
    }

    #[test]
    fn precedence_base_by_request() {
        let d = defaults();
        // base only (DROP has no cell override for grip_height)
        let c = compose_from(&d, &req("DROP", "cell"), Some(cell()), Some(item()), None, None).unwrap();
        assert_eq!(c.params.grip_height, 40);
        assert!(!c.params.avoid);
        // by[PICK][cell]
        let c = compose_from(&d, &req("PICK", "cell"), Some(cell()), Some(item()), None, None).unwrap();
        assert_eq!(c.params.grip_height, 50);
        assert!(c.params.avoid);
        assert_eq!(c.task.grip_height, 50, "params are applied onto the PLC task");
        // by[PICK][station]
        let c = compose_from(&d, &req("PICK", "station"), Some(cell()), Some(item()), None, None).unwrap();
        assert_eq!(c.params.grip_height, 55);
        assert!(c.params.find_station_item);
        // request overrides everything; null keys are ignored
        let mut r = req("PICK", "cell");
        r.params = json!({ "grip_height": 60, "avoid": null, "lift_up_height": 1000 });
        let c = compose_from(&d, &r, Some(cell()), Some(item()), None, None).unwrap();
        assert_eq!(c.params.grip_height, 60);
        assert!(c.params.avoid);
        assert_eq!(c.task.lift_up_height, 1000);
    }

    #[test]
    fn position_and_item_count() {
        let d = defaults();
        let c = compose_from(&d, &req("PICK", "cell"), Some(cell()), Some(item()), None, None).unwrap();
        assert_eq!(c.task.item.count, 3);
        assert_eq!(c.task.position[0], 12000.0);
        // stock unknown: PICK assumes the 3 tires sit on the floor → grip the bottom one (mid-tire)
        assert_eq!(c.task.position[2], 1500.0 + 120.0);
        assert_eq!(c.task.position[3], 381.0 - 30.0);
        assert_eq!(c.task.cell.id, 101);
        assert_eq!(c.task.task_type, gr_proto::CMD_TASK_PICK);
        assert!(c.warnings.iter().any(|w| w.contains("stock unknown")), "{:?}", c.warnings);
        // stock known: 5 tires, taking 3 → grip the 3rd from the bottom; DROP lands on top of 5
        let c = compose_from(&d, &req("PICK", "cell"), Some(cell()), Some(item()), Some((1001, 5)), None).unwrap();
        assert_eq!(c.task.position[2], 1500.0 + 2.0 * 240.0 + 120.0);
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        let c = compose_from(&d, &req("DROP", "cell"), Some(cell()), Some(item()), Some((1001, 5)), None).unwrap();
        assert_eq!(c.task.position[2], 1500.0 + 5.0 * 240.0 + 120.0);
        // stock too small / different item → warnings
        let c = compose_from(&d, &req("PICK", "cell"), Some(cell()), Some(item()), Some((1002, 2)), None).unwrap();
        assert!(c.warnings.iter().any(|w| w.contains("stock 2 < count 3")));
        assert!(c.warnings.iter().any(|w| w.contains("holds item 1002")));
        let mut r = req("MOVE", "cell");
        r.position_override = Some([1.0, 2.0, 3.0, 4.0]);
        let c = compose_from(&d, &r, Some(cell()), None, None, None).unwrap();
        assert_eq!(c.task.position, [1.0, 2.0, 3.0, 4.0]);
    }

    /// MOVE 모드 — 없으면 옛 동작(stack, 빈 셀이면 바닥 + 500), top/avoid 는 Z 9999(GR2 하강 없는 이동).
    #[test]
    fn move_modes() {
        let d = defaults();
        let mv = |params: Json, item: Option<StockItem>, stock: Option<(u32, u32)>| {
            let mut r = req("MOVE", "cell");
            r.item_code = item.as_ref().map(|i| i.code);
            r.count = 0;
            r.params = params;
            compose_from(&d, &r, Some(cell()), item, stock, None)
        };
        // 모드 없음: 빈/모르는 셀은 예전과 같은 바닥 + 500, Avoid 는 기본값 그대로
        let c = mv(json!({}), None, None).unwrap();
        assert_eq!(c.task.position[2], 1500.0 + MOVE_CLEARANCE);
        let a = c.move_audit.clone().unwrap();
        assert_eq!((a.mode, a.defaulted, a.stack_height), (MoveMode::Stack, true, Some(0.0)));
        assert!(c.warnings.is_empty(), "품목·수량 없는 MOVE 는 경고 없음: {:?}", c.warnings);
        assert_eq!((c.task.item.code, c.task.item.count), (0, 0));
        // stack: 재고 5 × 240 위 + 여유 100
        let c = mv(json!({ "move_mode": "stack", "move_clearance": 100 }), Some(item()), Some((1001, 5))).unwrap();
        assert_eq!(c.task.position[2], 1500.0 + 5.0 * 240.0 + 100.0);
        assert_eq!(c.move_audit.as_ref().unwrap().stack_height, Some(1200.0));
        assert!(!c.task.avoid);
        // 재고가 있는데 품목 높이를 모르면 경고(스택 높이 없이 계산)
        let c = mv(json!({ "move_mode": "stack" }), None, Some((0, 3))).unwrap();
        assert!(c.warnings.iter().any(|w| w.contains("top 모드")), "{:?}", c.warnings);
        // top: Z 9999, 품목·수량 없어도 경고 없음, Avoid 꺼짐(기본값이 켜 두었어도)
        let mut da = defaults();
        da.base.avoid = true;
        let mut r = req("MOVE", "cell");
        r.item_code = None;
        r.count = 0;
        r.params = json!({ "move_mode": "top" });
        let c = compose_from(&da, &r, Some(cell()), None, Some((1001, 4)), None).unwrap();
        assert_eq!(c.task.position[2], MOVE_TOP_Z);
        assert_eq!((c.task.position[0], c.task.position[1]), (12000.0, 3000.0));
        assert!(!c.task.avoid && !c.params.avoid);
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        // avoid: Avoid 켜짐 + Z 9999
        let c = mv(json!({ "move_mode": "avoid" }), None, None).unwrap();
        assert_eq!(c.task.position[2], MOVE_TOP_Z);
        assert!(c.task.avoid);
        assert_eq!(c.move_audit.unwrap().mode, MoveMode::Avoid);
        // 기본값 by[MOVE][cell] 로도 정할 수 있고 요청이 이긴다
        let mut db = defaults();
        db.by.entry("MOVE".into()).or_default().insert("cell".into(), json!({ "move_mode": "top" }));
        let mut r = req("MOVE", "cell");
        r.item_code = None;
        let c = compose_from(&db, &r, Some(cell()), None, None, None).unwrap();
        assert_eq!(c.task.position[2], MOVE_TOP_Z);
        r.params = json!({ "move_mode": "stack" });
        assert_eq!(compose_from(&db, &r, Some(cell()), None, None, None).unwrap().task.position[2], 1500.0 + MOVE_CLEARANCE);
        // 잘못된 값은 400, 다른 종류에는 영향 없음
        assert!(mv(json!({ "move_mode": "fly" }), None, None).is_err());
        assert!(mv(json!({ "move_clearance": -1 }), None, None).is_err());
        let mut p = req("PICK", "cell");
        p.params = json!({ "move_mode": "top" });
        let c = compose_from(&d, &p, Some(cell()), Some(item()), Some((1001, 5)), None).unwrap();
        assert!(c.move_audit.is_none() && c.task.position[2] < 9000.0);
    }

    /// 바닥 Z 가 음수인 셀(바닥 평탄도 보정) — 경고 없이 작성되고 Z 는 바닥 + 스택 + 그립이다
    /// (PLC 의 INVALID_CELL_POSZ 는 스테이션만 본다).
    #[test]
    fn negative_floor_cell_composes_without_warning() {
        let d = defaults();
        let floor = -8.8f32;
        let c = compose_from(&d, &req("PICK", "cell"), Some(CellInfo { position: [12000.0, 3000.0, floor], ..cell() }), Some(item()), Some((1001, 5)), None).unwrap();
        assert_eq!(c.task.position[2], floor + 2.0 * 240.0 + 120.0);
        assert!(!c.warnings.iter().any(|w| w.contains("INVALID_CELL_POSZ")), "{:?}", c.warnings);
        // DROP 도 같은 규칙 — 바닥은 그대로 더해진다
        let c = compose_from(&d, &req("DROP", "cell"), Some(CellInfo { position: [12000.0, 3000.0, floor], ..cell() }), Some(item()), Some((1001, 5)), None).unwrap();
        assert_eq!(c.task.position[2], floor + 5.0 * 240.0 + 120.0);
        // 바닥이 양수면 이 경고는 없다
        let c = compose_from(&d, &req("PICK", "cell"), Some(cell()), Some(item()), Some((1001, 5)), None).unwrap();
        assert!(!c.warnings.iter().any(|w| w.contains("INVALID_CELL_POSZ")), "{:?}", c.warnings);
    }

    /// 미리 넣기: 셀 101 에 DROP 1 이 아직 진행 중이면 뒤이은 PICK 은 재고 5 가 아니라 6 을 보고 Z 를 잡는다.
    #[test]
    fn pick_after_queued_drop_uses_projected_stock() {
        use crate::ledger::{LedgerEntry, TaskState};
        let d = defaults();
        let mut drop = compose_from(&d, &req("DROP", "cell"), Some(cell()), Some(item()), Some((1001, 5)), None).unwrap().task;
        drop.item.count = 1;
        let queued: LedgerEntry = serde_json::from_value(json!({
            "id": "q", "seq": 1, "work_id": 1, "task_id": 1, "origin": "scenario", "plc_name": "GR2", "request": null, "resolved": null,
            "position": [0.0, 0.0, 0.0, 0.0], "plc_task": drop, "state": "queued", "state_at": "", "created_at": "", "submitted_at": null,
            "ended_at": null, "ack": null, "header": null, "plc": null, "error": null, "history": []
        }))
        .unwrap();
        let base = Some(crate::stock::StockEntry { cell_id: 101, item_code: 1001, count: 5, ..Default::default() });
        let p = crate::stock::project(101, base, std::slice::from_ref(&queued));
        assert_eq!(p.count(), 6);
        let s = p.projected.as_ref().unwrap();
        let mut r = req("PICK", "cell");
        r.count = 1;
        let c = compose_from(&d, &r, Some(cell()), Some(item()), Some((s.item_code, s.count)), None).unwrap();
        assert_eq!(c.task.position[2], 1500.0 + 5.0 * 240.0 + 120.0, "top tire of 6");
        // 표만 봤다면 5 개 중 맨 위 → 한 단 낮게 내려가 앞서 놓은 타이어에 부딪친다
        let stale = compose_from(&d, &r, Some(cell()), Some(item()), Some((1001, 5)), None).unwrap();
        assert_eq!(c.task.position[2] - stale.task.position[2], 240.0);
        // 끝나서 표에 접힌 뒤에는 대기 목록에서 빠진다 — 같은 6
        let mut done = queued;
        done.state = TaskState::Completed;
        let folded = Some(crate::stock::StockEntry { cell_id: 101, item_code: 1001, count: 6, ..Default::default() });
        assert_eq!(crate::stock::project(101, folded, &[done]).count(), 6);
    }

    #[test]
    fn warnings_and_measure_flag() {
        let d = defaults();
        let mut r = req("MEASURE", "cell");
        r.item_code = None;
        r.target = None;
        let c = compose_from(&d, &r, None, None, None, None).unwrap();
        assert!(c.params.measure_item);
        assert!(c.warnings.iter().any(|w| w.contains("target not set")));
        assert!(c.warnings.iter().any(|w| w.contains("item not set")));
        assert!(c.warnings.iter().any(|w| w.contains("MeasureItem assumed")));
        let c = compose_from(&d, &req("UP", "cell"), None, None, None, None).unwrap();
        assert!(c.warnings.is_empty());
        assert!(compose_from(&d, &req("FLY", "cell"), None, None, None, None).is_err());
    }

    #[test]
    fn stack_max_blocks_drop_unless_flagged() {
        let d = defaults();
        let spec = ItemSpec { stack_max: 4, ..Default::default() };
        let mut r = req("DROP", "cell");
        r.count = 1;
        // 3 + 1 = 4 → allowed, computed Z
        let c = compose_from(&d, &r, Some(cell()), Some(item()), Some((1001, 3)), Some(&spec)).unwrap();
        let l = c.stack_limit.clone().unwrap();
        assert_eq!((l.count_after, l.over_limit, l.blocked.is_none()), (4, false, true));
        assert_eq!(c.stack_z.as_ref().unwrap().z_source, "computed");
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        // 4 + 1 = 5 → blocked (warning announces the refusal)
        let c = compose_from(&d, &r, Some(cell()), Some(item()), Some((1001, 4)), Some(&spec)).unwrap();
        let l = c.stack_limit.clone().unwrap();
        assert!(l.over_limit && l.blocked.is_some() && !l.ignored);
        assert!(c.warnings.iter().any(|w| w.contains("제출 거부 예정") && w.contains("StackMax")), "{:?}", c.warnings);
        // stock unknown counts as empty: dropping 5 at once is still over
        r.count = 5;
        assert!(compose_from(&d, &r, Some(cell()), Some(item()), None, Some(&spec)).unwrap().stack_limit.unwrap().blocked.is_some());
        // flag → not blocked, marked ignored
        r.count = 1;
        r.ignore_stack_max = true;
        let c = compose_from(&d, &r, Some(cell()), Some(item()), Some((1001, 4)), Some(&spec)).unwrap();
        let l = c.stack_limit.unwrap();
        assert!(l.ignored && l.blocked.is_none());
        assert!(c.warnings.iter().any(|w| w.contains("무시")));
        // PICK from an over-full cell only warns; stations and unlimited items are not checked
        let c = compose_from(&d, &req("PICK", "cell"), Some(cell()), Some(item()), Some((1001, 6)), Some(&spec)).unwrap();
        assert!(c.stack_limit.as_ref().unwrap().over_limit && c.stack_limit.as_ref().unwrap().blocked.is_none());
        assert!(stack_limit(TaskType::Drop, "station", Some(&spec), Some(9), 1, false).is_none());
        assert!(stack_limit(TaskType::Drop, "cell", Some(&ItemSpec::default()), Some(9), 1, false).is_none());
        assert!(stack_limit(TaskType::Move, "cell", Some(&spec), Some(9), 1, false).is_none());
        // 잰 프로파일이 있으면 Z 를 그것이 끈다 — 4 단째에 놓으니 아래 3 개는 위 2·1·0 개를 인 상태
        let table = measured_spec(4, 4.0, 4);
        let c = compose_from(&d, &r, Some(cell()), Some(item()), Some((1001, 3)), Some(&table)).unwrap();
        let z = c.stack_z.unwrap();
        // 그립 기준이 mid 라 절대 프로파일 갈래가 아니라 파생 곡선으로 쌓는다 → 232 + 236 + 240
        assert_eq!((z.z_source, z.level, z.base), ("curve", 4, 232.0 + 236.0 + 240.0));
        assert_eq!(c.task.position[2], 1500.0 + 708.0 + 120.0);
        // position override: no stack Z audit, the limit still applies
        let mut o = req("DROP", "cell");
        o.count = 2;
        o.position_override = Some([1.0, 2.0, 3.0, 4.0]);
        let c = compose_from(&d, &o, Some(cell()), Some(item()), Some((1001, 3)), Some(&spec)).unwrap();
        assert!(c.stack_z.is_none() && c.stack_limit.unwrap().blocked.is_some());
    }

    #[test]
    fn pick_bead_and_compression_drive_compose_z() {
        let mut d = defaults();
        d.grip_ref = "pick_bead".into();
        let it = StockItem { lower_bid_height: 20.0, upper_bid_height: 220.0, ..item() };
        // 5 단 스택을 통째로 잰 품목 — 그 크기는 절대 프로파일을 환산 없이 쓴다
        let spec = measured_spec(5, 8.0, 5);
        // PICK 1 of 5 — 잡는 건 맨 위(5 단): 잰 절대 상부 비드 1100 − PickBeadOffset 30
        let mut r = req("PICK", "cell");
        r.count = 1;
        let c = compose_from(&d, &r, Some(cell()), Some(it.clone()), Some((1001, 5)), Some(&spec)).unwrap();
        let z = c.stack_z.clone().unwrap();
        assert_eq!((z.z_source, z.grip_ref, z.above, z.base), ("profile", "pick_bead", 0, 0.0));
        assert_eq!(z.upper_bead, Some(880.0 + 220.0), "잰 절대값 그대로");
        assert_eq!(c.task.position[2], 1500.0 + 1100.0 - 30.0);
        assert!(z.used.iter().any(|u| u.starts_with("grip=pick_bead abs(n=5,L5)")), "{:?}", z.used);
        // compose · /api/stock/z · 팔렛 계획이 같은 함수를 쓴다
        assert_eq!(c.task.position[2], stack_z_with(TaskType::Pick, 1500.0, &it, Some(&spec), "pick_bead", 5, 1).z);
        // 5개를 한 번에 = 맨 아래(1 단) → 잰 절대 비드 188, 그립 158
        r.count = 5;
        let c = compose_from(&d, &r, Some(cell()), Some(it.clone()), Some((1001, 5)), Some(&spec)).unwrap();
        assert_eq!(c.task.position[2], 1500.0 + 158.0);
        // 요청이 기본 그립 기준을 덮어쓴다
        r.grip_ref = Some("mid".into());
        let c = compose_from(&d, &r, Some(cell()), Some(it.clone()), Some((1001, 5)), Some(&spec)).unwrap();
        assert_eq!(c.task.position[2], 1500.0 + 120.0);
        // DROP 은 새 타이어 위에 아무것도 없으므로 눌림 없는 그립
        let mut o = req("DROP", "cell");
        o.count = 1;
        let c = compose_from(&d, &o, Some(cell()), Some(it.clone()), Some((1001, 5)), Some(&spec)).unwrap();
        assert_eq!(c.stack_z.as_ref().unwrap().grip, 190.0);
        assert_eq!(c.task.position[2], 1500.0 + 1120.0 + 190.0);
        // 눌림이 Height 를 다 먹으면 제한 경고가 compose 경고로 올라온다(잰 적 없는 품목의 대체 모형)
        r.grip_ref = None;
        r.count = 1;
        let hard = ItemSpec { stack_max: 5, compression: Some(200.0), ..Default::default() };
        let c = compose_from(&d, &r, Some(cell()), Some(it.clone()), Some((1001, 5)), Some(&hard)).unwrap();
        assert!(c.warnings.iter().any(|w| w.starts_with("Z:")), "{:?}", c.warnings);
        // 처음 재는 품목: bead+offset 을 시켜도 mid(Height/2) 로 잡고, 그 사실이 audit·경고에 남는다
        let fresh = ItemSpec { stack_max: 5, compression: Some(8.0), ..Default::default() };
        let c = compose_from(&d, &r, Some(cell()), Some(it.clone()), Some((1001, 5)), Some(&fresh)).unwrap();
        let z = c.stack_z.clone().unwrap();
        assert_eq!((z.grip_ref, z.grip, z.upper_bead), ("mid", 120.0, None));
        assert!(z.used.iter().any(|u| u.contains("grip=mid (측정 없음")), "{:?}", z.used);
        assert!(c.warnings.iter().any(|w| w.contains("측정된 비드가 없어")), "{:?}", c.warnings);
        // 잰 적 없는 크기(4 단)는 파생 곡선으로 환산한다 — audit 이 그렇게 말한다
        let mut q = req("PICK", "cell");
        q.count = 1;
        let c = compose_from(&d, &q, Some(cell()), Some(it.clone()), Some((1001, 4)), Some(&spec)).unwrap();
        let z = c.stack_z.clone().unwrap();
        assert_eq!((z.z_source, z.grip_ref), ("curve", "pick_bead"));
        assert!(z.used.iter().any(|u| u.contains("converted")), "{:?}", z.used);
        // /api/stock/z · 팔렛 계획과 같은 함수 — 세 갈래(프로파일·곡선·mid) 모두에서 값이 같다
        for s in [&spec, &fresh] {
            for (n, cnt) in [(5u32, 1u32), (5, 5), (3, 3)] {
                let mut q = req("PICK", "cell");
                q.count = cnt as u8;
                let got = compose_from(&d, &q, Some(cell()), Some(it.clone()), Some((1001, n)), Some(s)).unwrap();
                assert_eq!(got.task.position[2], stack_z_with(TaskType::Pick, 1500.0, &it, Some(s), "pick_bead", n, cnt).z);
            }
        }
    }
}
