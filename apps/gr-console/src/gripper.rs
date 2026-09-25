//! `/api/robots/{id}/gripper` — GR2 그리퍼 토크 제어(`FB_Gripper`) 모니터링.
//!
//! - `live` : `WEBMON.Gripper`(웹몬 주기 스냅샷) 그대로 + G 축 위치(`WEBMON.Axis[G].Position`) + 코드 이름
//!   (`CodeName`/`ModeName`/`ErrorName`/`TimeoutName` — 계약의 `LGR_Const_Gripper` 상수에서 푼다).
//!   실시간은 `/api/status/stream` 의 `webmon.Gripper` 가 같은 값이라 화면은 그쪽을 쓰고, 여기는 한 번 읽기용.
//! - `tune` : `GRIP_TUNE.Tune`(느린 주기) — `Mech[2][34]` 는 JSON 0-based(`Mech[0]` = PLC `Mech[1]` 파지 속도 곡선,
//!   `Mech[1]` = `Mech[2]` 느린 측정 속도), `ScaleByInch[13]` 의 0 번이 12 인치. `bins` 가 곡선의 x 축
//!   (`PARA.Drive.RangeMin/RangeMax["G"]` 를 34 칸)을 말한다.
//! - `para` : 그리퍼 관련 `PARA.Machine.G_*` / `PARA.Task.G_*` 값.
//!
//! LEARN 명령은 `POST /api/robots/{id}/command/gripper-learn`(`ledger::robot_cmd`) — OPC UA `Command.B3_Spare.Spare_X0` 1 s 펄스.
//! `PUT …/gripper/scale-by-inch` 는 `GRIP_TUNE.Tune.ScaleByInch[12..24]` 13 개 Real 을 한 번에 쓴다(표준 접근 DB, S7 직접 쓰기).

use axum::Router;
use axum::extract::{Path, State};
use axum::routing::{get, put};
use plc_layout::Contract;
use serde::Deserialize;
use serde_json::{Map, Value as Json, json};

use crate::error::{ApiError, ApiResult};
use crate::plc::{PlcHandle, ensure_db};
use crate::state::AppState;

const WEBMON: &str = "WEBMON";
const TUNE_DB: &str = "GRIP_TUNE";
const PARA_DB: &str = "PARA";
/// `LGR_GripperTune.Mech` 의 위치 칸 수(`Array[1..2, 0..33]`).
pub const MECH_BINS: usize = 34;
/// `ScaleByInch` 인치 범위(`Array[12..24]`).
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
        for (grp, keys) in [("Machine", &PARA_MACHINE[..]), ("Task", &PARA_TASK[..])] {
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
    Ok(axum::Json(json!({
        "robot": r.id,
        "robot_name": r.name,
        "plc": h.name(),
        "at": w.at,
        "live": live_json(&h.contract, &w.json),
        "bins": bins(para),
        "tune": tune,
        "tune_at": tune_at,
        "tune_error": tune_error,
        "para": para_json(para),
    })))
}

#[derive(Deserialize)]
struct ScaleBody {
    /// 12..24 인치 순서의 13 개(%). 0 또는 100 = 조정 없음(PLC 규칙).
    #[serde(rename = "ScaleByInch")]
    scale_by_inch: Vec<f64>,
}

/// 13 개 Real 을 S7 바이트열로 — 값은 유한해야 하고 음수는 뜻이 없다.
pub fn encode_scale(values: &[f64]) -> Result<Vec<u8>, String> {
    let n = INCH_HI - INCH_LO + 1;
    if values.len() != n {
        return Err(format!("ScaleByInch 는 {n} 개(12..24 인치)여야 합니다 — 받은 개수 {}", values.len()));
    }
    let mut out = Vec::with_capacity(n * 4);
    for (i, v) in values.iter().enumerate() {
        if !v.is_finite() || *v < 0.0 || *v > 1000.0 {
            return Err(format!("ScaleByInch[{}] = {v} — 0~1000 % 사이의 값이어야 합니다", INCH_LO + i));
        }
        out.extend_from_slice(&(*v as f32).to_be_bytes());
    }
    Ok(out)
}

async fn put_scale(State(st): State<AppState>, Path(robot): Path<u8>, axum::Json(b): axum::Json<ScaleBody>) -> ApiResult<Json> {
    let (r, h) = st.robot_and_plc_required(Some(robot), "그리퍼 ScaleByInch 쓰기")?;
    let bytes = encode_scale(&b.scale_by_inch).map_err(ApiError::BadRequest)?;
    let _busy = st.shutdown.enter(format!("그리퍼 {TUNE_DB}.Tune.ScaleByInch 쓰기 ({})", h.name()))?;
    if !h.has_db(TUNE_DB) {
        return Err(ApiError::BadRequest(format!("{} 설정에 {TUNE_DB} 가 없습니다 (gr-console.toml [[plcs]] slow 에 \"{TUNE_DB}\" 추가)", h.name())));
    }
    ensure_db(h, TUNE_DB)?;
    if !h.health_now().connected {
        return Err(ApiError::PlcUnavailable(format!("{} S7 not connected", h.name())));
    }
    let first = format!("Tune.ScaleByInch[{INCH_LO}]");
    let m = h.layout(TUNE_DB).and_then(|l| l.find(&first)).ok_or_else(|| ApiError::Internal(format!("{TUNE_DB} layout: no member {first}")))?;
    h.write(TUNE_DB, m.offset, bytes).await.map_err(|e| ApiError::PlcUnavailable(format!("{} write {TUNE_DB}.Tune.ScaleByInch: {e}", h.name())))?;
    tracing::info!(robot = %r.name, plc = h.name(), values = ?b.scale_by_inch, "gripper ScaleByInch written");
    Ok(axum::Json(json!({ "ok": true, "robot": r.id, "plc": h.name(), "ScaleByInch": b.scale_by_inch })))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/robots/{id}/gripper", get(snapshot)).route("/api/robots/{id}/gripper/scale-by-inch", put(put_scale))
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
        let p = json!({ "Machine": { "G_TorqRefDia": 508.0, "ID": 2 }, "Task": { "G_HoldFactor": 100.0, "Z_Something": 1 } });
        assert_eq!(para_json(Some(&p)), json!({ "G_TorqRefDia": 508.0, "G_HoldFactor": 100.0 }));
    }

    #[test]
    fn scale_encodes_13_reals_big_endian() {
        let v: Vec<f64> = (0..13).map(|i| 100.0 + i as f64).collect();
        let b = encode_scale(&v).unwrap();
        assert_eq!(b.len(), 52);
        assert_eq!(&b[..4], &100.0f32.to_be_bytes());
        assert!(encode_scale(&v[..12]).is_err());
        assert!(encode_scale(&[f64::NAN; 13]).is_err());
    }
}
