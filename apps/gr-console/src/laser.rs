//! `/api/laser/*` — gripper laser sensor diagnostics (`LASERDIAG`) and the Z-offset auto calibration switch.
//!
//! Every route takes `?robot=<id>` (absent = default robot); a robot whose PLC has no `LASERDIAG` (GR1) gets a 400.
//! Reads come from the robot's status PLC slow-tier snapshot (`LASERDIAG` + `PARA.Sensor`). Writes flip one Bool
//! (`ZCal.Enable` or `Reset`) with a read-modify-write of that single byte; the PLC (`UL_LaserDiag`) does the
//! rest and clears `Reset` / `ZCal.Enable` itself.

use axum::Router;
use axum::extract::{Query, State};
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::{Value as Json, json};

use crate::error::{ApiError, ApiResult};
use crate::plc::{PlcHandle, ensure_db};
use crate::state::AppState;

const DB: &str = "LASERDIAG";
/// Ring size of `LASERDIAG.Entry` (0..49).
const RING: usize = 50;

/// `PARA.Sensor` members that belong to the laser diagnostics / calibration.
const PARA_KEYS: [&str; 15] = [
    "GIDL_ZOffset",
    "GIDF_ZOffset",
    "GIDR_ZOffset",
    "GIDB_ZOffset",
    "LaserDiag_SpreadLimit",
    "LaserDiag_PlanarLimit",
    "LaserDiag_JumpDist",
    "LaserDiag_JumpCountLimit",
    "LaserDiag_ConsecLimit",
    "LaserDiag_BiasLimit",
    "LaserDiag_BiasMinSamples",
    "LaserDiag_BiasAlpha",
    "LaserZCal_SampleCount",
    "LaserZCal_MaxStdDev",
    "LaserZCal_MaxCorrection",
];

/// Diagnostic entries newest first (`Head` = next write position, `Count` = valid entries).
fn ordered_entries(db: &Json) -> Vec<Json> {
    let entries = db["Entry"].as_array().cloned().unwrap_or_default();
    let n = entries.len().min(RING);
    if n == 0 {
        return Vec::new();
    }
    let head = db["Head"].as_i64().unwrap_or(0).rem_euclid(n as i64) as usize;
    let count = (db["Count"].as_i64().unwrap_or(0).max(0) as usize).min(n);
    (1..=count).map(|i| entries[(head + n - i) % n].clone()).collect()
}

/// The laser-related `PARA.Sensor` members (missing ones are left out).
fn para_sensor(para: Option<&Json>) -> Json {
    let mut out = serde_json::Map::new();
    if let Some(s) = para.and_then(|p| p.get("Sensor")) {
        for k in PARA_KEYS {
            if let Some(v) = s.get(k) {
                out.insert(k.into(), v.clone());
            }
        }
    }
    Json::Object(out)
}

fn set_bit(byte: u8, bit: u8, v: bool) -> u8 {
    if v { byte | (1 << bit) } else { byte & !(1 << bit) }
}

/// `?robot=<id>` — absent = default robot.
#[derive(Deserialize)]
struct RobotQuery {
    robot: Option<u8>,
}

async fn snapshot(State(st): State<AppState>, Query(q): Query<RobotQuery>) -> ApiResult<Json> {
    let (r, h) = st.robot_and_plc(q.robot)?;
    ensure_db(h, DB)?;
    let snap = h.snap();
    let Some(d) = snap.db(DB) else { return Err(ApiError::PlcUnavailable(format!("{}.{DB} not read yet", h.name()))) };
    let data = &*d.json;
    let para = snap.db("PARA").map(|p| &*p.json);
    Ok(axum::Json(json!({
        "robot": r.id,
        "plc": h.name(),
        "at": d.at,
        "total": data["Total"],
        "count": data["Count"],
        "sensor": data["Sensor"],
        "zcal": data["ZCal"],
        "entries": ordered_entries(data),
        "para": para_sensor(para),
    })))
}

#[derive(Deserialize)]
struct ZCalBody {
    enable: bool,
}

