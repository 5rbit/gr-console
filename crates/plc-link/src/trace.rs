//! `Trace` chunks (wire-spec section 6.2): the PLC samples up to 32 variables every cycle and pushes them
//! as a fixed-size `LNK_Trace` payload. This module reads the member offsets from the contract UDT and
//! converts a chunk between its wire bytes and a header plus rows of raw 32-bit words.
//!
//! ```no_run
//! # use plc_link::{Codec, Message};
//! # use plc_link::trace::TraceLayout;
//! # fn f(codec: &Codec, m: &Message) -> Result<(), plc_link::LinkError> {
//! let layout = TraceLayout::for_message(codec, "Trace")?;
//! let chunk = layout.decode(&m.payload)?;
//! for row in &chunk.rows {
//!     let tick_ms = row[0] as i32;
//!     let first_channel = row[1];
//!     println!("{tick_ms} {first_channel:08X}");
//! }
//! # Ok(()) }
//! ```

use plc_layout::Contract;
use plc_layout::ast::{Field, Prim, TypeRef};
use plc_layout::decode::{Value, decode_prim};
use plc_layout::encode::encode_prim;
use plc_layout::layout::resolve_dims;

use crate::codec::{Codec, Message};
use crate::error::{ErrCode, LinkError};

/// UDT of a Trace chunk.
pub const TRACE_UDT: &str = "LNK_Trace";
/// Largest number of channels a `TraceCfg` may ask for (`LNK_TRACE_CH_MAX`).
pub const TRACE_CH_MAX: u16 = 32;

/// Header of a Trace chunk: everything before the rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TraceHeader {
    /// `TraceCfg.CfgId` this chunk belongs to.
    pub cfg_id: u16,
    pub chan_count: u16,
    /// Sample divider in PLC cycles (1 = every cycle).
    pub divider: u16,
    /// Valid rows.
    pub count: u16,
    /// DWords per row (`1 + chan_count`).
    pub row_words: u16,
    /// Samples dropped before this chunk (0 = lossless).
    pub overrun: u16,
    /// PLC cycle counter of row 0.
    pub first_cycle: u32,
    /// Tick of row 0 in ms (same as `rows[0][0]`).
    pub tick0: i32,
    /// PLC wall clock of row 0 as wire DTL text (`YYYY-MM-DD HH:MM:SS.mmm`); empty when unset.
    pub time0: String,
}

/// A decoded Trace chunk.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TraceChunk {
    pub header: TraceHeader,
    /// `header.count` rows of `header.row_words` raw words: element 0 is the sample tick in ms (bit pattern
    /// of a DInt), elements 1.. are the channel values (a Real is its IEEE-754 bit pattern, a Bool 0 or 1,
    /// narrower integers are zero-extended).
    pub rows: Vec<Vec<u32>>,
}

impl TraceChunk {
    /// Chunk from complete rows; `row_words`, `count` and `tick0` follow from them.
    pub fn from_rows(cfg_id: u16, chan_count: u16, divider: u16, rows: Vec<Vec<u32>>) -> TraceChunk {
        let header = TraceHeader {
            cfg_id,
            chan_count,
            divider,
            count: rows.len() as u16,
            row_words: chan_count + 1,
            overrun: 0,
            first_cycle: 0,
            tick0: rows.first().map(|r| Self::tick_of(r)).unwrap_or(0),
            time0: String::new(),
        };
        TraceChunk { header, rows }
    }

    /// Sample tick of a row in ms.
    pub fn tick_of(row: &[u32]) -> i32 {
        row.first().copied().unwrap_or(0) as i32
    }

    /// Raw word of channel `c` in a row.
    pub fn channel_of(row: &[u32], c: usize) -> Option<u32> {
        row.get(c + 1).copied()
    }

    /// `FirstCycle` the next chunk of the same configuration must start at.
    pub fn next_cycle(&self) -> u32 {
        self.header.first_cycle.wrapping_add(self.header.count as u32 * self.header.divider.max(1) as u32)
    }
}

fn bad(msg: impl Into<String>) -> LinkError {
    LinkError::new(ErrCode::BadLength, msg)
}

/// Member offsets of an `LNK_Trace` payload, read once from the contract.
#[derive(Clone, Debug)]
pub struct TraceLayout {
    udt: String,
    size: usize,
    cfg_id: u32,
    chan_count: u32,
    divider: u32,
    count: u32,
    row_words: u32,
    overrun: u32,
    first_cycle: u32,
    tick0: u32,
    time0: u32,
    data: u32,
    capacity: usize,
}

