//! 작업 작성(compose) 통합 — 켜진 팔렛 스테이션 대상 요청에 `pallet` 이 있으면 **품목의 패턴**으로
//! 슬롯 XY · 단 Z(기존 스택 규칙) · 드래그 방향(로봇 헤드 방향 보정 후)을 넣는다.
//!
//! - 입고 흐름(`in`) + PICK → UseDragIn, DragInDir = 슬롯 방향 바이트
//! - 출하 흐름(`out`) + DROP → UseDragOut, DragOutDir = 슬롯 방향 바이트
//! - 방향 0 인 슬롯은 드래그를 끈다(기본값에 드래그가 켜져 있어도). Dist/Height 는 기본값·params 그대로.
//! - 팔렛 스테이션에는 컨베이어 트래킹 보정(`station_offset`)을 쓰지 않는다 — 감사 기록에 `station_offset_skipped`.

use gr_proto::{StationPara, StockItem, TaskType};
use serde::{Deserialize, Serialize};

use super::profiles::{ItemPallet, Profiles, RobotDir};
use super::{AreaCheck, DragKind, GenInput, Library, area_check_xy, generate};
use crate::error::ApiError;
use crate::issue::Composed;
use crate::state::AppState;

/// `TaskRequest.pallet` — `{seq, level}` 또는 `{auto: true}`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PalletRef {
    /// 1-based 작업 순서(사양서 순서표의 순번).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u8>,
    /// 1-based 단(없으면 1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    /// 스테이션 재고 개수로 다음 순번·단을 고른다.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub auto: bool,
}

/// compose 결과와 원장에 남기는 팔렛 근거.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PalletAudit {
    pub station_id: u16,
    /// 패턴을 가진 품목(2026-09-21 이전 기록은 0).
    #[serde(default)]
    pub item_code: u32,
    /// 드래그 방향 보정을 가져온 로봇과 그 변환(없으면 0 · 변환 없음).
    #[serde(default)]
    pub robot: u8,
    #[serde(default)]
    pub dir_rotation: u16,
    #[serde(default)]
    pub dir_mirror_x: bool,
    #[serde(default)]
    pub dir_mirror_y: bool,
    pub flow: String,
    pub drag_kind: DragKind,
    pub pattern: u8,
    pub od: f32,
    pub gap: f32,
    pub pitch: f32,
    pub rotation: u16,
    pub mirror_x: bool,
    pub mirror_y: bool,
    pub pallet_size: f32,
    pub seq: u8,
    pub level: u32,
    pub slot: String,
    pub auto: bool,
    #[serde(default)]
    pub stock_used: Option<u32>,
    pub offset: [f32; 2],
    pub xy: [f32; 2],
    pub spec_drag_dir: u8,
    /// 품목 배치 변환만 건 방향(로봇 보정 전).
    #[serde(default)]
    pub layout_drag_dir: u8,
    pub drag_dir: u8,
    pub drag_type: u8,
    /// `in`(UseDragIn) · `out`(UseDragOut) · `none`
    pub drag_applied: String,
    /// 콘솔이 드래그 플래그를 정한 쪽 — 흐름과 작업 종류가 맞을 때만.
    #[serde(default)]
    pub drag_side: Option<DragKind>,
    /// 팔렛 스테이션에는 트래킹 보정을 쓰지 않는다(항상 true — 기록으로 남긴다).
    pub station_offset_skipped: bool,
    #[serde(default)]
    pub area: Option<AreaCheck>,
    #[serde(default)]
    pub warnings: Vec<String>,
    /// 슬롯을 만든 흐름·패턴의 편집 시각(`pallet_flow/pallet_pattern.updated_at`) — 나중에 패턴을 고쳐도
    /// 이 작업이 어느 판으로 만들어졌는지 남는다.
    #[serde(default)]
    pub flow_updated_at: String,
    #[serde(default)]
    pub pattern_updated_at: String,
}

pub struct Prepared {
    pub audit: PalletAudit,
    /// `stack_z_with` 에 넘길 스택 개수(단에서 거꾸로 만든 값).
    pub n_for_z: u32,
}

