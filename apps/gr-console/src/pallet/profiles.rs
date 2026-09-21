//! 팔렛 설정(콘솔 소유) — 패턴은 **품목**, 드래그 방향 보정은 **로봇**, 팔렛 여부는 **스테이션**(결정 2026-09-21).
//!
//! - `pallet_item`: 품목 코드별 입고/출하 Flow · Pattern(비면 OD 자동) · Gap · 배치 Rotation/Mirror · PalletSize.
//!   위치 좌표는 GR1·GR2 가 같이 쓴다.
//! - `pallet_robot`: 헤드 방향이 로봇마다 달라 드래그 인/아웃 방향만 다르다 → 방향 코드에만 거는 변환(없으면 그대로).
//! - `pallet_station`: 켜진 스테이션만 compose 가 팔렛 슬롯을 쓴다(그 전에는 기존 스테이션 동작 그대로).

use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use super::{DEFAULT_GAP, DEFAULT_PALLET_SIZE, Library, Transform};
use crate::db::Db;
use crate::error::ApiError;
use crate::util::now_str;
use gr_proto::TaskType;

pub const GAP_MAX: f32 = 500.0;
pub const PALLET_SIZE_MIN: f32 = 500.0;
pub const PALLET_SIZE_MAX: f32 = 4000.0;
const NOTE_MAX: usize = 500;

fn check_note(note: &str) -> Result<(), String> {
    if note.chars().count() > NOTE_MAX {
        return Err(format!("note is longer than {NOTE_MAX} characters"));
    }
    Ok(())
}

fn blank_none(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

// ── 품목 ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ItemPallet {
    pub code: u32,
    /// PICK/MEASURE 에 쓰는 흐름(보통 입고 `in`). 없으면 `flow_out`.
    pub flow_in: Option<String>,
    /// DROP 에 쓰는 흐름(보통 출하 `out`). 없으면 `flow_in`.
    pub flow_out: Option<String>,
    /// `None` = OuterDiameter 로 자동.
    pub pattern: Option<u8>,
    pub gap: f32,
    pub rotation: u16,
    pub mirror_x: bool,
    pub mirror_y: bool,
    pub pallet_size: f32,
    pub note: String,
    pub updated_at: String,
}

impl Default for ItemPallet {
    fn default() -> Self {
        ItemPallet {
            code: 0,
            flow_in: None,
            flow_out: None,
            pattern: None,
            gap: DEFAULT_GAP,
            rotation: 0,
            mirror_x: false,
            mirror_y: false,
            pallet_size: DEFAULT_PALLET_SIZE,
            note: String::new(),
            updated_at: String::new(),
        }
    }
}

impl ItemPallet {
    /// 배치 변환(좌표와 방향 둘 다) — GR1·GR2 공통.
    pub fn transform(&self) -> Transform {
        Transform { rotation: self.rotation, mirror_x: self.mirror_x, mirror_y: self.mirror_y }
    }

    /// 작업 종류에 맞는 흐름 — DROP 은 출하 먼저, 그 밖은 입고 먼저. 둘 다 없으면 `None`.
    pub fn flow_for(&self, tt: TaskType) -> Option<&str> {
        let (a, b) = match tt {
            TaskType::Drop => (&self.flow_out, &self.flow_in),
            _ => (&self.flow_in, &self.flow_out),
        };
        a.as_deref().or(b.as_deref())
    }

    /// Flow 는 편집 저장소(`lib`)에 있어야 한다. 입고·출하 중 하나는 있어야 한다.
    pub fn validate(&self, lib: &Library) -> Result<(), String> {
        if self.code == 0 {
            return Err("code is required".into());
        }
        if self.flow_in.is_none() && self.flow_out.is_none() {
            return Err("flow_in 또는 flow_out 중 하나는 있어야 합니다".into());
        }
        for f in [&self.flow_in, &self.flow_out].into_iter().flatten() {
            let Some(flow) = lib.flow(f) else {
                return Err(format!("unknown Flow {f} (available: {})", lib.ids()));
            };
            if let Some(p) = self.pattern
                && flow.pattern(p).is_none()
            {
                return Err(format!("Flow {} 에 Pattern {p} 이 없습니다", flow.id));
            }
        }
        self.transform().validate()?;
        if !self.gap.is_finite() || !(0.0..=GAP_MAX).contains(&self.gap) {
            return Err(format!("Gap {} must be 0..={GAP_MAX}", self.gap));
        }
        if !self.pallet_size.is_finite() || !(PALLET_SIZE_MIN..=PALLET_SIZE_MAX).contains(&self.pallet_size) {
            return Err(format!("PalletSize {} must be {PALLET_SIZE_MIN}..={PALLET_SIZE_MAX}", self.pallet_size));
        }
        check_note(&self.note)
    }
}

