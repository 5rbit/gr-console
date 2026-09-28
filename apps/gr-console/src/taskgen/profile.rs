//! 스테이션 프로파일 — 로봇이 이 스테이션에서 하는 일(역할)과 DROP 방식. 콘솔이 소유한다(PLC 에는 방향 필드가 없다:
//! `Req` 는 픽 스테이션이면 "화물 있음", 드롭 스테이션이면 "비어 있음" 을 뜻할 뿐 어느 쪽인지는 말하지 않는다).
//! 프로파일이 없는 스테이션은 정책이 보지 않는다 — 추정(`guess`)은 화면에서 적용할 때만 저장된다.

use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::ApiError;
use crate::util::now_str;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StationRole {
    Pick,
    Drop,
    Both,
    #[default]
    Off,
}

impl StationRole {
    pub fn picks(self) -> bool {
        matches!(self, StationRole::Pick | StationRole::Both)
    }
    pub fn drops(self) -> bool {
        matches!(self, StationRole::Drop | StationRole::Both)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            StationRole::Pick => "pick",
            StationRole::Drop => "drop",
            StationRole::Both => "both",
            StationRole::Off => "off",
        }
    }
}

/// DROP 방식 — 하나씩(비어 있어야) · 스택(스테이션 최대 높이와 품목 StackMax 까지) · 팔렛(다음 슬롯).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DropMode {
    #[default]
    Single,
    Stack,
    Pallet,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StationProfile {
    pub station_id: u16,
    pub role: StationRole,
    pub drop_mode: DropMode,
    /// 스택 스테이션에 쌓을 수 있는 최대 높이(mm, 0 = 높이 제한 없음). 단수 한도는 품목 규격(StackMax)이 따로 건다.
    pub max_height_mm: f32,
    pub weight: f32,
    /// 준비된 뒤 이만큼(초) 못 만들면 경고(0 = 없음).
    pub max_wait_s: u32,
    pub note: String,
    pub updated_at: String,
}

pub fn list(db: &Db) -> Result<Vec<StationProfile>, ApiError> {
    let docs: Vec<String> = db.with(|c| {
        let mut st = c.prepare("SELECT doc_json FROM station_profile ORDER BY station_id")?;
        let rows = st.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;
    Ok(docs.into_iter().filter_map(|d| serde_json::from_str(&d).ok()).collect())
}

pub fn save(db: &Db, mut p: StationProfile) -> Result<StationProfile, ApiError> {
    if !(2001..=2999).contains(&p.station_id) {
        return Err(ApiError::BadRequest(format!("스테이션 id {} 는 2001..2999 밖", p.station_id)));
    }
    p.note = p.note.trim().to_string();
    p.updated_at = now_str();
    let doc = serde_json::to_string(&p)?;
    db.with(|c| {
        c.execute(
            "INSERT INTO station_profile (station_id, doc_json, updated_at) VALUES (?1, ?2, ?3) ON CONFLICT(station_id) DO UPDATE SET doc_json = excluded.doc_json, updated_at = excluded.updated_at",
            (p.station_id, &doc, &p.updated_at),
        )
    })?;
    Ok(p)
}

pub fn remove(db: &Db, id: u16) -> Result<(), ApiError> {
    db.with(|c| c.execute("DELETE FROM station_profile WHERE station_id = ?1", [id]))?;
    Ok(())
}

/// GRM 연결(슬롯 번호)로 역할을 추정한다 — 라인 시작(prev 0 · next ≠ 0)에는 로봇이 내려놓고(DROP),
/// 라인 끝(next 0 · prev ≠ 0)에서는 집는다(PICK). 연결이 없으면 둘 다, 가운데는 끔. 팔렛 스테이션은 DROP + 팔렛.
pub fn guess(id: u16, prev: u8, next: u8, pallet: bool) -> (StationProfile, String) {
    let base = StationProfile { station_id: id, ..Default::default() };
    if pallet {
        return (StationProfile { role: StationRole::Drop, drop_mode: DropMode::Pallet, ..base }, "팔렛 스테이션".into());
    }
    let (role, why) = match (prev, next) {
        (0, 0) => (StationRole::Both, "컨베이어 연결 없음"),
        (0, _) => (StationRole::Drop, "라인 시작(ConnectionPrev 0) — 로봇이 내려놓는 자리"),
        (_, 0) => (StationRole::Pick, "라인 끝(ConnectionNext 0) — 로봇이 집는 자리"),
        _ => (StationRole::Off, "라인 가운데 — 지나가는 자리"),
    };
    (StationProfile { role, ..base }, why.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses_follow_the_conveyor_line() {
        assert_eq!(guess(2103, 0, 2, false).0.role, StationRole::Drop);
        assert_eq!(guess(2101, 2, 0, false).0.role, StationRole::Pick);
        assert_eq!(guess(2102, 3, 1, false).0.role, StationRole::Off);
        assert_eq!(guess(2201, 0, 0, false).0.role, StationRole::Both);
        let (p, _) = guess(2301, 0, 2, true);
        assert_eq!((p.role, p.drop_mode), (StationRole::Drop, DropMode::Pallet));
    }

    #[test]
    fn profiles_round_trip() {
        let db = Db::open_memory().unwrap();
        save(&db, StationProfile { station_id: 2101, role: StationRole::Pick, ..Default::default() }).unwrap();
        save(&db, StationProfile { station_id: 2102, role: StationRole::Drop, drop_mode: DropMode::Stack, max_height_mm: 1200.0, ..Default::default() }).unwrap();
        assert!(save(&db, StationProfile { station_id: 401, ..Default::default() }).is_err(), "cells have no profile");
        let all = list(&db).unwrap();
        assert_eq!(all.iter().map(|p| (p.station_id, p.role)).collect::<Vec<_>>(), vec![(2101, StationRole::Pick), (2102, StationRole::Drop)]);
        assert_eq!(all[1].max_height_mm, 1200.0);
        remove(&db, 2101).unwrap();
        assert_eq!(list(&db).unwrap().len(), 1);
        // 옛 문서(필드 모자람)도 읽힌다.
        let p: StationProfile = serde_json::from_str(r#"{"station_id":2105,"role":"both"}"#).unwrap();
        assert_eq!((p.role, p.drop_mode), (StationRole::Both, DropMode::Single));
    }
}
