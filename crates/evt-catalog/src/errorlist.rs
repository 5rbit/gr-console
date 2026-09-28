//! ErrorList registry (`plc/contract/<PLC>/errorlist.json`, written by `gr-contract errorlist` from the ErrorList
//! workbook) and the v2 EVTLOG row encoding (`docs/evtlog/encoding-v2.md`).
//!
//! v2 rows keep the 34 B entry: `Cat` 6 ALARM (Src = 1 FAULT / 2 WARN) · 18 OPERATOR · 19 INFO,
//! `Code = transition × 10000 + ErrorList number`, Src / A / B = values (clear rows: A = flicker count, B = active ms).
//! Old catalog rows keep their meaning: a v2 ALARM row is told apart by `Code >= 10000` (catalog codes are < 10000).

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

pub const CAT_ALARM: u8 = 6;
pub const CAT_OPERATOR: u8 = 18;
pub const CAT_INFO: u8 = 19;
/// `Code = trans * TRANS_MUL + num`.
pub const TRANS_MUL: u32 = 10_000;
/// Old catalog ALARM rows (`{alarm}` = (area Src, bit A) through alarms.json).
pub const ALM_RAISED: u32 = 601;
pub const ALM_CLEARED: u32 = 602;

/// ErrorList level (sheet + code letter).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Level {
    Alarm,
    Warn,
    Operator,
    Info,
}

impl Level {
    pub const ALL: [Level; 4] = [Level::Alarm, Level::Warn, Level::Operator, Level::Info];

    pub fn letter(self) -> char {
        match self {
            Level::Alarm => 'F',
            Level::Warn => 'W',
            Level::Operator => 'O',
            Level::Info => 'I',
        }
    }
    pub fn from_letter(c: char) -> Option<Level> {
        match c.to_ascii_uppercase() {
            'F' => Some(Level::Alarm),
            'W' => Some(Level::Warn),
            'O' => Some(Level::Operator),
            'I' => Some(Level::Info),
            _ => None,
        }
    }
    /// PLC byte area (`alarm_area` enum id: 1 FAULT, 2 WARN, 3 OPERATOR, 4 INFO).
    pub fn area(self) -> &'static str {
        match self {
            Level::Alarm => "FAULT",
            Level::Warn => "WARN",
            Level::Operator => "EVENT",
            Level::Info => "INFO",
        }
    }
    pub fn area_id(self) -> u32 {
        match self {
            Level::Alarm => 1,
            Level::Warn => 2,
            Level::Operator => 3,
            Level::Info => 4,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Level::Alarm => "Alarm",
            Level::Warn => "Warn",
            Level::Operator => "Operator",
            Level::Info => "Info",
        }
    }
    /// Catalog level id the PLC writes (ERROR 4, WARN 3, INFO 2).
    pub fn lvl(self) -> u8 {
        match self {
            Level::Alarm => 4,
            Level::Warn => 3,
            Level::Operator | Level::Info => 2,
        }
    }
    /// v2 category.
    pub fn cat(self) -> u8 {
        match self {
            Level::Alarm | Level::Warn => CAT_ALARM,
            Level::Operator => CAT_OPERATOR,
            Level::Info => CAT_INFO,
        }
    }
}

/// `F0101` → (Alarm, 101).
pub fn parse_label(s: &str) -> Option<(Level, u32)> {
    let s = s.trim();
    let mut it = s.chars();
    let level = Level::from_letter(it.next()?)?;
    let digits = it.as_str();
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u32 = digits.parse().ok()?;
    (n > 0).then_some((level, n))
}

pub fn label(level: Level, num: u32) -> String {
    format!("{}{num:04}", level.letter())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    State,
    Momentary,
    Log,
}

/// Console type (Events tab filter): the level, with Info split by HMI class TASK.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Ty {
    Alarm,
    Warn,
    Operator,
    Info,
    Task,
}

impl Ty {
    pub const ALL: [Ty; 5] = [Ty::Alarm, Ty::Warn, Ty::Operator, Ty::Info, Ty::Task];

