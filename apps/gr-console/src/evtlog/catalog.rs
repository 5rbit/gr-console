//! Catalog + per-PLC renderers → API rows (`text` rendered at read time; raw fields kept).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use evt_catalog::errorlist::{self, CAT_ALARM, CAT_INFO, CAT_OPERATOR, TRANS_MUL};
use evt_catalog::{AlarmTable, Catalog, Class, ErrorList, Ev, Formats, Level, Renderer, Ty};
use rusqlite::types::Value;
use serde::Serialize;
use serde_json::{Value as Json, json};
use time::{OffsetDateTime, PrimitiveDateTime, UtcOffset};

use super::store::Row;

/// Built-in copy for a packaged console without `plc/evtlog/catalog.toml` next to it.
const EMBEDDED: &str = include_str!("../../../../plc/evtlog/catalog.toml");
const EMBEDDED_FORMATS: &str = include_str!("../../../../plc/evtlog/errorlist-format.toml");

/// `errorlist-format.toml` next to the catalog, else the built-in copy. Returns the text (parsed per PLC).
pub fn load_formats(catalog_path: &Path) -> String {
    let path = catalog_path.with_file_name("errorlist-format.toml");
    match std::fs::read_to_string(&path) {
        Ok(t) if Formats::parse(&t, false).is_ok() => t,
        Ok(_) => {
            tracing::warn!(path = %path.display(), "errorlist-format.toml does not parse — built-in copy used");
            EMBEDDED_FORMATS.to_string()
        }
        Err(_) => EMBEDDED_FORMATS.to_string(),
    }
}

/// `plc/contract/<PLC>/errorlist.json` (missing → empty: ErrorList rows then render as their code only).
pub fn load_errorlist(path: &Path) -> ErrorList {
    match std::fs::read_to_string(path) {
        Ok(t) => ErrorList::parse(&t).unwrap_or_else(|e| {
            tracing::warn!(path = %path.display(), "errorlist.json: {e}");
            ErrorList::default()
        }),
        Err(_) => ErrorList::default(),
    }
}

/// One PLC's rendering inputs.
pub struct PlcTexts<'a> {
    /// Config name (`GR2`).
    pub name: String,
    pub consts: &'a HashMap<String, i64>,
    pub alarms: AlarmTable,
    pub errorlist: ErrorList,
    /// GRM reads the `[grm]` format table.
    pub grm: bool,
}

impl<'a> PlcTexts<'a> {
    #[cfg(test)]
    pub fn new(name: &str, consts: &'a HashMap<String, i64>, alarms: AlarmTable) -> PlcTexts<'a> {
        PlcTexts { name: name.to_string(), consts, alarms, errorlist: ErrorList::default(), grm: name.to_ascii_uppercase().contains("GRM") }
    }
}

/// SQL fragment + its arguments.
pub type Cond = (String, Vec<Value>);

/// `code` ≥ 10000 and a known transition — an ErrorList row of the category.
fn el_rows(cat: u8) -> String {
    format!("(cat = {cat} AND code >= {TRANS_MUL} AND code < {})", 5 * TRANS_MUL)
}

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
    /// ErrorList type (`Alarm` / `Warn` / `Operator` / `Info` / `Task`), `None` for rows outside the ErrorList.
    pub etype: Option<&'static str>,
    /// ErrorList code (`F3119`) when the row names exactly one.
    pub ecode: Option<String>,
    /// `raise` / `clear` / `momentary` / `summary` (ErrorList rows and bit alarm rows).
    pub trans: Option<&'static str>,
    /// English text when it differs from `text` (ErrorList / alarm texts).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_en: Option<String>,
}

pub struct Texts {
    pub catalog: Arc<Catalog>,
    by_plc: HashMap<String, Renderer>,
    plain: Renderer,
    /// PLC DTL is local wall-clock time; the console's offset is taken once (the plant does not switch DST).
    pub offset: UtcOffset,
}

fn ev(r: &Row) -> Ev<'_> {
    Ev { plc: &r.plc, cat: r.cat, lvl: r.lvl, src: r.src, code: r.code, a: r.a, b: r.b, ctx: r.ctx, detail: r.detail.as_deref() }
}