/// 재고 `stock` 개가 있을 때 다음 작업의 (단, 순번). 한 단 = 패턴 슬롯 수.
/// DROP 은 아래 단부터 순번대로 채우고, PICK/MEASURE 는 맨 윗단을 순번대로 비운다.
pub fn auto_ref(tt: TaskType, stock: u32, per_level: usize) -> Result<(u32, u8), String> {
    let k = per_level.max(1) as u32;
    match tt {
        TaskType::Drop => Ok((stock / k + 1, (stock % k + 1) as u8)),
        _ => {
            if stock == 0 {
                return Err("스테이션 재고가 0 입니다 — 집을 슬롯이 없습니다 (pallet.auto)".into());
            }
            let idx = stock - 1;
            let left_on_level = idx % k + 1;
            Ok((idx / k + 1, (k - left_on_level + 1) as u8))
        }
    }
}

/// 단 `level` 의 Z 에 쓸 스택 개수 — DROP: 아래 단 수, PICK/MEASURE: 아래 단 수 + 집는 개수.
pub fn level_stock(tt: TaskType, level: u32, count: u32) -> u32 {
    let below = level.max(1) - 1;
    match tt {
        TaskType::Drop => below,
        _ => below + count.max(1),
    }
}

/// 팔렛 스테이션으로 켜져 있나.
pub fn is_pallet_station(st: &AppState, station_id: u16) -> Result<bool, ApiError> {
    Ok(Profiles::new(st.db.clone()).station(station_id)?.is_some_and(|s| s.enabled))
}

/// 순수 부분 — 패턴 저장소(`lib`)·품목 팔렛 설정·로봇 방향·요청·품목으로 슬롯을 정한다. `station_id` 는 호출자가 채운다.
#[allow(clippy::too_many_arguments)]
pub fn prepare_pure(lib: &Library, ip: &ItemPallet, robot: &RobotDir, pref: &PalletRef, tt: TaskType, item: &StockItem, count: u32, center: [f32; 2], stock: Option<u32>) -> Result<Prepared, String> {
    if !matches!(tt, TaskType::Pick | TaskType::Drop | TaskType::Measure) {
        return Err(format!("pallet 은 PICK/DROP/MEASURE 에만 씁니다 ({})", tt.name()));
    }
    let od = item.outer_diameter;
    if od.is_nan() || od <= 0.0 {
        return Err(format!("품목 {} 의 OuterDiameter 가 없어 팔렛 패턴을 고를 수 없습니다", item.code));
    }
    let flow = ip.flow_for(tt).ok_or_else(|| format!("품목 {} 팔렛 설정에 Flow 가 없습니다", item.code))?;
    let g = generate(lib, &GenInput { flow, pattern: ip.pattern, od, gap: ip.gap, transform: ip.transform(), center, pallet_size: ip.pallet_size, dir_transform: robot.transform() })?;
    let k = g.slots.len();
    let (level, seq, stock_used) = if pref.auto {
        let n = stock.unwrap_or(0);
        let (l, s) = auto_ref(tt, n, k)?;
        (l, s, Some(n))
    } else {
        let s = pref.seq.ok_or("pallet.seq 가 없습니다 — seq(1..) 를 주거나 auto: true")?;
        (pref.level.unwrap_or(1).max(1) as u32, s, None)
    };
    let Some(slot) = g.slots.iter().find(|s| s.seq == seq) else {
        return Err(format!("pallet.seq {seq} 는 Pattern {} 의 1..{k} 밖입니다", g.pattern));
    };
    let side = match (tt, g.drag_kind) {
        (TaskType::Pick, DragKind::In) => Some(DragKind::In),
        (TaskType::Drop, DragKind::Out) => Some(DragKind::Out),
        _ => None,
    };
    let mut warnings: Vec<String> = g.errors.clone();
    let own = format!("{} ", slot.slot);
    warnings.extend(g.warnings.iter().filter(|w| !w.contains("팔렛 밖") || w.starts_with(&own)).cloned());
    if side.is_none() && tt != TaskType::Measure {
        warnings.push(format!("Flow {} 는 {} 기준인데 작업이 {} — 드래그를 넣지 않았습니다", g.flow, g.drag_kind.task_type().name(), tt.name()));
    }
    let drag_applied = match side {
        Some(k) if slot.drag_dir != 0 => k.as_str(),
        _ => "none",
    };
    let audit = PalletAudit {
        station_id: 0,
        item_code: item.code,
        robot: robot.robot,
        dir_rotation: robot.rotation,
        dir_mirror_x: robot.mirror_x,
        dir_mirror_y: robot.mirror_y,
        flow: g.flow.clone(),
        drag_kind: g.drag_kind,
        pattern: g.pattern,
        od,
        gap: g.gap,
        pitch: g.pitch,
        rotation: g.transform.rotation,
        mirror_x: g.transform.mirror_x,
        mirror_y: g.transform.mirror_y,
        pallet_size: g.pallet_size,
        seq,
        level,
        slot: slot.slot.clone(),
        auto: pref.auto,
        stock_used,
        offset: [slot.offset_x, slot.offset_y],
        xy: [slot.x, slot.y],
        spec_drag_dir: slot.spec_drag_dir,
        layout_drag_dir: slot.layout_drag_dir,
        drag_dir: slot.drag_dir,
        drag_type: slot.drag_type,
        drag_applied: drag_applied.into(),
        drag_side: side,
        station_offset_skipped: true,
        area: None,
        warnings,
        flow_updated_at: g.flow_updated_at.clone(),
        pattern_updated_at: g.pattern_updated_at.clone(),
    };
    Ok(Prepared { audit, n_for_z: level_stock(tt, level, count) })
}