    pub fn name(self) -> &'static str {
        match self {
            Ty::Alarm => "Alarm",
            Ty::Warn => "Warn",
            Ty::Operator => "Operator",
            Ty::Info => "Info",
            Ty::Task => "Task",
        }
    }
    pub fn parse(s: &str) -> Option<Ty> {
        Ty::ALL.into_iter().find(|t| t.name().eq_ignore_ascii_case(s.trim()))
    }
}

/// v2 row transition (`Code / 10000`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Trans {
    Raise = 1,
    Clear = 2,
    Momentary = 3,
    Summary = 4,
}

impl Trans {
    pub fn from_id(n: u32) -> Option<Trans> {
        Some(match n {
            1 => Trans::Raise,
            2 => Trans::Clear,
            3 => Trans::Momentary,
            4 => Trans::Summary,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Trans::Raise => "raise",
            Trans::Clear => "clear",
            Trans::Momentary => "momentary",
            Trans::Summary => "summary",
        }
    }
    pub fn parse(s: &str) -> Option<Trans> {
        [Trans::Raise, Trans::Clear, Trans::Momentary, Trans::Summary].into_iter().find(|t| t.name().eq_ignore_ascii_case(s.trim()))
    }
}

/// A decoded v2 row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2 {
    pub level: Level,
    pub trans: Trans,
    pub num: u32,
}

impl V2 {
    pub fn label(&self) -> String {
        label(self.level, self.num)
    }
}

/// `(cat, src, code)` of a v2 row, `None` for catalog rows.
pub fn decode_v2(cat: u8, src: u32, code: u32) -> Option<V2> {
    let trans = Trans::from_id(code / TRANS_MUL)?;
    let num = code % TRANS_MUL;
    if num == 0 {
        return None;
    }
    let level = match cat {
        CAT_ALARM => match src {
            1 => Level::Alarm,
            2 => Level::Warn,
            _ => return None,
        },
        CAT_OPERATOR => Level::Operator,
        CAT_INFO => Level::Info,
        _ => return None,
    };
    Some(V2 { level, trans, num })
}

/// `(cat, src for ALARM rows, code)` — the PLC side of [`decode_v2`] (demo, tests).
pub fn encode_v2(level: Level, trans: Trans, num: u32) -> (u8, Option<u32>, u32) {
    let src = matches!(level, Level::Alarm | Level::Warn).then(|| level.area_id());
    (level.cat(), src, trans as u32 * TRANS_MUL + num)
}

/// One ErrorList row for one PLC.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ElEntry {
    pub level: Option<Level>,
    /// HMI alarm class suffix: FAULT / WARN / OPERATOR / INFO / TASK.
    pub hmi_class: String,
    /// `F0101`
    pub code: String,
    pub num: u32,
    /// FAULT / WARN / EVENT / INFO
    pub area: String,
    pub byte: Option<u32>,
    pub bit: Option<u8>,
    pub kind: Kind,
    /// Shown on the HMI (discrete alarm).
    pub hmi: bool,
    pub text_en: String,
    pub text_ko: String,
    /// HMI EventText (`[O0105] X Axis - Homing`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hmi_text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cause_ko: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cause_en: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub action_ko: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub action_en: String,
    #[serde(default)]
    pub a_meaning: String,
    #[serde(default)]
    pub b_meaning: String,
    #[serde(rename = "where", default)]
    pub at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub condition: String,
    /// Catalog event this row replaces (old rows of that event read as this type).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog: Option<String>,
    /// `Src=N` of the catalog reference, when the row names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_src: Option<u32>,
    /// Catalog event merged into this alarm (ErrorList "알람 통합"), documentation only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_event: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    /// Fault sheets: 프로그램 확인.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub program: String,
    /// Sheet row (1-based).
    #[serde(default)]
    pub row: u32,
}

