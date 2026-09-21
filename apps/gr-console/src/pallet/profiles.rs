//! 스테이션별 팔렛 프로파일(`pallet_profile`, 콘솔 소유) — Flow · Gap · Rotation · MirrorX/Y · PalletSize · Enabled.
//!
//! 현장에 팔렛 스테이션이 아직 정해지지 않아 기본 프로파일은 없다. 운전자가 화면에서 만들고 켜야
//! compose 가 그 스테이션에 팔렛 슬롯을 쓴다(그 전에는 기존 스테이션 동작 그대로).

use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use super::{DEFAULT_FLOW, DEFAULT_GAP, DEFAULT_PALLET_SIZE, Library, Transform};
use crate::db::Db;
use crate::error::ApiError;
use crate::util::now_str;

pub const GAP_MAX: f32 = 500.0;
pub const PALLET_SIZE_MIN: f32 = 500.0;
pub const PALLET_SIZE_MAX: f32 = 4000.0;
const NOTE_MAX: usize = 500;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub station_id: u16,
    pub flow: String,
    pub gap: f32,
    pub rotation: u16,
    pub mirror_x: bool,
    pub mirror_y: bool,
    pub pallet_size: f32,
    pub enabled: bool,
    pub note: String,
    pub updated_at: String,
}

impl Default for Profile {
    fn default() -> Self {
        Profile {
            station_id: 0,
            flow: DEFAULT_FLOW.into(),
            gap: DEFAULT_GAP,
            rotation: 0,
            mirror_x: false,
            mirror_y: false,
            pallet_size: DEFAULT_PALLET_SIZE,
            enabled: false,
            note: String::new(),
            updated_at: String::new(),
        }
    }
}

impl Profile {
    pub fn transform(&self) -> Transform {
        Transform { rotation: self.rotation, mirror_x: self.mirror_x, mirror_y: self.mirror_y }
    }

    /// Flow 는 편집 저장소(`lib`)에 있어야 한다.
    pub fn validate(&self, lib: &Library) -> Result<(), String> {
        if self.station_id == 0 {
            return Err("station_id is required".into());
        }
        if lib.flow(&self.flow).is_none() {
            return Err(format!("unknown Flow {} (available: {})", self.flow, lib.ids()));
        }
        self.transform().validate()?;
        if !self.gap.is_finite() || !(0.0..=GAP_MAX).contains(&self.gap) {
            return Err(format!("Gap {} must be 0..={GAP_MAX}", self.gap));
        }
        if !self.pallet_size.is_finite() || !(PALLET_SIZE_MIN..=PALLET_SIZE_MAX).contains(&self.pallet_size) {
            return Err(format!("PalletSize {} must be {PALLET_SIZE_MIN}..={PALLET_SIZE_MAX}", self.pallet_size));
        }
        if self.note.chars().count() > NOTE_MAX {
            return Err(format!("note is longer than {NOTE_MAX} characters"));
        }
        Ok(())
    }
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<Profile> {
    Ok(Profile {
        station_id: r.get::<_, i64>(0)? as u16,
        flow: r.get(1)?,
        gap: r.get::<_, f64>(2)? as f32,
        rotation: r.get::<_, i64>(3)? as u16,
        mirror_x: r.get::<_, i64>(4)? != 0,
        mirror_y: r.get::<_, i64>(5)? != 0,
        pallet_size: r.get::<_, f64>(6)? as f32,
        enabled: r.get::<_, i64>(7)? != 0,
        note: r.get(8)?,
        updated_at: r.get(9)?,
    })
}

const COLS: &str = "station_id, flow, gap, rotation, mirror_x, mirror_y, pallet_size, enabled, note, updated_at";

pub struct Profiles {
    db: Db,
}

impl Profiles {
    pub fn new(db: Db) -> Profiles {
        Profiles { db }
    }

    pub fn list(&self) -> Result<Vec<Profile>, ApiError> {
        Ok(self.db.with(|c| {
            let mut st = c.prepare(&format!("SELECT {COLS} FROM pallet_profile ORDER BY station_id"))?;
            let it = st.query_map([], row)?;
            it.collect()
        })?)
    }

