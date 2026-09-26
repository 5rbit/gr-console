//! EVTLOG ring collector: header from the fast tier, body read only when `Total` advances.
//!
//! Ring rule (PLC contract): the entry written with Seq `s` sits at index `(Head - (Total - s) - 1) mod capacity`.
//! Epoch: `BootId` changed or `Total` went backwards → the PLC restarted / the DB was reinitialized.

use plc_layout::Layout;

use super::store::CollState;

/// Byte offsets resolved from the contract layout (nothing hard-coded beyond the member names).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RingGeo {
    pub capacity: u32,
    pub entry_off: u32,
    pub stride: u32,
    sig: u32,
    total: u32,
    head: u32,
    boot_id: u32,
    dropped: u32,
    // relative to the entry start
    e_seq: u32,
    e_time: u32,
    e_cat: u32,
    e_lvl: u32,
    e_src: u32,
    e_code: u32,
    e_a: u32,
    e_b: u32,
    e_ctx: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Header {
    pub layout_sig: u32,
    pub total: u32,
    pub head: i16,
    pub boot_id: u16,
    pub dropped: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    pub seq: u32,
    /// DTL bytes → (year, month, day, hour, minute, second, ms); `None` when the DTL is empty / invalid.
    pub time: Option<(i32, u8, u8, u8, u8, u8, u16)>,
    pub cat: u8,
    pub lvl: u8,
    pub src: u16,
    pub code: u16,
    pub a: i32,
    pub b: i32,
    pub ctx: u32,
}

fn be16(b: &[u8], o: u32) -> u16 {
    let o = o as usize;
    u16::from_be_bytes([b[o], b[o + 1]])
}
fn be32(b: &[u8], o: u32) -> u32 {
    let o = o as usize;
    u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

impl RingGeo {
    pub fn from_layout(l: &Layout) -> Option<RingGeo> {
        let off = |p: &str| l.find(p).map(|m| m.offset);
        let (e0, _) = l.range_of("Entry[0]")?;
        let (lo, hi) = l.range_of("Entry")?;
        let e1 = off("Entry[1].Seq").and_then(|s1| off("Entry[0].Seq").map(|s0| s1 - s0));
        let stride = e1.unwrap_or(hi - lo);
        if stride == 0 {
            return None;
        }
        let rel = |m: &str| off(&format!("Entry[0].{m}")).map(|o| o - e0);
        Some(RingGeo {
            capacity: (hi - lo) / stride,
            entry_off: lo,
            stride,
            sig: off("LayoutSig")?,
            total: off("Total")?,
            head: off("Head")?,
            boot_id: off("BootId")?,
            dropped: off("Dropped")?,
            e_seq: rel("Seq")?,
            e_time: rel("Time")?,
            e_cat: rel("Cat")?,
            e_lvl: rel("Lvl")?,
            e_src: rel("Src")?,
            e_code: rel("Code")?,
            e_a: rel("A")?,
            e_b: rel("B")?,
            e_ctx: rel("Ctx")?,
        })
    }

    /// Header fields from DB bytes that start at offset 0 (the fast-tier snapshot).
    pub fn header(&self, raw: &[u8]) -> Option<Header> {
        let need = [self.sig, self.total, self.boot_id, self.dropped].into_iter().max()? as usize + 4;
        if raw.len() < need {
            return None;
        }
        Some(Header { layout_sig: be32(raw, self.sig), total: be32(raw, self.total), head: be16(raw, self.head) as i16, boot_id: be16(raw, self.boot_id), dropped: be32(raw, self.dropped) })
    }

    /// One entry from its `stride` bytes.
    pub fn entry(&self, b: &[u8]) -> Entry {
        let t = self.e_time as usize;
        let year = be16(b, self.e_time);
        let (mon, day, hour, min, sec) = (b[t + 2], b[t + 3], b[t + 5], b[t + 6], b[t + 7]);
        let ns = be32(b, self.e_time + 8);
        let time = (year >= 1970 && (1..=12).contains(&mon) && (1..=31).contains(&day) && hour < 24 && min < 60 && sec < 60).then_some((
            i32::from(year),
            mon,
            day,
            hour,
            min,
            sec,
            (ns / 1_000_000).min(999) as u16,
        ));
        Entry {
            seq: be32(b, self.e_seq),
            time,
            cat: b[self.e_cat as usize],
            lvl: b[self.e_lvl as usize],
            src: be16(b, self.e_src),
            code: be16(b, self.e_code),
            a: be32(b, self.e_a) as i32,
            b: be32(b, self.e_b) as i32,
            ctx: be32(b, self.e_ctx),
        }
    }
}

/// Index of the entry with Seq `seq`.
pub fn index_of(seq: u32, total: u32, head: i16, capacity: u32) -> u32 {
    (i64::from(head) - (i64::from(total) - i64::from(seq)) - 1).rem_euclid(i64::from(capacity)) as u32
}

/// A contiguous run of ring slots: `n` entries from index `idx`, the first holding Seq `seq`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub idx: u32,
    pub n: u32,
    pub seq: u32,
}

