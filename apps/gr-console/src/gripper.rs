//! `/api/robots/{id}/gripper` — GR2 그리퍼 토크 제어(`FB_CL_Gripper`) 모니터링.
//!
//! - `live` : `WEBMON.Gripper`(웹몬 주기 스냅샷) 그대로 + G 축 위치(`WEBMON.Axis[G].Position`) + 코드 이름
//!   (`CodeName`/`ModeName`/`ErrorName`/`TimeoutName` — 계약의 `LGR_Const_Gripper` 상수에서 푼다).
//!   실시간은 `/api/status/stream` 의 `webmon.Gripper` 가 같은 값이라 화면은 그쪽을 쓰고, 여기는 한 번 읽기용.
//! - `tune` : `GRIP_TUNE.Tune`(느린 주기) — `Mech[2][34]` 는 JSON 0-based(`Mech[0]` = PLC `Mech[1]` 파지 속도 곡선,
//!   `Mech[1]` = `Mech[2]` 느린 측정 속도). `bins` 가 곡선의 x 축(`PARA.Drive.RangeMin/RangeMax["G"]` 를 34 칸)을 말한다.
//! - `para` : 그리퍼 관련 `PARA.Machine.G_*` / `PARA.Task.G_*` / `PARA.Sensor.G_Inch*` 값, `inch_table` 은 인치별 표 + PLC 적용 인치(`Band`).
//!
//! LEARN 명령은 `POST /api/robots/{id}/command/gripper-learn`(`ledger::robot_cmd`) — OPC UA `Command.B3_Spare.Spare_X0` 1 s 펄스.
//! 읽기 전용이다 — 인치별 % 는 PARA(HMI/CSV)가, 학습 곡선은 PLC LEARN 이 쓴다(콘솔의 GRIP_TUNE 쓰기는 V1.6.1 에서 없앴다).

use axum::Router;
use axum::extract::{Path, State};
use axum::routing::get;
use plc_layout::Contract;
use serde_json::{Map, Value as Json, json};

use crate::error::{ApiError, ApiResult};
use crate::plc::{PlcHandle, ensure_db};
use crate::state::AppState;

const WEBMON: &str = "WEBMON";
const TUNE_DB: &str = "GRIP_TUNE";
const PARA_DB: &str = "PARA";
/// `LGR_GripperTune.Mech` 의 위치 칸 수(`Array[1..2, 0..33]`).
pub const MECH_BINS: usize = 34;
/// 인치별 토크 표 범위(`PARA.Sensor.G_Inch12..24_*`).
pub const INCH_LO: usize = 12;
pub const INCH_HI: usize = 24;
/// G 축 = `Axis["X".."G"]` 의 네 번째(JSON 0-based 3).
const G_AXIS: usize = 3;

const PARA_MACHINE: [&str; 6] = ["G_TorqRefDia", "G_OpenTorq_Nm", "G_CloseTorq_Nm", "G_MeasureTorq_Nm", "G_ProtectForce_N", "G_TorqMaxPct"];
const PARA_TASK: [&str; 17] = [
    "G_OpenTorq_Pct",
    "G_CloseTorq_Pct",
    "G_MeasureTorq_Pct",
    "G_RetryTorq_Pct",
    "G_MeasureApproachMargin",
    "G_MeasureSlowSpd",
    "G_ContactDwell",
    "G_ContactFactor_k",
    "G_ObstacleMargin",
    "G_GripRetryMax",
    "G_AtSpeedTol",
    "G_HoldTorqPct",
    "G_ErrorHoldTime",
    "G_HoldFactor",
    "G_HoldReduceTime",
    "G_MotorTempMax",
    "G_LoadAvgMax",
];

/// `PARA.Sensor.G_Inch{12..24}_{OpenPct,MeasPct}` 이름들 — OpenPct p450~p462, MeasPct p463~p475 순서.
fn inch_keys() -> Vec<String> {
    let mut v: Vec<String> = (INCH_LO..=INCH_HI).map(|i| format!("G_Inch{i}_OpenPct")).collect();
    v.extend((INCH_LO..=INCH_HI).map(|i| format!("G_Inch{i}_MeasPct")));
    v
}

