//! Console-side tunables of a task (snake_case, used by defaults profiles / overrides) and their
//! application onto `TaskData`.

use serde::{Deserialize, Serialize};

use crate::task::TaskData;

/// 드래그 거리 기본값(mm) — 운전자 결정(2026-09-18). 요청·기본값이 거리를 안 정하면 이 값을 쓴다.
pub const DEFAULT_DRAG_DIST: u16 = 150;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskParams {
    pub lift_up_height: u16,
    pub grip_height: u16,
    pub pre_grip_delta: u16,
    pub grip_back_delta: u16,
    pub blend_up_distance: u16,
    pub blend_down_distance: u16,
    pub lift_up_creep_distance: u16,
    pub lift_down_creep_distance: u16,
    pub lift_up_after_complete: bool,
    pub lift_up_partial: bool,
    pub measure_floor: bool,
    pub measure_item: bool,
    pub measure_sku: bool,
    pub adjust_center: bool,
    pub find_station_item: bool,
    pub avoid: bool,
    pub outbound: bool,
    pub use_drag_out: bool,
    pub drag_out_height: u16,
    pub drag_out_dist: u16,
    pub drag_out_dir: u8,
    pub use_drag_in: bool,
    pub drag_in_height: u16,
    pub drag_in_dist: u16,
    pub drag_in_dir: u8,
}

impl Default for TaskParams {
    fn default() -> Self {
        Self {
            lift_up_height: 2500,
            grip_height: 40,
            pre_grip_delta: 10,
            grip_back_delta: 5,
            blend_up_distance: 300,
            blend_down_distance: 300,
            lift_up_creep_distance: 30,
            lift_down_creep_distance: 30,
            lift_up_after_complete: true,
            lift_up_partial: false,
            measure_floor: false,
            measure_item: false,
            measure_sku: false,
            adjust_center: false,
            find_station_item: false,
            avoid: false,
            outbound: false,
            use_drag_out: false,
            drag_out_height: 0,
            drag_out_dist: DEFAULT_DRAG_DIST,
            drag_out_dir: 0,
            use_drag_in: false,
            drag_in_height: 0,
            drag_in_dist: DEFAULT_DRAG_DIST,
            drag_in_dir: 0,
        }
    }
}

impl TaskParams {
    /// Overlays a partial JSON object (snake_case keys) onto these params.
    pub fn overlay(&self, partial: &serde_json::Value) -> Result<TaskParams, serde_json::Error> {
        let mut base = serde_json::to_value(self)?;
        if let (Some(b), Some(p)) = (base.as_object_mut(), partial.as_object()) {
            for (k, v) in p {
                if !v.is_null() {
                    b.insert(k.clone(), v.clone());
                }
            }
        }
        serde_json::from_value(base)
    }

    pub fn apply(&self, t: &mut TaskData) {
        t.lift_up_height = self.lift_up_height;
        t.grip_height = self.grip_height;
        t.pre_grip_delta = self.pre_grip_delta;
        t.grip_back_delta = self.grip_back_delta;
        t.blend_up_distance = self.blend_up_distance;
        t.blend_down_distance = self.blend_down_distance;
        t.lift_up_creep_distance = self.lift_up_creep_distance;
        t.lift_down_creep_distance = self.lift_down_creep_distance;
        t.lift_up_after_complete = self.lift_up_after_complete;
        t.lift_up_partial = self.lift_up_partial;
        t.measure_floor = self.measure_floor;
        t.measure_item = self.measure_item;
        t.measure_sku = self.measure_sku;
        t.adjust_center = self.adjust_center;
        t.find_station_item = self.find_station_item;
        t.avoid = self.avoid;
        t.outbound = self.outbound;
        t.use_drag_out = self.use_drag_out;
        t.drag_out_height = self.drag_out_height;
        t.drag_out_dist = self.drag_out_dist;
        t.drag_out_dir = self.drag_out_dir;
        t.use_drag_in = self.use_drag_in;
        t.drag_in_height = self.drag_in_height;
        t.drag_in_dist = self.drag_in_dist;
        t.drag_in_dir = self.drag_in_dir;
    }

    pub fn from_task(t: &TaskData) -> TaskParams {
        TaskParams {
            lift_up_height: t.lift_up_height,
            grip_height: t.grip_height,
            pre_grip_delta: t.pre_grip_delta,
            grip_back_delta: t.grip_back_delta,
            blend_up_distance: t.blend_up_distance,
            blend_down_distance: t.blend_down_distance,
            lift_up_creep_distance: t.lift_up_creep_distance,
            lift_down_creep_distance: t.lift_down_creep_distance,
            lift_up_after_complete: t.lift_up_after_complete,
            lift_up_partial: t.lift_up_partial,
            measure_floor: t.measure_floor,
            measure_item: t.measure_item,
            measure_sku: t.measure_sku,
            adjust_center: t.adjust_center,
            find_station_item: t.find_station_item,
            avoid: t.avoid,
            outbound: t.outbound,
            use_drag_out: t.use_drag_out,
            drag_out_height: t.drag_out_height,
            drag_out_dist: t.drag_out_dist,
            drag_out_dir: t.drag_out_dir,
            use_drag_in: t.use_drag_in,
            drag_in_height: t.drag_in_height,
            drag_in_dist: t.drag_in_dist,
            drag_in_dir: t.drag_in_dir,
        }
    }
}

/// 파라미터 값의 종류 — 검사와 화면 입력이 같은 표를 본다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamKind {
    Bool,
    U8,
    U16,
}

