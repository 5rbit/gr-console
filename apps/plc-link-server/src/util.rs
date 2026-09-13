//! Time stamps, hex, sequence numbers.

use std::sync::OnceLock;

use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

static OFFSET: OnceLock<UtcOffset> = OnceLock::new();

/// Determines the local UTC offset. Call before starting threads (the lookup can fail on some platforms
/// once several threads run); later calls reuse the first result.
pub fn init_local_offset() -> UtcOffset {
    *OFFSET.get_or_init(|| UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC))
}

pub fn now_local() -> OffsetDateTime {
    OffsetDateTime::now_utc().to_offset(init_local_offset())
}

/// RFC 3339 local time with millisecond precision.
pub fn ts(t: OffsetDateTime) -> String {
    let t = t.replace_nanosecond(t.millisecond() as u32 * 1_000_000).unwrap_or(t);
    t.format(&Rfc3339).unwrap_or_default()
}

pub fn now_ts() -> String {
    ts(now_local())
}

/// Wire DTL text of the current local time (`YYYY-MM-DDTHH:MM:SS.mmm`).
pub fn dtl_now_text() -> String {
    let t = now_local();
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}", t.year(), t.month() as u8, t.day(), t.hour(), t.minute(), t.second(), t.millisecond())
}

/// `YYYYMMDD` of the local date.
pub fn date_stamp() -> String {
    let t = now_local();
    format!("{:04}{:02}{:02}", t.year(), t.month() as u8, t.day())
}

/// Upper-case hex without separators.
pub fn hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789ABCDEF";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(H[(b >> 4) as usize] as char);
        s.push(H[(b & 15) as usize] as char);
    }
    s
}

/// Hex text → bytes. Whitespace, `:`/`-` separators and a `0x` prefix are ignored.
pub fn parse_hex(text: &str) -> Result<Vec<u8>, String> {
    let t = text.trim();
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    let digits: Vec<u8> = t.bytes().filter(|b| !b.is_ascii_whitespace() && *b != b':' && *b != b'-').collect();
    if !digits.len().is_multiple_of(2) {
        return Err(format!("odd number of hex digits ({})", digits.len()));
    }
    digits
        .chunks(2)
        .map(|p| {
            let s = std::str::from_utf8(p).map_err(|_| "invalid hex".to_string())?;
            u8::from_str_radix(s, 16).map_err(|_| format!("invalid hex digits {s:?}"))
        })
        .collect()
}

/// Per-sender sequence numbers: 1, 2, … 65535, 1.
#[derive(Clone, Copy, Debug, Default)]
pub struct SeqGen(u16);

impl SeqGen {
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u16 {
        self.0 = plc_link::codec::next_seq(self.0);
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        assert_eq!(hex(&[0, 0x1f, 0xab]), "001FAB");
        assert_eq!(parse_hex("0x00 1f:AB").unwrap(), vec![0, 0x1f, 0xab]);
        assert!(parse_hex("abc").is_err());
        assert!(parse_hex("zz").is_err());
    }

    #[test]
    fn seq_wraps_to_one() {
        let mut s = SeqGen(u16::MAX - 1);
        assert_eq!((s.next(), s.next(), s.next()), (u16::MAX, 1, 2));
    }

    #[test]
    fn dtl_text_shape() {
        let t = dtl_now_text();
        assert_eq!(t.len(), 23);
        assert_eq!(&t[10..11], "T");
    }
}