/// 인치별 수동 토크 표(p450~p475). 적용 인치는 PLC 판정 `WEBMON.Gripper.Band`(규격 내경/25.4 반올림, 12..24 끝값, 0 = 자동)를
/// 그대로 쓴다 — 콘솔이 다시 계산하지 않는다. 그 인치의 % 가 0 이면 자동(사양 Nm 환산).
/// `applied` = active & `Fallback` & (OpenPct > 0 | MeasPct > 0).
pub fn inch_table(para: Option<&Json>, band: i64, fallback: bool) -> Json {
    let s = para.and_then(|p| p.get("Sensor"));
    let f = |k: &str| s.and_then(|s| s.get(k)).and_then(Json::as_f64).unwrap_or(0.0);
    let rows: Vec<Json> = (INCH_LO..=INCH_HI)
        .map(|inch| {
            let (open, meas) = (f(&format!("G_Inch{inch}_OpenPct")), f(&format!("G_Inch{inch}_MeasPct")));
            let active = band == inch as i64;
            json!({ "inch": inch, "open_pct": open, "meas_pct": meas, "active": active, "applied": active && fallback && (open > 0.0 || meas > 0.0) })
        })
        .collect();
    json!({ "band": band, "band_source": "WEBMON.Gripper.Band (PLC 판정)", "rows": rows })
}

/// 계약 상수 `GRIP_ST_*` 같은 접두어 묶음에서 값 → 이름. 접두어를 뗀 이름(`HOLDING`)으로 돌려주고, 없으면 숫자 그대로.
/// `exclude` 는 같은 접두어로 시작하는 다른 묶음(`GRIP_` 모드 이름에서 `GRIP_ST_`·`GRIP_E_`·`GRIP_TO_`·`GRIP_OWNER_`)을 뺀다.
pub fn const_name(consts: &std::collections::HashMap<String, i64>, prefix: &str, exclude: &[&str], value: i64) -> String {
    let mut hit: Option<&str> = None;
    for (k, v) in consts {
        if *v != value || !k.starts_with(prefix) || exclude.iter().any(|x| k.starts_with(x)) {
            continue;
        }
        let name = &k[prefix.len()..];
        // 같은 값이 둘이면 짧은 이름(정본)을 고른다 — 결정적이어야 화면이 흔들리지 않는다.
        if hit.is_none_or(|h| name.len() < h.len() || (name.len() == h.len() && name < h)) {
            hit = Some(name);
        }
    }
    hit.map(str::to_string).unwrap_or_else(|| value.to_string())
}

fn code_names(c: &Contract, live: &Json) -> Map<String, Json> {
    let i = |k: &str| live.get(k).and_then(Json::as_i64).unwrap_or(0);
    let mut m = Map::new();
    m.insert("CodeName".into(), json!(const_name(&c.consts, "GRIP_ST_", &[], i("Code"))));
    m.insert("ModeName".into(), json!(const_name(&c.consts, "GRIP_", &["GRIP_ST_", "GRIP_E_", "GRIP_TO_", "GRIP_OWNER_"], i("Mode"))));
    m.insert("ErrorName".into(), json!(const_name(&c.consts, "GRIP_E_", &[], i("ErrorCode"))));
    m.insert("TimeoutName".into(), json!(const_name(&c.consts, "GRIP_TO_", &[], i("Timeout"))));
    // 상수표에 GRIP_OWNER_NONE 이 없다 — 0 은 "아무도 안 쥠".
    let owner = match i("Owner") {
        0 => "NONE".to_string(),
        v => const_name(&c.consts, "GRIP_OWNER_", &[], v),
    };
    m.insert("OwnerName".into(), json!(owner));
    m.insert("RejectName".into(), json!(const_name(&c.consts, "GRIP_E_", &[], i("Reject"))));
    m
}

