//! Framing detection from the first bytes of a connection.

use crate::framing::Framing;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sniff {
    Found(Framing),
    /// Not enough bytes to decide yet.
    NeedMore,
    /// Does not start like any framing.
    Unknown,
}

const PATTERNS: &[(&[u8], Framing)] = &[(&[0x47, 0x53], Framing::Frame), (b"GET ", Framing::Http), (b"POST", Framing::Http), (b"HTTP/", Framing::Http)];

/// `47 53` → FRAME, `{` → NDJSON, `GET ` / `POST` / `HTTP/` → HTTP.
pub fn sniff_detail(buf: &[u8]) -> Sniff {
    if buf.is_empty() {
        return Sniff::NeedMore;
    }
    if buf[0] == b'{' {
        return Sniff::Found(Framing::Ndjson);
    }
    let mut need = false;
    for (pat, f) in PATTERNS {
        if buf.starts_with(pat) {
            return Sniff::Found(*f);
        }
        if buf.len() < pat.len() && pat.starts_with(buf) {
            need = true;
        }
    }
    if need { Sniff::NeedMore } else { Sniff::Unknown }
}

/// The framing, or `None` when undecided or unknown (see [`sniff_detail`]).
pub fn sniff(buf: &[u8]) -> Option<Framing> {
    match sniff_detail(buf) {
        Sniff::Found(f) => Some(f),
        _ => None,
    }
}
