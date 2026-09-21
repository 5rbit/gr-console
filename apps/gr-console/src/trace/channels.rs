//! Trace channel catalogue and PEEK descriptors.
//!
//! The PLC samples a channel with `PEEK`, which only reaches standard-access (non-optimized) data blocks and the
//! I / Q / M areas, and only in widths of one bit, one, two or four bytes. This module turns the contract's member
//! table into the list of paths that satisfy those rules, builds the `LNK_TraceChan` descriptors the PLC expects,
//! and decodes the raw 32-bit words a chunk carries back into numbers.

use plc_layout::Contract;
use plc_layout::ast::Prim;
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

/// PEEK area code for a data block (wire-spec section 6.1). The PLC also accepts I / Q / M (0x81 / 0x82 / 0x83),
/// which the console does not offer yet because the contract has no address table for them.
pub const AREA_DB: u8 = 0x84;

/// Largest channel count a `LNK_TraceCfg` can carry.
pub const CHAN_MAX: usize = 32;
/// Smallest `FlushMs` the PLC accepts.
pub const FLUSH_MIN_MS: u16 = 20;
/// Byte offset the PLC refuses beyond (standard-access DBs and I / Q / M stay inside 64 KB).
pub const OFFSET_MAX: u32 = 65535;

/// How a channel's raw 32-bit word is read back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Bool,
    /// Unsigned integer of `bits` bits.
    Uint,
    /// Signed integer of `bits` bits (sign-extended from `bits`).
    Int,
    /// IEEE-754 single precision.
    Real,
    /// TIME in milliseconds (a signed 32-bit value).
    TimeMs,
}

/// One traceable variable.
#[derive(Clone, Debug, Serialize)]
pub struct Channel {
    /// `<DB>.<member path>`, e.g. `WEBMON.Axis[1].Position`.
    pub path: String,
    pub db: String,
    pub db_no: u16,
    pub offset: u32,
    /// Bit number for a `Bool`.
    pub bit: Option<u8>,
    /// PLC value width: 0 bit, 1, 2 or 4 bytes.
    pub width: u8,
    pub kind: Kind,
    /// Number of significant bits (8 / 16 / 32; 1 for a Bool).
    pub bits: u8,
    /// PLC type name, for display.
    pub type_name: String,
}

impl Channel {
    /// The `LNK_TraceChan` JSON the PLC reads.
    pub fn descriptor(&self) -> Json {
        json!({
            "Area": AREA_DB,
            "Width": self.width,
            "Bit": self.bit.unwrap_or(0),
            "DbNo": self.db_no,
            "Offset": self.offset,
        })
    }

    /// Value of one sample word, as a number for charts and tables.
    pub fn value(&self, word: u32) -> f64 {
        match self.kind {
            Kind::Bool => f64::from(word & 1),
            Kind::Uint => f64::from(word),
            Kind::Int | Kind::TimeMs => f64::from(sign_extend(word, self.bits)),
            Kind::Real => f64::from(f32::from_bits(word)),
        }
    }
}

/// Sign-extends the low `bits` of `word`.
fn sign_extend(word: u32, bits: u8) -> i32 {
    match bits {
        8 => i32::from(word as u8 as i8),
        16 => i32::from(word as u16 as i16),
        _ => word as i32,
    }
}

/// Width, kind and significant bits of a traceable primitive; `None` for types PEEK cannot deliver in one word
/// (8-byte types, DTL, String, Date / TOD).
fn shape(p: Prim) -> Option<(u8, Kind, u8)> {
    Some(match p {
        Prim::Bool => (0, Kind::Bool, 1),
        Prim::Byte | Prim::USInt | Prim::Char => (1, Kind::Uint, 8),
        Prim::SInt => (1, Kind::Int, 8),
        Prim::Word | Prim::UInt => (2, Kind::Uint, 16),
        Prim::Int => (2, Kind::Int, 16),
        Prim::DWord | Prim::UDInt => (4, Kind::Uint, 32),
        Prim::DInt => (4, Kind::Int, 32),
        Prim::Real => (4, Kind::Real, 32),
        Prim::Time => (4, Kind::TimeMs, 32),
        _ => return None,
    })
}

