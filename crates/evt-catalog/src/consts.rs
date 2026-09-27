//! `EVT_*` PLC constants (`gr-contract gen-evt` → `EVT_Const_Gen` tag table). The PLC sources use these names
//! verbatim, so the naming rule here is a contract:
//!
//! * `EVT_CAPACITY` / `EVT_LAST` (Int), `EVT_MIN_LEVEL_DEFAULT` (USInt), `EVT_MAX_PER_SCAN_DEFAULT` (Int),
//!   `EVT_CATMASK_ALL` (DWord)
//! * `EVT_LVL_<LEVEL>`, `EVT_CAT_<CAT>` (USInt)
//! * `EVT_<EVENT NAME>` (UInt, the code) for events the PLC emits; `code = "*"` and console events are skipped
//! * `EVT_<ENUM>_<VALUE NAME>` (UInt) for enums with `plc_const = true` (and the PLC in the enum `plc` list, if any); see
//!   [`const_ident`]

use std::collections::HashSet;

use crate::{Catalog, CatalogError, invalid};

pub const MAX_NAME_LEN: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstSpec {
    pub name: String,
    pub data_type: &'static str,
    pub value: String,
    pub comment_en: String,
    pub comment_ko: String,
}

/// `Drive.MCS_TO_ACS[Z,0]` → `DRIVE_MCS_TO_ACS_Z_0`: upper case, every non-alphanumeric → `_`, runs collapsed,
/// leading/trailing `_` trimmed.
pub fn const_ident(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_uppercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

/// `GR2_PLC` / `gr2` → `GR2`.
pub fn plc_short(plc: &str) -> String {
    let u = plc.to_ascii_uppercase();
    u.strip_suffix("_PLC").map(str::to_string).unwrap_or(u)
}

fn valid_ident(s: &str) -> bool {
    let mut it = s.chars();
    it.next().is_some_and(|c| c.is_ascii_alphabetic()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !s.contains("__") && !s.ends_with('_')
}

fn short(text: &str, max: usize) -> String {
    let t = text.trim();
    if t.chars().count() <= max { t.to_string() } else { format!("{}…", t.chars().take(max - 1).collect::<String>()) }
}

impl Catalog {
    /// Constant list for one PLC (`plc` = `GR2_PLC`, `GR2`, ...), in table order. Fails on an invalid or duplicate name.
    pub fn constants(&self, plc: &str) -> Result<Vec<ConstSpec>, CatalogError> {
        let p = &self.const_prefix;
        let who = plc_short(plc);
        let mut out = Vec::new();
        let mut push = |name: String, data_type: &'static str, value: String, en: String, ko: String| out.push(ConstSpec { name, data_type, value, comment_en: en, comment_ko: ko });
        push(format!("{p}CAPACITY"), "Int", self.capacity.to_string(), format!("{} entries", self.db), format!("{} 항목 수", self.db));
        push(format!("{p}LAST"), "Int", (self.capacity as i64 - 1).to_string(), format!("{}.Entry upper bound", self.db), format!("{}.Entry 상한", self.db));
        push(format!("{p}MIN_LEVEL_DEFAULT"), "USInt", self.min_level.to_string(), "Default Cfg.MinLevel".into(), "Cfg.MinLevel 기본값".into());
        push(format!("{p}MAX_PER_SCAN_DEFAULT"), "Int", self.max_per_scan.to_string(), "Default Cfg.MaxPerScan".into(), "Cfg.MaxPerScan 기본값".into());
        push(format!("{p}CATMASK_ALL"), "DWord", format!("16#{:08X}", self.cat_mask_all()), "Cfg.CatMask, every category".into(), "Cfg.CatMask 전 분류".into());
        for l in &self.levels {
            push(format!("{p}LVL_{}", const_ident(&l.name)), "USInt", l.id.to_string(), format!("Level {}", l.name), format!("레벨 {}", l.name));
        }
        for c in &self.cats {
            push(format!("{p}CAT_{}", const_ident(&c.name)), "USInt", c.id.to_string(), format!("Category {} (CatMask bit {})", c.name, c.id), format!("분류 {}", c.name));
        }
        let mut events: Vec<_> = self.events.iter().filter(|e| e.emitted_by(&who)).filter_map(|e| e.code.map(|c| (c, e))).collect();
        events.sort_by_key(|(c, _)| *c);
        for (code, e) in events {
            let lvl = self.level_name(e.lvl).unwrap_or("?");
            push(format!("{p}{}", const_ident(&e.name)), "UInt", code.to_string(), format!("{} {lvl}", e.cat_name), short(&e.text, 80));
        }
        for (name, en) in self.enums.iter().filter(|(_, e)| e.consts_for(&who)) {
            for (v, label) in &en.values {
                if *v < 0 || *v > i64::from(u16::MAX) {
                    return Err(invalid(format!("enum {name}: value {v} does not fit UInt")));
                }
                push(format!("{p}{}_{}", const_ident(name), const_ident(label)), "UInt", v.to_string(), format!("{name} = {label}"), format!("{name} = {label}"));
            }
        }
        let mut seen = HashSet::new();
        for k in &out {
            if k.name.len() > MAX_NAME_LEN {
                return Err(invalid(format!("constant name longer than {MAX_NAME_LEN}: {}", k.name)));
            }
            if !valid_ident(&k.name) {
                return Err(invalid(format!("not a valid SCL identifier: {}", k.name)));
            }
            if !seen.insert(k.name.to_ascii_uppercase()) {
                return Err(invalid(format!("duplicate constant {}", k.name)));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ident_rule() {
        assert_eq!(const_ident("Drive.MCS_TO_ACS[Z,0]"), "DRIVE_MCS_TO_ACS_Z_0");
        assert_eq!(const_ident("AUTO forced (W6024)"), "AUTO_FORCED_W6024");
        assert_eq!(const_ident("File missing -> defaults"), "FILE_MISSING_DEFAULTS");
        assert_eq!(const_ident("Drive.HomePos[X]"), "DRIVE_HOMEPOS_X");
        assert_eq!(const_ident("SSW_Auto off"), "SSW_AUTO_OFF");
        assert_eq!(plc_short("GR2_PLC"), "GR2");
        assert_eq!(plc_short("grm"), "GRM");
    }

    #[test]
    fn gr2_constants() {
        let c = crate::tests::repo_catalog();
        let k = c.constants("GR2_PLC").unwrap();
        let get = |n: &str| k.iter().find(|x| x.name == n).map(|x| (x.data_type, x.value.as_str()));
        assert_eq!(get("EVT_CAPACITY"), Some(("Int", "1000")));
        assert_eq!(get("EVT_LAST"), Some(("Int", "999")));
        assert_eq!(get("EVT_CATMASK_ALL"), Some(("DWord", "16#0003FFFE")));
        assert_eq!(get("EVT_MIN_LEVEL_DEFAULT"), Some(("USInt", "2")));
        assert_eq!(get("EVT_MAX_PER_SCAN_DEFAULT"), Some(("Int", "32")));
        assert_eq!(get("EVT_LVL_WARN"), Some(("USInt", "3")));
        assert_eq!(get("EVT_CAT_ANTICOL"), Some(("USInt", "13")));
        assert_eq!(get("EVT_GRIP_REQ"), Some(("UInt", "701")));
        assert_eq!(get("EVT_TASK_STEP_TIMEOUT"), Some(("UInt", "417")));
        assert_eq!(get("EVT_PENDING_END_AUTO_FORCED_W6024"), Some(("UInt", "2")));
        assert_eq!(get("EVT_PARA_DRIVE_MCS_TO_ACS_Z_0"), Some(("UInt", "41")));
        assert_eq!(get("EVT_ALARM_AREA_FAULT"), Some(("UInt", "1")));
        assert!(get("EVT_STEP_CHANGED").is_none(), "code \"*\" has no constant");
        assert!(get("EVT_CON_EVT_GAP").is_none(), "console events stay in the console");
        // GR1 does not emit GR2-only events, GRM none of the robot ones
        let gr1 = c.constants("GR1_PLC").unwrap();
        assert!(!gr1.iter().any(|x| x.name == "EVT_GRIP_REQ"));
        assert!(gr1.iter().any(|x| x.name == "EVT_GRIP_RETRY"));
        let grm = c.constants("GRM").unwrap();
        assert!(grm.iter().any(|x| x.name == "EVT_MODE_CHANGED"));
        assert!(!grm.iter().any(|x| x.name == "EVT_TASK_ACCEPTED"));
        // enum `plc` limits the value constants, not the events
        assert!(grm.iter().any(|x| x.name == "EVT_ST_MEAS_DONE" && x.value == "2"));
        assert!(grm.iter().any(|x| x.name == "EVT_COMM_BLOCK_GET" && x.value == "3"));
        assert!(grm.iter().any(|x| x.name == "EVT_CMD_GCS_NEW" && x.value == "520"));
        assert!(!k.iter().any(|x| x.name.starts_with("EVT_ST_MEAS") || x.name.starts_with("EVT_COMM_BLOCK_") || x.name == "EVT_CMD_GCS_NEW"));
        assert!(!gr1.iter().any(|x| x.name.starts_with("EVT_TRACK_CAUSE_")));
    }

    #[test]
    fn duplicate_names_fail() {
        let mut c = crate::tests::repo_catalog();
        let first = c.events.iter().find(|e| e.code.is_some() && !e.console).unwrap().clone();
        let mut dup = first.clone();
        dup.code = Some(9999);
        c.events.push(dup);
        let err = c.constants("GR2").unwrap_err().to_string();
        assert!(err.contains("duplicate constant"), "{err}");
    }
}