fn item_row(r: &rusqlite::Row) -> rusqlite::Result<ItemPallet> {
    Ok(ItemPallet {
        code: r.get::<_, i64>(0)? as u32,
        flow_in: r.get(1)?,
        flow_out: r.get(2)?,
        pattern: r.get::<_, Option<i64>>(3)?.map(|p| p as u8),
        gap: r.get::<_, f64>(4)? as f32,
        rotation: r.get::<_, i64>(5)? as u16,
        mirror_x: r.get::<_, i64>(6)? != 0,
        mirror_y: r.get::<_, i64>(7)? != 0,
        pallet_size: r.get::<_, f64>(8)? as f32,
        note: r.get(9)?,
        updated_at: r.get(10)?,
    })
}

const ITEM_COLS: &str = "code, flow_in, flow_out, pattern, gap, rotation, mirror_x, mirror_y, pallet_size, note, updated_at";

// ── 로봇 ────────────────────────────────────────────────────────────────────

/// 로봇별 드래그 방향 변환 — 품목 배치 변환 뒤 방향 코드에만 건다(좌표는 그대로).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RobotDir {
    pub robot: u8,
    pub rotation: u16,
    pub mirror_x: bool,
    pub mirror_y: bool,
    pub note: String,
    pub updated_at: String,
}

impl RobotDir {
    pub fn transform(&self) -> Transform {
        Transform { rotation: self.rotation, mirror_x: self.mirror_x, mirror_y: self.mirror_y }
    }
    pub fn validate(&self) -> Result<(), String> {
        self.transform().validate()?;
        check_note(&self.note)
    }
}

fn robot_row(r: &rusqlite::Row) -> rusqlite::Result<RobotDir> {
    Ok(RobotDir {
        robot: r.get::<_, i64>(0)? as u8,
        rotation: r.get::<_, i64>(1)? as u16,
        mirror_x: r.get::<_, i64>(2)? != 0,
        mirror_y: r.get::<_, i64>(3)? != 0,
        note: r.get(4)?,
        updated_at: r.get(5)?,
    })
}

// ── 스테이션 ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PalletStation {
    pub station_id: u16,
    pub enabled: bool,
    pub note: String,
    pub updated_at: String,
}

impl Default for PalletStation {
    fn default() -> Self {
        PalletStation { station_id: 0, enabled: true, note: String::new(), updated_at: String::new() }
    }
}

impl PalletStation {
    pub fn validate(&self) -> Result<(), String> {
        if self.station_id == 0 {
            return Err("station_id is required".into());
        }
        check_note(&self.note)
    }
}

fn station_row(r: &rusqlite::Row) -> rusqlite::Result<PalletStation> {
    Ok(PalletStation { station_id: r.get::<_, i64>(0)? as u16, enabled: r.get::<_, i64>(1)? != 0, note: r.get(2)?, updated_at: r.get(3)? })
}

// ── 저장소 ──────────────────────────────────────────────────────────────────

pub struct Profiles {
    db: Db,
}

impl Profiles {
    pub fn new(db: Db) -> Profiles {
        Profiles { db }
    }

    pub fn items(&self) -> Result<Vec<ItemPallet>, ApiError> {
        Ok(self.db.with(|c| {
            let mut st = c.prepare(&format!("SELECT {ITEM_COLS} FROM pallet_item ORDER BY code"))?;
            let it = st.query_map([], item_row)?;
            it.collect()
        })?)
    }

    pub fn item(&self, code: u32) -> Result<Option<ItemPallet>, ApiError> {
        Ok(self.db.with(|c| c.query_row(&format!("SELECT {ITEM_COLS} FROM pallet_item WHERE code = ?1"), [code], item_row).optional())?)
    }