/// 파라미터 하나의 정의(키 · 종류 · 단위). [`TaskParams`] 의 필드와 1:1 — 테스트가 맞춤을 확인한다.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ParamSpec {
    pub key: &'static str,
    pub kind: ParamKind,
    /// 표시 단위(`mm`, 없으면 빈 문자열).
    pub unit: &'static str,
}

const fn spec(key: &'static str, kind: ParamKind, unit: &'static str) -> ParamSpec {
    ParamSpec { key, kind, unit }
}

/// 모든 작업 파라미터의 정의 — 기본값 저장·시나리오 저장·작성 요청 검사가 이 표 하나로 한다.
pub const PARAM_SPECS: &[ParamSpec] = &[
    spec("lift_up_height", ParamKind::U16, "mm"),
    spec("grip_height", ParamKind::U16, "mm"),
    spec("pre_grip_delta", ParamKind::U16, "mm"),
    spec("grip_back_delta", ParamKind::U16, "mm"),
    spec("blend_up_distance", ParamKind::U16, "mm"),
    spec("blend_down_distance", ParamKind::U16, "mm"),
    spec("lift_up_creep_distance", ParamKind::U16, "mm"),
    spec("lift_down_creep_distance", ParamKind::U16, "mm"),
    spec("lift_up_after_complete", ParamKind::Bool, ""),
    spec("lift_up_partial", ParamKind::Bool, ""),
    spec("measure_floor", ParamKind::Bool, ""),
    spec("measure_item", ParamKind::Bool, ""),
    spec("measure_sku", ParamKind::Bool, ""),
    spec("adjust_center", ParamKind::Bool, ""),
    spec("find_station_item", ParamKind::Bool, ""),
    spec("avoid", ParamKind::Bool, ""),
    spec("outbound", ParamKind::Bool, ""),
    spec("use_drag_out", ParamKind::Bool, ""),
    spec("drag_out_height", ParamKind::U16, "mm"),
    spec("drag_out_dist", ParamKind::U16, "mm"),
    spec("drag_out_dir", ParamKind::U8, ""),
    spec("use_drag_in", ParamKind::Bool, ""),
    spec("drag_in_height", ParamKind::U16, "mm"),
    spec("drag_in_dist", ParamKind::U16, "mm"),
    spec("drag_in_dir", ParamKind::U8, ""),
];

/// 부분 파라미터(JSON 객체) 검사 — 모르는 키(오타), 종류가 틀린 값, 범위를 넘는 값을 한 줄씩 돌려준다(없으면 빈 목록).
/// `extra` 는 [`TaskParams`] 밖에서 따로 읽는 키(예: MOVE 의 `move_mode`·`move_clearance`) — 여기서는 통과시킨다.
/// `null` 값은 "상속"이라 통과.
pub fn check_partial(partial: &serde_json::Value, extra: &[&str]) -> Vec<String> {
    let Some(obj) = partial.as_object() else {
        return if partial.is_null() { Vec::new() } else { vec!["파라미터는 객체여야 합니다".into()] };
    };
    let mut out = Vec::new();
    for (k, v) in obj {
        if v.is_null() || extra.contains(&k.as_str()) {
            continue;
        }
        let Some(s) = PARAM_SPECS.iter().find(|s| s.key == k) else {
            out.push(format!("{k}: 모르는 파라미터(오타?)"));
            continue;
        };
        let ok = match s.kind {
            ParamKind::Bool => v.is_boolean(),
            ParamKind::U8 => v.as_u64().is_some_and(|n| n <= u8::MAX as u64),
            ParamKind::U16 => v.as_u64().is_some_and(|n| n <= u16::MAX as u64),
        };
        if !ok {
            let want = match s.kind {
                ParamKind::Bool => "true/false".to_string(),
                ParamKind::U8 => format!("0..{} 정수", u8::MAX),
                ParamKind::U16 => format!("0..{} 정수{}", u16::MAX, if s.unit.is_empty() { String::new() } else { format!("({})", s.unit) }),
            };
            out.push(format!("{k}: {v} — {want}"));
        }
    }
    out
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// 표와 구조체가 어긋나면(필드를 더하고 표를 잊으면) 검사가 새 키를 "모르는 파라미터" 로 막는다 — 여기서 잡는다.
    #[test]
    fn specs_match_the_struct() {
        let v = serde_json::to_value(TaskParams::default()).unwrap();
        let obj = v.as_object().unwrap();
        assert_eq!(obj.len(), PARAM_SPECS.len());
        for s in PARAM_SPECS {
            let val = obj.get(s.key).unwrap_or_else(|| panic!("{} not in TaskParams", s.key));
            assert_eq!(val.is_boolean(), s.kind == ParamKind::Bool, "{}", s.key);
        }
        assert!(check_partial(&v, &[]).is_empty(), "기본값 전체는 통과");
    }

    #[test]
    fn check_partial_reports_typos_types_and_ranges() {
        let p = serde_json::json!({ "grip_heigth": 40, "grip_height": -1, "lift_up_partial": 1, "drag_in_dir": 300, "blend_up_distance": 12.5, "avoid": null, "move_mode": "top" });
        let e = check_partial(&p, &["move_mode"]);
        assert_eq!(e.len(), 5, "{e:?}");
        assert!(e.iter().any(|m| m.starts_with("grip_heigth: 모르는")));
        assert!(check_partial(&serde_json::json!({ "grip_height": 55, "avoid": true }), &[]).is_empty());
        assert!(!check_partial(&serde_json::json!([1]), &[]).is_empty());
    }
}