impl ElEntry {
    pub fn level(&self) -> Level {
        self.level.or_else(|| self.code.chars().next().and_then(Level::from_letter)).unwrap_or(Level::Info)
    }
    pub fn ty(&self) -> Ty {
        match self.level() {
            Level::Alarm => Ty::Alarm,
            Level::Warn => Ty::Warn,
            Level::Operator => Ty::Operator,
            Level::Info if self.hmi_class.eq_ignore_ascii_case("TASK") => Ty::Task,
            Level::Info => Ty::Info,
        }
    }
    /// Text in the asked language, the other one when empty.
    pub fn text(&self, en: bool) -> &str {
        let (a, b) = if en { (&self.text_en, &self.text_ko) } else { (&self.text_ko, &self.text_en) };
        if a.is_empty() { b } else { a }
    }
}

/// `plc/contract/<PLC>/errorlist.json`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ErrorListFile {
    #[serde(default)]
    pub plc: String,
    #[serde(default)]
    pub generated_by: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub entries: Vec<ElEntry>,
}

/// What a catalog event name reads as (old rows).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogRef {
    pub event: String,
    pub src: Option<u32>,
    pub ty: Ty,
    /// ErrorList code when exactly one row names this (event, src).
    pub code: Option<String>,
}

/// Indexed ErrorList of one PLC.
#[derive(Clone, Debug, Default)]
pub struct ErrorList {
    pub entries: Vec<ElEntry>,
    by_code: HashMap<(Level, u32), usize>,
    by_catalog: HashMap<String, Vec<usize>>,
}

impl ErrorList {
    pub fn parse(json: &str) -> Result<ErrorList, serde_json::Error> {
        let f: ErrorListFile = serde_json::from_str(json)?;
        Ok(ErrorList::new(f.entries))
    }

    pub fn new(entries: Vec<ElEntry>) -> ErrorList {
        let mut by_code = HashMap::new();
        let mut by_catalog: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            by_code.entry((e.level(), e.num)).or_insert(i);
            if let Some(c) = &e.catalog {
                by_catalog.entry(c.to_ascii_uppercase()).or_default().push(i);
            }
        }
        ErrorList { entries, by_code, by_catalog }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, level: Level, num: u32) -> Option<&ElEntry> {
        self.by_code.get(&(level, num)).map(|i| &self.entries[*i])
    }

    pub fn by_label(&self, code: &str) -> Option<&ElEntry> {
        let (l, n) = parse_label(code)?;
        self.get(l, n)
    }

    /// Type (and code when unique) of an old catalog row: every row naming the event agrees on the type, or the
    /// row's Src picks one.
    pub fn for_catalog(&self, event: &str, src: u32) -> Option<(Ty, Option<&ElEntry>)> {
        let idx = self.by_catalog.get(&event.to_ascii_uppercase())?;
        let rows: Vec<&ElEntry> = idx.iter().map(|i| &self.entries[*i]).collect();
        let by_src: Vec<&ElEntry> = rows.iter().copied().filter(|e| e.catalog_src == Some(src)).collect();
        let pick = if by_src.is_empty() { rows } else { by_src };
        let ty = pick[0].ty();
        if pick.iter().any(|e| e.ty() != ty) {
            return None;
        }
        Some((ty, (pick.len() == 1).then(|| pick[0])))
    }

    /// Every (event, src) → type this list gives old rows (SQL for the type filter).
    pub fn catalog_refs(&self) -> Vec<CatalogRef> {
        let mut out = Vec::new();
        let mut names: Vec<&String> = self.by_catalog.keys().collect();
        names.sort();
        for name in names {
            let rows: Vec<&ElEntry> = self.by_catalog[name].iter().map(|i| &self.entries[*i]).collect();
            let srcs: Vec<u32> = {
                let mut v: Vec<u32> = rows.iter().filter_map(|e| e.catalog_src).collect();
                v.sort_unstable();
                v.dedup();
                v
            };
            let all_ty = rows[0].ty();
            if rows.iter().all(|e| e.ty() == all_ty) {
                let code = (rows.len() == 1).then(|| rows[0].code.clone());
                out.push(CatalogRef { event: name.clone(), src: None, ty: all_ty, code });
                continue;
            }
            for s in srcs {
                if let Some((ty, e)) = self.for_catalog(name, s) {
                    out.push(CatalogRef { event: name.clone(), src: Some(s), ty, code: e.map(|e| e.code.clone()) });
                }
            }
        }
        out
    }

    /// Info numbers whose HMI class is TASK.
    pub fn task_nums(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.entries.iter().filter(|e| e.ty() == Ty::Task).map(|e| e.num).collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}