    /// 검증은 호출자(라우트)가 먼저 한다. Flow 는 저장소의 표기(대문자 id)로 맞춰 저장한다.
    pub fn upsert_item(&self, mut p: ItemPallet, lib: &Library) -> Result<ItemPallet, ApiError> {
        for f in [&mut p.flow_in, &mut p.flow_out] {
            *f = blank_none(f.take());
            if let Some(id) = f.as_ref().and_then(|v| lib.flow(v)).map(|fl| fl.id.clone()) {
                *f = Some(id);
            }
        }
        p.note = p.note.trim().to_string();
        p.updated_at = now_str();
        self.db.with(|c| {
            c.execute(
                &format!(
                    "INSERT INTO pallet_item ({ITEM_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) ON CONFLICT(code) DO UPDATE SET flow_in=excluded.flow_in, flow_out=excluded.flow_out, pattern=excluded.pattern, gap=excluded.gap, rotation=excluded.rotation, mirror_x=excluded.mirror_x, mirror_y=excluded.mirror_y, pallet_size=excluded.pallet_size, note=excluded.note, updated_at=excluded.updated_at"
                ),
                rusqlite::params![p.code, p.flow_in, p.flow_out, p.pattern, p.gap as f64, p.rotation, p.mirror_x as i64, p.mirror_y as i64, p.pallet_size as f64, p.note, p.updated_at],
            )
        })?;
        Ok(p)
    }

    pub fn remove_item(&self, code: u32) -> Result<bool, ApiError> {
        Ok(self.db.with(|c| c.execute("DELETE FROM pallet_item WHERE code = ?1", [code]))? > 0)
    }

    pub fn robots(&self) -> Result<Vec<RobotDir>, ApiError> {
        Ok(self.db.with(|c| {
            let mut st = c.prepare("SELECT robot, rotation, mirror_x, mirror_y, note, updated_at FROM pallet_robot ORDER BY robot")?;
            let it = st.query_map([], robot_row)?;
            it.collect()
        })?)
    }

    /// 저장된 행이 없으면 변환 없음(기본값).
    pub fn robot(&self, robot: u8) -> Result<RobotDir, ApiError> {
        let got = self.db.with(|c| c.query_row("SELECT robot, rotation, mirror_x, mirror_y, note, updated_at FROM pallet_robot WHERE robot = ?1", [robot], robot_row).optional())?;
        Ok(got.unwrap_or(RobotDir { robot, ..Default::default() }))
    }

    pub fn upsert_robot(&self, mut r: RobotDir) -> Result<RobotDir, ApiError> {
        r.note = r.note.trim().to_string();
        r.updated_at = now_str();
        self.db.with(|c| {
            c.execute(
                "INSERT INTO pallet_robot (robot, rotation, mirror_x, mirror_y, note, updated_at) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(robot) DO UPDATE SET rotation=excluded.rotation, mirror_x=excluded.mirror_x, mirror_y=excluded.mirror_y, note=excluded.note, updated_at=excluded.updated_at",
                rusqlite::params![r.robot, r.rotation, r.mirror_x as i64, r.mirror_y as i64, r.note, r.updated_at],
            )
        })?;
        Ok(r)
    }

    pub fn stations(&self) -> Result<Vec<PalletStation>, ApiError> {
        Ok(self.db.with(|c| {
            let mut st = c.prepare("SELECT station_id, enabled, note, updated_at FROM pallet_station ORDER BY station_id")?;
            let it = st.query_map([], station_row)?;
            it.collect()
        })?)
    }

    pub fn station(&self, station_id: u16) -> Result<Option<PalletStation>, ApiError> {
        Ok(self.db.with(|c| c.query_row("SELECT station_id, enabled, note, updated_at FROM pallet_station WHERE station_id = ?1", [station_id], station_row).optional())?)
    }

    pub fn upsert_station(&self, mut s: PalletStation) -> Result<PalletStation, ApiError> {
        s.note = s.note.trim().to_string();
        s.updated_at = now_str();
        self.db.with(|c| {
            c.execute(
                "INSERT INTO pallet_station (station_id, enabled, note, updated_at) VALUES (?1,?2,?3,?4) ON CONFLICT(station_id) DO UPDATE SET enabled=excluded.enabled, note=excluded.note, updated_at=excluded.updated_at",
                rusqlite::params![s.station_id, s.enabled as i64, s.note, s.updated_at],
            )
        })?;
        Ok(s)
    }

