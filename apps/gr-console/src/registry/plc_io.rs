//! PLC ⇄ local registry.
//!
//! - import: decode `CELL.Count + Cell[..]` / `STATION.Count + Station[..]` from the latest S7 snapshot into sqlite
//! - push: encode the whole local table into the PLC DB bytes (base = current DB image), write the array
//!   range(s) first and `Count` **last** (the PLC's `Findindex_*` only scans up to `Count`), then read the
//!   written ranges back and byte-compare
//!
//! Tables: GR2 `CELL.Cell[i]` / GRM `CELL.Cell[i]` are `LGR_Cell_Info`; GR2 `STATION.Station[i]` is
//! `LGR_Station_Para`; GRM `STATION.Station[i]` is `LGR_STATION` whose `.Para` is `LGR_Station_Para` — for
//! GRM only the `Para` sub-struct of each element is rewritten (the rest is live runtime state).

use gr_proto::{CellInfo, StationPara};
use plc_layout::Layout;
use serde::Serialize;
use serde_json::{Value as Json, json};

use super::diff::{Diff, classify_by};
use super::{CellEntry, StationEntry};
use crate::config::PlcRole;
use crate::error::ApiError;
use crate::plc::PlcHandle;
use crate::state::AppState;
use crate::util::now_str;

/// Resolves a frontend id (`gr2_s7`), a config name (`GR2`) or a lowercase alias (`grm`) to a handle.
pub fn resolve_plc<'a>(st: &'a AppState, name: &str) -> Result<&'a PlcHandle, ApiError> {
    let n = name.trim();
    let n = n.strip_suffix("_s7").unwrap_or(n);
    if n.is_empty() {
        return st.status_plc();
    }
    st.plc(n)
}

