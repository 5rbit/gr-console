//! Registries: tire codes (items), cells, stations, defaults — sqlite-backed.
//!
//! - `plc_io`: PLC snapshot → local (import) and local → PLC S7 write + read-back verify (push)
//! - `diff`: local vs PLC classification (`same|local_only|plc_only|changed`)
//! - `xlsx`: Excel / CSV export + import (header-mapped, row-validated)
//! - `routes`: `/api/items`, `/api/cells`, `/api/stations`, `/api/registry`, `/api/defaults`, `/api/issue/compose`

pub mod diff;
pub mod plc_io;
pub mod routes;
pub mod xlsx;

use std::collections::HashMap;
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
        Self { version: 1, updated_at: now_str(), base: TaskParams::default(), by }
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
        let rows: Vec<(i64, String, String, String, String)> = self.db.with(|c| {
            let mut st = c.prepare("SELECT code, name, item_json, note, updated_at FROM tire_codes ORDER BY code")?;
            let it = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?;
            it.collect()
        })?;
        Ok(rows
            .into_iter()
            .map(|(code, name, item, note, updated_at)| ItemEntry { code: code as u32, name, item: serde_json::from_str(&item).unwrap_or_default(), note, updated_at })
            .collect())
    }
    pub fn item(&self, code: u32) -> Result<Option<ItemEntry>, ApiError> {
        Ok(self.items()?.into_iter().find(|i| i.code == code))
    }
    pub fn upsert_item(&self, code: u32, name: &str, item: &StockItem, note: &str) -> Result<ItemEntry, ApiError> {
        let mut item = item.clone();
        item.code = code;
        let now = now_str();
        self.db.with(|c| {
            c.execute(
                "INSERT INTO tire_codes (code, name, item_json, note, updated_at) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(code) DO UPDATE SET name=excluded.name, item_json=excluded.item_json, note=excluded.note, updated_at=excluded.updated_at",
                (code, name, serde_json::to_string(&item).unwrap_or_default(), note, &now),
            )
        })?;
        Ok(ItemEntry { code, name: name.into(), item, note: note.into(), updated_at: now })
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
            .map(|(id, source, cell, dirty, seen, updated_at)| CellEntry { id: id as u16, cell: serde_json::from_str(&cell).unwrap_or_default(), source, dirty: dirty != 0, plc_seen_at: seen, updated_at })
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
            .map(|(id, source, para, dirty, seen, updated_at)| StationEntry { id: id as u16, para: serde_json::from_str(&para).unwrap_or_default(), source, dirty: dirty != 0, plc_seen_at: seen, updated_at })
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
        match self.db.setting("defaults")? {
            Some(s) => Ok(serde_json::from_str(&s).unwrap_or_default()),
            None => Ok(Defaults::default()),
        }
    }
    pub fn save_defaults(&self, mut d: Defaults) -> Result<Defaults, ApiError> {
        d.updated_at = now_str();
        d.version += 1;
        self.db.set_setting("defaults", &serde_json::to_string(&d)?)?;
        Ok(d)
    }
}
