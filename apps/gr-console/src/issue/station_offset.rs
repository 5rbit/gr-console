//! 스테이션 보정 — GRM `StationCenterAdjust` 를 콘솔 쪽에서 똑같이 재현하는 순수 계산.
//!
//! GRM 은 자기 시나리오 경로(`RobotScenarioTask` → `StationCenterAdjust`)로 보내는 스테이션 태스크에만
//! 측정 트래킹(`STATION.Station[id MOD 100].Tracking.Now`)을 더한다. 콘솔이 `OPCUA.GR[n].CMD` 로 쓰는
//! 태스크는 GRM 이 그대로 중계하고 GR2 는 `Task.Position` 으로 곧장 가므로, 콘솔이 같은 보정을 직접
//! 넣지 않으면 컨베이어 방향·레이저 측정 오프셋이 빠진 채로 로봇이 움직인다.
//!
//! 식(PLC 원문 그대로, `docs/station-offset.md`):
//!
//! | RotateType | TX'                       | TY'                       |
//! |-----------:|---------------------------|---------------------------|
//! | 1, 5       | Now.TaskOffset[X]         | -OD × 0.5                 |
//! | 2, 6       | Now.TaskOffset[X]         | +OD × 0.5                 |
//! | 3, 7       | +OD × 0.5                 | Now.TaskOffset[Y]         |
//! | 4, 8       | -OD × 0.5                 | Now.TaskOffset[Y]         |
//! | 그 외(0)   | Now.TaskOffset[X]         | Now.TaskOffset[Y]         |
//!
//! X = Info.X + TX', Y = Info.Y + TY'. Z·G 는 건드리지 않는다. 여기는 AppState 를 모른다 — 스냅샷 읽기는
//! `issue/mod.rs` 가 하고 결과만 넘긴다.
//!
//! 측정 트래킹이 없으면(스냅샷 없음 또는 `Now.OutterDiameter = 0`) **등록 품목 스펙의 OuterDiameter** 로
//! 대신 보정한다(`OdSource::ItemSpec`) — GR2 `isValidTaskArea` 의 StationCenterOffset 과 **완전히 같은 식**
//! (`gr2_center_offset`: 축은 RotateType, 가로축은 0). Info.Position 이 벽/스토퍼 기준이라 보정을 아예 빼면
//! 로봇이 벽으로 가기 때문이다. 품목 외경도 없으면 보정 없이(`OdSource::None`) 예전처럼 막는다.

use gr_proto::{StationPara, TaskType};
use serde::{Deserialize, Deserializer, Serialize};

/// `isValidTrackingData` 를 부르는 `FB_CL_Station` 의 AllowOffsetRange.
pub const ALLOW_OFFSET_RANGE: f32 = 300.0;
/// 스냅샷이 이보다 오래되면 경고 — GRM `OPCUA` 는 fast(500 ms) 라 5 초, `STATION` 은 slow(5000 ms) 라
/// 한 주기 늦는 게 정상이므로 12 초.
pub fn stale_limit_ms(source: &str) -> i64 {
    if source == "STATION" { 12_000 } else { 5_000 }
}
/// GR2 `isValidTaskArea` 마진 — 스테이션 TaskType ≠ 0 이면 650, 아니면 300.
pub const MARGIN_TASKTYPE: f32 = 650.0;
pub const MARGIN_PLAIN: f32 = 300.0;

/// PLC 배열 슬롯 — PLC 는 `STATION.Station[id MOD 100]` 으로 찾는다(1..=32, `STATION_MIN..STATION_MAX`).
pub fn slot_of(id: u16) -> Option<usize> {
    let n = (id % 100) as usize;
    (1..=32).contains(&n).then_some(n)
}

/// `LGR_Station_TrackingData` 한 벌.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Tracking {
    pub od: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Tracking {
    pub fn is_zero(&self) -> bool {
        self.od == 0.0 && self.tx == 0.0 && self.ty == 0.0
    }
}

/// 요청 플래그 — 기본 켜짐. JSON 은 `true/false`, `"auto"/"on"/"off"`, `null` 을 받는다.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OffsetSwitch {
    #[default]
    Auto,
    Off,
}

impl OffsetSwitch {
    pub fn is_default(&self) -> bool {
        *self == OffsetSwitch::Auto
    }
}