fn scalar(c: &Contract, fields: &[Field], path: &str, want: Prim) -> Result<u32, LinkError> {
    let (ty, off) = c.locate(fields, path)?;
    match &ty {
        TypeRef::Prim(p) if *p == want => Ok(off),
        other => Err(LinkError::parse(format!("{path}: expected {want:?}, found {other:?}"))),
    }
}

impl TraceLayout {
    /// Reads the layout of [`TRACE_UDT`] from a contract.
    pub fn new(contract: &Contract) -> Result<Self, LinkError> {
        Self::from_udt(contract, TRACE_UDT)
    }

    /// Reads the layout of the UDT a registry message carries (e.g. `"Trace"`).
    pub fn for_message(codec: &Codec, message: &str) -> Result<Self, LinkError> {
        let spec = codec.spec_by_name(message)?;
        let udt = spec.udt.as_deref().ok_or_else(|| LinkError::new(ErrCode::UnknownType, format!("{message} has no payload UDT")))?;
        Self::from_udt(codec.contract(), udt)
    }

    /// Reads the layout of a UDT shaped like [`TRACE_UDT`].
    pub fn from_udt(contract: &Contract, udt: &str) -> Result<Self, LinkError> {
        let decl = contract.udt(udt)?;
        let f = &decl.fields;
        let (ty, data) = contract.locate(f, "Data")?;
        let TypeRef::Array { dims, elem } = &ty else {
            return Err(LinkError::parse(format!("{udt}.Data: expected an array, found {ty:?}")));
        };
        if **elem != TypeRef::Prim(Prim::DWord) {
            return Err(LinkError::parse(format!("{udt}.Data: expected an array of DWord, found {elem:?}")));
        }
        let capacity = resolve_dims(contract, dims)?.iter().map(|(lo, hi)| (hi - lo + 1).max(0) as usize).product();
        Ok(TraceLayout {
            udt: udt.to_string(),
            size: contract.size_of_udt(udt)? as usize,
            cfg_id: scalar(contract, f, "CfgId", Prim::UInt)?,
            chan_count: scalar(contract, f, "ChanCount", Prim::UInt)?,
            divider: scalar(contract, f, "Divider", Prim::UInt)?,
            count: scalar(contract, f, "Count", Prim::UInt)?,
            row_words: scalar(contract, f, "RowWords", Prim::UInt)?,
            overrun: scalar(contract, f, "Overrun", Prim::UInt)?,
            first_cycle: scalar(contract, f, "FirstCycle", Prim::UDInt)?,
            tick0: scalar(contract, f, "Tick0", Prim::DInt)?,
            time0: scalar(contract, f, "Time0", Prim::Dtl)?,
            data,
            capacity,
        })
    }

    /// Payload size of one chunk.
    pub fn size(&self) -> usize {
        self.size
    }

    /// `Data` elements, i.e. the largest `Count * RowWords`.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Rows that fit in one chunk with `chan_count` channels (0 when the channel count is impossible).
    pub fn rows_per_chunk(&self, chan_count: u16) -> usize {
        if chan_count == 0 || chan_count > TRACE_CH_MAX { 0 } else { self.capacity / (chan_count as usize + 1) }
    }

    fn u16_at(&self, b: &[u8], off: u32) -> u16 {
        let i = off as usize;
        u16::from_be_bytes([b[i], b[i + 1]])
    }

    fn u32_at(&self, b: &[u8], off: u32) -> u32 {
        let i = off as usize;
        u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
    }

    /// Checks `ChanCount`, `RowWords` and `Count` against each other and against the chunk capacity.
    fn check(&self, h: &TraceHeader) -> Result<(), LinkError> {
        if h.chan_count > TRACE_CH_MAX {
            return Err(bad(format!("{}: ChanCount {} above {TRACE_CH_MAX}", self.udt, h.chan_count)));
        }
        if h.row_words != h.chan_count + 1 {
            return Err(bad(format!("{}: RowWords {} but ChanCount {} (expected {})", self.udt, h.row_words, h.chan_count, h.chan_count + 1)));
        }
        let words = h.count as usize * h.row_words as usize;
        if words > self.capacity {
            return Err(bad(format!("{}: Count {} × RowWords {} = {words} words, capacity {}", self.udt, h.count, h.row_words, self.capacity)));
        }
        Ok(())
    }