/// `both` → status PLC (GR2) then GRM; otherwise the one named.
pub fn plc_targets<'a>(st: &'a AppState, name: &str) -> Result<Vec<&'a PlcHandle>, ApiError> {
    if name.trim().eq_ignore_ascii_case("both") {
        let gr2 = st.status_plc()?;
        let grm = st.grm_plc().ok_or_else(|| ApiError::NotFound(format!("plc {}", st.cfg.cmd.grm_plc)))?;
        Ok(vec![gr2, grm])
    } else {
        Ok(vec![resolve_plc(st, name)?])
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ImportSummary {
    pub imported: usize,
    pub updated: usize,
    pub removed: usize,
    pub skipped: usize,
    pub errors: Vec<Json>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PushResult {
    pub plc: String,
    pub db: String,
    pub written_bytes: usize,
    pub count: usize,
    pub verified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mismatch_at: Option<u32>,
    pub writes: usize,
}

// ---- snapshot decoding

fn snapshot_json(h: &PlcHandle, db: &str) -> Result<std::sync::Arc<Json>, ApiError> {
    let snap = h.snap();
    let d = snap.db(db).ok_or_else(|| ApiError::PlcUnavailable(format!("{}.{db} not read yet", h.name())))?;
    Ok(d.json.clone())
}

/// `Cell[0..Count)` of the latest snapshot, zero ids dropped.
pub fn plc_cells(h: &PlcHandle) -> Result<Vec<CellInfo>, ApiError> {
    let j = snapshot_json(h, "CELL")?;
    let count = j["Count"].as_u64().unwrap_or(0) as usize;
    let list = j["Cell"].as_array().cloned().unwrap_or_default();
    Ok(list.iter().take(count).filter_map(|c| serde_json::from_value::<CellInfo>(c.clone()).ok()).filter(|c| c.id != 0).collect())
}

/// `Station[0..Count)` of the latest snapshot — GR2 elements are `LGR_Station_Para`, GRM elements carry it in `.Para`.
pub fn plc_stations(h: &PlcHandle) -> Result<Vec<StationPara>, ApiError> {
    let j = snapshot_json(h, "STATION")?;
    let count = j["Count"].as_u64().unwrap_or(0) as usize;
    let list = j["Station"].as_array().cloned().unwrap_or_default();
    Ok(list
        .iter()
        .take(count)
        .filter_map(|s| {
            let para = if s.get("Para").is_some() { &s["Para"] } else { s };
            serde_json::from_value::<StationPara>(para.clone()).ok()
        })
        .filter(|p| p.info.id != 0)
        .collect())
}

// ---- import (PLC → local)

pub fn import_cells(st: &AppState, h: &PlcHandle) -> Result<ImportSummary, ApiError> {
    let now = now_str();
    let existing = st.registry.cells()?;
    let mut s = ImportSummary::default();
    for cell in plc_cells(h)? {
        match existing.iter().find(|e| e.id == cell.id) {
            Some(e) if e.cell == cell && !e.dirty => s.skipped += 1,
            Some(_) => s.updated += 1,
            None => s.imported += 1,
        }
        st.registry.upsert_cell(&cell, "plc", false, Some(now.clone()))?;
    }
    Ok(s)
}

pub fn import_stations(st: &AppState, h: &PlcHandle) -> Result<ImportSummary, ApiError> {
    let now = now_str();
    let existing = st.registry.stations()?;
    let mut s = ImportSummary::default();
    for para in plc_stations(h)? {
        match existing.iter().find(|e| e.id == para.info.id) {
            Some(e) if e.para == para && !e.dirty => s.skipped += 1,
            Some(_) => s.updated += 1,
            None => s.imported += 1,
        }
        st.registry.upsert_station(&para, "plc", false, Some(now.clone()))?;
    }
    Ok(s)
}

// ---- diff (local vs PLC)

pub fn diff_cells(st: &AppState, h: &PlcHandle) -> Result<Vec<Diff<CellEntry>>, ApiError> {
    let local: Vec<(u16, CellEntry)> = st.registry.cells()?.into_iter().map(|e| (e.id, e)).collect();
    let plc: Vec<(u16, CellEntry)> =
        plc_cells(h)?.into_iter().map(|c| (c.id, CellEntry { id: c.id, cell: c, source: "plc".into(), dirty: false, plc_seen_at: None, updated_at: String::new() })).collect();
    Ok(classify_by(&local, &plc, |a, b| a.cell == b.cell))
}

pub fn diff_stations(st: &AppState, h: &PlcHandle) -> Result<Vec<Diff<StationEntry>>, ApiError> {
    let local: Vec<(u16, StationEntry)> = st.registry.stations()?.into_iter().map(|e| (e.id, e)).collect();
    let plc: Vec<(u16, StationEntry)> =
        plc_stations(h)?.into_iter().map(|p| (p.info.id, StationEntry { id: p.info.id, para: p, source: "plc".into(), dirty: false, plc_seen_at: None, updated_at: String::new() })).collect();
    Ok(classify_by(&local, &plc, |a, b| a.para == b.para))
}

// ---- push (local → PLC)

/// PLC indices of an array member, from the layout member paths (`Cell[1].Id` → 1).
fn array_indices(layout: &Layout, array: &str) -> Vec<i64> {
    let prefix = format!("{array}[");
    let mut v: Vec<i64> = layout
        .members
        .iter()
        .filter_map(|m| {
            let rest = m.path.strip_prefix(&prefix)?;
            let end = rest.find(']')?;
            rest[..end].parse::<i64>().ok()
        })
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

fn member_range(layout: &Layout, path: &str) -> Result<(u32, u32), ApiError> {
    layout.range_of(path).ok_or_else(|| ApiError::Internal(format!("layout {}: no member {path}", layout.name)))
}

fn guard(h: &PlcHandle, db: &str, force: bool) -> Result<(), ApiError> {
    let hl = h.health_now();
    if !hl.connected {
        return Err(ApiError::PlcUnavailable(format!("{} S7 not connected", h.name())));
    }
    if !hl.db_ok(db) {
        return Err(ApiError::LayoutMismatch { plc: h.name().into(), db: db.into(), detail: hl.mismatch_detail(db) });
    }
    if h.cfg.role == PlcRole::Gr
        && !force
        && let Some(v) = h.decode_path("OPCUA", "STAT.Mode.Auto")
        && v.as_bool() == Some(true)
    {
        return Err(ApiError::Conflict(format!("{} is in AUTO mode — table writes are refused while tasks may be validated (force=1 overrides)", h.name())));
    }
    Ok(())
}

/// Current DB image as the encode base (snapshot if it has the full layout size, else a fresh read).
async fn base_bytes(h: &PlcHandle, db: &str, layout: &Layout) -> Result<Vec<u8>, ApiError> {
    let size = layout.size as usize;
    if let Some(d) = h.snap().db(db)
        && d.raw.len() == size
    {
        return Ok(d.raw.as_ref().clone());
    }
    h.read(db, 0, size).await.map_err(|e| ApiError::PlcUnavailable(format!("{} read {db}: {e}", h.name())))
}

async fn write_and_verify(h: &PlcHandle, db: &str, buf: &[u8], ranges: &[(u32, u32)], count: usize) -> Result<PushResult, ApiError> {
    let mut written = 0usize;
    for (s, e) in ranges {
        let (s, e) = (*s as usize, *e as usize);
        h.write(db, s as u32, buf[s..e].to_vec()).await.map_err(|err| ApiError::PlcUnavailable(format!("{} write {db}@{s}: {err}", h.name())))?;
        written += e - s;
    }
    // read the whole span once and compare only the ranges we wrote (other bytes may be live state)
    let lo = ranges.iter().map(|r| r.0).min().unwrap_or(0) as usize;
    let hi = ranges.iter().map(|r| r.1).max().unwrap_or(0) as usize;
    let back = h.read(db, lo as u32, hi - lo).await.map_err(|e| ApiError::PlcUnavailable(format!("{} read-back {db}: {e}", h.name())))?;
    let mut mismatch_at = None;
    'outer: for (s, e) in ranges {
        for (off, b) in buf.iter().enumerate().take(*e as usize).skip(*s as usize) {
            if back.get(off - lo) != Some(b) {
                mismatch_at = Some(off as u32);
                break 'outer;
            }
        }
    }
    Ok(PushResult { plc: h.name().into(), db: db.into(), written_bytes: written, count, verified: mismatch_at.is_none(), mismatch_at, writes: ranges.len() })
}

pub async fn push_cells(st: &AppState, h: &PlcHandle, force: bool) -> Result<PushResult, ApiError> {
    let db = "CELL";
    let layout = h.layout(db).ok_or_else(|| ApiError::PlcUnavailable(format!("{} has no {db} layout", h.name())))?;
    guard(h, db, force)?;
    let mut entries = st.registry.cells()?;
    entries.sort_by_key(|e| e.id);
    let idx = array_indices(layout, "Cell");
    if entries.len() > idx.len() {
        return Err(ApiError::BadRequest(format!("{} cells exceed PLC capacity {}", entries.len(), idx.len())));
    }
    let zero = serde_json::to_value(CellInfo::default())?;
    let arr: Vec<Json> = (0..idx.len()).map(|k| entries.get(k).map(|e| serde_json::to_value(&e.cell)).transpose().map(|v| v.unwrap_or_else(|| zero.clone()))).collect::<Result<_, _>>()?;
    let mut buf = base_bytes(h, db, layout).await?;
    h.contract.encode_path(db, "Cell", &Json::Array(arr), &mut buf)?;
    h.contract.encode_path(db, "Count", &json!(entries.len()), &mut buf)?;
    let ranges = vec![member_range(layout, "Cell")?, member_range(layout, "Count")?];
    let res = write_and_verify(h, db, &buf, &ranges, entries.len()).await?;
    if res.verified {
        let ids: Vec<u16> = entries.iter().map(|e| e.id).collect();
        st.registry.mark_cells_synced(&ids, &now_str())?;
    }
    Ok(res)
}

pub async fn push_stations(st: &AppState, h: &PlcHandle, force: bool) -> Result<PushResult, ApiError> {
    let db = "STATION";
    let layout = h.layout(db).ok_or_else(|| ApiError::PlcUnavailable(format!("{} has no {db} layout", h.name())))?;
    guard(h, db, force)?;
    let mut entries = st.registry.stations()?;
    entries.sort_by_key(|e| e.id);
    let idx = array_indices(layout, "Station");
    if entries.len() > idx.len() {
        return Err(ApiError::BadRequest(format!("{} stations exceed PLC capacity {}", entries.len(), idx.len())));
    }
    let lo = *idx.first().ok_or_else(|| ApiError::Internal("STATION.Station has no elements".into()))?;
    // shape: nested (`Station[i].Para.Info.Id`, GRM LGR_STATION) or flat (`Station[i].Info.Id`, GR2 LGR_Station_Para)
    let nested = layout.find(&format!("Station[{lo}].Para.Info.Id")).is_some();
    let zero = serde_json::to_value(StationPara::default())?;
    let mut buf = base_bytes(h, db, layout).await?;
    let mut ranges = Vec::new();
    if nested {
        for (k, i) in idx.iter().enumerate() {
            let v = match entries.get(k) {
                Some(e) => serde_json::to_value(&e.para)?,
                None => zero.clone(),
            };
            let path = format!("Station[{i}].Para");
            h.contract.encode_path(db, &path, &v, &mut buf)?;
            ranges.push(member_range(layout, &path)?);
        }
    } else {
        let arr: Vec<Json> = (0..idx.len()).map(|k| entries.get(k).map(|e| serde_json::to_value(&e.para)).transpose().map(|v| v.unwrap_or_else(|| zero.clone()))).collect::<Result<_, _>>()?;
        h.contract.encode_path(db, "Station", &Json::Array(arr), &mut buf)?;
        ranges.push(member_range(layout, "Station")?);
    }
    h.contract.encode_path(db, "Count", &json!(entries.len()), &mut buf)?;
    ranges.push(member_range(layout, "Count")?);
    let res = write_and_verify(h, db, &buf, &ranges, entries.len()).await?;
    if res.verified {
        let ids: Vec<u16> = entries.iter().map(|e| e.id).collect();
        st.registry.mark_stations_synced(&ids, &now_str())?;
    }
    Ok(res)
}

/// Aggregates per-PLC results (`both`) into one envelope the UI can show as a single verification.
pub fn aggregate(results: Vec<PushResult>) -> Json {
    if results.len() == 1 {
        return serde_json::to_value(&results[0]).unwrap_or(Json::Null);
    }
    let written: usize = results.iter().map(|r| r.written_bytes).sum();
    let count = results.first().map(|r| r.count).unwrap_or(0);
    let verified = results.iter().all(|r| r.verified);
    let mismatch_at = results.iter().find_map(|r| r.mismatch_at);
    json!({ "plc": "both", "db": results.first().map(|r| r.db.clone()).unwrap_or_default(), "written_bytes": written, "count": count, "verified": verified,
        "mismatch_at": mismatch_at, "writes": results.iter().map(|r| r.writes).sum::<usize>(), "results": results })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout_with(paths: &[(&str, u32, u32)]) -> Layout {
        Layout {
            name: "T".into(),
            size: 64,
            members: paths.iter().map(|(p, off, size)| plc_layout::Member { path: (*p).into(), offset: *off, bit: None, prim: plc_layout::Prim::UInt, size: *size }).collect(),
        }
    }

    #[test]
    fn array_indices_from_member_paths() {
        let l = layout_with(&[("Count", 0, 2), ("Cell[1].Id", 2, 2), ("Cell[1].Row", 4, 2), ("Cell[2].Id", 6, 2), ("Cell[3].Id", 10, 2)]);
        assert_eq!(array_indices(&l, "Cell"), vec![1, 2, 3]);
        assert!(array_indices(&l, "Station").is_empty());
        assert_eq!(member_range(&l, "Cell").unwrap(), (2, 12));
        assert_eq!(member_range(&l, "Count").unwrap(), (0, 2));
    }

    #[test]
    fn aggregate_both_reports_first_mismatch() {
        let a = PushResult { plc: "GR2".into(), db: "CELL".into(), written_bytes: 10, count: 3, verified: true, mismatch_at: None, writes: 2 };
        let b = PushResult { plc: "GRM".into(), db: "CELL".into(), written_bytes: 10, count: 3, verified: false, mismatch_at: Some(7), writes: 2 };
        let j = aggregate(vec![a, b]);
        assert_eq!(j["plc"], "both");
        assert_eq!(j["written_bytes"], 20);
        assert_eq!(j["verified"], false);
        assert_eq!(j["mismatch_at"], 7);
        assert_eq!(j["results"].as_array().unwrap().len(), 2);
    }
}