impl<'de> Deserialize<'de> for OffsetSwitch {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            B(bool),
            S(String),
        }
        Ok(match Option::<Raw>::deserialize(d)? {
            None | Some(Raw::B(true)) => OffsetSwitch::Auto,
            Some(Raw::B(false)) => OffsetSwitch::Off,
            Some(Raw::S(s)) => match s.trim().to_ascii_lowercase().as_str() {
                "" | "auto" | "on" | "true" => OffsetSwitch::Auto,
                "off" | "false" | "none" => OffsetSwitch::Off,
                other => return Err(serde::de::Error::custom(format!("station_offset: unknown value '{other}' (auto|off)"))),
            },
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OffsetMode {
    /// 트래킹을 읽어 더했다.
    #[default]
    Auto,
    /// `position_override` 가 위치를 통째로 정했다 — 보정 생략.
    Override,
    /// 요청이 보정을 껐다(`station_offset: "off"`).
    Off,
}

/// 보정에 쓴 외경이 어디서 왔는가.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OdSource {
    /// GRM `Tracking.Now.OutterDiameter` — 실측값(원래 동작).
    Tracking,
    /// 측정값이 없어 등록 품목 스펙의 `OuterDiameter` 로 대신했다(GR2 `isValidTaskArea` 와 같은 식).
    ItemSpec,
    /// 쓸 외경이 없다 — 보정 0.
    #[default]
    None,
}

/// GRM 스냅샷에서 읽은 그 슬롯의 살아 있는 상태.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiveStation {
    /// GRM `Para.RotateType`(StationCenterAdjust 가 쓰는 값).
    pub rotate_type: u8,
    pub now: Tracking,
    pub staged: Tracking,
    pub measuring_error: bool,
    pub data_mismatch: bool,
    /// `"OPCUA"`(fast) | `"STATION"`(slow)
    pub source: String,
    pub snapshot_at: String,
    /// 계산 시점 기준 스냅샷 나이(ms). 시각을 못 읽으면 None.
    pub age_ms: Option<i64>,
    /// GRM S7 연결이 끊겨 스냅샷이 멈춰 있다.
    pub disconnected: bool,
}

/// GR2 `isValidTaskArea` 가 보는 값 — TaskType 은 슬롯 `id MOD 100`, RotateType·Info 는 `Findindex_STATION(id)`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gr2Station {
    pub task_type: u8,
    pub rotate_type: u8,
    pub info_position: [f32; 3],
    /// 레지스트리 값으로 대신했다(GR2 스냅샷 없음).
    pub from_registry: bool,
}

pub struct OffsetInput<'a> {
    pub station_id: u16,
    pub task_type: TaskType,
    pub switch: OffsetSwitch,
    pub override_position: bool,
    /// 레지스트리의 스테이션 파라미터(센서·트래킹 여부 판정, RotateType 비교).
    pub para: &'a StationPara,
    /// GRM 스냅샷(없으면 None — 이유는 `live_missing`).
    pub live: Option<LiveStation>,
    pub live_missing: Option<String>,
    pub gr2: Option<Gr2Station>,
    /// 태스크 품목 외경(GR2 영역 검사는 측정 OD 가 아니라 이 값을 쓴다).
    pub item_od: f32,
    /// 보정 전 위치 — X·Y 는 Info 위치(또는 override), Z·G 는 콘솔 규칙.
    pub position: [f32; 4],
}

/// 원장에 남는 감사 기록 — 무엇을 읽고 무엇을 더했는지.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StationOffsetAudit {
    pub station_id: u16,
    pub slot: u16,
    pub rotate_type: u8,
    /// GRM 측정 OD(`Tracking.Now.OutterDiameter`) — 없으면 0.
    pub od: f32,
    /// 실제로 보정에 쓴 외경 — `od_source` 가 어디서 왔는지 말해 준다.
    pub od_used: f32,
    pub od_source: OdSource,
    pub now_tx: f32,
    pub now_ty: f32,
    pub tx_applied: f32,
    pub ty_applied: f32,
    pub base_xy: [f32; 2],
    pub final_xy: [f32; 2],
    /// `"OPCUA"` | `"STATION"` | None(스냅샷 없음)
    pub source: Option<String>,
    pub snapshot_at: Option<String>,
    pub age_ms: Option<i64>,
    pub mode: OffsetMode,
    pub warnings: Vec<String>,
    /// 제출 거부 사유 — 있으면 `create(submit)` / `submit` 이 409 로 막는다.
    pub blocked: Option<String>,
    /// GR2 영역 검사 기대 위치(Info ± 품목 OD/2)와 마진.
    pub gr2_expected_xy: Option<[f32; 2]>,
    pub gr2_margin: Option<f32>,
}

/// 보정이 붙는 종류 — PICK·DROP·MEASURE (GRM 은 모든 종류에 붙이지만 콘솔은 이 셋만).
pub fn applies_to(tt: TaskType) -> bool {
    matches!(tt, TaskType::Pick | TaskType::Drop | TaskType::Measure)
}