    /// Wire bytes → header and rows. Rejects a payload of the wrong size and a header whose row geometry
    /// does not fit the chunk.
    pub fn decode(&self, payload: &[u8]) -> Result<TraceChunk, LinkError> {
        if payload.len() != self.size {
            return Err(bad(format!("{}: {} payload bytes, expected {}", self.udt, payload.len(), self.size)));
        }
        let time0 = match decode_prim(payload, Prim::Dtl, self.time0, None)? {
            Value::Str(s) => s,
            other => other.to_json().to_string(),
        };
        let header = TraceHeader {
            cfg_id: self.u16_at(payload, self.cfg_id),
            chan_count: self.u16_at(payload, self.chan_count),
            divider: self.u16_at(payload, self.divider),
            count: self.u16_at(payload, self.count),
            row_words: self.u16_at(payload, self.row_words),
            overrun: self.u16_at(payload, self.overrun),
            first_cycle: self.u32_at(payload, self.first_cycle),
            tick0: self.u32_at(payload, self.tick0) as i32,
            time0,
        };
        self.check(&header)?;
        let width = header.row_words as usize;
        let mut rows = Vec::with_capacity(header.count as usize);
        for r in 0..header.count as usize {
            let start = self.data as usize + r * width * 4;
            rows.push((0..width).map(|w| self.u32_at(payload, (start + w * 4) as u32)).collect());
        }
        Ok(TraceChunk { header, rows })
    }

    /// Header and rows → wire bytes. The rows must match `count` and `row_words`.
    pub fn encode(&self, chunk: &TraceChunk) -> Result<Vec<u8>, LinkError> {
        let h = &chunk.header;
        self.check(h)?;
        if chunk.rows.len() != h.count as usize {
            return Err(bad(format!("{}: {} rows but Count {}", self.udt, chunk.rows.len(), h.count)));
        }
        let mut b = vec![0u8; self.size];
        for (off, v) in [(self.cfg_id, h.cfg_id), (self.chan_count, h.chan_count), (self.divider, h.divider), (self.count, h.count), (self.row_words, h.row_words), (self.overrun, h.overrun)] {
            b[off as usize..off as usize + 2].copy_from_slice(&v.to_be_bytes());
        }
        let fc = self.first_cycle as usize;
        b[fc..fc + 4].copy_from_slice(&h.first_cycle.to_be_bytes());
        let t0 = self.tick0 as usize;
        b[t0..t0 + 4].copy_from_slice(&h.tick0.to_be_bytes());
        encode_prim(&mut b, Prim::Dtl, self.time0, None, &Value::Str(h.time0.clone()), "Time0")?;
        let width = h.row_words as usize;
        for (r, row) in chunk.rows.iter().enumerate() {
            if row.len() != width {
                return Err(bad(format!("{}: row {r} has {} words, expected {width}", self.udt, row.len())));
            }
            let start = self.data as usize + r * width * 4;
            for (w, x) in row.iter().enumerate() {
                b[start + w * 4..start + w * 4 + 4].copy_from_slice(&x.to_be_bytes());
            }
        }
        Ok(b)
    }
}

