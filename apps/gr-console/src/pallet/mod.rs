//! 팔렛타이징 패턴 — 사양서 R4(`spec_r4.json`, `tools/pallet/gen_spec.mjs` 생성물)의 정규화 좌표·순서·드래그
//! 방향으로 슬롯 위치를 만든다.
//!
//! 좌표는 전부 **기계 축**(GR X/Y)이다. 사양서 슬라이드는 화면 방향이 기계 축과 반대로 그려져 있어
//! 생성기가 이미 기계 축으로 바꿔 두었다(`screen_axes`). 드래그 방향 1..8 도 기계 축 코드이며 PLC FC
//! `DragDelta` 의 비트(`DRAGTYPE_n = 1 << (n-1)`)와 같다:
//!
//! ```text
//!   1 = X+ Y−   2 = X+   3 = X+ Y+
//!   4 = Y−               5 = Y+
//!   6 = X− Y−   7 = X−   8 = X− Y+
//! ```
//!
//! 실제 오프셋 = 정규화 좌표 `u` × (OuterDiameter + Gap) / MinDistance(u). 사양서 패턴은 MinDistance = 1 이라
//! `u × (OuterDiameter + Gap)` 과 같고, 운전자가 슬롯을 옮겨 MinDistance 가 1 에서 벗어나도 가장 가까운 두 타이어의
//! 중심거리가 늘 OuterDiameter + Gap 이 되게 맞춘다. 회전·미러는 오프셋과 드래그 방향을 **같은 변환**으로
//! 돌린다 — 둘이 어긋나면 드래그가 이웃 타이어 쪽으로 들어간다.
//!
//! 패턴 데이터는 편집 가능한 저장소(`store.rs`, sqlite)에서 읽는다. 내장 JSON(`seed()`)은 첫 기동 채우기·초기화·
//! 비교 기준으로만 쓴다. 생성기는 순수 함수라 테스트는 `Library::from_seed` 로 sqlite 없이 돈다.

pub mod compose;
pub mod edit;
pub mod profiles;
pub mod routes;
pub mod store;

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::issue::station_offset::{Gr2Station, MARGIN_PLAIN, MARGIN_TASKTYPE, gr2_center_offset};

/// 저장소 흐름의 출처.
pub const SOURCE_SPEC: &str = "spec_r4";
pub const SOURCE_CUSTOM: &str = "custom";
pub const SOURCE_IMPORT: &str = "import";
/// 외경 경계가 정수로 적혀 있어(813 / 814) 이 폭 이하의 틈은 이어진 것으로 본다(mm).
pub const OD_BOUNDARY_TOL: f32 = 1.0;

/// 운전자가 따로 주지 않을 때의 타이어 간격(mm).
pub const DEFAULT_GAP: f32 = 50.0;
pub const DEFAULT_PALLET_SIZE: f32 = 1600.0;
/// 기본 흐름(프로파일·요청이 없을 때).
pub const DEFAULT_FLOW: &str = "HP_IN";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DragKind {
    /// 입고 — 로봇이 팔렛에서 집는다(PICK, Drag-in @Pick).
    In,
    /// 출하 — 로봇이 팔렛에 놓는다(DROP, Drag-out @Drop).
    Out,
}