// ---------------------------------------------------------------- value formatting

/// `plc/evtlog/errorlist-format.toml`: per ErrorList code, how Src / A / B read. The ErrorList stays a human
/// document; this file only adds what a machine needs (enum names, scaling, a short label).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct FormatFile {
    /// GR1 / GR2 (one robot sheet)
    #[serde(default)]
    pub gr: BTreeMap<String, FormatHint>,
    #[serde(default)]
    pub grm: BTreeMap<String, FormatHint>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct FormatHint {
    /// `raw` · `none` · `hex` · `onoff` · `enum:<name>` · `bits:<name>` · `div:<n>[ unit]` · `unit:<unit>`
    pub a: Option<String>,
    pub b: Option<String>,
    pub src: Option<String>,
    pub a_label: Option<String>,
    pub b_label: Option<String>,
    pub src_label: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Formats {
    by_code: HashMap<String, FormatHint>,
}

impl Formats {
    /// The `[gr]` or `[grm]` table (the two ErrorLists reuse numbers with other meanings).
    pub fn parse(text: &str, grm: bool) -> Result<Formats, toml::de::Error> {
        let f: FormatFile = toml::from_str(text)?;
        let m = if grm { f.grm } else { f.gr };
        Ok(Formats { by_code: m.into_iter().map(|(k, v)| (k.trim().to_ascii_uppercase(), v)).collect() })
    }
    /// Exact code, else the longest `PREFIX*` key.
    pub fn get(&self, code: &str) -> Option<&FormatHint> {
        let code = code.to_ascii_uppercase();
        self.by_code.get(&code).or_else(|| self.by_code.iter().filter_map(|(k, h)| k.strip_suffix('*').filter(|p| code.starts_with(*p)).map(|p| (p.len(), h))).max_by_key(|(n, _)| *n).map(|(_, h)| h))
    }
    /// Every spec string (validation of the enum names against the catalog).
    pub fn specs(&self) -> impl Iterator<Item = (&str, &str)> {
        self.by_code.iter().flat_map(|(k, h)| [&h.a, &h.b, &h.src].into_iter().flatten().map(move |s| (k.as_str(), s.as_str())))
    }
}

/// Parsed value spec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValFmt {
    None,
    Raw(Option<String>),
    Hex,
    OnOff,
    Enum(String),
    Bits(String),
    Div(u32, Option<String>),
}

pub fn parse_spec(s: &str) -> Option<ValFmt> {
    let s = s.trim();
    Some(match s {
        "none" => ValFmt::None,
        "raw" => ValFmt::Raw(None),
        "hex" => ValFmt::Hex,
        "onoff" => ValFmt::OnOff,
        _ => {
            let (k, v) = s.split_once(':')?;
            let v = v.trim();
            match k.trim() {
                "enum" => ValFmt::Enum(v.to_string()),
                "bits" => ValFmt::Bits(v.to_string()),
                "unit" => ValFmt::Raw(Some(v.to_string())),
                "div" => {
                    let (n, unit) = match v.split_once(' ') {
                        Some((n, u)) => (n, Some(u.trim().to_string()).filter(|u| !u.is_empty())),
                        None => (v, None),
                    };
                    ValFmt::Div(n.parse().ok().filter(|d: &u32| *d > 0)?, unit)
                }
                _ => return None,
            }
        }
    })
}

