//! 작업 파라미터 기본값(`Defaults`)의 구조 검사 · 관대한 읽기 · 변경 이력 · 내보내기/가져오기.
//!
//! 저장본은 settings 표의 JSON 한 덩어리(`defaults`)다. 이 모듈이 그 덩어리의 **모양**을 지킨다:
//! - 저장(`validate`) — 모르는 키(오타), 틀린 종류·범위, 모르는 작업 종류·대상, MOVE 옵션, 상황 키, 그립 기준을
//!   모두 한 번에 모아 거부한다(예전에는 상황 층만 봤고, `by` 의 오타·범위는 작성 때 400 으로 터졌다).
//! - 읽기(`load_lenient`) — 저장본 일부가 깨져도 **깨진 키만** 버리고 나머지는 살린다(예전에는 하나만 틀려도
//!   통째로 공장값이 되고 다음 저장이 덮어써 운전자 설정이 사라졌다). 원본은 `defaults.recovered` 에 남긴다.
//! - 이력 — 저장마다 이전/이후를 `settings_history` 에(되돌리기).

use std::collections::BTreeMap;

use gr_proto::{TaskParams, check_partial};
use serde_json::{Map, Value as Json, json};

use super::{Defaults, SITUATIONS};

/// `by` 의 작업 종류 키.
pub const BY_TYPES: [&str; 5] = ["PICK", "DROP", "MOVE", "MEASURE", "UP"];
/// `by` 의 대상 키.
pub const KINDS: [&str; 2] = ["cell", "station"];
/// MOVE 층에만 있는, `TaskParams` 밖의 키(`issue::move_options`).
pub const MOVE_EXTRA: [&str; 2] = ["move_mode", "move_clearance"];
/// 그립 기준 — 저장 때 받는 값(옛 이름 포함). 저장본에는 정본(`mid`·`pick_bead`)만 남는다.
pub const GRIP_REFS: [&str; 4] = ["mid", "pick_bead", "bead", "bead+offset"];
/// 이력 보존 줄 수(키마다).
pub const HISTORY_KEEP: i64 = 200;

fn move_extra_problems(layer: &Json, at: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(v) = layer.get("move_mode").filter(|v| !v.is_null())
        && !v.as_str().is_some_and(|m| ["stack", "top", "avoid"].contains(&m))
    {
        out.push(format!("{at}.move_mode: {v} — stack | top | avoid"));
    }
    if let Some(v) = layer.get("move_clearance").filter(|v| !v.is_null())
        && !v.as_f64().is_some_and(|c| c.is_finite() && (0.0..=5000.0).contains(&c))
    {
        out.push(format!("{at}.move_clearance: {v} — 0..5000 mm"));
    }
    out
}

/// 층 하나(부분 파라미터)의 문제 — `at` 는 위치 이름(`by.PICK.cell`).
fn layer_problems(layer: &Json, at: &str, move_layer: bool) -> Vec<String> {
    if !layer.is_object() {
        return vec![format!("{at}: 파라미터 객체여야 합니다")];
    }
    let extra: &[&str] = if move_layer { &MOVE_EXTRA } else { &[] };
    let mut out: Vec<String> = check_partial(layer, extra).into_iter().map(|p| format!("{at}.{p}")).collect();
    if move_layer {
        out.extend(move_extra_problems(layer, at));
    }
    out
}

/// 저장 전 구조 검사 — 문제를 전부 모아 돌려준다(없으면 빈 목록).
pub fn validate(d: &Defaults) -> Vec<String> {
    let mut out = Vec::new();
    match serde_json::to_value(&d.base) {
        Ok(b) => out.extend(check_partial(&b, &[]).into_iter().map(|p| format!("base.{p}"))),
        Err(e) => out.push(format!("base: {e}")),
    }
    for (t, kinds) in &d.by {
        if !BY_TYPES.contains(&t.as_str()) {
            out.push(format!("by.{t}: 모르는 작업 종류 — {}", BY_TYPES.join(" | ")));
            continue;
        }
        for (k, layer) in kinds {
            if !KINDS.contains(&k.as_str()) {
                out.push(format!("by.{t}.{k}: 모르는 대상 — cell | station"));
                continue;
            }
            out.extend(layer_problems(layer, &format!("by.{t}.{k}"), t == "MOVE"));
        }
    }
    for (k, layer) in &d.situations {
        if !SITUATIONS.contains(&k.as_str()) {
            out.push(format!("situations.{k}: 모르는 상황 — {}", SITUATIONS.join(" | ")));
            continue;
        }
        out.extend(layer_problems(layer, &format!("situations.{k}"), false));
    }
    if !GRIP_REFS.contains(&d.grip_ref.as_str()) {
        out.push(format!("grip_ref: '{}' — mid | pick_bead", d.grip_ref));
    }
    out
}