    pub fn remove_station(&self, station_id: u16) -> Result<bool, ApiError> {
        Ok(self.db.with(|c| c.execute("DELETE FROM pallet_station WHERE station_id = ?1", [station_id]))? > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pallet::seed;

    #[test]
    fn item_crud_and_validation() {
        let lib = Library::from_seed(seed());
        let s = Profiles::new(Db::open_memory().unwrap());
        assert!(s.items().unwrap().is_empty());
        let p = ItemPallet { code: 1001, flow_in: Some("hp_in".into()), flow_out: Some(" op_out ".into()), gap: 30.0, rotation: 90, mirror_x: true, note: " test ".into(), ..Default::default() };
        assert!(p.validate(&lib).is_ok());
        let saved = s.upsert_item(p, &lib).unwrap();
        assert_eq!((saved.flow_in.as_deref(), saved.flow_out.as_deref(), saved.note.as_str()), (Some("HP_IN"), Some("OP_OUT"), "test"));
        let got = s.item(1001).unwrap().unwrap();
        assert_eq!(got, saved);
        assert_eq!(got.transform(), Transform { rotation: 90, mirror_x: true, mirror_y: false });
        assert_eq!((got.flow_for(TaskType::Pick), got.flow_for(TaskType::Drop), got.flow_for(TaskType::Measure)), (Some("HP_IN"), Some("OP_OUT"), Some("HP_IN")));
        // 한쪽만 있으면 다른 작업도 그 흐름을 쓴다
        let only_in = ItemPallet { flow_out: None, ..got.clone() };
        assert_eq!(only_in.flow_for(TaskType::Drop), Some("HP_IN"));
        s.upsert_item(ItemPallet { pattern: Some(4), ..got.clone() }, &lib).unwrap();
        assert_eq!(s.item(1001).unwrap().unwrap().pattern, Some(4));
        assert_eq!(s.items().unwrap().len(), 1);
        assert!(s.remove_item(1001).unwrap());
        assert!(!s.remove_item(1001).unwrap());
        assert!(s.item(1001).unwrap().is_none());

        let base = ItemPallet { code: 1, flow_in: Some("HP_IN".into()), ..Default::default() };
        assert_eq!(base.gap, 50.0, "default gap");
        assert!(base.validate(&lib).is_ok());
        assert!(ItemPallet { code: 0, ..base.clone() }.validate(&lib).is_err());
        assert!(ItemPallet { flow_in: None, ..base.clone() }.validate(&lib).is_err(), "at least one flow");
        assert!(ItemPallet { flow_in: Some("X".into()), ..base.clone() }.validate(&lib).is_err());
        assert!(ItemPallet { pattern: Some(7), ..base.clone() }.validate(&lib).is_err(), "no P7");
        assert!(ItemPallet { rotation: 45, ..base.clone() }.validate(&lib).is_err());
        assert!(ItemPallet { gap: -1.0, ..base.clone() }.validate(&lib).is_err());
        assert!(ItemPallet { gap: f32::NAN, ..base.clone() }.validate(&lib).is_err());
        assert!(ItemPallet { pallet_size: 100.0, ..base.clone() }.validate(&lib).is_err());
        // a flow that exists only in the edited store is valid; a removed seed flow is not
        let mut edited = lib.clone();
        edited.flows.retain(|f| f.id != "OP_IN_S5");
        edited.flows.push(crate::pallet::Flow { id: "SITE_A".into(), ..lib.flow("HP_IN").unwrap().clone() });
        assert!(ItemPallet { flow_in: Some("site_a".into()), ..base.clone() }.validate(&edited).is_ok());
        assert!(ItemPallet { flow_in: Some("OP_IN_S5".into()), ..base.clone() }.validate(&edited).is_err());
    }

    #[test]
    fn robot_and_station_crud() {
        let s = Profiles::new(Db::open_memory().unwrap());
        // 저장 전에는 변환 없음
        assert_eq!(s.robot(2).unwrap().transform(), Transform::default());
        s.upsert_robot(RobotDir { robot: 2, rotation: 180, ..Default::default() }).unwrap();
        assert_eq!(s.robot(2).unwrap().rotation, 180);
        assert_eq!(s.robots().unwrap().len(), 1);
        assert!(RobotDir { rotation: 30, ..Default::default() }.validate().is_err());

        assert!(s.station(2021).unwrap().is_none());
        assert!(PalletStation::default().validate().is_err());
        s.upsert_station(PalletStation { station_id: 2021, ..Default::default() }).unwrap();
        assert!(s.station(2021).unwrap().unwrap().enabled, "default on");
        s.upsert_station(PalletStation { station_id: 2021, enabled: false, ..Default::default() }).unwrap();
        assert!(!s.station(2021).unwrap().unwrap().enabled);
        assert!(s.remove_station(2021).unwrap());
        assert!(s.stations().unwrap().is_empty());
    }
}