impl DragKind {
    pub fn task_type(self) -> gr_proto::TaskType {
        match self {
            DragKind::In => gr_proto::TaskType::Pick,
            DragKind::Out => gr_proto::TaskType::Drop,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            DragKind::In => "in",
            DragKind::Out => "out",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SizeClass {
    pub pattern: u8,
    pub od_min: f32,
    pub od_max: f32,
}

/// 슬라이드에 그려진 치수(mm) — 정규화 근거(참고값, 생성에는 쓰지 않는다).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Drawn {
    pub d_mm: f32,
    pub min_dist_mm: f32,
    pub gap_mm: f32,
    pub centroid_mm: [f32; 2],
    pub centered: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpecSlot {
    pub slot: String,
    pub seq: u8,
    pub u: [f32; 2],
    pub drag_dir: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpecPattern {
    pub pattern: u8,
    pub od_min: f32,
    pub od_max: f32,
    #[serde(default)]
    pub order_text: String,
    #[serde(default)]
    pub drawn: Drawn,
    pub slots: Vec<SpecSlot>,
    /// 사양서 전사 메모(읽기 전용 참고).
    #[serde(default)]
    pub notes: Vec<String>,
    /// 운전자 메모(편집).
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenAxes {
    pub right: String,
    pub down: String,
}

impl Default for ScreenAxes {
    /// 기계 축 보기(X+ 오른쪽 · Y+ 위 = 화면 아래가 Y−).
    fn default() -> Self {
        ScreenAxes { right: "X+".into(), down: "Y-".into() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Flow {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub source_slide: u8,
    #[serde(default)]
    pub also_slides: Vec<u8>,
    pub drag_kind: DragKind,
    #[serde(default)]
    pub reference: bool,
    #[serde(default)]
    pub robot: String,
    #[serde(default)]
    pub zone: String,
    #[serde(default)]
    pub screen_axes: ScreenAxes,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub patterns: Vec<SpecPattern>,
    /// 저장소 출처 — `spec_r4` · `custom` · `import`(내장 JSON 에는 없다).
    #[serde(default)]
    pub source: String,
    /// 초기화·비교 기준인 사양서 흐름 id.
    #[serde(default)]
    pub based_on: Option<String>,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub updated_at: String,
}

impl Flow {
    pub fn pattern(&self, p: u8) -> Option<&SpecPattern> {
        self.patterns.iter().find(|x| x.pattern == p)
    }

    pub fn max_od(&self) -> f32 {
        self.patterns.iter().map(|c| c.od_max).fold(0.0, f32::max)
    }

    /// 외경으로 패턴을 고른다 — "od_min 이상인 가장 큰 패턴". 그 패턴의 od_max 를 넘으면 다음 패턴과의 틈이
    /// [`OD_BOUNDARY_TOL`] 이하일 때만(정수 경계 813 / 814) 그 패턴으로 보고, 더 넓은 틈은 "no pattern for OD" 오류다.
    pub fn pattern_for_od(&self, od: f32) -> Result<u8, String> {
        if !od.is_finite() || od <= 0.0 {
            return Err(format!("OuterDiameter {od} must be > 0"));
        }
        let mut ps: Vec<&SpecPattern> = self.patterns.iter().collect();
        if ps.is_empty() {
            return Err(format!("Flow {} 에 Pattern 이 없습니다", self.id));
        }
        ps.sort_by(|a, b| a.od_min.total_cmp(&b.od_min));
        let Some(i) = ps.iter().rposition(|p| od >= p.od_min) else {
            return Err(format!("no pattern for OD {od} — Flow {} 의 가장 작은 OdMin 은 {} 입니다", self.id, ps[0].od_min));
        };
        let p = ps[i];
        if od <= p.od_max {
            return Ok(p.pattern);
        }
        match ps.get(i + 1) {
            Some(n) if n.od_min - p.od_max <= OD_BOUNDARY_TOL => Ok(p.pattern),
            Some(n) => Err(format!("no pattern for OD {od} — Flow {} 의 P{} OdMax {} 와 P{} OdMin {} 사이가 비어 있습니다", self.id, p.pattern, p.od_max, n.pattern, n.od_min)),
            None => Err(format!("no pattern for OD {od} — Flow {} 의 상한은 {} 입니다", self.id, p.od_max)),
        }
    }
}

fn default_pallet_size() -> f32 {
    DEFAULT_PALLET_SIZE
}

/// 사양 문서 — 내장 `spec_r4.json` 과 내보내기/가져오기(`/api/pallet/export.json`)가 같은 모양이다.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Spec {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub generator: String,
    #[serde(default = "default_pallet_size")]
    pub pallet_size_mm: f32,
    #[serde(default)]
    pub normalization: String,
    #[serde(default)]
    pub size_classes: Vec<SizeClass>,
    #[serde(default)]
    pub size_class_notes: Vec<String>,
    pub flows: Vec<Flow>,
}

impl Spec {
    pub fn flow(&self, id: &str) -> Option<&Flow> {
        self.flows.iter().find(|f| f.id.eq_ignore_ascii_case(id.trim()))
    }
}

/// 내장 사양(공장 기준본) — 첫 기동 채우기·초기화·비교에만 쓴다. 잘못된 JSON 은 테스트가 먼저 잡는다.
pub fn seed() -> &'static Spec {
    static S: OnceLock<Spec> = OnceLock::new();
    S.get_or_init(|| serde_json::from_str(include_str!("spec_r4.json")).expect("pallet/spec_r4.json"))
}

/// 생성기가 읽는 패턴 모음(메모리) — 저장소에서 읽거나(`store::PalletStore::library`) 내장 사양으로 만든다.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Library {
    pub flows: Vec<Flow>,
}

impl Library {
    /// 내장 사양 그대로 — 저장소 첫 채우기와 sqlite 없는 테스트용.
    pub fn from_seed(sp: &Spec) -> Library {
        Library { flows: sp.flows.iter().map(|f| Flow { source: SOURCE_SPEC.into(), based_on: Some(f.id.clone()), ..f.clone() }).collect() }
    }

    pub fn flow(&self, id: &str) -> Option<&Flow> {
        self.flows.iter().find(|f| f.id.eq_ignore_ascii_case(id.trim()))
    }

    pub fn ids(&self) -> String {
        self.flows.iter().map(|f| f.id.as_str()).collect::<Vec<_>>().join(", ")
    }
}

/// 정규화 좌표의 최소 중심거리 — 슬롯이 둘 미만이면 `None`.
pub fn norm_min_distance(slots: &[SpecSlot]) -> Option<f32> {
    let mut m = f32::INFINITY;
    for i in 0..slots.len() {
        for j in i + 1..slots.len() {
            m = m.min((slots[i].u[0] - slots[j].u[0]).hypot(slots[i].u[1] - slots[j].u[1]));
        }
    }
    m.is_finite().then_some(m)
}

// ── 드래그 방향 ─────────────────────────────────────────────────────────────

/// 방향 코드 → 기계 축 부호(X, Y). `DragDelta` 의 CASE 표와 같다.
pub fn dir_vec(d: u8) -> Option<(i8, i8)> {
    Some(match d {
        1 => (1, -1),
        2 => (1, 0),
        3 => (1, 1),
        4 => (0, -1),
        5 => (0, 1),
        6 => (-1, -1),
        7 => (-1, 0),
        8 => (-1, 1),
        _ => return None,
    })
}

pub fn dir_from_vec(v: (i8, i8)) -> u8 {
    (1..=8).find(|d| dir_vec(*d) == Some(v)).unwrap_or(0)
}

/// PLC `DragInDir`/`DragOutDir` 바이트(`LGR_Const_Task_DragType`) — 0 = 드래그 없음.
pub fn drag_type_byte(d: u8) -> u8 {
    if (1..=8).contains(&d) { 1 << (d - 1) } else { 0 }
}

// ── 변환 ────────────────────────────────────────────────────────────────────

/// 팔렛 배치 변환. 순서: MirrorX(X → −X) · MirrorY(Y → −Y) 먼저, 그다음 Rotation(X+ 에서 Y+ 쪽으로 도는 방향).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transform {
    pub rotation: u16,
    pub mirror_x: bool,
    pub mirror_y: bool,
}

impl Transform {
    pub fn validate(&self) -> Result<(), String> {
        match self.rotation {
            0 | 90 | 180 | 270 => Ok(()),
            r => Err(format!("Rotation {r} must be 0, 90, 180 or 270")),
        }
    }
    fn quarter(&self) -> u8 {
        (self.rotation / 90 % 4) as u8
    }
    pub fn apply(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        let x = if self.mirror_x { -x } else { x };
        let y = if self.mirror_y { -y } else { y };
        match self.quarter() {
            1 => [-y, x],
            2 => [-x, -y],
            3 => [y, -x],
            _ => [x, y],
        }
    }
    pub fn apply_dir(&self, d: u8) -> u8 {
        let Some((x, y)) = dir_vec(d) else { return 0 };
        let x = if self.mirror_x { -x } else { x };
        let y = if self.mirror_y { -y } else { y };
        dir_from_vec(match self.quarter() {
            1 => (-y, x),
            2 => (-x, -y),
            3 => (y, -x),
            _ => (x, y),
        })
    }
}

// ── 생성 ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct GenInput<'a> {
    pub flow: &'a str,
    /// `None` = 외경으로 자동.
    pub pattern: Option<u8>,
    pub od: f32,
    pub gap: f32,
    pub transform: Transform,
    /// 팔렛 중심(기계 좌표) — 스테이션 Info.Position 또는 수동 입력.
    pub center: [f32; 2],
    pub pallet_size: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Slot {
    pub seq: u8,
    pub slot: String,
    /// 변환 뒤 정규화 좌표.
    pub u: [f32; 2],
    pub offset_x: f32,
    pub offset_y: f32,
    pub x: f32,
    pub y: f32,
    /// 사양서 그대로의 방향(변환 전).
    pub spec_drag_dir: u8,
    pub drag_dir: u8,
    pub drag_type: u8,
    /// 팔렛 밖으로 나간 길이(mm, X·Y) — 0 이하면 안.
    pub overhang: [f32; 2],
}

#[derive(Clone, Debug, Serialize)]
pub struct Generated {
    pub flow: String,
    pub flow_name: String,
    pub drag_kind: DragKind,
    pub reference: bool,
    pub pattern: u8,
    pub pattern_auto: bool,
    pub od_range: [f32; 2],
    pub od: f32,
    pub gap: f32,
    pub pitch: f32,
    pub transform: Transform,
    pub center: [f32; 2],
    pub pallet_size: f32,
    pub min_distance: f32,
    pub slots: Vec<Slot>,
    pub warnings: Vec<String>,
    /// 배치가 물리적으로 안 되는 것(겹침).
    pub errors: Vec<String>,
    pub drawn: Drawn,
    pub notes: Vec<String>,
    /// 편집 좌표의 최소 중심거리(정규화 단위, 슬롯 1개면 0) — 사양서 패턴은 1.
    pub min_distance_norm: f32,
    /// 오프셋 배율 = 1 / min_distance_norm (슬롯 1개면 1).
    pub scale: f32,
    pub flow_source: String,
    pub based_on: Option<String>,
    pub flow_updated_at: String,
    pub pattern_updated_at: String,
    pub pattern_note: String,
}

/// 0.1 mm 반올림(−0 은 0 으로 — JSON 에 `-0.0` 이 보이지 않게).
pub fn r1(v: f32) -> f32 {
    let r = (v * 10.0).round() / 10.0;
    if r == 0.0 { 0.0 } else { r }
}
fn r4(v: f32) -> f32 {
    let r = (v * 10_000.0).round() / 10_000.0;
    if r == 0.0 { 0.0 } else { r }
}

/// 순수 생성기 — 흐름·패턴 오류는 `Err`, 배치 문제(겹침·팔렛 밖)는 결과의 `errors`/`warnings`.
pub fn generate(lib: &Library, inp: &GenInput) -> Result<Generated, String> {
    let flow = lib.flow(inp.flow).ok_or_else(|| format!("unknown Flow {} (available: {})", inp.flow, lib.ids()))?;
    if !inp.od.is_finite() || inp.od <= 0.0 {
        return Err(format!("OuterDiameter {} must be > 0", inp.od));
    }
    if !inp.gap.is_finite() {
        return Err("Gap must be a number".into());
    }
    if !inp.pallet_size.is_finite() || inp.pallet_size <= 0.0 {
        return Err(format!("PalletSize {} must be > 0", inp.pallet_size));
    }
    inp.transform.validate()?;
    let auto = inp.pattern.is_none();
    let no = match inp.pattern {
        Some(p) => p,
        None => flow.pattern_for_od(inp.od)?,
    };
    let pat =
        flow.pattern(no).ok_or_else(|| format!("Flow {} 에 Pattern {no} 이 없습니다 (있는 것: {})", flow.id, flow.patterns.iter().map(|p| p.pattern.to_string()).collect::<Vec<_>>().join(", ")))?;
    if pat.slots.is_empty() {
        return Err(format!("Flow {} Pattern {no} 에 슬롯이 없습니다", flow.id));
    }

    let pitch = inp.od + inp.gap;
    // 편집으로 최소 중심거리가 1 에서 벗어나도 가장 가까운 두 타이어의 중심거리 = OuterDiameter + Gap.
    let min_norm = norm_min_distance(&pat.slots).filter(|d| *d > 1e-6);
    let scale = min_norm.map(|d| 1.0 / d).unwrap_or(1.0);
    let k = pitch * scale;
    let half = inp.pallet_size / 2.0;
    let mut warnings = Vec::new();
    let mut errors = Vec::new();
    if flow.reference {
        warnings.push(format!("Flow {} 는 사양서의 흐린 참고본입니다 — 현장 적용 전에 확인", flow.id));
    }
    if !auto && (inp.od < pat.od_min || inp.od > pat.od_max) {
        warnings.push(format!("OuterDiameter {} 는 Pattern {no} 범위 {}~{} 밖입니다", inp.od, pat.od_min, pat.od_max));
    }

    let mut raw: Vec<[f32; 2]> = Vec::with_capacity(pat.slots.len());
    let mut slots: Vec<Slot> = pat
        .slots
        .iter()
        .map(|s| {
            let u = inp.transform.apply(s.u);
            let off = [u[0] * k, u[1] * k];
            raw.push(off);
            let dir = inp.transform.apply_dir(s.drag_dir);
            Slot {
                seq: s.seq,
                slot: s.slot.clone(),
                u: [r4(u[0]), r4(u[1])],
                offset_x: r1(off[0]),
                offset_y: r1(off[1]),
                x: r1(inp.center[0] + off[0]),
                y: r1(inp.center[1] + off[1]),
                spec_drag_dir: s.drag_dir,
                drag_dir: dir,
                drag_type: drag_type_byte(dir),
                overhang: [r1(off[0].abs() + inp.od / 2.0 - half), r1(off[1].abs() + inp.od / 2.0 - half)],
            }
        })
        .collect();
    slots.sort_by_key(|s| s.seq);

    for s in &slots {
        for (axis, i) in [("X", 0usize), ("Y", 1)] {
            if s.overhang[i] > 0.0 {
                warnings.push(format!("{} (Seq {}) 팔렛 밖 {axis} {} mm", s.slot, s.seq, s.overhang[i]));
            }
        }
    }
    let mut min_distance = f32::INFINITY;
    for i in 0..raw.len() {
        for j in i + 1..raw.len() {
            min_distance = min_distance.min((raw[i][0] - raw[j][0]).hypot(raw[i][1] - raw[j][1]));
        }
    }
    if !min_distance.is_finite() {
        min_distance = 0.0;
    }
    if raw.len() > 1 && min_distance + 0.05 < inp.od {
        errors.push(format!("최소 중심 거리 {} < OuterDiameter {} — 타이어가 겹칩니다 (Gap {})", r1(min_distance), inp.od, inp.gap));
    }

    Ok(Generated {
        flow: flow.id.clone(),
        flow_name: flow.name.clone(),
        drag_kind: flow.drag_kind,
        reference: flow.reference,
        pattern: no,
        pattern_auto: auto,
        od_range: [pat.od_min, pat.od_max],
        od: inp.od,
        gap: inp.gap,
        pitch,
        transform: inp.transform,
        center: inp.center,
        pallet_size: inp.pallet_size,
        min_distance: r1(min_distance),
        slots,
        warnings,
        errors,
        drawn: pat.drawn.clone(),
        notes: pat.notes.clone(),
        min_distance_norm: r4(min_norm.unwrap_or(0.0)),
        scale: r4(scale),
        flow_source: flow.source.clone(),
        based_on: flow.based_on.clone(),
        flow_updated_at: flow.updated_at.clone(),
        pattern_updated_at: pat.updated_at.clone(),
        pattern_note: pat.note.clone(),
    })
}

// ── GR2 isValidTaskArea 선검사 ──────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AreaReject {
    pub seq: u8,
    pub slot: String,
    pub axis: String,
    /// GR2 invalidCode(411 X · 412 Y) 예상.
    pub code: u16,
    pub delta: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AreaCheck {
    pub station_id: u16,
    pub task_type: u8,
    pub rotate_type: u8,
    pub margin: f32,
    pub expected_xy: [f32; 2],
    pub from_registry: bool,
    pub rejects: Vec<AreaReject>,
}

/// GR2 `isValidTaskArea` 와 같은 식 — 기대 중심 = Info.Position + RotateType 별 ±OD/2, 축마다
/// `|pos − 기대| < 마진`(TaskType ≠ 0 이면 650, 아니면 300)이어야 통과. 판정은 GR2 가 하고 여기는 예고만.
pub fn area_check(slots: &[Slot], g: &Gr2Station, station_id: u16, od: f32) -> AreaCheck {
    let pts: Vec<(u8, String, [f32; 2])> = slots.iter().map(|s| (s.seq, s.slot.clone(), [s.x, s.y])).collect();
    area_check_xy(&pts, g, station_id, od)
}

/// `(seq, slot, [x, y])` 목록으로 — compose 는 최종 위치 한 점만 본다.
pub fn area_check_xy(points: &[(u8, String, [f32; 2])], g: &Gr2Station, station_id: u16, od: f32) -> AreaCheck {
    let margin = if g.task_type != 0 { MARGIN_TASKTYPE } else { MARGIN_PLAIN };
    let (cx, cy) = gr2_center_offset(g.rotate_type, od);
    let exp = [g.info_position[0] + cx, g.info_position[1] + cy];
    let mut rejects = Vec::new();
    for (seq, slot, xy) in points {
        for (axis, v, e, code) in [("X", xy[0], exp[0], 411u16), ("Y", xy[1], exp[1], 412)] {
            let d = (v - e).abs();
            if d.is_nan() || d >= margin {
                rejects.push(AreaReject { seq: *seq, slot: slot.clone(), axis: axis.into(), code, delta: r1(d) });
            }
        }
    }
    AreaCheck { station_id, task_type: g.task_type, rotate_type: g.rotate_type, margin, expected_xy: [r1(exp[0]), r1(exp[1])], from_registry: g.from_registry, rejects }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_TRANSFORMS: [(u16, bool, bool); 16] = {
        let mut out = [(0u16, false, false); 16];
        let mut i = 0;
        while i < 16 {
            out[i] = ((i as u16 % 4) * 90, (i / 4) % 2 == 1, (i / 8) % 2 == 1);
            i += 1;
        }
        out
    };

    fn t(rotation: u16, mirror_x: bool, mirror_y: bool) -> Transform {
        Transform { rotation, mirror_x, mirror_y }
    }

    /// PLC FC `DragDelta` 그대로(테스트 기준).
    fn drag_delta(axis_x: bool, dir: u8, dist: f32) -> f32 {
        let r2 = std::f32::consts::SQRT_2;
        match (axis_x, dir) {
            (true, 0b0000_0001) | (true, 0b0000_0100) => dist / r2,
            (true, 0b0000_0010) => dist,
            (true, 0b0010_0000) | (true, 0b1000_0000) => -dist / r2,
            (true, 0b0100_0000) => -dist,
            (false, 0b0000_0001) | (false, 0b0010_0000) => -dist / r2,
            (false, 0b0000_0100) | (false, 0b1000_0000) => dist / r2,
            (false, 0b0000_1000) => -dist,
            (false, 0b0001_0000) => dist,
            _ => 0.0,
        }
    }

    #[test]
    fn dir_codes_match_drag_delta() {
        for d in 1..=8u8 {
            let (sx, sy) = dir_vec(d).unwrap();
            let b = drag_type_byte(d);
            assert_eq!(b.count_ones(), 1);
            let diag = sx != 0 && sy != 0;
            let k = if diag { 100.0 / std::f32::consts::SQRT_2 } else { 100.0 };
            assert!((drag_delta(true, b, 100.0) - sx as f32 * k).abs() < 0.01, "dir {d} X");
            assert!((drag_delta(false, b, 100.0) - sy as f32 * k).abs() < 0.01, "dir {d} Y");
            assert_eq!(dir_from_vec((sx, sy)), d);
        }
        assert_eq!(drag_type_byte(0), 0);
        assert_eq!(drag_type_byte(9), 0);
        assert_eq!(dir_from_vec((0, 0)), 0);
    }

    #[test]
    fn transform_moves_dirs_with_offsets() {
        for (rot, mx, my) in ALL_TRANSFORMS {
            let tr = t(rot, mx, my);
            for d in 1..=8u8 {
                let (sx, sy) = dir_vec(d).unwrap();
                let v = tr.apply([sx as f32, sy as f32]);
                let got = dir_vec(tr.apply_dir(d)).unwrap();
                assert_eq!((got.0 as f32, got.1 as f32), (v[0], v[1]), "rot {rot} mx {mx} my {my} dir {d}");
                // DragDelta of the transformed byte = transformed displacement
                let b = drag_type_byte(tr.apply_dir(d));
                let k = if sx != 0 && sy != 0 { 1.0 / std::f32::consts::SQRT_2 } else { 1.0 };
                assert!((drag_delta(true, b, 1.0) - v[0] * k).abs() < 1e-4);
                assert!((drag_delta(false, b, 1.0) - v[1] * k).abs() < 1e-4);
            }
            assert_eq!(tr.apply_dir(0), 0);
        }
        // spot checks: rot90 turns X+ into Y+, mirror_x flips X only
        assert_eq!(t(90, false, false).apply_dir(2), 5);
        assert_eq!(t(90, false, false).apply([1.0, 0.0]), [0.0, 1.0]);
        assert_eq!(t(0, true, false).apply_dir(3), 8);
        assert_eq!(t(0, false, true).apply_dir(3), 1);
        assert_eq!(t(180, false, false).apply_dir(1), 8);
        assert!(t(45, false, false).validate().is_err());
    }

    fn min_dist(pts: &[[f32; 2]]) -> f32 {
        let mut m = f32::INFINITY;
        for i in 0..pts.len() {
            for j in i + 1..pts.len() {
                m = m.min((pts[i][0] - pts[j][0]).hypot(pts[i][1] - pts[j][1]));
            }
        }
        m
    }

    /// 두 점 집합이 같은가(순서 무관) — 가장 가까운 짝의 최대 거리.
    fn set_residual(a: &[[f32; 2]], b: &[[f32; 2]]) -> f32 {
        let mut used = vec![false; b.len()];
        let mut worst = 0.0f32;
        for p in a {
            let (i, d) = b.iter().enumerate().filter(|(i, _)| !used[*i]).map(|(i, q)| (i, (p[0] - q[0]).hypot(p[1] - q[1]))).min_by(|x, y| x.1.total_cmp(&y.1)).unwrap();
            used[i] = true;
            worst = worst.max(d);
        }
        worst
    }

    fn lib() -> Library {
        Library::from_seed(seed())
    }

    #[test]
    fn spec_parses_and_is_consistent() {
        let sp = seed();
        assert_eq!(sp.pallet_size_mm, 1600.0);
        let ids: Vec<&str> = sp.flows.iter().map(|f| f.id.as_str()).collect();
        for want in ["HP_IN", "OP_IN", "OP_OUT", "OP_EXT_IN", "IN1_ALT_HP", "IN1_ALT_OP", "OP_IN_S5"] {
            assert!(ids.contains(&want), "flow {want} missing");
        }
        let hp = sp.flow("HP_IN").unwrap();
        for f in &sp.flows {
            assert!(!f.patterns.is_empty(), "{}", f.id);
            for p in &f.patterns {
                let n = p.pattern as usize;
                let tag = format!("{} P{}", f.id, p.pattern);
                assert_eq!(p.slots.len(), n, "{tag}: slot count");
                let mut seqs: Vec<u8> = p.slots.iter().map(|s| s.seq).collect();
                seqs.sort();
                assert_eq!(seqs, (1..=n as u8).collect::<Vec<_>>(), "{tag}: seq permutation");
                let mut labels: Vec<String> = p.slots.iter().map(|s| s.slot.clone()).collect();
                labels.sort();
                let mut want: Vec<String> = (1..=n).map(|i| format!("C#{i}")).collect();
                want.sort();
                assert_eq!(labels, want, "{tag}: slot labels");
                assert!(p.slots.iter().all(|s| s.drag_dir <= 8), "{tag}: dir range");
                // 입고는 첫 작업이 드래그 없음, 출하는 마지막 작업이 드래그 없음(사양서 전 표에서 성립)
                let by_seq = |k: u8| p.slots.iter().find(|s| s.seq == k).unwrap();
                match f.drag_kind {
                    DragKind::In => assert_eq!(by_seq(1).drag_dir, 0, "{tag}: first seq dir"),
                    DragKind::Out => assert_eq!(by_seq(n as u8).drag_dir, 0, "{tag}: last seq dir"),
                }
                let pts: Vec<[f32; 2]> = p.slots.iter().map(|s| s.u).collect();
                assert!((min_dist(&pts) - 1.0).abs() < 1e-3, "{tag}: normalized min distance {}", min_dist(&pts));
                let class = sp.size_classes.iter().find(|c| c.pattern == p.pattern).unwrap();
                assert_eq!((p.od_min, p.od_max), (class.od_min, class.od_max), "{tag}: od range");
                // HP 와 같은 점 집합(회전·미러 허용)
                let refp: Vec<[f32; 2]> = hp.pattern(p.pattern).unwrap().slots.iter().map(|s| s.u).collect();
                let best = ALL_TRANSFORMS.iter().map(|(r, mx, my)| set_residual(&pts.iter().map(|q| t(*r, *mx, *my).apply(*q)).collect::<Vec<_>>(), &refp)).fold(f32::INFINITY, f32::min);
                assert!(best < 0.01, "{tag}: geometry differs from HP_IN (residual {best})");
            }
        }
        assert_eq!(hp.patterns.iter().map(|p| p.pattern).collect::<Vec<_>>(), vec![2, 3, 4, 5, 6, 8, 9]);
    }

    #[test]
    fn pattern_by_outer_diameter() {
        let l = lib();
        let hp = l.flow("HP_IN").unwrap();
        for (od, p) in [
            (937.0, Some(2)),
            (814.0, Some(2)),
            (813.5, Some(3)),
            (780.0, Some(3)),
            (700.0, Some(4)),
            (662.0, Some(5)),
            (640.0, Some(5)),
            (601.0, Some(5)),
            (600.0, Some(6)),
            (545.0, Some(8)),
            (520.0, Some(9)),
            (1.0, Some(9)),
        ] {
            assert_eq!(hp.pattern_for_od(od).ok(), p, "od {od}");
        }
        assert!(hp.pattern_for_od(937.1).unwrap_err().contains("no pattern for OD"));
        assert!(hp.pattern_for_od(0.0).is_err());
        assert!(hp.pattern_for_od(f32::NAN).is_err());
        // 편집으로 생긴 넓은 틈은 오류, 1 mm 이하 정수 경계는 이어진 것으로 본다
        let mut gap = hp.clone();
        gap.patterns.retain(|p| p.pattern != 4); // P5 ..662 · (663..755 비움) · P3 756..
        assert_eq!(gap.pattern_for_od(662.0).ok(), Some(5));
        assert!(gap.pattern_for_od(662.5).unwrap_err().contains("no pattern for OD"), "a wide gap is not bridged");
        assert!(gap.pattern_for_od(700.0).unwrap_err().contains("P5 OdMax 662"));
        assert_eq!(gap.pattern_for_od(760.0).ok(), Some(3));
        let empty = Flow { patterns: vec![], ..hp.clone() };
        assert!(empty.pattern_for_od(700.0).is_err());
    }

    fn mk(flow: &str, od: f32, tr: Transform) -> Generated {
        generate(&lib(), &GenInput { flow, pattern: None, od, gap: DEFAULT_GAP, transform: tr, center: [10_000.0, 3_000.0], pallet_size: DEFAULT_PALLET_SIZE }).unwrap()
    }

    #[test]
    fn generate_scales_places_and_drags() {
        let g = mk("HP_IN", 780.0, Transform::default());
        assert_eq!((g.pattern, g.pattern_auto, g.pitch), (3, true, 830.0));
        assert!((g.min_distance - 830.0).abs() < 0.2, "{}", g.min_distance);
        assert!(g.errors.is_empty());
        let s2 = g.slots.iter().find(|s| s.seq == 2).unwrap();
        assert_eq!((s2.slot.as_str(), s2.drag_dir, s2.drag_type), ("C#2", 3, 0b100));
        assert_eq!(s2.x, r1(10_000.0 + s2.offset_x));
        assert_eq!(s2.y, r1(3_000.0 + s2.offset_y));
        // 사양 u × pitch
        let spec_u = seed().flow("HP_IN").unwrap().pattern(3).unwrap().slots.iter().find(|s| s.seq == 2).unwrap().u;
        assert!((s2.offset_x - spec_u[0] * 830.0).abs() < 1.0);
        assert!((g.min_distance_norm - 1.0).abs() < 1e-3 && (g.scale - 1.0).abs() < 1e-3);
        assert_eq!(g.flow_source, SOURCE_SPEC);
        assert_eq!(mk("HP_IN", 700.0, Transform::default()).slots.len(), 4);
        assert_eq!(mk("OP_OUT", 500.0, Transform::default()).pattern, 9);
        // 없는 외경·패턴·흐름
        let l = lib();
        assert!(generate(&l, &GenInput { flow: "HP_IN", pattern: None, od: 950.0, gap: 50.0, transform: Transform::default(), center: [0.0, 0.0], pallet_size: 1600.0 }).is_err());
        assert!(generate(&l, &GenInput { flow: "IN1_ALT_HP", pattern: Some(3), od: 780.0, gap: 50.0, transform: Transform::default(), center: [0.0, 0.0], pallet_size: 1600.0 }).is_err());
        assert!(generate(&l, &GenInput { flow: "NOPE", pattern: None, od: 780.0, gap: 50.0, transform: Transform::default(), center: [0.0, 0.0], pallet_size: 1600.0 }).is_err());
    }

    #[test]
    fn edited_pattern_is_rescaled_to_min_distance() {
        // 슬롯을 안쪽으로 당겨 MinDistance 0.5 → 생성기가 ×2 로 벌려 최소 중심거리 = OuterDiameter + Gap
        let mut l = lib();
        let f = l.flows.iter_mut().find(|f| f.id == "HP_IN").unwrap();
        let p = f.patterns.iter_mut().find(|p| p.pattern == 2).unwrap();
        for s in &mut p.slots {
            s.u = [s.u[0] * 0.5, s.u[1] * 0.5];
        }
        p.slots[1].drag_dir = 5;
        p.updated_at = "2026-09-15T10:00:00+09:00".into();
        let g = generate(&l, &GenInput { flow: "HP_IN", pattern: Some(2), od: 850.0, gap: 50.0, transform: Transform::default(), center: [0.0, 0.0], pallet_size: 1600.0 }).unwrap();
        assert!((g.min_distance_norm - 0.5).abs() < 1e-3, "{}", g.min_distance_norm);
        assert!((g.scale - 2.0).abs() < 1e-2);
        assert!((g.min_distance - 900.0).abs() < 0.3, "{}", g.min_distance);
        assert!(g.errors.is_empty());
        let s2 = g.slots.iter().find(|s| s.slot == "C#2").unwrap();
        assert_eq!((s2.drag_dir, s2.drag_type), (5, 0x10));
        assert_eq!(g.pattern_updated_at, "2026-09-15T10:00:00+09:00");
        // 슬롯 1개 패턴은 배율 1
        let p = l.flows.iter_mut().find(|f| f.id == "HP_IN").unwrap().patterns.iter_mut().find(|p| p.pattern == 2).unwrap();
        p.slots.truncate(1);
        p.slots[0].u = [0.25, 0.0];
        let g = generate(&l, &GenInput { flow: "HP_IN", pattern: Some(2), od: 850.0, gap: 50.0, transform: Transform::default(), center: [0.0, 0.0], pallet_size: 1600.0 }).unwrap();
        assert_eq!((g.scale, g.min_distance_norm, g.slots[0].offset_x), (1.0, 0.0, 225.0));
    }

    #[test]
    fn generate_transform_is_consistent_per_slot() {
        let base = mk("OP_IN", 640.0, Transform::default());
        for (rot, mx, my) in ALL_TRANSFORMS {
            let tr = t(rot, mx, my);
            let g = mk("OP_IN", 640.0, tr);
            assert_eq!(g.slots.len(), base.slots.len());
            for (a, b) in base.slots.iter().zip(&g.slots) {
                assert_eq!((a.seq, &a.slot), (b.seq, &b.slot), "seq order is not changed by transforms");
                let want = tr.apply([a.offset_x, a.offset_y]);
                assert!((b.offset_x - want[0]).abs() < 0.11 && (b.offset_y - want[1]).abs() < 0.11, "rot {rot} {mx} {my} {}", a.slot);
                assert_eq!(b.drag_dir, tr.apply_dir(a.drag_dir));
                assert_eq!(b.drag_type, drag_type_byte(b.drag_dir));
                assert_eq!(b.spec_drag_dir, a.spec_drag_dir);
            }
            assert!((g.min_distance - base.min_distance).abs() < 0.2);
        }
    }

    #[test]
    fn fit_warnings_and_overlap_errors() {
        // P2 at its maximum outer diameter does not fit 1600 with a 50 mm gap
        let g = mk("HP_IN", 937.0, Transform::default());
        assert!(g.slots.iter().any(|s| s.overhang[0] > 0.0));
        assert!(g.warnings.iter().any(|w| w.contains("팔렛 밖")), "{:?}", g.warnings);
        // negative gap overlaps
        let l = lib();
        let g = generate(&l, &GenInput { flow: "HP_IN", pattern: None, od: 700.0, gap: -20.0, transform: Transform::default(), center: [0.0, 0.0], pallet_size: 1600.0 }).unwrap();
        assert!(g.errors.iter().any(|e| e.contains("겹칩니다")));
        // pattern override outside its range warns
        let g = generate(&l, &GenInput { flow: "HP_IN", pattern: Some(4), od: 780.0, gap: 50.0, transform: Transform::default(), center: [0.0, 0.0], pallet_size: 1600.0 }).unwrap();
        assert!(g.warnings.iter().any(|w| w.contains("범위")));
        // reference flow warns
        assert!(mk("OP_IN_S5", 700.0, Transform::default()).warnings.iter().any(|w| w.contains("참고")));
    }

    #[test]
    fn area_check_uses_gr2_margin() {
        let g = mk("HP_IN", 700.0, Transform::default()); // P4 offsets ≈ ±375
        let plain = Gr2Station { task_type: 0, rotate_type: 0, info_position: [10_000.0, 3_000.0, 0.0], from_registry: true };
        let a = area_check(&g.slots, &plain, 2021, 700.0);
        assert_eq!(a.margin, 300.0);
        assert_eq!(a.rejects.len(), 8, "every slot is > 300 from the centre on both axes: {:?}", a.rejects);
        let tasked = Gr2Station { task_type: 1, ..plain };
        let a = area_check(&g.slots, &tasked, 2021, 700.0);
        assert_eq!((a.margin, a.rejects.len()), (650.0, 0));
        // RotateType shifts the expected centre by OD/2
        let rot = Gr2Station { rotate_type: 2, ..tasked };
        let a = area_check(&g.slots, &rot, 2021, 700.0);
        assert_eq!(a.expected_xy, [10_000.0, 3_350.0]);
        assert!(a.rejects.iter().all(|r| r.code == 412));
    }
}