/// Label + format guessed from an ErrorList meaning (`시작 위치 (mm×10)` → `시작 위치`, div 10 mm). Empty meaning →
/// `None` (the value is shown only when it is not 0).
pub fn guess(meaning: &str) -> Option<(String, ValFmt)> {
    let m = meaning.trim();
    if m.is_empty() {
        return None;
    }
    let lower = m.to_lowercase();
    let fmt = if lower.contains("×100") || lower.contains("x100") {
        ValFmt::Div(100, None)
    } else if lower.contains("mm×10") || lower.contains("mm x10") {
        ValFmt::Div(10, Some("mm".into()))
    } else if lower.contains("×10") {
        ValFmt::Div(10, None)
    } else if lower.contains("on/off") || lower.contains("on·off") {
        ValFmt::OnOff
    } else if lower.split(|c: char| !c.is_alphanumeric()).any(|w| w == "ms") {
        ValFmt::Raw(Some("ms".into()))
    } else {
        ValFmt::Raw(None)
    };
    let cut = m.find([';', '(']).map(|i| &m[..i]).unwrap_or(m);
    let mut name = cut.to_string();
    for scale in ["mm×10", "mm x10", "×100", "×10"] {
        name = name.replace(scale, "");
    }
    let mut name = name.trim().trim_end_matches([',', '.']).trim().to_string();
    if let ValFmt::Raw(Some(u)) = &fmt {
        name = name.trim_end_matches(u.as_str()).trim().to_string();
    }
    // a PLC member path reads by its last part (Axis["X"].PV.LagError → LagError)
    if name.chars().count() > 20
        && !name.contains(' ')
        && let Some((_, last)) = name.rsplit_once('.')
    {
        name = last.to_string();
    }
    Some((name, fmt))
}