/// Whether a DB can be traced at all: standard access and a known DB number.
pub fn traceable_db(c: &Contract, db: &str) -> bool {
    c.dbs.get(db).is_some_and(|d| !d.optimized) && c.db_number(db).is_some()
}

/// Traceable DB names, sorted.
pub fn databases(c: &Contract) -> Vec<String> {
    let mut v: Vec<String> = c.dbs.keys().filter(|n| traceable_db(c, n)).cloned().collect();
    v.sort();
    v
}

/// Channels of one DB, in layout order. An unknown or optimized DB yields an empty list.
pub fn channels_of(c: &Contract, db: &str) -> Vec<Channel> {
    if !traceable_db(c, db) {
        return Vec::new();
    }
    let (Ok(layout), Some(db_no)) = (c.layout_db(db), c.db_number(db)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for m in layout.members {
        let Some((width, kind, bits)) = shape(m.prim) else { continue };
        if m.offset > OFFSET_MAX || m.offset + u32::from(width.max(1)) > OFFSET_MAX + 1 {
            continue;
        }
        out.push(Channel { path: format!("{db}.{}", m.path), db: db.to_string(), db_no, offset: m.offset, bit: m.bit, width, kind, bits, type_name: m.prim.name().to_string() });
    }
    out
}

/// Every traceable channel of every traceable DB whose path contains `query` (case-insensitive), at most `limit`.
/// Returns the matches and the total number of matches before the limit.
pub fn search(c: &Contract, query: &str, limit: usize) -> (Vec<Channel>, usize) {
    let q = query.trim().to_ascii_lowercase();
    let mut total = 0;
    let mut out = Vec::new();
    for db in databases(c) {
        for ch in channels_of(c, &db) {
            if !q.is_empty() && !ch.path.to_ascii_lowercase().contains(&q) {
                continue;
            }
            total += 1;
            if out.len() < limit {
                out.push(ch);
            }
        }
    }
    (out, total)
}

/// Resolves one `<DB>.<member>` path. The error text names what was wrong, for the API response.
pub fn resolve(c: &Contract, path: &str) -> Result<Channel, String> {
    let (db, member) = path.split_once('.').ok_or_else(|| format!("{path}: expected <DB>.<member>"))?;
    if !c.dbs.contains_key(db) {
        return Err(format!("{path}: DB {db} is not in the contract"));
    }
    if c.dbs.get(db).is_some_and(|d| d.optimized) {
        return Err(format!("{path}: DB {db} uses optimized access, which PEEK cannot read"));
    }
    let db_no = c.db_number(db).ok_or_else(|| format!("{path}: DB number of {db} unknown (missing .xml in the contract)"))?;
    let layout = c.layout_db(db).map_err(|e| format!("{path}: {e}"))?;
    let m = layout.find(member).ok_or_else(|| format!("{path}: no such member"))?;
    let (width, kind, bits) = shape(m.prim).ok_or_else(|| format!("{path}: type {} cannot be traced", m.prim.name()))?;
    if m.offset > OFFSET_MAX || m.offset + u32::from(width.max(1)) > OFFSET_MAX + 1 {
        return Err(format!("{path}: offset {} is beyond the PLC limit {OFFSET_MAX}", m.offset));
    }
    Ok(Channel { path: path.to_string(), db: db.to_string(), db_no, offset: m.offset, bit: m.bit, width, kind, bits, type_name: m.prim.name().to_string() })
}

/// The `LNK_TraceCfg` JSON for a start (`Cmd` 1) or stop (`Cmd` 0) request.
pub fn config_json(cmd: u8, cfg_id: u16, divider: u16, flush_ms: u16, chans: &[Channel]) -> Json {
    let mut list: Vec<Json> = chans.iter().map(Channel::descriptor).collect();
    // The PLC reads only the first ChanCount entries, but a JSON reader fills what it is given: pad so a shorter
    // configuration never leaves stale entries behind.
    while list.len() < CHAN_MAX {
        list.push(json!({"Area": 0, "Width": 0, "Bit": 0, "DbNo": 0, "Offset": 0}));
    }
    json!({
        "Cmd": cmd,
        "CfgId": cfg_id,
        "ChanCount": chans.len(),
        "Divider": divider.max(1),
        "FlushMs": flush_ms.max(FLUSH_MIN_MS),
        "Chan": list,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract() -> Contract {
        let mut c = Contract::new();
        c.add_db_source(
            "DATA_BLOCK \"T\"\n{ S7_Optimized_Access := 'FALSE' }\nVERSION : 0.1\n   STRUCT \n      Sig : DWord;\n      Flag : Bool;\n      Small : SInt;\n      Count : Int;\n      Pos : Real;\n      Span : Time;\n      Big : LReal;\n      When : DTL;\n   END_STRUCT;\n\nBEGIN\nEND_DATA_BLOCK\n",
        )
        .unwrap();
        c.add_db_source("DATA_BLOCK \"O\"\n{ S7_Optimized_Access := 'TRUE' }\nVERSION : 0.1\n   STRUCT \n      X : Real;\n   END_STRUCT;\n\nBEGIN\nEND_DATA_BLOCK\n").unwrap();
        c.db_numbers.insert("T".to_string(), 7);
        c.db_numbers.insert("O".to_string(), 8);
        c
    }

    #[test]
    fn catalogue_skips_optimized_dbs_and_wide_types() {
        let c = contract();
        assert_eq!(databases(&c), vec!["T".to_string()]);
        let paths: Vec<String> = channels_of(&c, "T").into_iter().map(|ch| ch.path).collect();
        assert_eq!(paths, ["T.Sig", "T.Flag", "T.Small", "T.Count", "T.Pos", "T.Span"]);
        assert!(channels_of(&c, "O").is_empty());
        assert!(resolve(&c, "T.Big").unwrap_err().contains("cannot be traced"));
        assert!(resolve(&c, "T.When").unwrap_err().contains("cannot be traced"));
        assert!(resolve(&c, "O.X").unwrap_err().contains("optimized"));
        assert!(resolve(&c, "T.Nope").unwrap_err().contains("no such member"));
        assert!(resolve(&c, "Nope").unwrap_err().contains("expected"));
    }

    #[test]
    fn descriptors_carry_db_number_offset_and_width() {
        let c = contract();
        let flag = resolve(&c, "T.Flag").unwrap();
        assert_eq!((flag.width, flag.bit, flag.offset), (0, Some(0), 4));
        assert_eq!(flag.descriptor(), json!({"Area": 0x84, "Width": 0, "Bit": 0, "DbNo": 7, "Offset": 4}));
        let pos = resolve(&c, "T.Pos").unwrap();
        assert_eq!((pos.width, pos.kind, pos.bits), (4, Kind::Real, 32));
        assert_eq!(pos.descriptor()["Width"], json!(4));
    }

    #[test]
    fn words_decode_by_kind() {
        let c = contract();
        assert_eq!(resolve(&c, "T.Pos").unwrap().value(1.5f32.to_bits()), 1.5);
        assert_eq!(resolve(&c, "T.Flag").unwrap().value(1), 1.0);
        assert_eq!(resolve(&c, "T.Small").unwrap().value(0xFF), -1.0);
        assert_eq!(resolve(&c, "T.Count").unwrap().value(0xFFFF), -1.0);
        assert_eq!(resolve(&c, "T.Sig").unwrap().value(0xFFFF_FFFF), 4294967295.0);
        assert_eq!(resolve(&c, "T.Span").unwrap().value(0xFFFF_FFFF), -1.0);
    }

    #[test]
    fn search_filters_and_limits() {
        let c = contract();
        let (hits, total) = search(&c, "po", 10);
        assert_eq!((hits.len(), total), (1, 1));
        assert_eq!(hits[0].path, "T.Pos");
        let (hits, total) = search(&c, "", 2);
        assert_eq!((hits.len(), total), (2, 6));
    }

    #[test]
    fn config_pads_the_channel_array_and_clamps_timing() {
        let c = contract();
        let chans = vec![resolve(&c, "T.Pos").unwrap()];
        let cfg = config_json(1, 5, 0, 1, &chans);
        assert_eq!(cfg["ChanCount"], json!(1));
        assert_eq!(cfg["Divider"], json!(1));
        assert_eq!(cfg["FlushMs"], json!(FLUSH_MIN_MS));
        assert_eq!(cfg["Chan"].as_array().unwrap().len(), CHAN_MAX);
        assert_eq!(cfg["Chan"][1]["Area"], json!(0));
    }
}
