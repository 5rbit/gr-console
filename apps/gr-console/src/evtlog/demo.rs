//! Demo EVTLOG: the PLC side of the ring on a demo JSON model (`EvtLog` FC semantics: level / category filter,
//! `MaxPerScan` per tick, Seq = Total, Head = next write index), so the collector and the Events tab run without a PLC.

use serde_json::{Value as Json, json};

use evt_catalog::Catalog;

fn get_u(m: &Json, ptr: &str) -> u64 {
    m.pointer(ptr).and_then(Json::as_u64).unwrap_or(0)
}

fn set(m: &mut Json, ptr: &str, v: Json) {
    if let Some(x) = m.pointer_mut(ptr) {
        *x = v;
    }
}

/// Start values like the DB after a download: LayoutSig, BootId, Cfg defaults from the catalog.
pub fn init(model: &mut Json, sig: u32, boot_id: u16, cat: &Catalog) {
    set(model, "/LayoutSig", json!(sig));
    set(model, "/BootId", json!(boot_id));
    set(model, "/Cfg/MinLevel", json!(cat.min_level));
    set(model, "/Cfg/CatMask", json!(cat.cat_mask_all()));
    set(model, "/Cfg/MaxPerScan", json!(cat.max_per_scan));
}

/// Per-tick start: resets the per-scan counter and simulates the logger's own run time.
pub fn begin_scan(model: &mut Json, run_us: u32) {
    set(model, "/Now/Count", json!(0));
    set(model, "/Stat/RunUs", json!(run_us));
    if u64::from(run_us) > get_u(model, "/Stat/MaxRunUs") {
        set(model, "/Stat/MaxRunUs", json!(run_us));
    }
}

/// One `EvtLog` call. Returns false when filtered out or over `MaxPerScan` (then `Dropped` counts it).
#[allow(clippy::too_many_arguments)]
pub fn push(model: &mut Json, time: &str, cat: u8, lvl: u8, src: u16, code: u16, a: i32, b: i32, ctx: u32) -> bool {
    let min = get_u(model, "/Cfg/MinLevel");
    let mask = get_u(model, "/Cfg/CatMask");
    if u64::from(lvl) < min || mask & (1u64 << cat) == 0 {
        return false;
    }
    let count = get_u(model, "/Now/Count");
    let max = get_u(model, "/Cfg/MaxPerScan");
    if count >= max {
        set(model, "/Dropped", json!(get_u(model, "/Dropped") + 1));
        set(model, "/Stat/MaxPerScanHit", json!(get_u(model, "/Stat/MaxPerScanHit") + 1));
        return false;
    }
    let cap = model.pointer("/Entry").and_then(Json::as_array).map(Vec::len).unwrap_or(0) as u64;
    if cap == 0 {
        return false;
    }
    let total = get_u(model, "/Total") + 1;
    let head = get_u(model, "/Head") % cap;
    set(model, &format!("/Entry/{head}"), json!({ "Seq": total, "Time": time, "Cat": cat, "Lvl": lvl, "Src": src, "Code": code, "A": a, "B": b, "Ctx": ctx }));
    set(model, "/Total", json!(total));
    set(model, "/Head", json!((head + 1) % cap));
    set(model, "/Now/Count", json!(count + 1));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evtlog::collect::{RingGeo, index_of};

    #[test]
    fn demo_ring_follows_the_plc_contract() {
        let c = plc_layout::Contract::load_dir(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plc/contract/GR2_PLC")).unwrap();
        let size = c.size_of_db("EVTLOG").unwrap() as usize;
        let mut m = c.decode_db("EVTLOG", &vec![0u8; size]).unwrap();
        let cat = crate::evtlog::catalog::load_catalog(std::path::Path::new("-")).unwrap();
        init(&mut m, 0x1234, 3, &cat);
        begin_scan(&mut m, 40);
        assert!(!push(&mut m, "2026-09-27 10:00:00.000", 7, 1, 0, 709, 0, 0, 0), "DEBUG below MinLevel INFO");
        for i in 0..1005 {
            if i % 32 == 0 {
                begin_scan(&mut m, 40);
            }
            assert!(push(&mut m, "2026-09-27 10:00:00.000", 3, 2, 20, 300, i, 200, 9));
        }
        let mut raw = vec![0u8; size];
        c.encode_db("EVTLOG", &m, &mut raw).unwrap();
        let g = RingGeo::from_layout(&c.layout_db("EVTLOG").unwrap()).unwrap();
        let h = g.header(&raw).unwrap();
        assert_eq!((h.total, h.head, h.boot_id), (1005, 5, 3));
        let idx = index_of(1005, h.total, h.head, g.capacity) as usize;
        let e = g.entry(&raw[54 + idx * 34..54 + (idx + 1) * 34]);
        assert_eq!((e.seq, e.a), (1005, 1004));
        // MaxPerScan: the 33rd call in one scan is dropped and counted
        begin_scan(&mut m, 40);
        for _ in 0..32 {
            assert!(push(&mut m, "2026-09-27 10:00:00.000", 3, 2, 20, 300, 0, 0, 0));
        }
        assert!(!push(&mut m, "2026-09-27 10:00:00.000", 3, 2, 20, 300, 0, 0, 0));
        assert_eq!(get_u(&m, "/Dropped"), 1);
    }
}