/// compose 결과에 슬롯 XY 와 드래그를 얹는다(Z 는 `n_for_z` 로 이미 계산됨).
pub fn apply_to(c: &mut Composed, audit: PalletAudit, has_override: bool) {
    let mut audit = audit;
    if has_override {
        audit.warnings.push("position_override 가 있어 팔렛 슬롯 XY 대신 그 위치를 씁니다".into());
    } else {
        c.task.position[0] = audit.xy[0];
        c.task.position[1] = audit.xy[1];
    }
    match audit.drag_side {
        Some(DragKind::In) => {
            c.params.use_drag_in = audit.drag_dir != 0;
            c.params.drag_in_dir = audit.drag_type;
            // 거리를 아무도 안 정했으면 기본 150 mm(운전자 결정 2026-09-18) — `issue::compose_from` 과 같은 값.
            if c.params.use_drag_in && c.params.drag_in_dist == 0 {
                c.params.drag_in_dist = gr_proto::DEFAULT_DRAG_DIST;
            }
        }
        Some(DragKind::Out) => {
            c.params.use_drag_out = audit.drag_dir != 0;
            c.params.drag_out_dir = audit.drag_type;
            if c.params.use_drag_out && c.params.drag_out_dist == 0 {
                c.params.drag_out_dist = gr_proto::DEFAULT_DRAG_DIST;
            }
        }
        None => {}
    }
    c.params.apply(&mut c.task);
    c.warnings.extend(audit.warnings.iter().map(|w| format!("팔렛: {w}")));
    c.pallet = Some(audit);
}

