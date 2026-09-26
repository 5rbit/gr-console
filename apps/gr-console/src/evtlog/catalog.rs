//! Catalog + per-PLC renderers → API rows (`text` rendered at read time; raw fields kept).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use evt_catalog::{AlarmTable, Catalog, Ev, Renderer};
use serde::Serialize;
use serde_json::{Value as Json, json};
use time::{OffsetDateTime, PrimitiveDateTime, UtcOffset};

use super::store::Row;

/// Built-in copy for a packaged console without `plc/evtlog/catalog.toml` next to it.
const EMBEDDED: &str = include_str!("../../../../plc/evtlog/catalog.toml");

/// The built-in copy (demo defaults, tests).
pub fn embedded() -> Catalog {
    Catalog::parse(EMBEDDED).expect("built-in plc/evtlog/catalog.toml is valid (evt-catalog tests)")
}

/// `[evtlog] catalog` when it exists, else the built-in copy.
pub fn load_catalog(path: &Path) -> anyhow::Result<Arc<Catalog>> {
    let (text, from) = match std::fs::read_to_string(path) {
        Ok(t) => (t, path.display().to_string()),
        Err(_) => (EMBEDDED.to_string(), "built-in".to_string()),
    };
    let c = Catalog::parse(&text).map_err(|e| anyhow::anyhow!("{from}: {e}"))?;
    tracing::info!(from = %from, version = c.version, events = c.events.len(), "event catalog loaded");
    Ok(Arc::new(c))
}

/// API row.
#[derive(Clone, Debug, Serialize)]
pub struct EventRow {
    pub id: i64,
    pub plc: String,
    pub origin: &'static str,
    pub epoch: i64,
    pub seq: Option<i64>,
    pub ts: String,
    pub ts_ms: i64,
    pub rx_ts: String,
    pub cat: u8,
    pub cat_name: String,
    pub lvl: u8,
    pub lvl_name: String,
    pub src: u32,
    pub code: u32,
    pub name: Option<String>,
    pub a: i64,
    pub b: i64,
    pub ctx: u32,
    pub detail: Option<String>,
    pub text: String,
}

pub struct Texts {
    pub catalog: Arc<Catalog>,
    by_plc: HashMap<String, Renderer>,
    plain: Renderer,
    /// PLC DTL is local wall-clock time; the console's offset is taken once (the plant does not switch DST).
    pub offset: UtcOffset,
}