impl Texts {
    /// `formats` = the text of `errorlist-format.toml` ([`load_formats`]).
    pub fn new(catalog: Arc<Catalog>, plcs: Vec<PlcTexts>, formats: &str) -> Texts {
        let fmt = |grm: bool| {
            Arc::new(Formats::parse(formats, grm).unwrap_or_else(|e| {
                tracing::warn!("errorlist-format.toml: {e}");
                Formats::default()
            }))
        };
        let (gr, grm) = (fmt(false), fmt(true));
        for (code, spec) in gr.specs().chain(grm.specs()) {
            let name = spec.split_once(':').filter(|(k, _)| *k == "enum" || *k == "bits").map(|(_, n)| n.trim());
            if errorlist::parse_spec(spec).is_none() || name.is_some_and(|n| !catalog.enums.contains_key(n)) {
                tracing::warn!(code, spec, "errorlist-format.toml: unknown spec or enum (value printed raw)");
            }
        }
        let by_plc = plcs
            .into_iter()
            .map(|p| {
                let f = if p.grm { grm.clone() } else { gr.clone() };
                (p.name, Renderer::new(catalog.clone(), p.consts, p.alarms).with_errorlist(p.errorlist, f))
            })
            .collect();
        let plain = Renderer::new(catalog.clone(), &HashMap::new(), AlarmTable::default());
        Texts { catalog, by_plc, plain, offset: UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC) }
    }

    pub fn renderer(&self, plc: &str) -> &Renderer {
        self.by_plc.get(plc).or_else(|| self.by_plc.iter().find(|(k, _)| k.eq_ignore_ascii_case(plc)).map(|(_, v)| v)).unwrap_or(&self.plain)
    }

    #[cfg(test)]
    pub fn text(&self, r: &Row) -> String {
        self.renderer(&r.plc).render(&ev(r))
    }

    pub fn classify(&self, r: &Row) -> Option<Class> {
        self.renderer(&r.plc).classify(&ev(r))
    }

    /// Rows of these types — the SQL form of [`Texts::classify`] (`None` = no condition).
    pub fn type_cond(&self, types: &[Ty]) -> Option<Cond> {
        if types.is_empty() {
            return None;
        }
        let bit_alarm = |src: u32| format!("(cat = {CAT_ALARM} AND src = {src} AND code IN ({}, {}))", errorlist::ALM_RAISED, errorlist::ALM_CLEARED);
        let mut args = Vec::new();
        let mut plcs: Vec<(&String, &Renderer)> = self.by_plc.iter().collect();
        plcs.sort_by_key(|(n, _)| n.as_str());
        let el_task = {
            let parts: Vec<String> = plcs
                .iter()
                .filter_map(|(n, r)| {
                    let nums = r.errorlist().task_nums();
                    (!nums.is_empty()).then(|| {
                        args.push(Value::Text(n.to_string()));
                        format!("(plc = ? AND code % {TRANS_MUL} IN ({}))", nums.iter().map(u32::to_string).collect::<Vec<_>>().join(","))
                    })
                })
                .collect();
            if parts.is_empty() { "0".to_string() } else { format!("({} AND ({}))", el_rows(CAT_INFO), parts.join(" OR ")) }
        };
        // the task list's arguments are used twice (Info = NOT task, Task = task)
        let task_args = args.clone();
        args.clear();
        let mut parts: Vec<String> = Vec::new();
        for ty in types {
            match ty {
                Ty::Alarm => parts.push(format!("({} AND src = 1) OR {}", el_rows(CAT_ALARM), bit_alarm(1))),
                Ty::Warn => parts.push(format!("({} AND src = 2) OR {}", el_rows(CAT_ALARM), bit_alarm(2))),
                Ty::Operator => parts.push(format!("{} OR {}", el_rows(CAT_OPERATOR), bit_alarm(3))),
                Ty::Info => {
                    parts.push(format!("({} AND NOT {el_task})", el_rows(CAT_INFO)));
                    args.extend(task_args.iter().cloned());
                }
                Ty::Task => {
                    parts.push(format!("{el_task} OR {}", bit_alarm(4)));
                    args.extend(task_args.iter().cloned());
                }
            }
            // catalog rows the ErrorList names
            for (name, r) in &plcs {
                let refs: Vec<String> = r.errorlist().catalog_refs().into_iter().filter(|c| c.ty == *ty).filter_map(|c| self.catalog_sql(&c.event, c.src)).collect();
                if !refs.is_empty() {
                    parts.push(format!("(plc = ? AND ({}))", refs.join(" OR ")));
                    args.push(Value::Text(name.to_string()));
                }
            }
        }
        Some((format!("({})", parts.join(" OR ")), args))
    }

    /// `(cat = 5 AND code = 513 AND src = 1)` for a catalog event name (+ Src).
    fn catalog_sql(&self, event: &str, src: Option<u32>) -> Option<String> {
        let e = self.catalog.event_by_name(event)?;
        let mut s = format!("cat = {}", e.cat);
        if let Some(c) = e.code {
            s.push_str(&format!(" AND code = {c}"));
        }
        if let Some(src) = src {
            s.push_str(&format!(" AND src = {src}"));
        }
        Some(format!("({s})"))
    }

    /// Rows of these ErrorList codes: ErrorList rows of any transition, bit alarm rows at the code's bit (alarms.json),
    /// catalog rows the ErrorList maps to exactly that code.
    pub fn code_cond(&self, codes: &[(Level, u32)]) -> Option<Cond> {
        if codes.is_empty() {
            return None;
        }
        let mut parts = Vec::new();
        let mut args = Vec::new();
        for (level, num) in codes {
            let all: Vec<String> = (1..=4).map(|t| (t * TRANS_MUL + num).to_string()).collect();
            let src = matches!(level, Level::Alarm | Level::Warn).then(|| format!(" AND src = {}", level.area_id())).unwrap_or_default();
            parts.push(format!("(cat = {} AND code IN ({}){src})", level.cat(), all.join(",")));
            let label = errorlist::label(*level, *num);
            let mut plcs: Vec<(&String, &Renderer)> = self.by_plc.iter().collect();
            plcs.sort_by_key(|(n, _)| n.as_str());
            for (name, r) in plcs {
                if matches!(level, Level::Alarm | Level::Warn) {
                    let area = level.area();
                    let bits: Vec<String> = r.alarm_bits(area, *num).iter().map(u32::to_string).collect();
                    if !bits.is_empty() {
                        parts.push(format!(
                            "(plc = ? AND cat = {CAT_ALARM} AND src = {} AND code IN ({}, {}) AND a IN ({}))",
                            level.area_id(),
                            errorlist::ALM_RAISED,
                            errorlist::ALM_CLEARED,
                            bits.join(",")
                        ));
                        args.push(Value::Text(name.clone()));
                    }
                }
                let refs: Vec<String> = r.errorlist().catalog_refs().into_iter().filter(|c| c.code.as_deref() == Some(label.as_str())).filter_map(|c| self.catalog_sql(&c.event, c.src)).collect();
                if !refs.is_empty() {
                    parts.push(format!("(plc = ? AND ({}))", refs.join(" OR ")));
                    args.push(Value::Text(name.clone()));
                }
            }
        }
        Some((format!("({})", parts.join(" OR ")), args))
    }

    pub fn row(&self, id: i64, r: &Row) -> EventRow {
        let c = &self.catalog;
        let rd = self.renderer(&r.plc);
        let e = ev(r);
        let text = rd.render_lang(&e, false);
        let en = rd.render_lang(&e, true);
        let class = rd.classify(&e);
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
            etype: class.as_ref().map(|c| c.ty.name()),
            ecode: class.as_ref().and_then(|c| c.code.clone()),
            trans: class.as_ref().and_then(|c| c.trans).map(|t| t.name()),
            text_en: (en != text).then_some(en),
            text,
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
        let mut t = Texts::new(cat, vec![PlcTexts::new("GR2", &consts, AlarmTable::default())], "");
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