/// 층에서 문제 있는 키만 걷어 낸다(관대한 읽기).
fn keep_good_keys(layer: &Json, at: &str, move_layer: bool, warn: &mut Vec<String>) -> Json {
    let Some(obj) = layer.as_object() else {
        warn.push(format!("{at}: 객체가 아니라 버림"));
        return json!({});
    };
    let mut keep = Map::new();
    for (k, v) in obj {
        let one = json!({ k.clone(): v.clone() });
        let probs = layer_problems(&one, at, move_layer);
        if probs.is_empty() {
            keep.insert(k.clone(), v.clone());
        } else {
            warn.extend(probs.into_iter().map(|p| format!("{p} (버림)")));
        }
    }
    Json::Object(keep)
}

/// 저장본 JSON → `Defaults` — 깨진 부분만 버리고 무엇을 버렸는지 돌려준다.
pub fn load_lenient(raw: &str) -> (Defaults, Vec<String>) {
    let mut warn = Vec::new();
    let mut d = Defaults::default();
    let v: Json = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(e) => {
            warn.push(format!("저장된 기본값 JSON 을 읽지 못해 공장값을 씁니다: {e}"));
            return (d, warn);
        }
    };
    let Some(o) = v.as_object() else {
        warn.push("저장된 기본값이 객체가 아니라 공장값을 씁니다".into());
        return (d, warn);
    };
    if let Some(n) = o.get("version").and_then(Json::as_u64) {
        d.version = n as u32;
    }
    if let Some(s) = o.get("updated_at").and_then(Json::as_str) {
        d.updated_at = s.to_string();
    }
    if let Some(s) = o.get("grip_ref").and_then(Json::as_str) {
        d.grip_ref = s.to_string();
    }
    if let Some(b) = o.get("base") {
        let good = keep_good_keys(b, "base", false, &mut warn);
        match TaskParams::default().overlay(&good) {
            Ok(p) => d.base = p,
            Err(e) => warn.push(format!("base: {e} — 공장값")),
        }
    }
    if let Some(by) = o.get("by").and_then(Json::as_object) {
        d.by.clear();
        for (t, kinds) in by {
            if !BY_TYPES.contains(&t.as_str()) {
                warn.push(format!("by.{t}: 모르는 작업 종류 (버림)"));
                continue;
            }
            let Some(kinds) = kinds.as_object() else {
                warn.push(format!("by.{t}: 객체가 아님 (버림)"));
                continue;
            };
            let m = d.by.entry(t.clone()).or_default();
            for (k, layer) in kinds {
                if !KINDS.contains(&k.as_str()) {
                    warn.push(format!("by.{t}.{k}: 모르는 대상 (버림)"));
                    continue;
                }
                m.insert(k.clone(), keep_good_keys(layer, &format!("by.{t}.{k}"), t == "MOVE", &mut warn));
            }
        }
    }
    if let Some(sit) = o.get("situations").and_then(Json::as_object) {
        for (k, layer) in sit {
            if !SITUATIONS.contains(&k.as_str()) {
                warn.push(format!("situations.{k}: 모르는 상황 (버림)"));
                continue;
            }
            let good = keep_good_keys(layer, &format!("situations.{k}"), false, &mut warn);
            d.situations.insert(k.clone(), good);
        }
    }
    (d, warn)
}

