//! Registries: tire codes (items), cells, stations, defaults — sqlite-backed.
//!
//! - `plc_io`: PLC snapshot → local (import) and local → PLC S7 write + read-back verify (push)
//! - `diff`: local vs PLC classification (`same|local_only|plc_only|changed`)
//! - `xlsx`: Excel / CSV export + import (header-mapped, row-validated)
//! - `routes`: `/api/items`, `/api/cells`, `/api/stations`, `/api/registry`, `/api/defaults`, `/api/issue/compose`
//! - `bulk`: `/api/cells/bulk` 병합 계획(append / replace_section / overwrite)
//! - `beads`: PLC 의 SKU 측정(MEASLOG Kind 2) → 품목 단별 비드 표본 적재·검증·자동 반영

pub mod beads;
pub mod bulk;
pub mod defaults_io;
pub mod diff;
pub mod dims;
pub mod plc_io;
pub mod routes;
pub mod spec;
pub mod xlsx;

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use gr_proto::{CellInfo, StationPara, StockItem, TaskParams};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::db::Db;
use crate::error::ApiError;
use crate::util::now_str;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ItemEntry {
    pub code: u32,
    pub name: String,
    pub item: StockItem,
    pub note: String,
    /// 콘솔 소유 부가 규격(`spec.rs`) — PLC 로 가지 않는다.
    #[serde(default)]
    pub spec: spec::ItemSpec,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CellEntry {
    pub id: u16,
    pub cell: CellInfo,
    pub source: String,
    pub dirty: bool,
    pub plc_seen_at: Option<String>,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StationEntry {
    pub id: u16,
    pub para: StationPara,
    pub source: String,
    pub dirty: bool,
    pub plc_seen_at: Option<String>,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Defaults {
    pub version: u32,
    pub updated_at: String,
    pub base: TaskParams,
    /// by[task_type][target_kind] = partial params (snake_case)
    pub by: HashMap<String, HashMap<String, Json>>,
    /// Where the gripper takes the tire, as a Z offset from the tire bottom: `mid` = Height/2 (기본),
    /// `pick_bead`(화면 이름 `bead+offset`) = UpperBead − `ItemSpec.pick_bead_offset`(기본 30 mm).
    /// 한 번도 재 본 적 없는 품목은 mid 로 내려간다. 비드 값은 그 타이어 **위에 얹힌 개수**만큼 눌림이 반영된다.
    /// 옛 값 `bead` 는 읽을 때 `pick_bead` 로 옮긴다(`spec::normalize_grip_ref`).
    pub grip_ref: String,
    /// 상황별 덮어쓰기(부분 파라미터) — 종류·대상별(`by`) **위**, 작성 카드 덮어쓰기 **아래**에 얹힌다.
    /// 키는 [`SITUATIONS`] 순서로 적용되고(뒤가 이긴다), 판정은 `issue::situations_for`.
    pub situations: BTreeMap<String, Json>,
    /// 읽을 때 걷어 낸 것(깨진 값·모르는 키) — 화면이 알린다. 요청으로는 받지 않고, 저장하면 사라진다.
    #[serde(default, skip_deserializing, skip_serializing_if = "Vec::is_empty")]
    pub load_warnings: Vec<String>,
}

/// 기본값 이력 한 줄(목록용 — JSON 본문은 되돌릴 때만 읽는다).
#[derive(Clone, Debug, Serialize)]
pub struct DefaultsHistoryRow {
    pub id: i64,
    pub version: u32,
    pub at: String,
    pub note: String,
    /// 이 저장이 바꾼 것(앞 몇 줄).
    pub changes: Vec<String>,
}

/// 상황 키 — 적용 순서(뒤가 이긴다). 화면(`lib/task/compose.ts` `SITUATIONS`)과 같은 목록이다.
/// - `measure_item` : MEASURE + MeasureItem(측정 플래그가 없으면 auto — 대상 재고 1개 이하 Item, 2개 이상 SKU)
/// - `measure_sku`  : MEASURE + MeasureSku
/// - `pallet_station` : 켜진 팔렛 스테이션에 `pallet` 슬롯을 단 작업(팔렛타이징)
/// - `multi_pick`   : 스테이션 PICK/DROP 인데 다음 작업이 **같은 스테이션 그룹**(PLC DoublePicking — LiftUpPartial 로
///   셀 Z + LiftUpHeight 까지만 올라가 다음으로 이동). 계획·시나리오는 자동, 단일 명령은 요청의 `multi_pick`.
pub const SITUATIONS: [&str; 4] = ["measure_item", "measure_sku", "pallet_station", "multi_pick"];

fn default_situations() -> BTreeMap<String, Json> {
    let mut m = BTreeMap::new();
    for k in SITUATIONS {
        m.insert(k.to_string(), serde_json::json!({}));
    }
    // Multi-Picking 은 부분 리프트가 핵심이다 — 비워 두면 PLC 가 끝까지 올라가 DoublePicking 이 되지 않는다.
    m.insert("multi_pick".to_string(), serde_json::json!({ "lift_up_partial": true }));
    m
}

impl Default for Defaults {
    fn default() -> Self {
        let mut by: HashMap<String, HashMap<String, Json>> = HashMap::new();
        for t in ["PICK", "DROP"] {
            let mut m = HashMap::new();
            m.insert("cell".to_string(), serde_json::json!({}));
            m.insert("station".to_string(), serde_json::json!({ "find_station_item": t == "PICK" }));
            by.insert(t.into(), m);
        }
        Self { version: 1, updated_at: now_str(), base: TaskParams::default(), by, grip_ref: "mid".into(), situations: default_situations(), load_warnings: Vec::new() }
    }
}

pub struct Registry {
    db: Db,
}

impl Registry {
    pub fn new(db: Db) -> Arc<Registry> {
        Arc::new(Registry { db })
    }

    // ---- items
    pub fn items(&self) -> Result<Vec<ItemEntry>, ApiError> {
        type Row = (i64, String, String, String, String, String);
        let rows: Vec<Row> = self.db.with(|c| {
            let mut st = c.prepare("SELECT code, name, item_json, note, updated_at, spec_json FROM tire_codes ORDER BY code")?;
            let it = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?;
            it.collect()
        })?;
        Ok(rows
            .into_iter()
            .map(|(code, name, item, note, updated_at, spec)| {
                let item: StockItem = serde_json::from_str(&item).unwrap_or_default();
                let spec: spec::ItemSpec = serde_json::from_str(&spec).unwrap_or_default();
                ItemEntry { code: code as u32, name, item, note, spec, updated_at }
            })
            .collect())
    }
    pub fn item(&self, code: u32) -> Result<Option<ItemEntry>, ApiError> {
        Ok(self.items()?.into_iter().find(|i| i.code == code))
    }
    /// Upsert keeping the stored `spec` (new rows get the default spec).
    pub fn upsert_item(&self, code: u32, name: &str, item: &StockItem, note: &str) -> Result<ItemEntry, ApiError> {
        self.upsert_item_full(code, name, item, note, None)
    }
    /// `spec = None` leaves an existing row's spec untouched (old clients / old Excel files don't carry it).
    pub fn upsert_item_full(&self, code: u32, name: &str, item: &StockItem, note: &str, spec: Option<&spec::ItemSpec>) -> Result<ItemEntry, ApiError> {
        let mut item = item.clone();
        item.code = code;
        let now = now_str();
        let item_json = serde_json::to_string(&item).unwrap_or_default();
        self.db.with(|c| match spec {
            Some(s) => c.execute(
                "INSERT INTO tire_codes (code, name, item_json, note, updated_at, spec_json) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(code) DO UPDATE SET name=excluded.name, item_json=excluded.item_json, note=excluded.note, updated_at=excluded.updated_at, spec_json=excluded.spec_json",
                (code, name, &item_json, note, &now, serde_json::to_string(s).unwrap_or_else(|_| "{}".into())),
            ),
            None => c.execute(
                "INSERT INTO tire_codes (code, name, item_json, note, updated_at) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(code) DO UPDATE SET name=excluded.name, item_json=excluded.item_json, note=excluded.note, updated_at=excluded.updated_at",
                (code, name, &item_json, note, &now),
            ),
        })?;
        self.item(code)?.ok_or_else(|| ApiError::Internal(format!("item {code} vanished after upsert")))
    }
    /// Replaces only the spec of an existing item. `false` = no such item.
    pub fn set_item_spec(&self, code: u32, spec: &spec::ItemSpec) -> Result<bool, ApiError> {
        let json = serde_json::to_string(spec).unwrap_or_else(|_| "{}".into());
        Ok(self.db.with(|c| c.execute("UPDATE tire_codes SET spec_json = ?1, updated_at = ?2 WHERE code = ?3", (json, now_str(), code)))? > 0)
    }
    pub fn delete_item(&self, code: u32) -> Result<bool, ApiError> {
        Ok(self.db.with(|c| c.execute("DELETE FROM tire_codes WHERE code = ?1", [code]))? > 0)
    }

    // ---- cells
    pub fn cells(&self) -> Result<Vec<CellEntry>, ApiError> {
        let rows: Vec<(i64, String, String, i64, Option<String>, String)> = self.db.with(|c| {
            let mut st = c.prepare("SELECT id, source, cell_json, dirty, plc_seen_at, updated_at FROM cells ORDER BY id")?;
            let it = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?;
            it.collect()
        })?;
        Ok(rows
            .into_iter()
            .map(|(id, source, cell, dirty, seen, updated_at)| CellEntry {
                id: id as u16,
                cell: serde_json::from_str(&cell).unwrap_or_default(),
                source,
                dirty: dirty != 0,
                plc_seen_at: seen,
                updated_at,
            })
            .collect())
    }
    pub fn cell(&self, id: u16) -> Result<Option<CellEntry>, ApiError> {
        Ok(self.cells()?.into_iter().find(|c| c.id == id))
    }
    pub fn upsert_cell(&self, cell: &CellInfo, source: &str, dirty: bool, plc_seen_at: Option<String>) -> Result<CellEntry, ApiError> {
        let now = now_str();
        self.db.with(|c| {
            c.execute(
                "INSERT INTO cells (id, source, cell_json, dirty, plc_seen_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET source=excluded.source, cell_json=excluded.cell_json, dirty=excluded.dirty, plc_seen_at=COALESCE(excluded.plc_seen_at, cells.plc_seen_at), updated_at=excluded.updated_at",
                (cell.id, source, serde_json::to_string(cell).unwrap_or_default(), dirty as i64, &plc_seen_at, &now),
            )
        })?;
        Ok(CellEntry { id: cell.id, cell: cell.clone(), source: source.into(), dirty, plc_seen_at, updated_at: now })
    }
    pub fn delete_cell(&self, id: u16) -> Result<bool, ApiError> {
        Ok(self.db.with(|c| c.execute("DELETE FROM cells WHERE id = ?1", [id]))? > 0)
    }
    /// After a verified push: rows now match the PLC.
    pub fn mark_cells_synced(&self, ids: &[u16], at: &str) -> Result<(), ApiError> {
        self.db.with(|c| {
            let mut st = c.prepare("UPDATE cells SET dirty = 0, plc_seen_at = ?1 WHERE id = ?2")?;
            for id in ids {
                st.execute((at, id))?;
            }
            Ok(())
        })?;
        Ok(())
    }

    // ---- stations
    pub fn stations(&self) -> Result<Vec<StationEntry>, ApiError> {
        let rows: Vec<(i64, String, String, i64, Option<String>, String)> = self.db.with(|c| {
            let mut st = c.prepare("SELECT id, source, station_json, dirty, plc_seen_at, updated_at FROM stations ORDER BY id")?;
            let it = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?;
            it.collect()
        })?;
        Ok(rows
            .into_iter()
            .map(|(id, source, para, dirty, seen, updated_at)| StationEntry {
                id: id as u16,
                para: serde_json::from_str(&para).unwrap_or_default(),
                source,
                dirty: dirty != 0,
                plc_seen_at: seen,
                updated_at,
            })
            .collect())
    }
    pub fn station(&self, id: u16) -> Result<Option<StationEntry>, ApiError> {
        Ok(self.stations()?.into_iter().find(|s| s.id == id))
    }
    pub fn upsert_station(&self, para: &StationPara, source: &str, dirty: bool, plc_seen_at: Option<String>) -> Result<StationEntry, ApiError> {
        let now = now_str();
        let id = para.info.id;
        self.db.with(|c| {
            c.execute(
                "INSERT INTO stations (id, source, station_json, dirty, plc_seen_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET source=excluded.source, station_json=excluded.station_json, dirty=excluded.dirty, plc_seen_at=COALESCE(excluded.plc_seen_at, stations.plc_seen_at), updated_at=excluded.updated_at",
                (id, source, serde_json::to_string(para).unwrap_or_default(), dirty as i64, &plc_seen_at, &now),
            )
        })?;
        Ok(StationEntry { id, para: para.clone(), source: source.into(), dirty, plc_seen_at, updated_at: now })
    }
    pub fn delete_station(&self, id: u16) -> Result<bool, ApiError> {
        Ok(self.db.with(|c| c.execute("DELETE FROM stations WHERE id = ?1", [id]))? > 0)
    }
    pub fn mark_stations_synced(&self, ids: &[u16], at: &str) -> Result<(), ApiError> {
        self.db.with(|c| {
            let mut st = c.prepare("UPDATE stations SET dirty = 0, plc_seen_at = ?1 WHERE id = ?2")?;
            for id in ids {
                st.execute((at, id))?;
            }
            Ok(())
        })?;
        Ok(())
    }

    // ---- defaults
    pub fn defaults(&self) -> Result<Defaults, ApiError> {
        let mut d: Defaults = match self.db.setting("defaults")? {
            Some(s) => {
                // 깨진 부분만 버린다(`defaults_io::load_lenient`). 무엇이든 버렸으면 원본을 `defaults.recovered` 에
                // 남긴다 — 다음 저장이 덮어써도 원본은 남는다.
                let (d, warn) = defaults_io::load_lenient(&s);
                if !warn.is_empty() {
                    tracing::error!(problems = ?warn, "stored defaults had invalid parts — kept the rest, raw copy in settings 'defaults.recovered'");
                    if self.db.setting("defaults.recovered")?.as_deref() != Some(s.as_str()) {
                        self.db.set_setting("defaults.recovered", &s)?;
                    }
                }
                Defaults { load_warnings: warn, ..d }
            }
            None => Defaults::default(),
        };
        for k in SITUATIONS {
            d.situations.entry(k.to_string()).or_insert_with(|| serde_json::json!({}));
        }
        // 옛 값(`bead`)·오타는 읽는 자리에서 정본으로 — 화면에도 저장된 값에도 `bead` 는 남지 않는다.
        d.grip_ref = spec::normalize_grip_ref(&d.grip_ref).into();
        Ok(d)
    }
    /// 저장(검사 → 버전 +1 → 이력 한 줄). 버전 충돌은 보지 않는다 — 화면 저장은 [`Self::save_defaults_checked`].
    #[cfg_attr(not(test), allow(dead_code))] // 화면은 버전을 보는 save_defaults_checked — 이것은 시험·내부용
    pub fn save_defaults(&self, d: Defaults) -> Result<Defaults, ApiError> {
        self.save_defaults_noted(d, "")
    }

    /// 화면의 저장 — 보낸 `version` 이 저장된 것과 다르면(다른 탭·세션이 먼저 저장) 409. 옛 스냅샷으로 남의
    /// 변경을 되돌리지 않게.
    pub fn save_defaults_checked(&self, d: Defaults, note: &str) -> Result<Defaults, ApiError> {
        let cur = self.defaults()?;
        if d.version != cur.version {
            return Err(ApiError::Conflict(format!("기본값이 다른 곳에서 먼저 바뀌었습니다(저장된 v{}, 보낸 v{}) — 다시 읽은 뒤 저장하세요", cur.version, d.version)));
        }
        self.save_defaults_noted(d, note)
    }

    /// 그립 기준만 — 전체를 다시 보내지 않는다(작업 명령 화면의 토글이 옛 스냅샷으로 남의 변경을 덮던 경로).
    pub fn set_grip_ref(&self, grip_ref: &str) -> Result<Defaults, ApiError> {
        let mut d = self.defaults()?;
        d.grip_ref = grip_ref.to_string();
        self.save_defaults_noted(d, "grip_ref")
    }

    fn save_defaults_noted(&self, mut d: Defaults, note: &str) -> Result<Defaults, ApiError> {
        // 구조 검사 — 문제를 한 번에 모아 거부한다(작성 때 400 으로 터지지 않게).
        let problems = defaults_io::validate(&d);
        if !problems.is_empty() {
            return Err(ApiError::BadRequest(format!("기본값 검사 실패 {}건 — {}", problems.len(), problems.join(" · "))));
        }
        for k in SITUATIONS {
            d.situations.entry(k.to_string()).or_insert_with(|| serde_json::json!({}));
        }
        let before = self.db.setting("defaults")?;
        let prev_version = before.as_deref().map(|s| defaults_io::load_lenient(s).0.version).unwrap_or(0);
        d.updated_at = now_str();
        d.version = d.version.max(prev_version) + 1;
        d.grip_ref = spec::normalize_grip_ref(&d.grip_ref).into();
        d.load_warnings.clear();
        let after = serde_json::to_string(&d)?;
        self.db.with(|c| {
            let tx = c.unchecked_transaction()?;
            tx.execute("INSERT INTO settings (key, value_json) VALUES ('defaults', ?1) ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json", [&after])?;
            tx.execute("INSERT INTO settings_history (key, version, at, note, before_json, after_json) VALUES ('defaults', ?1, ?2, ?3, ?4, ?5)", (d.version, &d.updated_at, note, &before, &after))?;
            tx.execute(
                "DELETE FROM settings_history WHERE key = 'defaults' AND id NOT IN (SELECT id FROM settings_history WHERE key = 'defaults' ORDER BY id DESC LIMIT ?1)",
                [defaults_io::HISTORY_KEEP],
            )?;
            tx.commit()
        })?;
        Ok(d)
    }

    /// 기본값 이력(최신 먼저).
    pub fn defaults_history(&self, limit: usize) -> Result<Vec<DefaultsHistoryRow>, ApiError> {
        type Row = (i64, u32, String, String, Option<String>, String);
        let rows: Vec<Row> = self.db.with(|c| {
            let mut q = c.prepare("SELECT id, version, at, note, before_json, after_json FROM settings_history WHERE key = 'defaults' ORDER BY id DESC LIMIT ?1")?;
            let it = q.query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?;
            it.collect()
        })?;
        Ok(rows
            .into_iter()
            .map(|(id, version, at, note, before, after)| {
                let b = before.as_deref().map(|s| defaults_io::load_lenient(s).0).unwrap_or_default();
                let a = defaults_io::load_lenient(&after).0;
                let mut changes = defaults_io::diff(&b, &a);
                changes.truncate(20);
                DefaultsHistoryRow { id, version, at, note, changes }
            })
            .collect())
    }

    /// 이력 한 줄의 **이후** 값으로 되돌린다(새 버전으로 저장 — 되돌리기도 이력에 남는다).
    pub fn restore_defaults(&self, id: i64) -> Result<Defaults, ApiError> {
        let after: Option<String> = self.db.with(|c| {
            c.query_row("SELECT after_json FROM settings_history WHERE key = 'defaults' AND id = ?1", [id], |r| r.get(0))
                .map(Some)
                .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })
        })?;
        let after = after.ok_or_else(|| ApiError::NotFound(format!("defaults history {id}")))?;
        let (mut d, _) = defaults_io::load_lenient(&after);
        d.version = self.defaults()?.version;
        self.save_defaults_noted(d, &format!("restore #{id}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 기본 그립 기준은 `mid`(Height/2)다. 옛 `bead` 는 읽을 때도 쓸 때도 `pick_bead` 로 옮긴다.
    /// 화면 저장은 버전을 본다(다른 탭이 먼저 저장하면 409). 저장마다 이력, 되돌리기도 새 버전.
    #[test]
    fn checked_save_history_and_restore() {
        let db = crate::db::Db::open_memory().unwrap();
        let reg = Registry::new(db);
        let d0 = reg.defaults().unwrap();
        let mut a = d0.clone();
        a.base.grip_height = 55;
        let s1 = reg.save_defaults_checked(a, "").unwrap();
        // 옛 스냅샷(d0)으로 또 저장 → 409
        let mut stale = d0.clone();
        stale.base.lift_up_height = 1000;
        assert!(matches!(reg.save_defaults_checked(stale, ""), Err(ApiError::Conflict(_))));
        // 그립 기준만은 버전 없이
        let s2 = reg.set_grip_ref("pick_bead").unwrap();
        assert_eq!((s2.grip_ref.as_str(), s2.base.grip_height), ("pick_bead", 55));
        assert!(reg.set_grip_ref("nope").is_err());
        let h = reg.defaults_history(10).unwrap();
        assert_eq!(h.len(), 2);
        assert!(h[1].changes.iter().any(|c| c.starts_with("base.grip_height: 40 → 55")), "{:?}", h[1].changes);
        // 첫 저장(55, mid) 으로 되돌리기
        let r = reg.restore_defaults(h[1].id).unwrap();
        assert_eq!((r.grip_ref.as_str(), r.base.grip_height), ("mid", 55));
        assert!(r.version > s2.version && s1.version < s2.version);
        assert_eq!(reg.defaults_history(10).unwrap()[0].note, format!("restore #{}", h[1].id));
    }

    /// 상황 층: 옛 저장값(키 없음)은 기본 층(Multi-Picking = 부분 리프트)을 받고, 모르는 키·잘못된 값은 저장이 막힌다.
    #[test]
    fn situations_default_and_validate() {
        let db = crate::db::Db::open_memory().unwrap();
        let reg = Registry::new(db.clone());
        db.set_setting("defaults", r#"{"version":3,"grip_ref":"mid"}"#).unwrap();
        let d = reg.defaults().unwrap();
        assert_eq!(d.situations.len(), SITUATIONS.len());
        assert_eq!(d.situations["multi_pick"], serde_json::json!({ "lift_up_partial": true }));
        let mut bad = d.clone();
        bad.situations.insert("moon".into(), serde_json::json!({}));
        assert!(reg.save_defaults(bad).is_err());
        let mut bad = d.clone();
        bad.situations.insert("measure_sku".into(), serde_json::json!({ "grip_height": "tall" }));
        assert!(reg.save_defaults(bad).is_err());
        let mut gap = d;
        gap.situations.remove("pallet_station");
        assert!(reg.save_defaults(gap).unwrap().situations.contains_key("pallet_station"));
    }

    #[test]
    fn defaults_grip_ref_is_mid_and_legacy_bead_moves() {
        let db = crate::db::Db::open_memory().unwrap();
        let reg = Registry::new(db.clone());
        assert_eq!(Defaults::default().grip_ref, "mid");
        assert_eq!(reg.defaults().unwrap().grip_ref, "mid", "새로 깐 콘솔");
        // 옛 값이 그대로 남아 있어도(마이그레이션 전 백업 등) 읽는 자리에서 옮긴다
        db.set_setting("defaults", r#"{"version":2,"grip_ref":"bead"}"#).unwrap();
        assert_eq!(reg.defaults().unwrap().grip_ref, "pick_bead");
        // 화면이나 가져오기가 `bead` 를 보내도 저장되는 값은 `pick_bead`
        let saved = reg.save_defaults(Defaults { grip_ref: "bead".into(), ..Default::default() }).unwrap();
        assert_eq!(saved.grip_ref, "pick_bead");
        assert!(db.setting("defaults").unwrap().unwrap().contains(r#""grip_ref":"pick_bead""#));
        assert_eq!(reg.save_defaults(Defaults { grip_ref: "bead+offset".into(), ..Default::default() }).unwrap().grip_ref, "pick_bead");
        // 모르는 값은 저장이 막힌다(예전에는 조용히 mid 로 바뀌었다).
        assert!(reg.save_defaults(Defaults { grip_ref: "nonsense".into(), ..Default::default() }).is_err());
    }
}
