//! PLC ⇄ local registry.
//!
//! - import: decode `CELL.Count + Cell[..]` / `STATION.Count + Station[..]` from the latest S7 snapshot into sqlite
//!   (`Count == 0` scans the whole array — the GRM PLC never maintains its `Count`; see `decode_table`)
//! - push: encode the whole local table into the PLC DB bytes (base = current DB image), write the array
//!   range(s) first and `Count` **last**, then read the written ranges back and byte-compare
//!   - cells are packed into `Cell[1..=n]`: GR2 `PL_Task_V2` (BlendUse) and `HMI_CellPos` loop `CELL_MIN..CELL.Count`
//!     (`Findindex_CELL` itself scans `CELL_MIN..CELL_MAX`)
//!   - stations go to slot `Station[id MOD 100]`: `isStationTask` → StationNo, and GRM `StationCenterAdjust`,
//!     GR2 `isValidTaskArea` (TaskType) / `PL_Task_V2` read `STATION.Station[StationNo]` directly
//!     (`Findindex_STATION` scans `STATION_MIN..STATION_MAX`, not `Count`)
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

/// `all` → every configured PLC (GR role in config order, then GRM); `both` (legacy) → status PLC then GRM;
/// otherwise the one named.
pub fn plc_targets<'a>(st: &'a AppState, name: &str) -> Result<Vec<&'a PlcHandle>, ApiError> {
    let n = name.trim();
    if n.eq_ignore_ascii_case("all") {
        let order = all_order(st.cfg.plcs.iter().map(|p| (p.name.as_str(), p.role)));
        let out: Vec<&PlcHandle> = order.iter().filter_map(|n| st.plc(n).ok()).collect();
        if out.is_empty() {
            return Err(ApiError::NotFound("no PLC configured".into()));
        }
        Ok(out)
    } else if n.eq_ignore_ascii_case("both") {
        let gr2 = st.status_plc()?;
        let grm = st.grm_plc().ok_or_else(|| ApiError::NotFound(format!("plc {}", st.cfg.cmd.grm_plc)))?;
        Ok(vec![gr2, grm])
    } else {
        Ok(vec![resolve_plc(st, name)?])
    }
}

/// Write order for `all`: GR role PLCs first (config order), GRM last — the GRM table mirrors the GR tables.
fn all_order<'a>(plcs: impl Iterator<Item = (&'a str, PlcRole)>) -> Vec<&'a str> {
    let (gr, grm): (Vec<_>, Vec<_>) = plcs.partition(|(_, r)| *r == PlcRole::Gr);
    gr.into_iter().chain(grm).map(|(n, _)| n).collect()
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

/// Rows decoded from a PLC table plus the elements that did not decode (`"Cell[3]: invalid type…"`).
#[derive(Clone, Debug, Default)]
pub struct Decoded<T> {
    pub rows: Vec<T>,
    pub bad: Vec<String>,
}

/// Decodes `{ Count, <array>: [...] }`.
///
/// `Count > 0` keeps `[0..Count)` — the GR PLC's `Findindex_*` only scans up to `Count`, so slots past it are
/// not live data. `Count == 0` (or missing) scans the whole array: the real GRM PLC leaves `CELL.Count` /
/// `STATION.Count` at 0 while its arrays hold the cells (2026-09-14). Zero ids are dropped either way.
///
/// `respect_count = false` always scans the whole array — stations sit at slot `id MOD 100`, so a live
/// station may be past `Count` (e.g. 2021 in slot 21 with Count = 4).
fn decode_table<T: serde::de::DeserializeOwned>(j: &Json, array: &str, respect_count: bool, elem: impl Fn(&Json) -> &Json, id: impl Fn(&T) -> u16) -> Decoded<T> {
    let count = if respect_count { j["Count"].as_u64().unwrap_or(0) as usize } else { 0 };
    let list = j[array].as_array().map(Vec::as_slice).unwrap_or_default();
    let take = if count > 0 { count.min(list.len()) } else { list.len() };
    let mut out = Decoded { rows: Vec::new(), bad: Vec::new() };
    for (i, v) in list.iter().take(take).enumerate() {
        match serde_json::from_value::<T>(elem(v).clone()) {
            Ok(t) if id(&t) != 0 => out.rows.push(t),
            Ok(_) => {}
            Err(e) => out.bad.push(format!("{array}[{i}]: {e}")),
        }
    }
    out
}