async fn zcal(State(st): State<AppState>, Query(q): Query<RobotQuery>, axum::Json(b): axum::Json<ZCalBody>) -> ApiResult<Json> {
    let (_, h) = st.robot_and_plc(q.robot)?;
    write_bool(&st, h, "ZCal.Enable", b.enable).await?;
    Ok(axum::Json(json!({ "ok": true, "plc": h.name(), "enable": b.enable })))
}

async fn reset(State(st): State<AppState>, Query(q): Query<RobotQuery>) -> ApiResult<Json> {
    let (_, h) = st.robot_and_plc(q.robot)?;
    write_bool(&st, h, "Reset", true).await?;
    Ok(axum::Json(json!({ "ok": true, "plc": h.name() })))
}

/// Sets one Bool member. The byte is read first so the PLC-owned bits next to it (`ZCal.Busy` / `Done` / `Error`)
/// keep their values; the window between read and write is one S7 round trip.
async fn write_bool(st: &AppState, h: &PlcHandle, path: &str, v: bool) -> Result<(), ApiError> {
    // 읽고-고쳐-쓰는 한 바이트 — 사이에 프로세스가 사라지면 옆 비트가 옛 값으로 덮인다.
    let _busy = st.shutdown.enter(format!("레이저 {DB}.{path} 쓰기 ({})", h.name()))?;
    if !h.has_db(DB) {
        return ensure_db(h, DB);
    }
    if !h.health_now().connected {
        return Err(ApiError::PlcUnavailable(format!("{} S7 not connected", h.name())));
    }
    ensure_db(h, DB)?;
    let m = h.layout(DB).and_then(|l| l.find(path)).ok_or_else(|| ApiError::Internal(format!("{DB} layout: no member {path}")))?;
    let bit = m.bit.ok_or_else(|| ApiError::Internal(format!("{DB}.{path} is not a Bool")))?;
    let cur = h.read(DB, m.offset, 1).await.map_err(|e| ApiError::PlcUnavailable(format!("{} read {DB}.{path}: {e}", h.name())))?;
    let byte = set_bit(cur.first().copied().unwrap_or(0), bit, v);
    h.write(DB, m.offset, vec![byte]).await.map_err(|e| ApiError::PlcUnavailable(format!("{} write {DB}.{path}: {e}", h.name())))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/laser", get(snapshot)).route("/api/laser/zcal", post(zcal)).route("/api/laser/reset", post(reset))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring(head: i64, count: i64) -> Json {
        let entries: Vec<Json> = (0..RING).map(|i| json!({ "Seq": i })).collect();
        json!({ "Head": head, "Count": count, "Entry": entries })
    }

    #[test]
    fn entries_newest_first_and_wrap() {
        let seqs = |db: &Json| ordered_entries(db).iter().map(|e| e["Seq"].as_i64().unwrap()).collect::<Vec<_>>();
        assert_eq!(seqs(&ring(3, 3)), vec![2, 1, 0]);
        assert_eq!(seqs(&ring(1, 3)), vec![0, 49, 48]);
        assert_eq!(seqs(&ring(0, 0)), Vec::<i64>::new());
        assert_eq!(seqs(&ring(7, 99)).len(), RING);
    }

    #[test]
    fn para_keeps_only_laser_members() {
        let p = json!({ "Sensor": { "GIDL_ZOffset": 1.25, "ItemDetectDistance_H": 800.0, "LaserZCal_SampleCount": 5.0 } });
        assert_eq!(para_sensor(Some(&p)), json!({ "GIDL_ZOffset": 1.25, "LaserZCal_SampleCount": 5.0 }));
        assert_eq!(para_sensor(None), json!({}));
    }

    #[test]
    fn bit_set_and_clear_keep_neighbours() {
        assert_eq!(set_bit(0b0000_0110, 0, true), 0b0000_0111);
        assert_eq!(set_bit(0b0000_0111, 0, false), 0b0000_0110);
        assert_eq!(set_bit(0b1000_0000, 3, true), 0b1000_1000);
    }
}