/// `(Src = task type)` / `Src = 축` inside a meaning → the Src label.
pub fn src_hint(meaning: &str) -> Option<String> {
    let i = meaning.find("Src =").or_else(|| meaning.find("Src="))?;
    let rest = meaning[i..].split_once('=')?.1;
    let end = rest.find([')', ';', ',']).unwrap_or(rest.len());
    let s = rest[..end].trim();
    (!s.is_empty()).then(|| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(code: &str, class: &str, catalog: Option<&str>, src: Option<u32>) -> ElEntry {
        let (level, num) = parse_label(code).unwrap();
        ElEntry { level: Some(level), code: code.into(), num, hmi_class: class.into(), catalog: catalog.map(str::to_string), catalog_src: src, ..Default::default() }
    }

    #[test]
    fn v2_round_trip_and_old_rows_stay_old() {
        for level in Level::ALL {
            for trans in [Trans::Raise, Trans::Clear, Trans::Momentary, Trans::Summary] {
                let (cat, src, code) = encode_v2(level, trans, 3119);
                assert_eq!(decode_v2(cat, src.unwrap_or(7), code), Some(V2 { level, trans, num: 3119 }));
                assert!(code <= u32::from(u16::MAX), "Code is a UInt");
            }
        }
        assert_eq!(encode_v2(Level::Warn, Trans::Clear, 1101), (6, Some(2), 21101));
        assert_eq!(encode_v2(Level::Info, Trans::Momentary, 301), (19, None, 30301));
        // catalog rows: ALM_RAISED 601 (Src = area), STEP 300, unknown transition, number 0, other categories
        assert_eq!(decode_v2(6, 1, 601), None);
        assert_eq!(decode_v2(6, 3, 10101), None, "cat 6 v2 needs Src 1/2");
        assert_eq!(decode_v2(19, 0, 50301), None);
        assert_eq!(decode_v2(19, 0, 20000), None);
        assert_eq!(decode_v2(3, 20, 10300), None);
        assert_eq!(parse_label("f0101"), Some((Level::Alarm, 101)));
        assert_eq!(parse_label("I5101"), Some((Level::Info, 5101)));
        assert_eq!(parse_label("X0101"), None);
        assert_eq!(parse_label("F101"), None);
        assert_eq!(label(Level::Operator, 105), "O0105");
    }

    #[test]
    fn types_catalog_refs_and_task_split() {
        let el = ErrorList::new(vec![
            entry("I0301", "TASK", Some("TASK_ACCEPTED"), None),
            entry("I0101", "INFO", Some("MODE_CHANGED"), None),
            entry("I0108", "INFO", Some("MODE_CHANGED"), None),
            entry("O0101", "OPERATOR", Some("CMD_JOG"), Some(1)),
            entry("O0201", "OPERATOR", Some("CMD_JOG"), Some(2)),
            entry("I5105", "INFO", Some("MIXED"), Some(1)),
            entry("O0510", "OPERATOR", Some("MIXED"), Some(2)),
            entry("F3119", "FAULT", None, None),
        ]);
        assert_eq!(el.by_label("F3119").map(|e| e.ty()), Some(Ty::Alarm));
        assert_eq!(el.task_nums(), vec![301]);
        assert_eq!(el.for_catalog("task_accepted", 0).map(|(t, e)| (t, e.map(|e| e.code.as_str()))), Some((Ty::Task, Some("I0301"))));
        assert_eq!(el.for_catalog("MODE_CHANGED", 0).map(|(t, e)| (t, e.is_some())), Some((Ty::Info, false)), "two rows: type only");
        assert_eq!(el.for_catalog("CMD_JOG", 2).map(|(t, e)| (t, e.map(|e| e.code.as_str()))), Some((Ty::Operator, Some("O0201"))));
        assert_eq!(el.for_catalog("MIXED", 9), None, "types disagree and Src picks none");
        assert_eq!(el.for_catalog("MIXED", 2).map(|x| x.0), Some(Ty::Operator));
        let refs = el.catalog_refs();
        assert!(refs.contains(&CatalogRef { event: "MIXED".into(), src: Some(1), ty: Ty::Info, code: Some("I5105".into()) }));
        assert!(refs.contains(&CatalogRef { event: "CMD_JOG".into(), src: None, ty: Ty::Operator, code: None }));
    }

    #[test]
    fn value_guesses_and_specs() {
        assert_eq!(guess("시작 위치 (mm×10)"), Some(("시작 위치".into(), ValFmt::Div(10, Some("mm".into())))));
        assert_eq!(guess("새 값 ×100 (Src = pNNN)"), Some(("새 값".into(), ValFmt::Div(100, None))));
        assert_eq!(guess("대기 ms"), Some(("대기".into(), ValFmt::Raw(Some("ms".into())))));
        assert_eq!(guess("On/Off (Src = bit)"), Some(("On/Off".into(), ValFmt::OnOff)));
        assert_eq!(guess("  "), None);
        assert_eq!(guess("Axis[\"X\"].PV.LagError (mm×10)").map(|g| g.0), Some("LagError".into()));
        assert_eq!(src_hint("Cell.Id (Src = task type)").as_deref(), Some("task type"));
        assert_eq!(src_hint("STATUS (Src = 1 BSEND, 2 BRCV)").as_deref(), Some("1 BSEND"));
        assert_eq!(src_hint("WorkId"), None);
        assert_eq!(parse_spec("div:10 mm"), Some(ValFmt::Div(10, Some("mm".into()))));
        assert_eq!(parse_spec("enum:task_type"), Some(ValFmt::Enum("task_type".into())));
        assert_eq!(parse_spec("unit:ms"), Some(ValFmt::Raw(Some("ms".into()))));
        assert_eq!(parse_spec("div:0"), None);
        assert_eq!(parse_spec("loud"), None);
        let text = "[gr.I0301]\nsrc = \"enum:task_type\"\na_label = \"Cell\"\n[gr.\"I01*\"]\na = \"hex\"\n[gr.\"I010*\"]\na = \"enum:mode\"\n[grm.I0301]\na = \"unit:ms\"\n";
        let f = Formats::parse(text, false).unwrap();
        assert_eq!(f.get("i0301").and_then(|h| h.a_label.as_deref()), Some("Cell"));
        assert_eq!(f.get("I0105").and_then(|h| h.a.as_deref()), Some("enum:mode"), "longest prefix");
        assert_eq!(f.get("I0110").and_then(|h| h.a.as_deref()), Some("hex"));
        assert!(f.get("I0201").is_none());
        assert_eq!(Formats::parse(text, true).unwrap().get("I0301").and_then(|h| h.a.as_deref()), Some("unit:ms"), "GRM has its own table");
        assert_eq!(f.specs().count(), 3);
    }
}