/// Decodes the payload of a received `Trace` message. Cache a [`TraceLayout`] when decoding many chunks.
pub fn decode_trace(codec: &Codec, m: &Message) -> Result<TraceChunk, LinkError> {
    let spec = codec.spec(m.id)?;
    let layout = TraceLayout::for_message(codec, &spec.name.clone())?;
    layout.decode(&m.payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRACE_SRC: &str = r#"TYPE "LNK_Trace"
VERSION : 0.1
   STRUCT
      CfgId : UInt;
      ChanCount : UInt;
      Divider : UInt;
      Count : UInt;
      RowWords : UInt;
      Overrun : UInt;
      FirstCycle : UDInt;
      Tick0 : DInt;
      Time0 {InstructionName := 'DTL'; LibVersion := '1.0'} : DTL;
      Data : Array[0..2031] of DWord;
   END_STRUCT;
END_TYPE
"#;

    fn layout() -> TraceLayout {
        let mut c = Contract::new();
        c.add_udt_source(TRACE_SRC).unwrap();
        TraceLayout::new(&c).unwrap()
    }

    fn rows(count: usize, chan: u16) -> Vec<Vec<u32>> {
        (0..count).map(|r| (0..=chan as usize).map(|w| (r * 100 + w) as u32).collect()).collect()
    }

    #[test]
    fn offsets_and_size_follow_the_contract() {
        let l = layout();
        assert_eq!((l.size(), l.capacity()), (8160, 2032));
        assert_eq!((l.cfg_id, l.chan_count, l.count, l.row_words, l.overrun), (0, 2, 6, 8, 10));
        assert_eq!((l.first_cycle, l.tick0, l.time0, l.data), (12, 16, 20, 32));
        assert_eq!(l.rows_per_chunk(3), 508);
        assert_eq!((l.rows_per_chunk(0), l.rows_per_chunk(33)), (0, 0));
    }

    #[test]
    fn empty_chunk_round_trips() {
        let l = layout();
        let mut chunk = TraceChunk::from_rows(7, 4, 2, Vec::new());
        chunk.header.time0 = "2026-09-16 10:20:30.500".to_string();
        let bytes = l.encode(&chunk).unwrap();
        assert_eq!(bytes.len(), 8160);
        let back = l.decode(&bytes).unwrap();
        assert_eq!(back.header.count, 0);
        assert!(back.rows.is_empty());
        assert_eq!(back, chunk);
        assert_eq!(back.next_cycle(), 0);
    }

    #[test]
    fn full_chunk_round_trips() {
        let l = layout();
        // 3 channels → 4 words per row → 508 rows fill the chunk exactly
        let n = l.rows_per_chunk(3);
        let mut chunk = TraceChunk::from_rows(1, 3, 1, rows(n, 3));
        chunk.header.first_cycle = 1000;
        chunk.header.overrun = 5;
        chunk.header.time0 = "2026-09-16 10:20:30.500".to_string();
        let bytes = l.encode(&chunk).unwrap();
        let back = l.decode(&bytes).unwrap();
        assert_eq!(back, chunk);
        assert_eq!(back.header.count as usize, n);
        assert_eq!(back.rows[n - 1], chunk.rows[n - 1]);
        assert_eq!(TraceChunk::tick_of(&back.rows[2]), 200);
        assert_eq!(TraceChunk::channel_of(&back.rows[2], 2), Some(203));
        assert_eq!(TraceChunk::channel_of(&back.rows[2], 3), None);
        assert_eq!(back.next_cycle(), 1000 + n as u32);
    }

    #[test]
    fn real_channel_keeps_its_bit_pattern() {
        let l = layout();
        let chunk = TraceChunk::from_rows(1, 1, 1, vec![vec![12u32, (-1.5f32).to_bits()]]);
        let back = l.decode(&l.encode(&chunk).unwrap()).unwrap();
        assert_eq!(f32::from_bits(TraceChunk::channel_of(&back.rows[0], 0).unwrap()), -1.5);
    }

    #[test]
    fn row_words_inconsistent_with_chan_count_is_rejected() {
        let l = layout();
        let mut chunk = TraceChunk::from_rows(1, 4, 1, rows(2, 4));
        chunk.header.row_words = 4;
        let e = l.encode(&chunk).unwrap_err();
        assert!(e.msg.contains("RowWords 4") && e.msg.contains("ChanCount 4"), "{e}");
        // and on the way in: patch RowWords in a valid chunk
        let mut bytes = l.encode(&TraceChunk::from_rows(1, 4, 1, rows(2, 4))).unwrap();
        bytes[l.row_words as usize + 1] = 9;
        let e = l.decode(&bytes).unwrap_err();
        assert_eq!(e.code, ErrCode::BadLength);
        assert!(e.msg.contains("RowWords 9"), "{e}");
    }

    #[test]
    fn count_beyond_the_chunk_is_rejected() {
        let l = layout();
        let mut bytes = l.encode(&TraceChunk::from_rows(1, 3, 1, rows(4, 3))).unwrap();
        // 509 rows × 4 words = 2036 > 2032
        bytes[l.count as usize..l.count as usize + 2].copy_from_slice(&509u16.to_be_bytes());
        let e = l.decode(&bytes).unwrap_err();
        assert!(e.msg.contains("capacity 2032"), "{e}");
        let mut chunk = TraceChunk::from_rows(1, 3, 1, rows(4, 3));
        chunk.header.count = 509;
        assert!(l.encode(&chunk).is_err());
    }

    #[test]
    fn too_many_channels_and_wrong_payload_size_are_rejected() {
        let l = layout();
        let mut bytes = l.encode(&TraceChunk::from_rows(1, 3, 1, rows(1, 3))).unwrap();
        bytes[l.chan_count as usize..l.chan_count as usize + 2].copy_from_slice(&33u16.to_be_bytes());
        assert!(l.decode(&bytes).unwrap_err().msg.contains("ChanCount 33"));
        assert!(l.decode(&bytes[..8159]).unwrap_err().msg.contains("8159 payload bytes"));
    }

    #[test]
    fn rows_must_match_the_count() {
        let l = layout();
        let mut chunk = TraceChunk::from_rows(1, 2, 1, rows(3, 2));
        chunk.header.count = 2;
        assert!(l.encode(&chunk).unwrap_err().msg.contains("3 rows but Count 2"));
        let mut chunk = TraceChunk::from_rows(1, 2, 1, rows(3, 2));
        chunk.rows[1].pop();
        assert!(l.encode(&chunk).unwrap_err().msg.contains("row 1 has 2 words"));
    }
}
