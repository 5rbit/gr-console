//! PLC event log catalog (`plc/evtlog/catalog.toml`, the single source of truth).
//!
//! * [`Catalog`]    parsed and validated catalog (levels, categories, enums, PLC and console events)
//! * [`consts`]     the `EVT_*` constant list `gr-contract gen-evt` writes into the PLC tag table
//! * [`render`]     template → text for one stored event (enum names from the PLC contract constants, alarms)
//! * [`errorlist`]  ErrorList registry (`errorlist.json`) and the ErrorList row encoding (Alarm / Warn / Operator / Info)
//!
//! The PLC stores numbers only; text is produced when an event is shown, so editing a template re-renders stored rows.

pub mod consts;
pub mod errorlist;
pub mod render;

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

pub use consts::{ConstSpec, const_ident, plc_short};
pub use errorlist::{ElEntry, ErrorList, Formats, Level, Trans, Ty};
pub use render::{AlarmEntry, AlarmTable, Class, Ev, Renderer};

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("catalog toml: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("catalog: {0}")]
    Invalid(String),
}

fn invalid(msg: impl Into<String>) -> CatalogError {
    CatalogError::Invalid(msg.into())
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Named {
    pub id: u8,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct EnumDef {
    /// Every constant of the PLC contract whose name starts with this prefix (shown without it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_const: Option<String>,
    pub values: BTreeMap<i64, String>,
    pub plc_const: bool,
    /// PLCs that get the `plc_const` constants (short names); `None` = all. Rendering is not filtered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plc: Option<Vec<String>>,
}

impl EnumDef {
    pub fn consts_for(&self, plc_short: &str) -> bool {
        self.plc_const && self.plc.as_ref().is_none_or(|l| l.iter().any(|p| p.eq_ignore_ascii_case(plc_short)))
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct EventDef {
    /// `None` = `code = "*"` (STEP: the code is the new step number).
    pub code: Option<u16>,
    pub name: String,
    pub cat: u8,
    pub cat_name: String,
    pub lvl: u8,
    /// PLCs that emit it (short names `GR1`/`GR2`/`GRM`); `None` = all.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plc: Option<Vec<String>>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b: Option<String>,
    /// `[[console]]`: produced by gr-console, never by a PLC.
    pub console: bool,
}

impl EventDef {
    pub fn emitted_by(&self, plc_short: &str) -> bool {
        !self.console && self.plc.as_ref().is_none_or(|l| l.iter().any(|p| p.eq_ignore_ascii_case(plc_short)))
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Catalog {
    pub version: u32,
    pub const_prefix: String,
    pub db: String,
    pub db_number: u16,
    pub entry_udt: String,
    pub capacity: u32,
    /// Default `Cfg.MinLevel` (level id).
    pub min_level: u8,
    pub max_per_scan: i32,
    pub levels: Vec<Named>,
    pub cats: Vec<Named>,
    pub enums: BTreeMap<String, EnumDef>,
    pub events: Vec<EventDef>,
}

#[derive(Deserialize)]
struct RawCatalog {
    version: u32,
    const_prefix: String,
    db: String,
    db_number: u16,
    entry_udt: String,
    capacity: u32,
    min_level: String,
    max_per_scan: i32,
    #[serde(default)]
    level: Vec<RawNamed>,
    #[serde(default)]
    cat: Vec<RawNamed>,
    #[serde(default, rename = "enum")]
    enums: BTreeMap<String, RawEnum>,
    #[serde(default)]
    event: Vec<RawEvent>,
    #[serde(default)]
    console: Vec<RawEvent>,
}

#[derive(Deserialize)]
struct RawNamed {
    id: u8,
    name: String,
}

#[derive(Deserialize)]
struct RawEnum {
    from_const: Option<String>,
    #[serde(default)]
    values: BTreeMap<String, String>,
    #[serde(default)]
    plc_const: bool,
    plc: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct RawEvent {
    code: toml::Value,
    name: String,
    cat: String,
    lvl: String,
    plc: Option<Vec<String>>,
    text: String,
    a: Option<toml::Value>,
    b: Option<toml::Value>,
}

/// `a = "text"` or `a = { name = "Dwell", unit = "ms" }` → one description line.
fn arg_doc(v: Option<toml::Value>) -> Option<String> {
    match v? {
        toml::Value::String(s) => Some(s),
        toml::Value::Table(t) => {
            let name = t.get("name").and_then(|x| x.as_str()).unwrap_or_default();
            let unit = t.get("unit").and_then(|x| x.as_str()).map(|u| format!(" ({u})")).unwrap_or_default();
            Some(format!("{name}{unit}"))
        }
        other => Some(other.to_string()),
    }
}

impl Catalog {
    pub fn parse(text: &str) -> Result<Catalog, CatalogError> {
        let raw: RawCatalog = toml::from_str(text)?;
        let levels: Vec<Named> = raw.level.into_iter().map(|n| Named { id: n.id, name: n.name }).collect();
        let cats: Vec<Named> = raw.cat.into_iter().map(|n| Named { id: n.id, name: n.name }).collect();
        for (what, list) in [("level", &levels), ("cat", &cats)] {
            let mut ids = HashSet::new();
            let mut names = HashSet::new();
            for n in list {
                if !ids.insert(n.id) || !names.insert(n.name.as_str()) {
                    return Err(invalid(format!("duplicate {what} {} / {}", n.id, n.name)));
                }
            }
        }
        if cats.iter().any(|c| c.id == 0 || c.id > 31) {
            return Err(invalid("cat ids must be 1..31 (CatMask bit)"));
        }
        let level_id = |s: &str| levels.iter().find(|l| l.name.eq_ignore_ascii_case(s)).map(|l| l.id).ok_or_else(|| invalid(format!("unknown level {s}")));
        let min_level = level_id(&raw.min_level)?;
        let mut enums = BTreeMap::new();
        for (name, e) in raw.enums {
            let mut values = BTreeMap::new();
            for (k, v) in e.values {
                let n: i64 = k.trim().parse().map_err(|_| invalid(format!("enum {name}: key {k} is not a number")))?;
                values.insert(n, v);
            }
            if e.from_const.is_some() && !values.is_empty() {
                return Err(invalid(format!("enum {name}: both from_const and values")));
            }
            if e.plc_const && e.from_const.is_some() {
                return Err(invalid(format!("enum {name}: plc_const needs inline values")));
            }
            enums.insert(name, EnumDef { from_const: e.from_const, values, plc_const: e.plc_const, plc: e.plc });
        }
        let mut events = Vec::new();
        let mut codes = HashSet::new();
        let mut names = HashSet::new();
        for (console, list) in [(false, raw.event), (true, raw.console)] {
            for e in list {
                let code = match &e.code {
                    toml::Value::String(s) if s == "*" => None,
                    toml::Value::Integer(n) => Some(u16::try_from(*n).map_err(|_| invalid(format!("{}: code {n} out of UInt range", e.name)))?),
                    other => return Err(invalid(format!("{}: code must be an integer or \"*\", got {other}", e.name))),
                };
                let cat = cats.iter().find(|c| c.name == e.cat).ok_or_else(|| invalid(format!("{}: unknown cat {}", e.name, e.cat)))?;
                if code.is_none() && cat.name != "STEP" {
                    return Err(invalid(format!("{}: code \"*\" is only allowed for STEP", e.name)));
                }
                if let Some(c) = code
                    && !codes.insert(c)
                {
                    return Err(invalid(format!("{}: duplicate code {c}", e.name)));
                }
                if !names.insert(e.name.clone()) {
                    return Err(invalid(format!("duplicate event name {}", e.name)));
                }
                let lvl = level_id(&e.lvl)?;
                let ev = EventDef { code, name: e.name, cat: cat.id, cat_name: cat.name.clone(), lvl, plc: e.plc, text: e.text, a: arg_doc(e.a), b: arg_doc(e.b), console };
                for enum_name in render::template_enums(&ev.text) {
                    if !enums.contains_key(&enum_name) {
                        return Err(invalid(format!("{}: template uses unknown enum {enum_name}", ev.name)));
                    }
                }
                events.push(ev);
            }
        }
        Ok(Catalog {
            version: raw.version,
            const_prefix: raw.const_prefix,
            db: raw.db,
            db_number: raw.db_number,
            entry_udt: raw.entry_udt,
            capacity: raw.capacity,
            min_level,
            max_per_scan: raw.max_per_scan,
            levels,
            cats,
            enums,
            events,
        })
    }

    pub fn load(path: &std::path::Path) -> Result<Catalog, CatalogError> {
        let text = std::fs::read_to_string(path).map_err(|e| invalid(format!("{}: {e}", path.display())))?;
        Catalog::parse(&text)
    }

    pub fn cat_name(&self, id: u8) -> Option<&str> {
        self.cats.iter().find(|c| c.id == id).map(|c| c.name.as_str())
    }
    pub fn cat_id(&self, name: &str) -> Option<u8> {
        self.cats.iter().find(|c| c.name.eq_ignore_ascii_case(name)).map(|c| c.id)
    }
    pub fn level_name(&self, id: u8) -> Option<&str> {
        self.levels.iter().find(|c| c.id == id).map(|c| c.name.as_str())
    }
    pub fn level_id(&self, name: &str) -> Option<u8> {
        self.levels.iter().find(|c| c.name.eq_ignore_ascii_case(name)).map(|c| c.id)
    }
    /// The definition of a stored (cat, code): by code, or the `"*"` entry of the category (STEP).
    pub fn event(&self, cat: u8, code: u32) -> Option<&EventDef> {
        self.events.iter().find(|e| e.cat == cat && e.code.is_some_and(|c| u32::from(c) == code)).or_else(|| self.events.iter().find(|e| e.cat == cat && e.code.is_none()))
    }
    pub fn event_by_name(&self, name: &str) -> Option<&EventDef> {
        self.events.iter().find(|e| e.name.eq_ignore_ascii_case(name))
    }
    /// Every category bit (`CatMask` with all categories enabled).
    pub fn cat_mask_all(&self) -> u32 {
        self.cats.iter().fold(0u32, |m, c| m | (1u32 << c.id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn repo_catalog() -> Catalog {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plc/evtlog/catalog.toml");
        Catalog::load(&p).expect("plc/evtlog/catalog.toml parses")
    }

    #[test]
    fn repo_catalog_is_valid() {
        let c = repo_catalog();
        assert_eq!(c.capacity, 1000);
        assert_eq!(c.cat_mask_all(), 0x000F_FFFE);
        assert_eq!((c.cat_id("ALARM"), c.cat_id("OPERATOR"), c.cat_id("INFO")), (Some(errorlist::CAT_ALARM), Some(errorlist::CAT_OPERATOR), Some(errorlist::CAT_INFO)));
        assert_eq!(c.enums.get("trans").map(|e| e.values.len()), Some(5));
        assert_eq!(c.event(3, 300).map(|e| e.name.as_str()), Some("STEP_CHANGED"));
        assert_eq!(c.event(7, 704).map(|e| e.name.as_str()), Some("GRIP_ERROR"));
        assert!(c.event(7, 9999).is_none());
        assert!(c.events.iter().any(|e| e.console && e.name == "CON_EVT_GAP"));
    }

    #[test]
    fn rejects_mistakes() {
        let base = "version = 1\nconst_prefix = \"EVT_\"\ndb = \"EVTLOG\"\ndb_number = 950\nentry_udt = \"EVT_Entry\"\ncapacity = 10\nmin_level = \"INFO\"\nmax_per_scan = 4\n[[level]]\nid = 2\nname = \"INFO\"\n[[cat]]\nid = 1\nname = \"SYS\"\n";
        let ev = |code: &str, text: &str| format!("{base}[[event]]\ncode = {code}\nname = \"X\"\ncat = \"SYS\"\nlvl = \"INFO\"\ntext = \"{text}\"\n");
        assert!(Catalog::parse(&ev("1", "ok {a}")).is_ok());
        assert!(Catalog::parse(&ev("\"*\"", "x")).is_err(), "* outside STEP");
        assert!(Catalog::parse(&ev("1", "{a:nope}")).is_err(), "unknown enum");
        assert!(Catalog::parse(&format!("{}[[event]]\ncode = 1\nname = \"Y\"\ncat = \"SYS\"\nlvl = \"INFO\"\ntext = \"\"\n", ev("1", ""))).is_err(), "duplicate code");
    }
}