    pub fn get(&self, station_id: u16) -> Result<Option<Profile>, ApiError> {
        Ok(self.db.with(|c| c.query_row(&format!("SELECT {COLS} FROM pallet_profile WHERE station_id = ?1"), [station_id], row).optional())?)
    }

    /// 검증은 호출자(라우트)가 먼저 한다. Flow 는 저장소의 표기(대문자 id)로 맞춰 저장한다.
    pub fn upsert(&self, mut p: Profile, lib: &Library) -> Result<Profile, ApiError> {
        if let Some(f) = lib.flow(&p.flow) {
            p.flow = f.id.clone();
        }
        p.note = p.note.trim().to_string();
        p.updated_at = now_str();
        self.db.with(|c| {
            c.execute(
                &format!(
                    "INSERT INTO pallet_profile ({COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(station_id) DO UPDATE SET flow=excluded.flow, gap=excluded.gap, rotation=excluded.rotation, mirror_x=excluded.mirror_x, mirror_y=excluded.mirror_y, pallet_size=excluded.pallet_size, enabled=excluded.enabled, note=excluded.note, updated_at=excluded.updated_at"
                ),
                rusqlite::params![p.station_id, p.flow, p.gap as f64, p.rotation, p.mirror_x as i64, p.mirror_y as i64, p.pallet_size as f64, p.enabled as i64, p.note, p.updated_at],
            )
        })?;
        Ok(p)
    }

    pub fn remove(&self, station_id: u16) -> Result<bool, ApiError> {
        Ok(self.db.with(|c| c.execute("DELETE FROM pallet_profile WHERE station_id = ?1", [station_id]))? > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pallet::seed;

    #[test]
    fn crud_and_validation() {
        let lib = Library::from_seed(seed());
        let s = Profiles::new(Db::open_memory().unwrap());
        assert!(s.list().unwrap().is_empty());
        let p = Profile { station_id: 2021, flow: "op_out".into(), gap: 30.0, rotation: 90, mirror_x: true, enabled: true, note: " test ".into(), ..Default::default() };
        assert!(p.validate(&lib).is_ok());
        let saved = s.upsert(p, &lib).unwrap();
        assert_eq!((saved.flow.as_str(), saved.note.as_str()), ("OP_OUT", "test"));
        let got = s.get(2021).unwrap().unwrap();
        assert_eq!(got, saved);
        assert_eq!(got.transform(), Transform { rotation: 90, mirror_x: true, mirror_y: false });
        // update in place
        s.upsert(Profile { enabled: false, ..got.clone() }, &lib).unwrap();
        assert!(!s.get(2021).unwrap().unwrap().enabled);
        assert_eq!(s.list().unwrap().len(), 1);
        assert!(s.remove(2021).unwrap());
        assert!(!s.remove(2021).unwrap());
        assert!(s.get(2021).unwrap().is_none());

        let base = Profile { station_id: 1, ..Default::default() };
        assert_eq!(base.gap, 50.0, "default gap");
        assert!(base.validate(&lib).is_ok());
        assert!(Profile { station_id: 0, ..base.clone() }.validate(&lib).is_err());
        assert!(Profile { flow: "X".into(), ..base.clone() }.validate(&lib).is_err());
        assert!(Profile { rotation: 45, ..base.clone() }.validate(&lib).is_err());
        assert!(Profile { gap: -1.0, ..base.clone() }.validate(&lib).is_err());
        assert!(Profile { gap: f32::NAN, ..base.clone() }.validate(&lib).is_err());
        assert!(Profile { pallet_size: 100.0, ..base.clone() }.validate(&lib).is_err());
        // a flow that exists only in the edited store is valid; a removed seed flow is not
        let mut edited = lib.clone();
        edited.flows.retain(|f| f.id != "OP_IN_S5");
        edited.flows.push(crate::pallet::Flow { id: "SITE_A".into(), ..lib.flow("HP_IN").unwrap().clone() });
        assert!(Profile { flow: "site_a".into(), ..base.clone() }.validate(&edited).is_ok());
        assert!(Profile { flow: "OP_IN_S5".into(), ..base.clone() }.validate(&edited).is_err());
    }
}