/// 두 기본값의 차이(경로: 이전 → 이후) — 가져오기 미리보기·이력 요약.
pub fn diff(before: &Defaults, after: &Defaults) -> Vec<String> {
    fn flat(prefix: &str, v: &Json, out: &mut BTreeMap<String, Json>) {
        match v {
            Json::Object(o) => {
                for (k, x) in o {
                    flat(&if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") }, x, out);
                }
            }
            other => {
                out.insert(prefix.to_string(), other.clone());
            }
        }
    }
    let pick = |d: &Defaults| json!({ "base": d.base, "by": d.by, "situations": d.situations, "grip_ref": d.grip_ref });
    let (mut a, mut b) = (BTreeMap::new(), BTreeMap::new());
    flat("", &pick(before), &mut a);
    flat("", &pick(after), &mut b);
    let mut out = Vec::new();
    for k in a.keys().chain(b.keys()).collect::<std::collections::BTreeSet<_>>() {
        let (x, y) = (a.get(k), b.get(k));
        if x != y {
            let s = |v: Option<&Json>| v.map(|v| v.to_string()).unwrap_or_else(|| "—".into());
            out.push(format!("{k}: {} → {}", s(x), s(y)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_collects_every_problem() {
        let mut d = Defaults::default();
        d.by.entry("PICK".into()).or_default().insert("cell".into(), json!({ "grip_heigth": 40, "grip_height": -3 }));
        d.by.entry("Pick".into()).or_default().insert("cell".into(), json!({}));
        d.by.entry("MOVE".into()).or_default().insert("cel".into(), json!({}));
        d.by.entry("MOVE".into()).or_default().insert("station".into(), json!({ "move_mode": "fly", "move_clearance": 9000 }));
        d.situations.insert("measure_sku".into(), json!({ "avoid": "yes" }));
        d.grip_ref = "nonsense".into();
        let p = validate(&d);
        for want in ["by.PICK.cell.grip_heigth", "by.PICK.cell.grip_height", "by.Pick", "by.MOVE.cel", "move_mode", "move_clearance", "situations.measure_sku.avoid", "grip_ref"] {
            assert!(p.iter().any(|m| m.contains(want)), "missing {want}: {p:?}");
        }
        assert!(validate(&Defaults::default()).is_empty(), "공장값은 통과");
        let mut ok = Defaults::default();
        ok.by.entry("MOVE".into()).or_default().insert("cell".into(), json!({ "move_mode": "top", "move_clearance": 300, "avoid": false }));
        assert!(validate(&ok).is_empty(), "{:?}", validate(&ok));
    }

    /// 한 키가 깨져도 나머지는 산다 — 예전에는 통째로 공장값이 되어 다음 저장이 운전자 설정을 지웠다.
    #[test]
    fn lenient_load_keeps_what_it_can() {
        let raw = r#"{"version":7,"grip_ref":"pick_bead","base":{"grip_height":55,"lift_up_height":"tall"},
            "by":{"PICK":{"station":{"find_station_item":true,"typo":1}},"MOVE":{"cell":{"move_mode":"top"}},"FLY":{"cell":{}}},
            "situations":{"multi_pick":{"lift_up_partial":true},"moon":{}}}"#;
        let (d, w) = load_lenient(raw);
        assert_eq!(d.version, 7);
        assert_eq!(d.base.grip_height, 55);
        assert_eq!(d.base.lift_up_height, TaskParams::default().lift_up_height, "깨진 값만 공장값");
        assert_eq!(d.by["PICK"]["station"], json!({ "find_station_item": true }));
        assert_eq!(d.by["MOVE"]["cell"], json!({ "move_mode": "top" }), "MOVE 옵션은 산다");
        assert!(!d.by.contains_key("FLY"));
        assert_eq!(d.situations["multi_pick"], json!({ "lift_up_partial": true }));
        assert!(!d.situations.contains_key("moon"));
        assert_eq!(w.len(), 4, "{w:?}");
        let (d2, w2) = load_lenient("not json");
        assert_eq!(d2.base, TaskParams::default());
        assert_eq!(w2.len(), 1);
    }

    #[test]
    fn diff_lists_changed_paths() {
        let a = Defaults::default();
        let mut b = a.clone();
        b.base.grip_height = 55;
        b.by.entry("PICK".into()).or_default().insert("cell".into(), json!({ "avoid": true }));
        let d = diff(&a, &b);
        assert!(d.iter().any(|l| l.starts_with("base.grip_height: 40 → 55")), "{d:?}");
        assert!(d.iter().any(|l| l.starts_with("by.PICK.cell.avoid: — → true")), "{d:?}");
    }
}