impl Texts {
    /// `plcs` = (config name, contract constants, alarm table).
    pub fn new(catalog: Arc<Catalog>, plcs: Vec<(String, &HashMap<String, i64>, AlarmTable)>) -> Texts {
        let by_plc = plcs.into_iter().map(|(n, consts, alarms)| (n, Renderer::new(catalog.clone(), consts, alarms))).collect();
        let plain = Renderer::new(catalog.clone(), &HashMap::new(), AlarmTable::default());
        Texts { catalog, by_plc, plain, offset: UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC) }
    }

    pub fn renderer(&self, plc: &str) -> &Renderer {
        self.by_plc.get(plc).or_else(|| self.by_plc.iter().find(|(k, _)| k.eq_ignore_ascii_case(plc)).map(|(_, v)| v)).unwrap_or(&self.plain)
    }

    pub fn text(&self, r: &Row) -> String {
        self.renderer(&r.plc).render(&Ev { plc: &r.plc, cat: r.cat, lvl: r.lvl, src: r.src, code: r.code, a: r.a, b: r.b, ctx: r.ctx, detail: r.detail.as_deref() })
    }

    pub fn row(&self, id: i64, r: &Row) -> EventRow {
        let c = &self.catalog;
        EventRow {
            id,
            plc: r.plc.clone(),
            origin: r.origin.as_str(),
            epoch: r.epoch,
            seq: r.seq,
            ts: self.fmt_ms(r.plc_ts),
            ts_ms: r.plc_ts,
            rx_ts: self.fmt_ms(r.rx_ts),
            cat: r.cat,
            cat_name: c.cat_name(r.cat).map(str::to_string).unwrap_or_else(|| format!("cat{}", r.cat)),
            lvl: r.lvl,
            lvl_name: c.level_name(r.lvl).map(str::to_string).unwrap_or_else(|| format!("L{}", r.lvl)),
            src: r.src,
            code: r.code,
            name: c.event(r.cat, r.code).map(|e| e.name.clone()),
            a: r.a,
            b: r.b,
            ctx: r.ctx,
            detail: r.detail.clone(),
            text: self.text(r),
        }
    }

    /// `YYYY-MM-DD HH:MM:SS.mmm` local.
    pub fn fmt_ms(&self, ms: i64) -> String {
        let Ok(t) = OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000) else { return String::new() };
        let t = t.to_offset(self.offset);
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}", t.year(), u8::from(t.month()), t.day(), t.hour(), t.minute(), t.second(), t.millisecond())
    }

    /// PLC DTL fields (local) → Unix ms.
    pub fn local_ms(&self, (y, mo, d, h, mi, s, ms): (i32, u8, u8, u8, u8, u8, u16)) -> Option<i64> {
        let month = time::Month::try_from(mo).ok()?;
        let date = time::Date::from_calendar_date(y, month, d).ok()?;
        let tm = time::Time::from_hms_milli(h, mi, s, ms).ok()?;
        let t = PrimitiveDateTime::new(date, tm).assume_offset(self.offset);
        Some((t.unix_timestamp_nanos() / 1_000_000) as i64)
    }

    /// `from` / `to` query values: Unix ms, or local `YYYY-MM-DD[T ]HH:MM[:SS[.mmm]]`, or RFC 3339.
    pub fn parse_time(&self, s: &str) -> Option<i64> {
        let s = s.trim();
        if let Ok(n) = s.parse::<i64>() {
            return Some(n);
        }
        if let Ok(t) = OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339) {
            return Some((t.unix_timestamp_nanos() / 1_000_000) as i64);
        }
        let b = s.as_bytes();
        let num = |r: std::ops::Range<usize>| s.get(r).and_then(|x| x.parse::<u32>().ok());
        if b.len() < 16 {
            return None;
        }
        let sec = if b.len() >= 19 { num(17..19)? } else { 0 };
        let ms = s.get(20..).map(|f| format!("{f:0<3}")).and_then(|f| f[..3].parse::<u16>().ok()).unwrap_or(0);
        self.local_ms((num(0..4)? as i32, num(5..7)? as u8, num(8..10)? as u8, num(11..13)? as u8, num(14..16)? as u8, sec as u8, ms))
    }

    /// `/api/events/catalog`: the catalog plus enums resolved against `plc`'s constants.
    pub fn catalog_json(&self, plc: Option<&str>) -> Json {
        let c = &self.catalog;
        let r = plc.map(|p| self.renderer(p)).or_else(|| self.by_plc.values().next()).unwrap_or(&self.plain);
        let enums: serde_json::Map<String, Json> =
            c.enums.keys().map(|name| (name.clone(), Json::Object(r.enum_values(name).map(|m| m.iter().map(|(k, v)| (k.to_string(), json!(v))).collect()).unwrap_or_default()))).collect();
        json!({
            "version": c.version, "capacity": c.capacity, "min_level": c.min_level, "max_per_scan": c.max_per_scan, "cat_mask_all": c.cat_mask_all(),
            "levels": c.levels, "cats": c.cats, "events": c.events, "enums": enums,
        })
    }
}

pub fn now_ms() -> i64 {
    (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evtlog::store::Origin;

    fn texts() -> Texts {
        let cat = Arc::new(Catalog::parse(EMBEDDED).unwrap());
        let consts: HashMap<String, i64> = [("GRIP_E_TIMEOUT".to_string(), 5)].into_iter().collect();
        let mut t = Texts::new(cat, vec![("GR2".into(), &consts, AlarmTable::default())]);
        t.offset = UtcOffset::from_hms(9, 0, 0).unwrap();
        t
    }

    #[test]
    fn rows_carry_raw_fields_and_rendered_text() {
        let t = texts();
        let ms = t.local_ms((2026, 9, 27, 10, 21, 3, 120)).unwrap();
        assert_eq!(t.fmt_ms(ms), "2026-09-27 10:21:03.120");
        assert_eq!(t.parse_time("2026-09-27T10:21:03.120"), Some(ms));
        assert_eq!(t.parse_time("2026-09-27 10:21"), Some(ms - 3120));
        assert_eq!(t.parse_time("2026-09-27T01:21:03.120Z"), Some(ms));
        assert_eq!(t.parse_time(&ms.to_string()), Some(ms));
        let r = Row { plc: "GR2".into(), epoch: 1, seq: Some(5), plc_ts: ms, rx_ts: ms + 40, cat: 7, lvl: 3, src: 5, code: 704, a: 5984, b: 38, ctx: 9, origin: Origin::Plc, detail: None };
        let row = t.row(11, &r);
        assert_eq!(row.text, "실패 TIMEOUT StopPos 598.4 mm · 38 %");
        assert_eq!((row.name.as_deref(), row.cat_name.as_str(), row.lvl_name.as_str(), row.rx_ts.as_str()), (Some("GRIP_ERROR"), "GRIP", "WARN", "2026-09-27 10:21:03.160"));
        // a PLC without constants still renders, enum values as numbers
        let mut other = r.clone();
        other.plc = "GR9".into();
        assert_eq!(t.row(12, &other).text, "실패 5 StopPos 598.4 mm · 38 %");
        let cj = t.catalog_json(Some("GR2"));
        assert_eq!(cj["enums"]["grip_err"]["5"], "TIMEOUT");
        assert_eq!(cj["enums"]["proc"]["20"], "Task");
    }
}