/// `StationCenterAdjust` 의 CASE — (TX', TY').
pub fn center_adjust(rotate_type: u8, now: Tracking) -> (f32, f32) {
    let half = now.od * 0.5;
    match rotate_type {
        1 | 5 => (now.tx, -half),
        2 | 6 => (now.tx, half),
        3 | 7 => (half, now.ty),
        4 | 8 => (-half, now.ty),
        _ => (now.tx, now.ty),
    }
}

/// `isValidTrackingData` — 0 정상, 1 OD 이상(≤300 또는 >1100), 2 오프셋 초과.
pub fn tracking_code(t: Tracking, allow: f32) -> u8 {
    if t.od <= 300.0 || t.od > 1100.0 {
        1
    } else if t.tx.abs() > allow || t.ty.abs() > allow {
        2
    } else {
        0
    }
}

/// GR2 `isValidTaskArea` 의 StationCenterOffset — 품목 외경 기준.
pub fn gr2_center_offset(rotate_type: u8, item_od: f32) -> (f32, f32) {
    let half = item_od * 0.5;
    match rotate_type {
        1 | 5 => (0.0, -half),
        2 | 6 => (0.0, half),
        3 | 7 => (half, 0.0),
        4 | 8 => (-half, 0.0),
        _ => (0.0, 0.0),
    }
}

/// 센서(IO-Link 모듈 + 포트)가 달린 측정 스테이션이거나 앞 스테이션에서 트래킹을 넘겨받는 스테이션.
pub fn expects_tracking(p: &StationPara) -> bool {
    let s = &p.sensor_settings;
    let sensors = s.io_link_master_module != 0 && (s.io_link_master_port_l != 0 || s.io_link_master_port_r != 0);
    let tracked = p.group != 0 && p.connection_prev != 0;
    sensors || tracked
}

fn f1(v: f32) -> String {
    format!("{:.1}", v)
}

fn signed1(v: f32) -> String {
    if v > 0.0 { format!("+{:.1}", v) } else { format!("{:.1}", v) }
}

/// 측정 트래킹이 없을 때 — 등록 품목 외경으로 GR2 `isValidTaskArea` 와 같은 보정을 넣는다.
/// `why` 는 왜 측정값이 없는지(거부 사유에 들어간다). 품목 외경도 없으면 보정 없이 예전 규칙대로 막는다.
fn item_spec_fallback(a: &mut StationOffsetAudit, inp: &OffsetInput, base: [f32; 2], pos: &mut [f32; 4], needs: bool, taking: bool, why: &str) {
    if inp.item_od <= 0.0 {
        a.od_source = OdSource::None;
        if needs && taking {
            a.blocked = Some(format!("스테이션 {} 트래킹 데이터 없음({why}) — 등록 품목 OuterDiameter 도 없어 보정할 수 없습니다. 보정 없이 보내려면 스테이션 보정을 끄세요", inp.station_id));
        }
        return;
    }
    let rt = a.rotate_type;
    let (tx, ty) = gr2_center_offset(rt, inp.item_od);
    a.od_source = OdSource::ItemSpec;
    a.od_used = inp.item_od;
    a.tx_applied = tx;
    a.ty_applied = ty;
    pos[0] = base[0] + tx;
    pos[1] = base[1] + ty;
    if tx == 0.0 && ty == 0.0 {
        // RotateType 0/정의 밖 — GR2 도 보정을 기대하지 않는다.
        a.warnings.push(format!("GRM 측정값 없음 — 등록 품목 OuterDiameter {} 가 있지만 RotateType {rt} 은 보정 축이 없습니다(GR2 도 보정 없음) — 보정 0", f1(inp.item_od)));
    } else {
        let (axis, v) = if tx != 0.0 { ("X", tx) } else { ("Y", ty) };
        a.warnings.push(format!("GRM 측정값 없음 — 등록 품목 OuterDiameter {} 로 보정 (RotateType {rt} → {axis} {})", f1(inp.item_od), signed1(v)));
    }
}