/// GR2 `isValidTaskArea` 선검사 — 최종 XY 로. 트래킹 보정 감사가 빠지므로 여기서 대신 예고한다.
pub fn attach_area(st: &AppState, robot: Option<u8>, station_id: u16, para: &StationPara, c: &mut Composed) {
    let Some(a) = c.pallet.as_mut() else { return };
    let (gr2, note) = crate::issue::gr2_station(st, robot, station_id, para);
    if let Some(n) = note {
        c.warnings.push(format!("팔렛: {n}"));
        a.warnings.push(n);
    }
    let Some(g) = gr2 else { return };
    let chk = area_check_xy(&[(a.seq, a.slot.clone(), [c.task.position[0], c.task.position[1]])], &g, station_id, a.od);
    for r in &chk.rejects {
        let w = format!("GR2 isValidTaskArea {}: |Δ| {} ≥ 마진 {} → GR2 거부 {} 예상", r.axis, r.delta, chk.margin, r.code);
        c.warnings.push(format!("팔렛: {w}"));
        a.warnings.push(w);
    }
    a.area = Some(chk);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::issue::{compose_from, parse_task_type};
    use crate::ledger::{Target, TaskRequest};
    use crate::registry::Defaults;
    use gr_proto::CellInfo;
    use serde_json::json;

    fn item() -> StockItem {
        StockItem { code: 1001, count: 1, inner_diameter: 381.0, outer_diameter: 780.0, height: 240.0, ..Default::default() }
    }
    fn profile(flow: &str) -> ItemPallet {
        ItemPallet { code: 1001, flow_in: Some(flow.into()), flow_out: Some(flow.into()), ..Default::default() }
    }
    fn station_cell() -> CellInfo {
        CellInfo { use_: true, id: 2021, section: 3, position: [11_200.0, 3_800.0, 100.0], ..Default::default() }
    }

    fn lib() -> Library {
        Library::from_seed(crate::pallet::seed())
    }

    fn composed(tt: &str, flow: &str, pref: PalletRef) -> Composed {
        composed_with(&lib(), tt, flow, pref)
    }

    fn composed_with(lib: &Library, tt: &str, flow: &str, pref: PalletRef) -> Composed {
        composed_base(lib, tt, flow, pref, gr_proto::TaskParams { drag_in_dist: 120, drag_in_height: 30, drag_out_dist: 150, ..Default::default() })
    }

    fn composed_base(lib: &Library, tt: &str, flow: &str, pref: PalletRef, base: gr_proto::TaskParams) -> Composed {
        let d = Defaults { grip_ref: "mid".into(), base, ..Default::default() };
        let req = TaskRequest {
            task_type: tt.into(),
            target: Some(Target { kind: "station".into(), id: 2021 }),
            item_code: Some(1001),
            count: 1,
            params: json!({}),
            pallet: Some(pref.clone()),
            ..Default::default()
        };
        let t = parse_task_type(tt).unwrap();
        let mut pr = prepare_pure(lib, &profile(flow), &RobotDir::default(), &pref, t, &item(), 1, [11_200.0, 3_800.0], None).unwrap();
        pr.audit.station_id = 2021;
        let mut c = compose_from(&d, &req, Some(station_cell()), Some(item()), Some((0, pr.n_for_z)), None).unwrap();
        apply_to(&mut c, pr.audit, false);
        c
    }

    /// 드래그 거리를 아무도 안 정하면 기본 150 mm — 옛 "Dist = 0 경고" 자리는 사라진다(결정 2026-09-18).
    #[test]
    fn drag_distance_defaults_to_150() {
        assert_eq!(gr_proto::DEFAULT_DRAG_DIST, 150);
        assert_eq!((gr_proto::TaskParams::default().drag_in_dist, gr_proto::TaskParams::default().drag_out_dist), (150, 150));
        // 기본값 문서에 0 이 박혀 있어도(옛 저장본) 팔렛 compose 가 150 을 넣는다
        let zero = gr_proto::TaskParams { drag_in_dist: 0, drag_out_dist: 0, drag_in_height: 30, ..Default::default() };
        let c = composed_base(&lib(), "PICK", "HP_IN", PalletRef { seq: Some(2), level: Some(2), auto: false }, zero.clone());
        assert!(c.task.use_drag_in);
        assert_eq!(c.task.drag_in_dist, 150);
        assert!(!c.warnings.iter().any(|w| w.contains("DragInDist")), "{:?}", c.warnings);
        let c = composed_base(&lib(), "DROP", "OP_OUT", PalletRef { seq: Some(1), level: Some(1), auto: false }, zero);
        assert!(c.task.use_drag_out);
        assert_eq!(c.task.drag_out_dist, 150);
        assert!(!c.warnings.iter().any(|w| w.contains("DragOutDist")), "{:?}", c.warnings);
    }

    #[test]
    fn auto_next_slot() {
        assert_eq!(auto_ref(TaskType::Drop, 0, 3), Ok((1, 1)));
        assert_eq!(auto_ref(TaskType::Drop, 2, 3), Ok((1, 3)));
        assert_eq!(auto_ref(TaskType::Drop, 3, 3), Ok((2, 1)));
        assert_eq!(auto_ref(TaskType::Pick, 6, 3), Ok((2, 1)));
        assert_eq!(auto_ref(TaskType::Pick, 5, 3), Ok((2, 2)));
        assert_eq!(auto_ref(TaskType::Pick, 4, 3), Ok((2, 3)));
        assert_eq!(auto_ref(TaskType::Pick, 3, 3), Ok((1, 1)));
        assert!(auto_ref(TaskType::Pick, 0, 3).is_err());
        assert_eq!((level_stock(TaskType::Drop, 1, 1), level_stock(TaskType::Drop, 3, 1), level_stock(TaskType::Pick, 1, 1), level_stock(TaskType::Pick, 3, 2)), (0, 2, 1, 4));
    }

    #[test]
    fn pick_on_inbound_sets_drag_in() {
        let c = composed("PICK", "HP_IN", PalletRef { seq: Some(2), level: Some(2), auto: false });
        let a = c.pallet.clone().unwrap();
        assert_eq!((a.pattern, a.slot.as_str(), a.drag_dir, a.drag_type, a.drag_applied.as_str()), (3, "C#2", 3, 4, "in"));
        assert_eq!([c.task.position[0], c.task.position[1]], a.xy);
        assert_eq!(a.xy[0], super::super::r1(11_200.0 + a.offset[0]));
        assert!(c.task.use_drag_in && !c.task.use_drag_out);
        assert_eq!((c.task.drag_in_dir, c.task.drag_in_dist, c.task.drag_in_height), (4, 120, 30));
        // 2단 PICK: 아래 1개 + 집는 타이어 중간
        assert_eq!(c.task.position[2], 100.0 + 240.0 + 120.0);
        assert!(a.station_offset_skipped);
        // 방향 없는 슬롯은 드래그를 끈다
        let c = composed("PICK", "HP_IN", PalletRef { seq: Some(1), ..Default::default() });
        assert!(!c.task.use_drag_in);
        assert_eq!(c.task.drag_in_dir, 0);
    }

    /// 같은 품목 패턴 — 좌표는 로봇과 무관, 드래그 방향만 로봇 헤드 방향만큼 돈다.
    #[test]
    fn robot_head_turns_only_the_drag_direction() {
        let pref = PalletRef { seq: Some(2), level: Some(1), auto: false };
        let run = |r: &RobotDir| prepare_pure(&lib(), &profile("HP_IN"), r, &pref, TaskType::Pick, &item(), 1, [11_200.0, 3_800.0], None).unwrap().audit;
        let gr1 = run(&RobotDir { robot: 1, ..Default::default() });
        let gr2 = run(&RobotDir { robot: 2, rotation: 180, ..Default::default() });
        assert_eq!(gr1.xy, gr2.xy, "좌표는 GR1·GR2 공통");
        assert_eq!((gr1.layout_drag_dir, gr1.drag_dir, gr1.drag_type), (3, 3, 4));
        // 3 (X+Y+) → 180° → 6 (X−Y−)
        assert_eq!((gr2.layout_drag_dir, gr2.drag_dir, gr2.drag_type), (3, 6, 32));
        assert_eq!((gr2.robot, gr2.dir_rotation, gr2.item_code), (2, 180, 1001));
    }

    /// 품목에 Pattern 을 고정하면 OD 자동 선택 대신 그 번호.
    #[test]
    fn item_pattern_overrides_od() {
        let ip = ItemPallet { pattern: Some(4), ..profile("HP_IN") };
        let a = prepare_pure(&lib(), &ip, &RobotDir::default(), &PalletRef { seq: Some(1), ..Default::default() }, TaskType::Pick, &item(), 1, [0.0, 0.0], None).unwrap().audit;
        assert_eq!(a.pattern, 4);
        let a = prepare_pure(&lib(), &profile("HP_IN"), &RobotDir::default(), &PalletRef { seq: Some(1), ..Default::default() }, TaskType::Pick, &item(), 1, [0.0, 0.0], None).unwrap().audit;
        assert_eq!(a.pattern, 3, "OD 780 → P3");
    }

    #[test]
    fn drop_on_outbound_sets_drag_out() {
        let c = composed("DROP", "OP_OUT", PalletRef { seq: Some(1), level: Some(1), auto: false });
        let a = c.pallet.clone().unwrap();
        // OP 출하 P3 수정 순서: 1번 = C#3, 방향 7
        assert_eq!((a.slot.as_str(), a.drag_dir, a.drag_type), ("C#3", 7, 64));
        assert!(c.task.use_drag_out && !c.task.use_drag_in);
        assert_eq!((c.task.drag_out_dir, c.task.drag_out_dist), (64, 150));
        assert_eq!(c.task.position[2], 100.0 + 120.0);
        // 출하 흐름에 PICK: 자리는 쓰고 드래그는 넣지 않는다
        let c = composed("PICK", "OP_OUT", PalletRef { seq: Some(1), ..Default::default() });
        assert_eq!(c.pallet.as_ref().unwrap().drag_applied, "none");
        assert!(!c.task.use_drag_in && !c.task.use_drag_out);
        assert!(c.warnings.iter().any(|w| w.contains("드래그를 넣지 않았습니다")), "{:?}", c.warnings);
    }

    #[test]
    fn rejects_bad_refs_and_resolves_auto() {
        let l = lib();
        let p = profile("HP_IN");
        let one = PalletRef { seq: Some(1), ..Default::default() };
        assert!(prepare_pure(&l, &p, &RobotDir::default(), &PalletRef { seq: Some(4), ..Default::default() }, TaskType::Pick, &item(), 1, [0.0, 0.0], None).is_err(), "P3 has 3 slots");
        assert!(prepare_pure(&l, &p, &RobotDir::default(), &PalletRef::default(), TaskType::Pick, &item(), 1, [0.0, 0.0], None).is_err(), "seq or auto required");
        assert!(prepare_pure(&l, &p, &RobotDir::default(), &one, TaskType::Move, &item(), 1, [0.0, 0.0], None).is_err());
        let no_od = StockItem { outer_diameter: 0.0, ..item() };
        assert!(prepare_pure(&l, &p, &RobotDir::default(), &one, TaskType::Pick, &no_od, 1, [0.0, 0.0], None).is_err());
        let a = prepare_pure(&l, &p, &RobotDir::default(), &PalletRef { auto: true, ..Default::default() }, TaskType::Drop, &item(), 1, [0.0, 0.0], Some(4)).unwrap();
        assert_eq!((a.audit.level, a.audit.seq, a.audit.stock_used, a.n_for_z), (2, 2, Some(4), 1));
        // a flow deleted from the store is refused even if the profile still names it
        let mut gone = l.clone();
        gone.flows.retain(|f| f.id != "HP_IN");
        assert!(matches!(prepare_pure(&gone, &p, &RobotDir::default(), &one, TaskType::Pick, &item(), 1, [0.0, 0.0], None), Err(e) if e.contains("unknown Flow")));
    }

    #[test]
    fn audit_records_edited_pattern_and_times() {
        let mut l = lib();
        let f = l.flows.iter_mut().find(|f| f.id == "HP_IN").unwrap();
        f.updated_at = "2026-09-15T09:00:00+09:00".into();
        let p3 = f.patterns.iter_mut().find(|p| p.pattern == 3).unwrap();
        p3.updated_at = "2026-09-15T09:30:00+09:00".into();
        p3.slots.iter_mut().find(|s| s.slot == "C#2").unwrap().drag_dir = 5;
        let c = composed_with(&l, "PICK", "HP_IN", PalletRef { seq: Some(2), level: Some(1), auto: false });
        let a = c.pallet.clone().unwrap();
        assert_eq!((a.slot.as_str(), a.drag_dir, a.drag_type), ("C#2", 5, 0x10));
        assert_eq!(c.task.drag_in_dir, 0x10);
        assert_eq!((a.flow_updated_at.as_str(), a.pattern_updated_at.as_str()), ("2026-09-15T09:00:00+09:00", "2026-09-15T09:30:00+09:00"));
        // old ledger rows without the fields still deserialize
        let mut v = serde_json::to_value(&a).unwrap();
        v.as_object_mut().unwrap().remove("flow_updated_at");
        v.as_object_mut().unwrap().remove("pattern_updated_at");
        let back: PalletAudit = serde_json::from_value(v).unwrap();
        assert_eq!(back.pattern_updated_at, "");
    }
}
