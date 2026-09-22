//! 레이아웃 맵용 GRM 스테이션 실시간 상태 — 슬롯 `id MOD 100` 의 `Status`·`Tracking.Now`·`Interlock`.
//!
//! 값은 GRM `Comm_CV` 가 매 스캔 `STATION.Station[n]` → `OPCUA.STATION[n]` 으로 복사하는 것 그대로다
//! (fast 부분 읽기 대상). 인터록 비트 뜻(`Comm_CV`, `FB_Station`):
//! - PI (CV → GRM): CVOK 컨베이어 정상 · Req 로봇 작업 요청 · MeasReq 측정 요청 · ItemExist 화물 있음
//! - PO (GRM → CV): CVNO 로봇 진입(컨베이어 정지 요구) · Comp 로봇 작업 완료 · MeasComp 측정 완료 · MeasErr 측정 오류
//!
//! `Status.State`(FB_Station): 0 대기 · 1 화물 수신 대기 · 2 측정 중 · 4 트래킹 보유 · 5 완료 · 10 오류.
use std::sync::{LazyLock, Mutex, PoisonError};

use serde::Serialize;
use serde_json::Value as Json;
use tokio::sync::broadcast;

use crate::error::ApiError;
use crate::state::AppState;

use super::station_offset;

/// GRM 스냅샷에서 찾은 스테이션 원소 하나.
pub(crate) struct GrmStationEl {
    pub el: Json,
    pub db: String,
    pub at: String,
    pub disconnected: bool,
}

/// GRM 슬롯 `id MOD 100` 원소 — fast `OPCUA.STATION[n]` 우선, 그 슬롯 Id 가 다르거나 없으면 slow
/// `STATION.Station[n]`. JSON 배열은 0 부터라 PLC 인덱스 n 은 `[n-1]`.
pub(crate) fn grm_station_el(st: &AppState, id: u16) -> Result<GrmStationEl, String> {
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
        return Ok(GrmStationEl { el: el.clone(), db: db.to_string(), at: d.at.clone(), disconnected: !h.health_now().connected });
    }
    Err(if seen.is_empty() { "GRM 스냅샷 없음".into() } else { format!("GRM 슬롯 Id 불일치: {} — 스테이션 푸시 필요", seen.join(", ")) })
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct StationPi {
    pub cvok: bool,
    pub req: bool,
    pub meas_req: bool,
    pub item_exist: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct StationPo {
    pub cvno: bool,
    pub comp: bool,
    pub meas_comp: bool,
    pub meas_err: bool,
}

/// `GET /api/stations/live` 한 줄. `live = false` 면 `why` 만 의미가 있다.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct StationLive {
    pub id: u16,
    pub live: bool,
    pub why: Option<String>,
    pub source: String,
    /// 스냅샷 나이 — `GET` 에만 싣는다. 스트림은 변화만 보내므로 비운다(`stale` 로 대신).
    pub age_ms: Option<i64>,
    pub disconnected: bool,
    /// GRM 이 끊겼거나 스냅샷이 `STALE_MS` 보다 오래됨.
    pub stale: bool,
    /// GRM `Para.RotateType` — 화물 그림의 벽 쪽(레지스트리 값이 아니라 실제값).
    pub rotate_type: u8,
    /// 측정 센서가 있는 스테이션(`SensorSettings.IOLinkMasterModule > 0`) — 측정 트래킹을 스스로 만든다.
    pub sensor: bool,
    pub state: u8,
    pub error_code: u8,
    pub item_detect: bool,
    pub has_tracking: bool,
    pub measuring: bool,
    pub measuring_error: bool,
    pub data_mismatch: bool,
    /// `Tracking.Now` — 측정 OD·흐름 가로 오프셋.
    pub od: f32,
    pub tx: f32,
    pub ty: f32,
    /// GRM `Para.ConnectionPrev` / `ConnectionNext` — 슬롯 번호(0 = 없음). 콘솔 레지스트리 값이 아니라 GRM 실제값.
    pub prev: u8,
    pub next: u8,
    /// 화물이 넘어갈 다음 스테이션 Id(GRM 연결 + 설정 `[conveyor] extra_links`, `stock::conveyor::links`).
    /// 0 = 라인 끝, null = 연결 없음.
    pub next_id: Option<u16>,
    pub pi: StationPi,
    pub po: StationPo,
}

fn b(j: &Json) -> bool {
    j.as_bool().unwrap_or(false)
}

fn f(j: &Json) -> f32 {
    j.as_f64().unwrap_or(0.0) as f32
}