/// Seqs `first..=last` → reads split at the ring wrap and at `max_n` entries (PDU budget).
pub fn plan(first: u32, last: u32, total: u32, head: i16, capacity: u32, max_n: u32) -> Vec<Span> {
    let mut out = Vec::new();
    if first == 0 || first > last || capacity == 0 {
        return out;
    }
    let max_n = max_n.max(1);
    let mut s = first;
    while s <= last {
        let idx = index_of(s, total, head, capacity);
        let n = (last - s + 1).min(capacity - idx).min(max_n);
        out.push(Span { idx, n, seq: s });
        s += n;
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    /// Seqs to read (inclusive); `None` = nothing new.
    pub range: Option<(u32, u32)>,
    /// `(lost, after_seq)`: more new entries than the ring holds.
    pub gap: Option<(u32, u32)>,
    /// A new epoch started (restart / reinitialized DB).
    pub new_epoch: bool,
}

/// Next step for a header; updates `st` as if the read succeeds (the caller persists it after storing the rows).
/// The very first header ever seen only starts tracking: its backlog is read, but no gap is reported for it.
pub fn decide(st: &mut CollState, h: &Header, capacity: u32) -> Decision {
    let mut new_epoch = false;
    let tracking = st.boot_id.is_some();
    match st.boot_id {
        None => {
            st.boot_id = Some(h.boot_id);
            st.epoch = st.epoch.max(1);
            st.last_seq = st.last_seq.min(h.total);
        }
        Some(b) if b != h.boot_id || h.total < st.last_seq => {
            st.epoch += 1;
            st.boot_id = Some(h.boot_id);
            st.last_seq = 0;
            new_epoch = true;
        }
        Some(_) => {}
    }
    if h.total <= st.last_seq {
        return Decision { range: None, gap: None, new_epoch };
    }
    let missing = h.total - st.last_seq;
    let (first, gap) = if missing > capacity {
        let first = h.total - capacity + 1;
        (first, (tracking || new_epoch).then_some((missing - capacity, st.last_seq)))
    } else {
        (st.last_seq + 1, None)
    };
    let range = Some((first, h.total));
    st.last_seq = h.total;
    Decision { range, gap, new_epoch }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_follows_the_ring_rule() {
        // capacity 10, 23 written: Head = 3, newest (23) at 2, 14 at 3
        assert_eq!(index_of(23, 23, 3, 10), 2);
        assert_eq!(index_of(21, 23, 3, 10), 0);
        assert_eq!(index_of(20, 23, 3, 10), 9);
        assert_eq!(index_of(14, 23, 3, 10), 3);
        // before the first wrap
        assert_eq!(index_of(1, 5, 5, 10), 0);
    }

    #[test]
    fn plan_splits_at_the_wrap_and_the_pdu_budget() {
        assert_eq!(plan(18, 23, 23, 3, 10, 100), vec![Span { idx: 7, n: 3, seq: 18 }, Span { idx: 0, n: 3, seq: 21 }]);
        assert_eq!(plan(14, 23, 23, 3, 10, 4), vec![Span { idx: 3, n: 4, seq: 14 }, Span { idx: 7, n: 3, seq: 18 }, Span { idx: 0, n: 3, seq: 21 }]);
        assert_eq!(plan(1, 5, 5, 5, 10, 2), vec![Span { idx: 0, n: 2, seq: 1 }, Span { idx: 2, n: 2, seq: 3 }, Span { idx: 4, n: 1, seq: 5 }]);
        assert!(plan(5, 4, 5, 5, 10, 2).is_empty());
        let total: u32 = plan(14, 23, 23, 3, 10, 3).iter().map(|s| s.n).sum();
        assert_eq!(total, 10);
    }

    fn hdr(total: u32, boot_id: u16) -> Header {
        Header { total, boot_id, head: (total % 10) as i16, ..Default::default() }
    }

    #[test]
    fn first_sight_reads_the_backlog_without_a_gap() {
        let mut st = CollState { epoch: 0, boot_id: None, last_seq: 0 };
        let d = decide(&mut st, &hdr(25, 7), 10);
        assert_eq!(d, Decision { range: Some((16, 25)), gap: None, new_epoch: false });
        assert_eq!(st, CollState { epoch: 1, boot_id: Some(7), last_seq: 25 });
        assert_eq!(decide(&mut st, &hdr(25, 7), 10).range, None);
        assert_eq!(decide(&mut st, &hdr(28, 7), 10), Decision { range: Some((26, 28)), gap: None, new_epoch: false });
    }

    #[test]
    fn falling_behind_more_than_the_ring_is_a_gap() {
        let mut st = CollState { epoch: 3, boot_id: Some(7), last_seq: 100 };
        let d = decide(&mut st, &hdr(125, 7), 10);
        assert_eq!(d, Decision { range: Some((116, 125)), gap: Some((15, 100)), new_epoch: false });
        assert_eq!(st.last_seq, 125);
    }

    #[test]
    fn restart_starts_a_new_epoch() {
        // BootId changed
        let mut st = CollState { epoch: 3, boot_id: Some(7), last_seq: 100 };
        let d = decide(&mut st, &hdr(4, 8), 10);
        assert_eq!(d, Decision { range: Some((1, 4)), gap: None, new_epoch: true });
        assert_eq!(st, CollState { epoch: 4, boot_id: Some(8), last_seq: 4 });
        // Total went backwards with the same BootId (DB reinitialized) — and wrapped already: gap in the new epoch
        let mut st = CollState { epoch: 1, boot_id: Some(8), last_seq: 500 };
        let d = decide(&mut st, &hdr(30, 8), 10);
        assert_eq!(d, Decision { range: Some((21, 30)), gap: Some((20, 0)), new_epoch: true });
        assert_eq!(st.epoch, 2);
    }

    #[test]
    fn geometry_and_entry_decode_from_the_contract() {
        let c = plc_layout::Contract::load_dir(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plc/contract/GR2_PLC")).unwrap();
        let l = c.layout_db("EVTLOG").unwrap();
        let g = RingGeo::from_layout(&l).unwrap();
        assert_eq!((g.capacity, g.entry_off, g.stride), (1000, 54, 34));
        let mut raw = vec![0u8; l.size as usize];
        let e = serde_json::json!({ "Seq": 77, "Time": "2026-09-27 10:21:03.120", "Cat": 7, "Lvl": 3, "Src": 5, "Code": 704, "A": -5984, "B": 38, "Ctx": 5123 });
        c.encode_path("EVTLOG", "Entry[2]", &e, &mut raw).unwrap();
        c.encode_path("EVTLOG", "Total", &serde_json::json!(77), &mut raw).unwrap();
        c.encode_path("EVTLOG", "Head", &serde_json::json!(3), &mut raw).unwrap();
        c.encode_path("EVTLOG", "BootId", &serde_json::json!(9), &mut raw).unwrap();
        let h = g.header(&raw).unwrap();
        assert_eq!((h.total, h.head, h.boot_id), (77, 3, 9));
        let at = (g.entry_off + 2 * g.stride) as usize;
        let got = g.entry(&raw[at..at + g.stride as usize]);
        assert_eq!(got, Entry { seq: 77, time: Some((2026, 9, 27, 10, 21, 3, 120)), cat: 7, lvl: 3, src: 5, code: 704, a: -5984, b: 38, ctx: 5123 });
        assert_eq!(index_of(77, h.total, h.head, g.capacity), 2);
        assert_eq!(g.entry(&[0u8; 34]).time, None);
    }
}