pub fn decode_cells(j: &Json) -> Decoded<CellInfo> {
    decode_table(j, "Cell", true, |v| v, |c: &CellInfo| c.id)
}

/// GR2 elements are `LGR_Station_Para`, GRM elements carry it in `.Para`. Whole array (slot rule).
pub fn decode_stations(j: &Json) -> Decoded<StationPara> {
    decode_table(j, "Station", false, |v| if v.get("Para").is_some() { &v["Para"] } else { v }, |p: &StationPara| p.info.id)
}

pub fn plc_cells(h: &PlcHandle) -> Result<Decoded<CellInfo>, ApiError> {
    Ok(decode_cells(&*snapshot_json(h, "CELL")?))
}

pub fn plc_stations(h: &PlcHandle) -> Result<Decoded<StationPara>, ApiError> {
    Ok(decode_stations(&*snapshot_json(h, "STATION")?))
}

fn bad_rows(bad: &[String]) -> Vec<Json> {
    bad.iter().map(|m| json!({ "row": 0, "sheet": "PLC", "message": m })).collect()
}

// ---- import (PLC → local)

pub fn import_cells(st: &AppState, h: &PlcHandle) -> Result<ImportSummary, ApiError> {
    let now = now_str();
    let existing = st.registry.cells()?;
    let decoded = plc_cells(h)?;
    let mut s = ImportSummary { errors: bad_rows(&decoded.bad), ..Default::default() };
    for cell in decoded.rows {
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
    let decoded = plc_stations(h)?;
    let mut s = ImportSummary { errors: bad_rows(&decoded.bad), ..Default::default() };
    for para in decoded.rows {
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
        plc_cells(h)?.rows.into_iter().map(|c| (c.id, CellEntry { id: c.id, cell: c, source: "plc".into(), dirty: false, plc_seen_at: None, updated_at: String::new() })).collect();
    Ok(classify_by(&local, &plc, |a, b| a.cell == b.cell))
}

pub fn diff_stations(st: &AppState, h: &PlcHandle) -> Result<Vec<Diff<StationEntry>>, ApiError> {
    let local: Vec<(u16, StationEntry)> = st.registry.stations()?.into_iter().map(|e| (e.id, e)).collect();
    let plc: Vec<(u16, StationEntry)> =
        plc_stations(h)?.rows.into_iter().map(|p| (p.info.id, StationEntry { id: p.info.id, para: p, source: "plc".into(), dirty: false, plc_seen_at: None, updated_at: String::new() })).collect();
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
    // 여러 조각으로 쓰고 다시 읽어 맞추는 한 구간 — 중간에 끊기면 PLC 표가 반만 바뀐다.
    let _busy = st.shutdown.enter(format!("CELL 표 PLC 쓰기 ({})", h.name()))?;
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

/// Station → PLC array index: slot `id MOD 100` (PLC `isStationTask`). For every PLC index in `idx` (in order)
/// the position in `ids` that goes there, or `None` for a slot that is zeroed. Refuses an id whose slot is not
/// an array index (`MOD 100` = 0 or > 32) and two ids sharing one slot (2003 & 2103).
fn station_slots(ids: &[u16], idx: &[i64]) -> Result<Vec<Option<usize>>, String> {
    let mut out: Vec<Option<usize>> = vec![None; idx.len()];
    let (lo, hi) = (idx.first().copied().unwrap_or(0), idx.last().copied().unwrap_or(0));
    for (k, id) in ids.iter().enumerate() {
        let slot = (*id % 100) as i64;
        let Some(pos) = idx.iter().position(|i| *i == slot) else {
            return Err(format!("스테이션 {id}: 슬롯 {slot}(id MOD 100)이 PLC 배열 Station[{lo}..{hi}] 밖입니다"));
        };
        if let Some(prev) = out[pos] {
            return Err(format!("스테이션 {} 와 {id} 가 같은 슬롯 Station[{slot}] 을 씁니다(id MOD 100)", ids[prev]));
        }
        out[pos] = Some(k);
    }
    Ok(out)
}

pub async fn push_stations(st: &AppState, h: &PlcHandle, force: bool) -> Result<PushResult, ApiError> {
    let _busy = st.shutdown.enter(format!("STATION 표 PLC 쓰기 ({})", h.name()))?;
    let db = "STATION";
    let layout = h.layout(db).ok_or_else(|| ApiError::PlcUnavailable(format!("{} has no {db} layout", h.name())))?;
    guard(h, db, force)?;
    let mut entries = st.registry.stations()?;
    entries.sort_by_key(|e| e.id);
    let idx = array_indices(layout, "Station");
    let lo = *idx.first().ok_or_else(|| ApiError::Internal("STATION.Station has no elements".into()))?;
    let ids: Vec<u16> = entries.iter().map(|e| e.id).collect();
    let slots = station_slots(&ids, &idx).map_err(ApiError::BadRequest)?;
    // shape: nested (`Station[i].Para.Info.Id`, GRM LGR_STATION) or flat (`Station[i].Info.Id`, GR2 LGR_Station_Para)
    let nested = layout.find(&format!("Station[{lo}].Para.Info.Id")).is_some();
    let zero = serde_json::to_value(StationPara::default())?;
    let mut buf = base_bytes(h, db, layout).await?;
    let mut ranges = Vec::new();
    if nested {
        for (k, i) in idx.iter().enumerate() {
            let v = match slots[k].and_then(|e| entries.get(e)) {
                Some(e) => serde_json::to_value(&e.para)?,
                None => zero.clone(),
            };
            let path = format!("Station[{i}].Para");
            h.contract.encode_path(db, &path, &v, &mut buf)?;
            ranges.push(member_range(layout, &path)?);
        }
    } else {
        let arr: Vec<Json> =
            (0..idx.len()).map(|k| slots[k].and_then(|e| entries.get(e)).map(|e| serde_json::to_value(&e.para)).transpose().map(|v| v.unwrap_or_else(|| zero.clone()))).collect::<Result<_, _>>()?;
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

/// Aggregates per-PLC results (`all` / `both`) into one envelope the UI can show as a single verification.
/// `plc` names the targets in write order (`GR1+GR2+GRM`); the per-PLC detail stays in `results`.
pub fn aggregate(results: Vec<PushResult>) -> Json {
    if results.len() == 1 {
        return serde_json::to_value(&results[0]).unwrap_or(Json::Null);
    }
    let written: usize = results.iter().map(|r| r.written_bytes).sum();
    let count = results.first().map(|r| r.count).unwrap_or(0);
    let verified = results.iter().all(|r| r.verified);
    let mismatch_at = results.iter().find_map(|r| r.mismatch_at);
    let plc = results.iter().map(|r| r.plc.as_str()).collect::<Vec<_>>().join("+");
    json!({ "plc": plc, "db": results.first().map(|r| r.db.clone()).unwrap_or_default(), "written_bytes": written, "count": count, "verified": verified,
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
    fn aggregate_all_reports_first_mismatch() {
        let r = |plc: &str, verified: bool, mismatch_at: Option<u32>| PushResult { plc: plc.into(), db: "CELL".into(), written_bytes: 10, count: 3, verified, mismatch_at, writes: 2 };
        let j = aggregate(vec![r("GR2", true, None), r("GRM", false, Some(7))]);
        assert_eq!(j["plc"], "GR2+GRM");
        assert_eq!(j["written_bytes"], 20);
        assert_eq!(j["verified"], false);
        assert_eq!(j["mismatch_at"], 7);
        assert_eq!(j["results"].as_array().unwrap().len(), 2);
        let j = aggregate(vec![r("GR1", true, None), r("GR2", true, None), r("GRM", true, None)]);
        assert_eq!(j["plc"], "GR1+GR2+GRM");
        assert_eq!(j["verified"], true);
        assert!(j["mismatch_at"].is_null());
        // one target → the plain result, not an envelope
        assert_eq!(aggregate(vec![r("GR1", true, None)])["plc"], "GR1");
    }

    #[test]
    fn all_order_puts_grm_last() {
        let cfg = [("GR2", PlcRole::Gr), ("GRM", PlcRole::Grm), ("GR1", PlcRole::Gr)];
        assert_eq!(all_order(cfg.into_iter()), vec!["GR2", "GR1", "GRM"]);
    }

    fn cell_json(id: u16) -> Json {
        serde_json::to_value(CellInfo { id, section: 1, position: [1.0, 2.0, 3.0], ..Default::default() }).unwrap()
    }

    #[test]
    fn cells_count_zero_scans_whole_array() {
        // real GRM: Count = 0 but the array holds cells (zero slots in between are skipped)
        let j = json!({ "Count": 0, "Cell": [cell_json(101), cell_json(0), cell_json(102), cell_json(0), cell_json(118)] });
        let d = decode_cells(&j);
        assert_eq!(d.rows.iter().map(|c| c.id).collect::<Vec<_>>(), vec![101, 102, 118]);
        assert!(d.bad.is_empty());
        // missing Count behaves like 0
        let j = json!({ "Cell": [cell_json(7)] });
        assert_eq!(decode_cells(&j).rows.len(), 1);
    }

    #[test]
    fn cells_count_positive_keeps_prefix() {
        // GR2: slots past Count are not live even if they still hold ids
        let j = json!({ "Count": 2, "Cell": [cell_json(101), cell_json(102), cell_json(103)] });
        assert_eq!(decode_cells(&j).rows.iter().map(|c| c.id).collect::<Vec<_>>(), vec![101, 102]);
        // Count larger than the array is clamped
        let j = json!({ "Count": 60, "Cell": [cell_json(101)] });
        assert_eq!(decode_cells(&j).rows.len(), 1);
    }

    #[test]
    fn stations_nested_para_and_bad_elements_reported() {
        let para = |id: u16| serde_json::to_value(StationPara { info: CellInfo { id, section: 3, ..Default::default() }, ..Default::default() }).unwrap();
        // GRM shape (`.Para`), Count = 0, one element that does not decode
        let j = json!({ "Count": 0, "Station": [{ "Para": para(2101) }, { "Para": { "Info": { "Id": "x" } } }, { "Para": para(0) }, { "Para": para(2104) }] });
        let d = decode_stations(&j);
        assert_eq!(d.rows.iter().map(|p| p.info.id).collect::<Vec<_>>(), vec![2101, 2104]);
        assert_eq!(d.bad.len(), 1);
        assert!(d.bad[0].starts_with("Station[1]:"), "{:?}", d.bad);
        assert_eq!(bad_rows(&d.bad)[0]["sheet"], "PLC");
        // GR2 flat shape: Count is ignored for stations — slot `id MOD 100` may lie past Count
        let j = json!({ "Count": 1, "Station": [para(2101), para(0), para(2003)] });
        assert_eq!(decode_stations(&j).rows.iter().map(|p| p.info.id).collect::<Vec<_>>(), vec![2101, 2003]);
    }

    #[test]
    fn station_slots_follow_id_mod_100() {
        let idx: Vec<i64> = (1..=32).collect();
        let s = station_slots(&[2003, 2021, 2101], &idx).unwrap();
        assert_eq!(s[2], Some(0), "2003 → Station[3]");
        assert_eq!(s[20], Some(1), "2021 → Station[21]");
        assert_eq!(s[0], Some(2), "2101 → Station[1]");
        assert_eq!(s.iter().filter(|x| x.is_some()).count(), 3, "every other slot is zeroed");
        assert!(station_slots(&[], &idx).unwrap().iter().all(Option::is_none));
        let e = station_slots(&[2003, 2103], &idx).unwrap_err();
        assert!(e.contains("2003") && e.contains("2103") && e.contains("Station[3]"), "{e}");
        let e = station_slots(&[2100], &idx).unwrap_err();
        assert!(e.contains("슬롯 0"), "{e}");
        let e = station_slots(&[2033], &idx).unwrap_err();
        assert!(e.contains("슬롯 33") && e.contains("Station[1..32]"), "{e}");
    }
}