/// `WEBMON.Gripper` + G 축 위치 + 이름들.
fn live_json(c: &Contract, webmon: &Json) -> Json {
    let mut live = webmon.get("Gripper").cloned().unwrap_or_else(|| Json::Object(Map::new()));
    let g_pos = webmon.pointer(&format!("/Axis/{G_AXIS}/Position")).cloned().unwrap_or(Json::Null);
    let g_target = webmon.pointer(&format!("/Axis/{G_AXIS}/Target")).cloned().unwrap_or(Json::Null);
    if let Some(o) = live.as_object_mut() {
        o.insert("GPos".into(), g_pos);
        o.insert("GTarget".into(), g_target);
        o.extend(code_names(c, &Json::Object(o.clone())));
    }
    live
}

/// 곡선의 x 축 — `PARA.Drive.RangeMin/RangeMax["G"]` 를 `MECH_BINS` 칸. PARA 가 없으면 설계 기본값(295~630).
fn bins(para: Option<&Json>) -> Json {
    let rng = |k: &str, d: f64| para.and_then(|p| p.pointer(&format!("/Drive/{k}/{G_AXIS}"))).and_then(Json::as_f64).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(d);
    let (lo, hi) = (rng("RangeMin", 295.0), rng("RangeMax", 630.0));
    let width = if hi > lo { (hi - lo) / MECH_BINS as f64 } else { 0.0 };
    json!({ "range_min": lo, "range_max": hi, "count": MECH_BINS, "width": width })
}

/// 그리퍼 관련 PARA 값만(없는 멤버는 뺀다).
fn para_json(para: Option<&Json>) -> Json {
    let mut out = Map::new();
    if let Some(p) = para {
        let inch: Vec<String> = inch_keys();
        let inch_refs: Vec<&str> = inch.iter().map(String::as_str).collect();
        for (grp, keys) in [("Machine", &PARA_MACHINE[..]), ("Task", &PARA_TASK[..]), ("Sensor", &inch_refs[..])] {
            if let Some(s) = p.get(grp) {
                for k in keys {
                    if let Some(v) = s.get(*k) {
                        out.insert((*k).to_string(), v.clone());
                    }
                }
            }
        }
    }
    Json::Object(out)
}

/// `GRIP_TUNE.Tune` — 설정에 없거나(GR1) 검증 실패면 `Err(사유)`.
fn tune_of(h: &PlcHandle) -> Result<(Json, String), String> {
    if !h.has_db(TUNE_DB) {
        return Err(format!("{} 설정에 {TUNE_DB} 가 없습니다 (gr-console.toml [[plcs]] slow 에 \"{TUNE_DB}\" 추가)", h.name()));
    }
    ensure_db(h, TUNE_DB).map_err(|e| e.to_string())?;
    let snap = h.snap();
    let d = snap.db(TUNE_DB).ok_or_else(|| format!("{}.{TUNE_DB} 를 아직 읽지 못했습니다", h.name()))?;
    Ok((d.json.get("Tune").cloned().unwrap_or(Json::Null), d.at.clone()))
}

