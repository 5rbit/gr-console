use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// Local wall-clock time as RFC 3339 with offset (falls back to UTC when the offset is unavailable).
pub fn now_str() -> String {
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    now.format(&Rfc3339).unwrap_or_default()
}