/// 전체 판정 — 최종 위치, 감사 기록(경고·거부 사유 포함).
pub fn evaluate(inp: &OffsetInput) -> (StationOffsetAudit, [f32; 4]) {
    let slot = slot_of(inp.station_id).map(|n| n as u16).unwrap_or(0);
    let mut pos = inp.position;
    let base = [pos[0], pos[1]];
    let mut a = StationOffsetAudit { station_id: inp.station_id, slot, base_xy: base, final_xy: base, rotate_type: inp.para.rotate_type, ..Default::default() };
    let taking = matches!(inp.task_type, TaskType::Pick | TaskType::Measure);
    let needs = expects_tracking(inp.para);

    if slot == 0 {
        a.warnings.push(format!("스테이션 {} 의 슬롯(id MOD 100)이 1..32 밖 — PLC 가 이 스테이션을 찾지 못합니다", inp.station_id));
    }
    if let Some(l) = &inp.live {
        a.rotate_type = l.rotate_type;
        a.od = l.now.od;
        a.now_tx = l.now.tx;
        a.now_ty = l.now.ty;
        a.source = Some(l.source.clone());
        a.snapshot_at = Some(l.snapshot_at.clone());
        a.age_ms = l.age_ms;
    }

    a.mode = if inp.override_position {
        OffsetMode::Override
    } else if inp.switch == OffsetSwitch::Off {
        OffsetMode::Off
    } else {
        OffsetMode::Auto
    };

    match a.mode {
        OffsetMode::Override => a.warnings.push("위치 직접 지정(position_override) — 스테이션 보정 생략".into()),
        OffsetMode::Off => a.warnings.push("스테이션 보정 꺼짐(요청) — Info 위치 그대로 보냅니다".into()),
        OffsetMode::Auto => match &inp.live {
            None => {
                let why = inp.live_missing.clone().unwrap_or_else(|| "GRM 스냅샷 없음".into());
                a.warnings.push(format!("GRM 트래킹을 읽지 못함({why})"));
                item_spec_fallback(&mut a, inp, base, &mut pos, needs, taking, &why);
            }
            Some(l) => {
                if l.rotate_type != inp.para.rotate_type {
                    a.warnings.push(format!("RotateType 불일치: GRM {} ≠ 레지스트리 {} — GRM 값으로 계산", l.rotate_type, inp.para.rotate_type));
                }
                if l.disconnected {
                    a.warnings.push("GRM S7 연결 끊김 — 마지막 스냅샷으로 계산".into());
                }
                if let Some(age) = l.age_ms.filter(|ms| *ms > stale_limit_ms(&l.source)) {
                    a.warnings.push(format!("GRM 스냅샷이 {:.1}초 전 값", age as f64 / 1000.0));
                }
                if l.now.od == 0.0 {
                    if l.now.tx != 0.0 || l.now.ty != 0.0 {
                        a.warnings.push(format!("OD=0 인데 TaskOffset 이 남아 있음 (TX {}, TY {})", f1(l.now.tx), f1(l.now.ty)));
                    }
                    item_spec_fallback(&mut a, inp, base, &mut pos, needs, taking, "OD=0, 측정·트래킹 미완료");
                } else {
                    let code = tracking_code(l.now, ALLOW_OFFSET_RANGE);
                    if code != 0 {
                        let what = if code == 1 { "OD 범위(300 < OD ≤ 1100) 밖" } else { "오프셋 |TX|·|TY| > 300" };
                        a.blocked =
                            Some(format!("스테이션 {} 트래킹 데이터 이상(isValidTrackingData = {code}: {what}; OD {}, TX {}, TY {})", inp.station_id, f1(l.now.od), f1(l.now.tx), f1(l.now.ty)));
                    }
                }
                if l.measuring_error {
                    a.warnings.push("GRM Status.MeasuringError = TRUE".into());
                }
                if l.data_mismatch {
                    a.warnings.push("GRM Status.DataMissMatch = TRUE".into());
                }
                if !l.staged.is_zero() {
                    a.warnings.push(format!("Tracking.Staged 에 보류 측정값 있음 (OD {}, TX {}, TY {})", f1(l.staged.od), f1(l.staged.tx), f1(l.staged.ty)));
                }
                // 측정 OD 가 있을 때만 GRM 식 그대로. OD 0 은 위에서 품목 스펙 폴백이 처리했다.
                if l.now.od != 0.0 {
                    a.od_source = OdSource::Tracking;
                    a.od_used = l.now.od;
                    let (tx, ty) = center_adjust(l.rotate_type, l.now);
                    a.tx_applied = tx;
                    a.ty_applied = ty;
                    pos[0] = base[0] + tx;
                    pos[1] = base[1] + ty;
                }
            }
        },
    }
    a.final_xy = [pos[0], pos[1]];

    // GR2 영역 선검사 — 실제 판정은 GR2 가 한다. 여기는 경고만.
    if let Some(g) = inp.gr2 {
        let margin = if g.task_type != 0 { MARGIN_TASKTYPE } else { MARGIN_PLAIN };
        let (cx, cy) = gr2_center_offset(g.rotate_type, inp.item_od);
        let exp = [g.info_position[0] + cx, g.info_position[1] + cy];
        a.gr2_expected_xy = Some(exp);
        a.gr2_margin = Some(margin);
        let src = if g.from_registry { " (레지스트리 값으로 검사)" } else { "" };
        for (axis, i, code) in [("X", 0usize, 411), ("Y", 1, 412)] {
            let d = (pos[i] - exp[i]).abs();
            // PLC: ABS(Pos1 - Pos2) < Margin 이어야 통과
            if d.is_nan() || d >= margin {
                a.warnings.push(format!("GR2 영역 검사 {axis}: |{} − {}| = {} ≥ 마진 {margin} → GR2 거부 {code} 예상{src}", f1(pos[i]), f1(exp[i]), f1(d)));
            }
        }
        if pos[2].is_nan() || pos[2] <= g.info_position[2] {
            a.warnings.push(format!("GR2 영역 검사 Z: {} ≤ 스테이션 Z {} → GR2 거부 413 예상{src}", f1(pos[2]), f1(g.info_position[2])));
        }
    }
    (a, pos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gr_proto::{CellInfo, SensorSettings};

    fn para(rot: u8) -> StationPara {
        StationPara { rotate_type: rot, info: CellInfo { id: 2102, use_: true, position: [7000.0, 6000.0, 1000.0], width: 1260.0, ..Default::default() }, ..Default::default() }
    }
    fn sensor_para(rot: u8) -> StationPara {
        StationPara { sensor_settings: SensorSettings { io_link_master_module: 1, io_link_master_port_l: 1, io_link_master_port_r: 2, ..Default::default() }, ..para(rot) }
    }
    fn live(rot: u8, od: f32, tx: f32, ty: f32) -> LiveStation {
        LiveStation { rotate_type: rot, now: Tracking { od, tx, ty }, source: "OPCUA".into(), snapshot_at: "2026-09-14T10:00:00+09:00".into(), age_ms: Some(200), ..Default::default() }
    }
    fn input<'a>(p: &'a StationPara, tt: TaskType, l: Option<LiveStation>) -> OffsetInput<'a> {
        OffsetInput {
            station_id: 2102,
            task_type: tt,
            switch: OffsetSwitch::Auto,
            override_position: false,
            para: p,
            live: l,
            live_missing: None,
            gr2: None,
            item_od: 640.0,
            position: [7000.0, 6000.0, 1500.0, 350.0],
        }
    }

    /// 등록 품목 외경도 없는 입력 — 폴백이 못 걸린다.
    fn input_no_item<'a>(p: &'a StationPara, tt: TaskType, l: Option<LiveStation>) -> OffsetInput<'a> {
        OffsetInput { item_od: 0.0, ..input(p, tt, l) }
    }

    #[test]
    fn slot_rule() {
        assert_eq!(slot_of(2003), Some(3));
        assert_eq!(slot_of(2021), Some(21));
        assert_eq!(slot_of(2132), Some(32));
        assert_eq!(slot_of(2100), None);
        assert_eq!(slot_of(2033), None);
    }

    #[test]
    fn center_adjust_all_rotate_types() {
        let now = Tracking { od: 640.0, tx: 40.0, ty: -25.0 };
        assert_eq!(center_adjust(0, now), (40.0, -25.0));
        assert_eq!(center_adjust(1, now), (40.0, -320.0));
        assert_eq!(center_adjust(5, now), (40.0, -320.0));
        assert_eq!(center_adjust(2, now), (40.0, 320.0));
        assert_eq!(center_adjust(6, now), (40.0, 320.0));
        assert_eq!(center_adjust(3, now), (320.0, -25.0));
        assert_eq!(center_adjust(7, now), (320.0, -25.0));
        assert_eq!(center_adjust(4, now), (-320.0, -25.0));
        assert_eq!(center_adjust(8, now), (-320.0, -25.0));
        assert_eq!(center_adjust(9, now), (40.0, -25.0), "정의 밖 값은 CASE 에 안 걸려 Now 그대로");
    }

    #[test]
    fn auto_applies_offset_to_xy_only() {
        for (rot, dx, dy) in
            [(0u8, 40.0f32, -25.0f32), (1, 40.0, -320.0), (2, 40.0, 320.0), (3, 320.0, -25.0), (4, -320.0, -25.0), (5, 40.0, -320.0), (6, 40.0, 320.0), (7, 320.0, -25.0), (8, -320.0, -25.0)]
        {
            let p = para(rot);
            let (a, pos) = evaluate(&input(&p, TaskType::Pick, Some(live(rot, 640.0, 40.0, -25.0))));
            assert_eq!(a.mode, OffsetMode::Auto);
            assert_eq!(pos, [7000.0 + dx, 6000.0 + dy, 1500.0, 350.0], "rot {rot}");
            assert_eq!((a.tx_applied, a.ty_applied), (dx, dy));
            assert_eq!(a.base_xy, [7000.0, 6000.0]);
            assert_eq!(a.final_xy, [pos[0], pos[1]]);
            assert_eq!(a.slot, 2);
            assert_eq!(a.source.as_deref(), Some("OPCUA"));
            assert_eq!(a.od_source, OdSource::Tracking, "rot {rot}");
            assert_eq!(a.od_used, 640.0, "rot {rot}");
            assert!(a.blocked.is_none(), "rot {rot}: {:?}", a.blocked);
            assert!(!a.warnings.iter().any(|w| w.contains("등록 품목")), "측정값이 있으면 폴백을 안 쓴다: {:?}", a.warnings);
        }
    }

    #[test]
    fn override_and_off_leave_position() {
        let p = sensor_para(6);
        let mut i = input(&p, TaskType::Pick, Some(live(6, 0.0, 0.0, 0.0)));
        i.override_position = true;
        let (a, pos) = evaluate(&i);
        assert_eq!(a.mode, OffsetMode::Override);
        assert_eq!(pos, i.position);
        assert_eq!(a.od_source, OdSource::None, "override 는 품목 폴백도 안 쓴다");
        assert!(a.blocked.is_none(), "override 는 거부하지 않는다");
        let mut i = input(&p, TaskType::Pick, Some(live(6, 640.0, 40.0, 0.0)));
        i.switch = OffsetSwitch::Off;
        let (a, pos) = evaluate(&i);
        assert_eq!(a.mode, OffsetMode::Off);
        assert_eq!(pos, i.position);
        assert_eq!((a.tx_applied, a.ty_applied), (0.0, 0.0));
        assert_eq!(a.od, 640.0, "읽은 값은 기록한다");
        assert_eq!(a.od_source, OdSource::None);
    }

    #[test]
    fn guards_no_data_blocks_take_on_tracked_station() {
        // 센서 스테이션 + OD 0 + 등록 품목 외경도 없음: PICK·MEASURE 거부, DROP 은 통과
        let p = sensor_para(6);
        for tt in [TaskType::Pick, TaskType::Measure] {
            let (a, _) = evaluate(&input_no_item(&p, tt, Some(live(6, 0.0, 0.0, 0.0))));
            assert!(a.blocked.as_deref().unwrap_or("").contains("OD=0"), "{tt:?}: {:?}", a.blocked);
            assert!(a.blocked.as_deref().unwrap_or("").contains("등록 품목 OuterDiameter 도 없어"), "{tt:?}: {:?}", a.blocked);
            assert_eq!(a.od_source, OdSource::None);
        }
        let (a, _) = evaluate(&input_no_item(&p, TaskType::Drop, Some(live(6, 0.0, 0.0, 0.0))));
        assert!(a.blocked.is_none());
        // 트래킹 스테이션(Group≠0 && Prev≠0)도 같다
        let p2 = StationPara { group: 1, connection_prev: 2, ..para(6) };
        assert!(evaluate(&input_no_item(&p2, TaskType::Pick, Some(live(6, 0.0, 0.0, 0.0)))).0.blocked.is_some());
        // 센서·트래킹 없는 스테이션은 OD 0 이 정상
        let p3 = para(0);
        assert!(evaluate(&input_no_item(&p3, TaskType::Pick, Some(live(0, 0.0, 0.0, 0.0)))).0.blocked.is_none());
        // 스냅샷 자체가 없고 품목 외경도 없으면: 트래킹 스테이션 PICK 거부, 일반 스테이션 경고만
        let mut i = input_no_item(&p, TaskType::Pick, None);
        i.live_missing = Some("GRM 미설정".into());
        let (a, pos) = evaluate(&i);
        assert!(a.blocked.as_deref().unwrap_or("").contains("GRM 미설정"));
        assert_eq!(pos, i.position);
        assert_eq!(a.od_source, OdSource::None);
        let p4 = para(6);
        let (a, _) = evaluate(&input_no_item(&p4, TaskType::Pick, None));
        assert!(a.blocked.is_none());
        assert!(a.warnings.iter().any(|w| w.contains("GRM 트래킹을 읽지 못함")));
    }

    #[test]
    fn item_spec_fallback_applies_per_rotate_type() {
        // 측정값이 없어도 등록 품목 외경(640)으로 RotateType 축에 ±320, 가로축 0 — GR2 식과 같다.
        for (rot, dx, dy) in [(1u8, 0.0f32, -320.0f32), (5, 0.0, -320.0), (2, 0.0, 320.0), (6, 0.0, 320.0), (3, 320.0, 0.0), (7, 320.0, 0.0), (4, -320.0, 0.0), (8, -320.0, 0.0)] {
            let p = sensor_para(rot);
            // (a) 스냅샷 없음
            let (a, pos) = evaluate(&input(&p, TaskType::Pick, None));
            assert_eq!(a.od_source, OdSource::ItemSpec, "rot {rot}");
            assert_eq!(a.od_used, 640.0, "rot {rot}");
            assert_eq!(a.od, 0.0, "측정값은 그대로 0");
            assert_eq!((a.tx_applied, a.ty_applied), (dx, dy), "rot {rot}");
            assert_eq!(pos, [7000.0 + dx, 6000.0 + dy, 1500.0, 350.0], "rot {rot}");
            assert!(a.blocked.is_none(), "폴백은 막지 않는다: {:?}", a.blocked);
            let w = a.warnings.join(" | ");
            assert!(w.contains("등록 품목 OuterDiameter 640.0 로 보정"), "rot {rot}: {w}");
            assert!(w.contains(&format!("RotateType {rot} →")), "rot {rot}: {w}");
            assert!(w.contains(if dx != 0.0 { "X " } else { "Y " }), "rot {rot}: {w}");
            // (b) 스냅샷은 있는데 아직 측정 전(OD 0) — 가로 오프셋이 남아 있어도 폴백은 0
            let (a, pos) = evaluate(&input(&p, TaskType::Measure, Some(live(rot, 0.0, 44.0, -7.0))));
            assert_eq!(a.od_source, OdSource::ItemSpec, "rot {rot}");
            assert_eq!((a.tx_applied, a.ty_applied), (dx, dy), "rot {rot}");
            assert_eq!(pos, [7000.0 + dx, 6000.0 + dy, 1500.0, 350.0], "rot {rot}");
            assert!(a.blocked.is_none(), "rot {rot}: {:?}", a.blocked);
            assert!(a.warnings.iter().any(|w| w.contains("TaskOffset 이 남아 있음")), "{:?}", a.warnings);
        }
    }

    #[test]
    fn item_spec_fallback_rotate_type_zero_leaves_position() {
        let p = sensor_para(0);
        let (a, pos) = evaluate(&input(&p, TaskType::Pick, Some(live(0, 0.0, 0.0, 0.0))));
        assert_eq!(a.od_source, OdSource::ItemSpec);
        assert_eq!(a.od_used, 640.0);
        assert_eq!((a.tx_applied, a.ty_applied), (0.0, 0.0));
        assert_eq!(pos, [7000.0, 6000.0, 1500.0, 350.0]);
        assert!(a.blocked.is_none());
        assert!(a.warnings.iter().any(|w| w.contains("RotateType 0 은 보정 축이 없습니다")), "{:?}", a.warnings);
    }

    #[test]
    fn item_spec_fallback_passes_gr2_precheck() {
        // 폴백은 GR2 isValidTaskArea 기대 위치와 정확히 같다 — 거리 0.
        let p = sensor_para(6);
        let g = Gr2Station { task_type: 0, rotate_type: 6, info_position: [7000.0, 6000.0, 1000.0], from_registry: false };
        let mut i = input(&p, TaskType::Pick, Some(live(6, 0.0, 0.0, 0.0)));
        i.gr2 = Some(g);
        let (a, pos) = evaluate(&i);
        assert_eq!(a.gr2_expected_xy, Some([7000.0, 6320.0]));
        assert_eq!([pos[0], pos[1]], [7000.0, 6320.0]);
        assert!(!a.warnings.iter().any(|w| w.contains("GR2 영역")), "{:?}", a.warnings);
    }

    #[test]
    fn guards_invalid_tracking_blocks_every_type() {
        assert_eq!(tracking_code(Tracking { od: 300.0, tx: 0.0, ty: 0.0 }, 300.0), 1);
        assert_eq!(tracking_code(Tracking { od: 1100.0, tx: 0.0, ty: 0.0 }, 300.0), 0);
        assert_eq!(tracking_code(Tracking { od: 1100.1, tx: 0.0, ty: 0.0 }, 300.0), 1);
        assert_eq!(tracking_code(Tracking { od: 640.0, tx: 300.0, ty: -300.0 }, 300.0), 0);
        assert_eq!(tracking_code(Tracking { od: 640.0, tx: 0.0, ty: -300.5 }, 300.0), 2);
        let p = para(6);
        for tt in [TaskType::Pick, TaskType::Drop, TaskType::Measure] {
            let (a, _) = evaluate(&input(&p, tt, Some(live(6, 1200.0, 0.0, 0.0))));
            assert!(a.blocked.as_deref().unwrap_or("").contains("isValidTrackingData = 1"), "{tt:?}");
            let (a, _) = evaluate(&input(&p, tt, Some(live(6, 640.0, 350.0, 0.0))));
            assert!(a.blocked.as_deref().unwrap_or("").contains("isValidTrackingData = 2"), "{tt:?}");
        }
    }

    #[test]
    fn guards_warnings() {
        let p = para(2);
        let mut l = live(6, 640.0, 10.0, 0.0);
        l.measuring_error = true;
        l.data_mismatch = true;
        l.staged = Tracking { od: 650.0, tx: 1.0, ty: 0.0 };
        l.age_ms = Some(9000);
        l.disconnected = true;
        let (a, _) = evaluate(&input(&p, TaskType::Pick, Some(l)));
        let w = a.warnings.join(" | ");
        for needle in ["RotateType 불일치", "MeasuringError", "DataMissMatch", "Staged", "9.0초 전", "연결 끊김"] {
            assert!(w.contains(needle), "{needle} not in {w}");
        }
        assert_eq!(a.rotate_type, 6, "GRM 값으로 계산");
        // slow STATION 폴백은 5 초대 나이가 정상 — 12 초까지 경고 없음
        let mut l = live(6, 640.0, 10.0, 0.0);
        l.source = "STATION".into();
        l.age_ms = Some(9000);
        assert!(!evaluate(&input(&p, TaskType::Pick, Some(l.clone()))).0.warnings.iter().any(|w| w.contains("초 전")));
        l.age_ms = Some(13_000);
        assert!(evaluate(&input(&p, TaskType::Pick, Some(l))).0.warnings.iter().any(|w| w.contains("13.0초 전")));
        assert_eq!(a.ty_applied, 320.0);
        assert!(a.blocked.is_none());
    }

    #[test]
    fn gr2_area_precheck() {
        // rot 6, 품목 OD 640 → 기대 Y = Info.Y + 320, 마진 300(TaskType 0)
        let p = para(6);
        let g = Gr2Station { task_type: 0, rotate_type: 6, info_position: [7000.0, 6000.0, 1000.0], from_registry: false };
        let mut i = input(&p, TaskType::Pick, Some(live(6, 640.0, 40.0, 0.0)));
        i.gr2 = Some(g);
        let (a, _) = evaluate(&i);
        assert_eq!(a.gr2_expected_xy, Some([7000.0, 6320.0]));
        assert_eq!(a.gr2_margin, Some(300.0));
        assert!(!a.warnings.iter().any(|w| w.contains("GR2 영역")), "{:?}", a.warnings);
        // 보정을 끄면 Y 가 320 모자라 → 412 경고(마진 300)
        i.switch = OffsetSwitch::Off;
        let (a, _) = evaluate(&i);
        assert!(a.warnings.iter().any(|w| w.contains("412")), "{:?}", a.warnings);
        // TaskType ≠ 0 이면 마진 650 이라 통과
        i.gr2 = Some(Gr2Station { task_type: 1, ..g });
        let (a, _) = evaluate(&i);
        assert!(!a.warnings.iter().any(|w| w.contains("GR2 영역")), "{:?}", a.warnings);
        // X 축(across-flow) 오프셋이 마진을 넘으면 411, Z 가 스테이션 아래면 413
        i.switch = OffsetSwitch::Auto;
        i.gr2 = Some(g);
        i.live = Some(live(6, 640.0, 299.0, 0.0));
        i.position[0] = 7010.0;
        i.position[2] = 900.0;
        let (a, _) = evaluate(&i);
        assert!(a.warnings.iter().any(|w| w.contains("411")), "{:?}", a.warnings);
        assert!(a.warnings.iter().any(|w| w.contains("413")), "{:?}", a.warnings);
    }

    #[test]
    fn switch_deserializes_bool_and_string() {
        #[derive(Deserialize)]
        struct R {
            #[serde(default)]
            station_offset: OffsetSwitch,
        }
        let p = |s: &str| serde_json::from_str::<R>(s).map(|r| r.station_offset);
        assert_eq!(p("{}").unwrap(), OffsetSwitch::Auto);
        assert_eq!(p(r#"{"station_offset":null}"#).unwrap(), OffsetSwitch::Auto);
        assert_eq!(p(r#"{"station_offset":false}"#).unwrap(), OffsetSwitch::Off);
        assert_eq!(p(r#"{"station_offset":true}"#).unwrap(), OffsetSwitch::Auto);
        assert_eq!(p(r#"{"station_offset":"off"}"#).unwrap(), OffsetSwitch::Off);
        assert_eq!(p(r#"{"station_offset":"AUTO"}"#).unwrap(), OffsetSwitch::Auto);
        assert!(p(r#"{"station_offset":"maybe"}"#).is_err());
        assert_eq!(serde_json::to_value(OffsetSwitch::Off).unwrap(), "off");
    }
}