async fn snapshot(State(st): State<AppState>, Path(robot): Path<u8>) -> ApiResult<Json> {
    let (r, h) = st.robot_and_plc(Some(robot))?;
    ensure_db(h, WEBMON)?;
    let snap = h.snap();
    let w = snap.db(WEBMON).ok_or_else(|| ApiError::PlcUnavailable(format!("{}.{WEBMON} not read yet", h.name())))?;
    let para = snap.db(PARA_DB).map(|p| &*p.json);
    let (tune, tune_at, tune_error) = match tune_of(h) {
        Ok((t, at)) => (t, Some(at), None),
        Err(e) => (Json::Null, None, Some(e)),
    };
    let live = live_json(&h.contract, &w.json);
    let band = live.get("Band").and_then(Json::as_i64).unwrap_or(0);
    let fallback = live.get("Fallback").and_then(Json::as_bool).unwrap_or(false);
    Ok(axum::Json(json!({
        "robot": r.id,
        "robot_name": r.name,
        "plc": h.name(),
        "at": w.at,
        "live": live,
        "bins": bins(para),
        "inch_table": inch_table(para, band, fallback),
        "tune": tune,
        "tune_at": tune_at,
        "tune_error": tune_error,
        "para": para_json(para),
    })))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/robots/{id}/gripper", get(snapshot))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn consts() -> std::collections::HashMap<String, i64> {
        [("GRIP_ST_IDLE", 0), ("GRIP_ST_HOLDING", 50), ("GRIP_LEARN", 6), ("GRIP_ST_LEARNING", 65), ("GRIP_E_LEARN", 8), ("GRIP_OWNER_LEARN", 5), ("GRIP_TO_NONE", 0)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect()
    }

    #[test]
    fn names_resolve_per_group() {
        let c = consts();
        assert_eq!(const_name(&c, "GRIP_ST_", &[], 50), "HOLDING");
        assert_eq!(const_name(&c, "GRIP_", &["GRIP_ST_", "GRIP_E_", "GRIP_TO_", "GRIP_OWNER_"], 6), "LEARN");
        assert_eq!(const_name(&c, "GRIP_E_", &[], 8), "LEARN");
        assert_eq!(const_name(&c, "GRIP_OWNER_", &[], 5), "LEARN");
        assert_eq!(const_name(&c, "GRIP_ST_", &[], 99), "99");
    }

    #[test]
    fn bins_follow_para_range_or_default() {
        let p = json!({ "Drive": { "RangeMin": [0.0, 0.0, 0.0, 300.0], "RangeMax": [0.0, 0.0, 0.0, 640.0] } });
        let b = bins(Some(&p));
        assert_eq!((b["range_min"].as_f64(), b["range_max"].as_f64(), b["count"].as_u64()), (Some(300.0), Some(640.0), Some(34)));
        assert!((b["width"].as_f64().unwrap() - 10.0).abs() < 1e-9);
        let d = bins(None);
        assert_eq!((d["range_min"].as_f64(), d["range_max"].as_f64()), (Some(295.0), Some(630.0)));
    }

    #[test]
    fn para_keeps_only_gripper_members() {
        let p = json!({ "Machine": { "G_TorqRefDia": 508.0, "ID": 2 }, "Task": { "G_HoldFactor": 100.0, "Z_Something": 1 }, "Sensor": { "G_Inch20_OpenPct": 20.0, "GIDL_ZOffset": 3.0 } });
        assert_eq!(para_json(Some(&p)), json!({ "G_TorqRefDia": 508.0, "G_HoldFactor": 100.0, "G_Inch20_OpenPct": 20.0 }));
        let keys = inch_keys();
        assert_eq!(keys.len(), 26);
        assert_eq!((keys[0].as_str(), keys[12].as_str(), keys[13].as_str(), keys[25].as_str()), ("G_Inch12_OpenPct", "G_Inch24_OpenPct", "G_Inch12_MeasPct", "G_Inch24_MeasPct"));
    }

    #[test]
    fn inch_table_follows_plc_band_and_zero_pct_is_auto() {
        let p = json!({ "Sensor": { "G_Inch15_OpenPct": 35.0, "G_Inch20_OpenPct": 20.0, "G_Inch20_MeasPct": 0.0 } });
        // Band 20 + Fallback → 20" 행만 active·applied; 나머지 12 행은 비활성
        let t = inch_table(Some(&p), 20, true);
        let rows = t["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 13);
        assert_eq!(rows[0]["inch"], json!(12));
        assert_eq!((rows[8]["inch"].as_i64(), rows[8]["active"].as_bool(), rows[8]["applied"].as_bool()), (Some(20), Some(true), Some(true)));
        assert_eq!(rows.iter().filter(|r| r["active"] == json!(true)).count(), 1);
        // Fallback 이 아니면 active 만; % 가 0 인 인치(13)는 active 여도 applied 가 아니다; Band 0 = 자동 → 전부 비활성
        assert_eq!(inch_table(Some(&p), 20, false)["rows"][8]["applied"].as_bool(), Some(false));
        assert_eq!(inch_table(Some(&p), 13, true)["rows"][1]["applied"].as_bool(), Some(false));
        assert!(inch_table(Some(&p), 0, true)["rows"].as_array().unwrap().iter().all(|r| r["active"] == json!(false)));
        assert_eq!(inch_table(None, 20, true)["band"], json!(20));
    }
}