/// 스냅샷 원소 → 한 줄(순수 함수).
pub fn live_of(id: u16, el: &Json) -> StationLive {
    let s = &el["Status"];
    let now = &el["Tracking"]["Now"];
    let pi = &el["Interlock"]["PI"];
    let po = &el["Interlock"]["PO"];
    StationLive {
        id,
        live: true,
        state: s["State"].as_u64().unwrap_or(0) as u8,
        error_code: s["ErrorCode"].as_u64().unwrap_or(0) as u8,
        item_detect: b(&s["ItemDetect"]),
        has_tracking: b(&s["HasTrackingData"]),
        measuring: b(&s["Measuring"]),
        measuring_error: b(&s["MeasuringError"]),
        data_mismatch: s["DataMissMatch"].as_array().is_some_and(|a| a.iter().any(|x| x.as_bool() == Some(true))),
        od: f(&now["OutterDiameter"]),
        tx: f(&now["TaskOffset"][0]),
        ty: f(&now["TaskOffset"][1]),
        prev: el["Para"]["ConnectionPrev"].as_u64().unwrap_or(0) as u8,
        next: el["Para"]["ConnectionNext"].as_u64().unwrap_or(0) as u8,
        rotate_type: el["Para"]["RotateType"].as_u64().unwrap_or(0) as u8,
        sensor: el["Para"]["SensorSettings"]["IOLinkMasterModule"].as_u64().unwrap_or(0) > 0,
        pi: StationPi { cvok: b(&pi["CVOK"]), req: b(&pi["Req"]), meas_req: b(&pi["MeasReq"]), item_exist: b(&pi["ItemExist"]) },
        po: StationPo { cvno: b(&po["CVNO"]), comp: b(&po["Comp"]), meas_comp: b(&po["MeasComp"]), meas_err: b(&po["MeasErr"]) },
        ..Default::default()
    }
}

/// 스냅샷이 이보다 오래되면 `stale`.
pub const STALE_MS: i64 = 3000;

pub fn station_live_rows(st: &AppState) -> Result<Vec<StationLive>, ApiError> {
    let mut out = Vec::new();
    for e in st.registry.stations()? {
        out.push(match grm_station_el(st, e.id) {
            Ok(g) => {
                let age = super::age_ms(&g.at);
                StationLive { source: g.db, age_ms: age, stale: g.disconnected || age.is_some_and(|a| a > STALE_MS), disconnected: g.disconnected, ..live_of(e.id, &g.el) }
            }
            Err(why) => StationLive { id: e.id, why: Some(why), ..Default::default() },
        });
    }
    let next = crate::stock::conveyor::links(&out.iter().filter(|r| r.live).map(|r| (r.id, r.prev, r.next)).collect::<Vec<_>>(), &st.cfg.conveyor.extra_links);
    for r in out.iter_mut() {
        r.next_id = next.get(&r.id).copied().flatten();
    }
    Ok(out)
}

/// 변화만 내보내는 허브 — `stock::conveyor` 루프(250 ms)가 매번 `publish` 하고, 값이 바뀐 때만 보낸다.
/// 맵은 `GET /api/stations/live/stream` 하나로 받는다(폴링 없음, 바뀔 때 한 번).
struct Hub {
    tx: broadcast::Sender<Vec<StationLive>>,
    last: Mutex<Vec<StationLive>>,
}

static HUB: LazyLock<Hub> = LazyLock::new(|| Hub { tx: broadcast::channel(16).0, last: Mutex::new(Vec::new()) });

/// 나이(`age_ms`)는 매번 달라지므로 비교·전송에서 뺀다. 바뀌었으면 true.
pub fn publish(rows: &[StationLive]) -> bool {
    let norm: Vec<StationLive> = rows.iter().map(|r| StationLive { age_ms: None, ..r.clone() }).collect();
    let mut last = HUB.last.lock().unwrap_or_else(PoisonError::into_inner);
    if *last == norm {
        return false;
    }
    *last = norm.clone();
    let _ = HUB.tx.send(norm);
    true
}

pub fn snapshot() -> Vec<StationLive> {
    HUB.last.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

pub fn subscribe() -> broadcast::Receiver<Vec<StationLive>> {
    HUB.tx.subscribe()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_status_tracking_and_interlock_bits() {
        let el = json!({
            "Para": {"Info": {"Id": 2101}},
            "Status": {"State": 4, "ErrorCode": 0, "ItemDetect": true, "HasTrackingData": true,
                       "DataMissMatch": [false, true], "Measuring": false, "MeasuringError": false},
            "Tracking": {"Now": {"OutterDiameter": 640.5, "TaskOffset": [12.0, -3.0]}},
            "Interlock": {"PI": {"CVOK": true, "Req": true, "MeasReq": false, "ItemExist": true},
                          "PO": {"CVNO": false, "Comp": true, "MeasComp": true, "MeasErr": false}}
        });
        let l = live_of(2101, &el);
        assert!(l.live && l.has_tracking && l.item_detect && l.data_mismatch);
        assert_eq!((l.state, l.od, l.tx, l.ty), (4, 640.5, 12.0, -3.0));
        assert_eq!(l.pi, StationPi { cvok: true, req: true, meas_req: false, item_exist: true });
        assert_eq!(l.po, StationPo { cvno: false, comp: true, meas_comp: true, meas_err: false });
    }

    #[test]
    fn publish_sends_only_changes_and_ignores_age() {
        let row = StationLive { id: 2101, live: true, age_ms: Some(100), ..Default::default() };
        assert!(publish(std::slice::from_ref(&row)), "first value");
        assert!(!publish(&[StationLive { age_ms: Some(900), ..row.clone() }]), "age alone is not a change");
        assert!(publish(&[StationLive { pi: StationPi { item_exist: true, ..Default::default() }, ..row.clone() }]));
        assert_eq!(snapshot()[0].age_ms, None, "the stream never carries age");
    }

    #[test]
    fn missing_fields_read_as_zero() {
        let l = live_of(2102, &json!({}));
        assert!(l.live && !l.has_tracking && !l.pi.cvok);
        assert_eq!(l.od, 0.0);
    }
}
