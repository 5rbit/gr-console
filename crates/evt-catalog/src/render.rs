//! Template → text (syntax in the catalog header). A [`Renderer`] is built once per PLC: `from_const` enums are
//! resolved against that PLC's contract constants and `{alarm}` against its alarm table.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::Catalog;
use crate::errorlist::{self, ElEntry, ErrorList, Formats, Level, Trans, Ty, V2, ValFmt};

/// One stored event, as numbers.
#[derive(Clone, Debug, Default)]
pub struct Ev<'a> {
    pub plc: &'a str,
    pub cat: u8,
    pub lvl: u8,
    pub src: u32,
    pub code: u32,
    pub a: i64,
    pub b: i64,
    pub ctx: u32,
    pub detail: Option<&'a str>,
}

/// `plc/contract/<PLC>/alarms.json` (`gr-contract alarms`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AlarmTable {
    #[serde(default)]
    pub entries: Vec<AlarmEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AlarmEntry {
    /// `FAULT` / `WARN` / `EVENT` / `TASK`
    pub area: String,
    /// byte * 8 + bit
    pub bit: u32,
    pub code: u32,
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub text_en: String,
    #[serde(default)]
    pub text_ko: String,
}

impl AlarmTable {
    pub fn parse(json: &str) -> Result<AlarmTable, serde_json::Error> {
        serde_json::from_str(json)
    }
    pub fn find(&self, area: &str, bit: u32) -> Option<&AlarmEntry> {
        self.entries.iter().find(|e| e.bit == bit && e.area.eq_ignore_ascii_case(area))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Src,
    A,
    B,
    Code,
    Ctx,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Fmt {
    Raw,
    Enum(String),
    Bits(String),
    Div(u32),
    Hex,
    OnOff,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Lit(String),
    Val(Field, Fmt),
    Alarm,
    Plc,
    Detail,
}

fn field(s: &str) -> Option<Field> {
    Some(match s {
        "src" => Field::Src,
        "a" => Field::A,
        "b" => Field::B,
        "code" => Field::Code,
        "ctx" => Field::Ctx,
        _ => return None,
    })
}

/// One `{...}` body → token; `None` = not a placeholder (kept literally).
fn placeholder(body: &str) -> Option<Tok> {
    match body {
        "alarm" => return Some(Tok::Alarm),
        "plc" => return Some(Tok::Plc),
        "detail" => return Some(Tok::Detail),
        _ => {}
    }
    if let Some((f, d)) = body.split_once('/') {
        let div: u32 = d.trim().parse().ok().filter(|d| *d > 0)?;
        return Some(Tok::Val(field(f.trim())?, Fmt::Div(div)));
    }
    let mut parts = body.split(':');
    let f = field(parts.next()?.trim())?;
    let fmt = match (parts.next(), parts.next(), parts.next()) {
        (None, _, _) => Fmt::Raw,
        (Some("hex"), None, _) => Fmt::Hex,
        (Some("onoff"), None, _) => Fmt::OnOff,
        (Some("bits"), Some(e), None) => Fmt::Bits(e.trim().to_string()),
        (Some(e), None, _) => Fmt::Enum(e.trim().to_string()),
        _ => return None,
    };
    Some(Tok::Val(f, fmt))
}

fn parse(text: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut lit = String::new();
    let mut rest = text;
    while let Some(i) = rest.find('{') {
        lit.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('}').and_then(|j| placeholder(&after[..j]).map(|t| (j, t))) {
            Some((j, tok)) => {
                if !lit.is_empty() {
                    out.push(Tok::Lit(std::mem::take(&mut lit)));
                }
                out.push(tok);
                rest = &after[j + 1..];
            }
            None => {
                lit.push('{');
                rest = after;
            }
        }
    }
    lit.push_str(rest);
    if !lit.is_empty() {
        out.push(Tok::Lit(lit));
    }
    out
}

/// Enum names a template refers to (catalog validation).
pub(crate) fn template_enums(text: &str) -> Vec<String> {
    parse(text)
        .into_iter()
        .filter_map(|t| match t {
            Tok::Val(_, Fmt::Enum(e) | Fmt::Bits(e)) => Some(e),
            _ => None,
        })
        .collect()
}

fn hex(v: i64) -> String {
    let u = v as u32;
    if (0..=0xFFFF).contains(&v) { format!("16#{u:04X}") } else { format!("16#{u:08X}") }
}

/// Decimals = digits of the divisor (10 → 1, 100 → 2); other divisors round up to the next power of ten.
fn scaled(v: i64, div: u32) -> String {
    let mut dec = 0usize;
    let mut p = 1u64;
    while p < u64::from(div) {
        p *= 10;
        dec += 1;
    }
    format!("{:.*}", dec, v as f64 / f64::from(div))
}

pub struct Renderer {
    cat: Arc<Catalog>,
    enums: HashMap<String, BTreeMap<i64, String>>,
    alarms: AlarmTable,
    templates: HashMap<String, Vec<Tok>>,
    errorlist: ErrorList,
    formats: Arc<Formats>,
}

/// What a row is in ErrorList terms (Events tab Type column / filter).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Class {
    pub ty: Ty,
    pub trans: Option<Trans>,
    /// `F3119`; `None` when the row maps to several ErrorList rows or the alarm bit has no code.
    pub code: Option<String>,
}

/// `850 ms`, `4.2 s`, `3 m 05 s`, `1 h 02 m`.
pub fn fmt_ms(ms: i64) -> String {
    let ms = ms.max(0);
    if ms < 1000 {
        return format!("{ms} ms");
    }
    if ms < 60_000 {
        return format!("{:.1} s", ms as f64 / 1000.0);
    }
    let s = ms / 1000;
    if s < 3600 { format!("{} m {:02} s", s / 60, s % 60) } else { format!("{} h {:02} m", s / 3600, (s % 3600) / 60) }
}

impl Renderer {
    /// `consts` = the PLC contract constants (`plc_layout::Contract::consts`); empty for console-only rows.
    pub fn new(cat: Arc<Catalog>, consts: &HashMap<String, i64>, alarms: AlarmTable) -> Renderer {
        let mut enums = HashMap::new();
        for (name, e) in &cat.enums {
            let map = match &e.from_const {
                Some(prefix) => {
                    // several names can share a value: prefer a non-RESERVED, then the shortest, then alphabetical
                    let mut cands: Vec<(&String, &i64)> = consts.iter().filter(|(k, _)| k.starts_with(prefix.as_str()) && k.len() > prefix.len()).collect();
                    cands.sort_by(|(a, _), (b, _)| (a.contains("RESERVED"), a.len(), a.as_str()).cmp(&(b.contains("RESERVED"), b.len(), b.as_str())));
                    let mut m = BTreeMap::new();
                    for (k, v) in cands {
                        m.entry(*v).or_insert_with(|| k[prefix.len()..].to_string());
                    }
                    m
                }
                None => e.values.clone(),
            };
            enums.insert(name.clone(), map);
        }
        let templates = cat.events.iter().map(|e| (e.name.clone(), parse(&e.text))).collect();
        Renderer { cat, enums, alarms, templates, errorlist: ErrorList::default(), formats: Arc::new(Formats::default()) }
    }

    /// ErrorList of this PLC (`errorlist.json`) + the console's value hints (`errorlist-format.toml`).
    pub fn with_errorlist(mut self, errorlist: ErrorList, formats: Arc<Formats>) -> Renderer {
        self.errorlist = errorlist;
        self.formats = formats;
        self
    }

    pub fn errorlist(&self) -> &ErrorList {
        &self.errorlist
    }

    /// Bit indexes (alarms.json) of an alarm code in an area — old ALARM rows carry the bit, not the code.
    pub fn alarm_bits(&self, area: &str, code: u32) -> Vec<u32> {
        self.alarms.entries.iter().filter(|e| e.code == code && e.area.eq_ignore_ascii_case(area)).map(|e| e.bit).collect()
    }

    /// v2 row → its type and code; old ALARM rows by area; other catalog rows by the ErrorList's catalog reference.
    pub fn classify(&self, ev: &Ev) -> Option<Class> {
        if let Some(v) = errorlist::decode_v2(ev.cat, ev.src, ev.code) {
            let ty = self.errorlist.get(v.level, v.num).map(ElEntry::ty).unwrap_or(match v.level {
                Level::Alarm => Ty::Alarm,
                Level::Warn => Ty::Warn,
                Level::Operator => Ty::Operator,
                Level::Info => Ty::Info,
            });
            return Some(Class { ty, trans: Some(v.trans), code: Some(v.label()) });
        }
        if ev.cat == errorlist::CAT_ALARM && (ev.code == errorlist::ALM_RAISED || ev.code == errorlist::ALM_CLEARED) {
            let ty = match ev.src {
                1 => Ty::Alarm,
                2 => Ty::Warn,
                3 => Ty::Operator,
                4 => Ty::Task,
                _ => return None,
            };
            let code = u32::try_from(ev.a).ok().and_then(|bit| self.alarm(ev.src, bit)).filter(|a| a.code != 0 && matches!(ty, Ty::Alarm | Ty::Warn)).map(|a| {
                let level = if ty == Ty::Alarm { Level::Alarm } else { Level::Warn };
                errorlist::label(level, a.code)
            });
            let trans = if ev.code == errorlist::ALM_RAISED { Trans::Raise } else { Trans::Clear };
            return Some(Class { ty, trans: Some(trans), code });
        }
        let def = self.cat.event(ev.cat, ev.code)?;
        let (ty, e) = self.errorlist.for_catalog(&def.name, ev.src)?;
        Some(Class { ty, trans: None, code: e.map(|e| e.code.clone()) })
    }

    pub fn enum_label(&self, name: &str, v: i64) -> Option<&str> {
        self.enums.get(name)?.get(&v).map(String::as_str)
    }

    /// Resolved values of one enum (catalog API).
    pub fn enum_values(&self, name: &str) -> Option<&BTreeMap<i64, String>> {
        self.enums.get(name)
    }

    /// `area` = `alarm_area` id. The alarm table keeps the ALARM DB member names (3 EVENT, 4 TASK),
    /// not the ErrorList level names of the enum.
    pub fn alarm(&self, area: u32, bit: u32) -> Option<&AlarmEntry> {
        let member = match area {
            3 => "EVENT",
            4 => "TASK",
            _ => self.enum_label("alarm_area", i64::from(area))?,
        };
        self.alarms.find(member, bit)
    }

    /// Text of one event; an unknown (cat, code) renders as `CAT code src=.. a=.. b=..`.
    pub fn render(&self, ev: &Ev) -> String {
        self.render_lang(ev, false)
    }

    /// `en`: ErrorList / alarm texts in English (catalog templates have one language).
    pub fn render_lang(&self, ev: &Ev, en: bool) -> String {
        if let Some(v) = errorlist::decode_v2(ev.cat, ev.src, ev.code) {
            return self.render_v2(ev, v, en);
        }
        let Some(def) = self.cat.event(ev.cat, ev.code) else { return self.fallback(ev) };
        let Some(toks) = self.templates.get(&def.name) else { return self.fallback(ev) };
        let mut s = String::new();
        for t in toks {
            match t {
                Tok::Lit(l) => s.push_str(l),
                Tok::Plc => s.push_str(ev.plc),
                Tok::Detail => s.push_str(ev.detail.unwrap_or("")),
                Tok::Alarm => s.push_str(&self.alarm_text(ev, en)),
                Tok::Val(f, fmt) => {
                    let v = match f {
                        Field::Src => i64::from(ev.src),
                        Field::A => ev.a,
                        Field::B => ev.b,
                        Field::Code => i64::from(ev.code),
                        Field::Ctx => i64::from(ev.ctx),
                    };
                    s.push_str(&self.value(v, fmt));
                }
            }
        }
        s.trim_end().to_string()
    }

    fn value(&self, v: i64, fmt: &Fmt) -> String {
        match fmt {
            Fmt::Raw => v.to_string(),
            Fmt::Hex => hex(v),
            Fmt::OnOff => (if v == 0 { "OFF" } else { "ON" }).to_string(),
            Fmt::Div(d) => scaled(v, *d),
            Fmt::Enum(e) => self.enum_label(e, v).map(str::to_string).unwrap_or_else(|| v.to_string()),
            Fmt::Bits(e) => {
                let u = v as u32;
                if u == 0 {
                    return "-".into();
                }
                (0..32).filter(|n| u & (1 << n) != 0).map(|n| self.enum_label(e, i64::from(n)).map(str::to_string).unwrap_or_else(|| format!("bit{n}"))).collect::<Vec<_>>().join(", ")
            }
        }
    }

    /// `F3119 Station interlock timeout`; unknown bit → `FAULT bit 594 (74.2)`.
    fn alarm_text(&self, ev: &Ev, en: bool) -> String {
        let bit = u32::try_from(ev.a).unwrap_or(u32::MAX);
        let area = self.enum_label("alarm_area", i64::from(ev.src)).unwrap_or("area?").to_string();
        match self.alarm(ev.src, bit) {
            Some(a) => {
                // HMI / ErrorList spelling: F0101. EVENT / TASK bits have no code.
                let text = if (en && !a.text_en.is_empty()) || a.text_ko.is_empty() { &a.text_en } else { &a.text_ko };
                let head = if a.code == 0 { area.clone() } else { format!("{}{:04}", area.chars().next().unwrap_or('?'), a.code) };
                format!("{head} {text}").trim_end().to_string()
            }
            None => format!("{area} bit {} ({}.{})", ev.a, ev.a.div_euclid(8), ev.a.rem_euclid(8)),
        }
    }

    /// `<ErrorList text> 발생 · <values>` — the code is not in the text (the Type column shows it).
    fn render_v2(&self, ev: &Ev, v: V2, en: bool) -> String {
        let code = v.label();
        let e = self.errorlist.get(v.level, v.num);
        let mut s = e.map(|e| e.text(en).to_string()).filter(|t| !t.is_empty()).unwrap_or_else(|| if en { format!("{code} (not in the ErrorList)") } else { format!("{code} (ErrorList 에 없음)") });
        let word = match (v.trans, en) {
            (Trans::Raise, false) => "발생",
            (Trans::Raise, true) => "raised",
            (Trans::Clear, false) => "해제",
            (Trans::Clear, true) => "cleared",
            (Trans::Momentary, _) => "",
            (Trans::Summary, false) => "반복 억제",
            (Trans::Summary, true) => "repeats suppressed",
        };
        if !word.is_empty() {
            s.push(' ');
            s.push_str(word);
        }
        let parts = match v.trans {
            Trans::Clear => {
                let mut p = vec![format!("{} {}", if en { "active" } else { "지속" }, fmt_ms(ev.b))];
                if ev.a > 0 {
                    p.push(format!("flicker {}", ev.a));
                }
                p
            }
            Trans::Summary => vec![if en { format!("{} rows", ev.a) } else { format!("{} 건", ev.a) }],
            Trans::Raise | Trans::Momentary => self.v2_values(ev, v.level, &code, e),
        };
        if !parts.is_empty() {
            s.push_str(" · ");
            s.push_str(&parts.join(" · "));
        }
        s
    }

    /// Src (Operator / Info only — ALARM rows carry the area there), A, B by the format hint, else guessed from the
    /// ErrorList meaning. A value without a meaning shows only when it is not 0 (an ALARM without values: B = the step).
    fn v2_values(&self, ev: &Ev, level: Level, code: &str, e: Option<&ElEntry>) -> Vec<String> {
        let none = errorlist::FormatHint::default();
        let h = self.formats.get(code).unwrap_or(&none);
        let (am, bm) = e.map(|e| (e.a_meaning.as_str(), e.b_meaning.as_str())).unwrap_or(("", ""));
        let alarm = matches!(level, Level::Alarm | Level::Warn);
        let mut out = Vec::new();
        if !alarm {
            let g = errorlist::src_hint(am).or_else(|| errorlist::src_hint(bm)).map(|l| (l, ValFmt::Raw(None)));
            out.extend(self.one(i64::from(ev.src), h.src.as_deref(), h.src_label.as_deref(), g, "Src"));
        }
        out.extend(self.one(ev.a, h.a.as_deref(), h.a_label.as_deref(), errorlist::guess(am), "A"));
        // an alarm with no attached value carries the step in B (like the old ALM_RAISED)
        let gb = errorlist::guess(bm).or_else(|| (alarm && am.trim().is_empty() && ev.b != 0).then(|| ("step".to_string(), ValFmt::Raw(None))));
        out.extend(self.one(ev.b, h.b.as_deref(), h.b_label.as_deref(), gb, "B"));
        out
    }

    fn one(&self, v: i64, spec: Option<&str>, label: Option<&str>, guessed: Option<(String, ValFmt)>, fallback: &str) -> Option<String> {
        let fmt = match spec.and_then(errorlist::parse_spec) {
            Some(f) => f,
            None => match &guessed {
                Some((_, f)) => f.clone(),
                None if v == 0 => return None,
                None => ValFmt::Raw(None),
            },
        };
        // an explicit empty label prints the value alone (`ON`)
        let name = match label {
            Some("") => String::new(),
            Some(l) => l.to_string(),
            None => guessed.map(|g| g.0).filter(|n| !n.is_empty() && n.chars().count() <= 20).unwrap_or_else(|| fallback.to_string()),
        };
        let val = match &fmt {
            ValFmt::None => return None,
            ValFmt::Raw(None) => v.to_string(),
            ValFmt::Raw(Some(u)) => format!("{v} {u}"),
            ValFmt::Hex => hex(v),
            ValFmt::OnOff => self.value(v, &Fmt::OnOff),
            ValFmt::Enum(e) => self.value(v, &Fmt::Enum(e.clone())),
            ValFmt::Bits(e) => self.value(v, &Fmt::Bits(e.clone())),
            ValFmt::Div(d, None) => scaled(v, *d),
            ValFmt::Div(d, Some(u)) => format!("{} {u}", scaled(v, *d)),
        };
        Some(if name.is_empty() { val } else { format!("{name} {val}") })
    }

    fn fallback(&self, ev: &Ev) -> String {
        let cat = self.cat.cat_name(ev.cat).map(str::to_string).unwrap_or_else(|| format!("cat{}", ev.cat));
        let mut s = format!("{cat} {} src={} a={} b={}", ev.code, ev.src, ev.a, ev.b);
        if let Some(d) = ev.detail.filter(|d| !d.is_empty()) {
            s.push(' ');
            s.push_str(d);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn consts() -> HashMap<String, i64> {
        [("MODE_AUTO", 64), ("MODE_READY", 32), ("MODE_FAULT", 128), ("MODE_RESERVED", 64), ("GRIP_E_TIMEOUT", 5), ("GRIP_OWNER_TASK", 1), ("CMD_TASK_PICK", 0x50)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect()
    }

    fn alarms() -> AlarmTable {
        AlarmTable {
            entries: vec![
                AlarmEntry { area: "FAULT".into(), bit: 594, code: 3119, class: "Fault".into(), text_en: "Station interlock timeout".into(), text_ko: "스테이션 인터록 타임아웃".into() },
                AlarmEntry { area: "FAULT".into(), bit: 8, code: 101, class: "Fault".into(), text_en: "EMS".into(), text_ko: String::new() },
                AlarmEntry { area: "EVENT".into(), bit: 3, code: 0, class: "Event".into(), text_en: "Robot auto allowed".into(), text_ko: String::new() },
            ],
        }
    }

    fn ev(cat: u8, code: u32, src: u32, a: i64, b: i64) -> Ev<'static> {
        Ev { plc: "GR2", cat, lvl: 2, src, code, a, b, ctx: 0, detail: None }
    }

    #[test]
    fn renders_the_plan_examples() {
        let c = crate::tests::repo_catalog();
        let r = Renderer::new(Arc::new(c), &consts(), alarms());
        // STEP: wildcard entry, proc enum, dwell
        assert_eq!(r.render(&ev(3, 300, 20, 2400, 200)), "Task 200→300  (200 체류 2400 ms)");
        // scale /10 with one decimal, from_const enum (GRIP_E_), owner
        assert_eq!(r.render(&ev(7, 704, 5, 5984, 38)), "실패 TIMEOUT StopPos 598.4 mm · 38 %");
        // from_const: RESERVED loses to the real name for the same value
        assert_eq!(r.render(&ev(2, 201, 0, 128, 64)), "AUTO → FAULT");
        // alarm lookup + unknown bit
        assert_eq!(r.render(&ev(6, 601, 1, 594, 400)), "F3119 스테이션 인터록 타임아웃 발생 (step 400)");
        assert_eq!(r.render(&ev(6, 602, 2, 17, 0)), "WARN bit 17 (2.1) 해제");
        assert_eq!(r.render(&ev(6, 602, 1, 8, 0)), "F0101 EMS 해제", "4-digit code, English when no Korean");
        assert_eq!(r.render(&ev(6, 601, 3, 3, 0)), "OPERATOR Robot auto allowed 발생 (step 0)", "code 0 is not shown");
        // bits
        assert_eq!(r.render(&ev(2, 205, 0, 0b1010, 0)), "AUTO 시작 불가 : PowerOnDrive, GRM_Connected");
        assert_eq!(r.render(&ev(12, 1202, 0, 0b1000_0001, 0)), "PI CVOK, SpareX7 (이전 -)");
        // hex, onoff, /100 with negative
        assert_eq!(r.render(&ev(1, 102, 1, 0x2522, 7)), "Programming error : block 1, Fault_ID 16#2522, DB 7");
        assert_eq!(r.render(&ev(10, 1004, 3, 0b101, 0)), "Module diag Drive : Fault, AnyFault");
        assert_eq!(r.render(&ev(14, 1404, 1, 3200, 1)), "GR1 heartbeat 멈춤 (3200 ms)");
        assert_eq!(r.render(&ev(4, 416, 0x10, 5200, 1200)), "사이클 ProcessTime 5200 ms · Travel 1200 mm (flags 16#0010)");
        assert_eq!(r.render(&ev(9, 901, 0, 1, 0)), "EMS ON");
        assert_eq!(r.render(&ev(15, 1502, 428, -525, 100)), "HMI p428 1.00 → -5.25");
        // unknown enum value → number; unknown code → raw fallback
        assert_eq!(r.render(&ev(3, 410, 999, 5, 400)), "999 400→410  (400 체류 5 ms)");
        assert_eq!(r.render(&ev(7, 799, 1, 2, 3)), "GRIP 799 src=1 a=2 b=3");
        assert_eq!(r.render(&ev(99, 1, 0, 0, 0)), "cat99 1 src=0 a=0 b=0");
    }

    fn el_entry(code: &str, class: &str, en: &str, ko: &str, a: &str, b: &str, catalog: Option<(&str, Option<u32>)>) -> ElEntry {
        let (level, num) = errorlist::parse_label(code).unwrap();
        ElEntry {
            level: Some(level),
            code: code.into(),
            num,
            hmi_class: class.into(),
            text_en: en.into(),
            text_ko: ko.into(),
            a_meaning: a.into(),
            b_meaning: b.into(),
            catalog: catalog.map(|c| c.0.to_string()),
            catalog_src: catalog.and_then(|c| c.1),
            ..Default::default()
        }
    }

    fn v2_renderer() -> Renderer {
        let el = ErrorList::new(vec![
            el_entry("F3119", "FAULT", "Station Interlock Timeout", "스테이션 인터록 타임아웃", "step (400 / 600)", "", None),
            el_entry("W1101", "WARN", "X Axis - Lag Error", "X축 Lag", "", "", None),
            el_entry("O0101", "OPERATOR", "X Axis - Jog Speed Forward", "X 축 Jog Speed 전진", "시작 위치 (mm×10)", "0 (Jog)", Some(("CMD_JOG", Some(1)))),
            el_entry("I0301", "TASK", "Task - Accepted to Buffer", "작업 수락 (Buff)", "Cell.Id (Src = task type)", "WorkId", Some(("TASK_ACCEPTED", None))),
            el_entry("I5101", "INFO", "Step - Changed", "Step 변경", "", "", Some(("STEP_CHANGED", None))),
        ]);
        let fmt = Formats::parse("[gr.I0301]\nsrc = \"enum:task_type\"\nsrc_label = \"Type\"\na_label = \"Cell\"\n[gr.O0101]\nb = \"none\"\n", false).unwrap();
        Renderer::new(Arc::new(crate::tests::repo_catalog()), &consts(), alarms()).with_errorlist(el, Arc::new(fmt))
    }

    #[test]
    fn v2_rows_render_from_the_errorlist() {
        let r = v2_renderer();
        let en = |e: Ev| r.render_lang(&e, true);
        assert_eq!(r.render(&ev(6, 13119, 1, 400, 0)), "스테이션 인터록 타임아웃 발생 · step 400");
        assert_eq!(en(ev(6, 13119, 1, 400, 0)), "Station Interlock Timeout raised · step 400");
        assert_eq!(r.render(&ev(6, 11101, 2, 0, 600)), "X축 Lag 발생 · step 600", "no meaning: B is the step when set");
        assert_eq!(r.render(&ev(6, 21101, 2, 3, 4200)), "X축 Lag 해제 · 지속 4.2 s · flicker 3");
        assert_eq!(en(ev(6, 21101, 2, 0, 125_000)), "X Axis - Lag Error cleared · active 2 m 05 s");
        assert_eq!(r.render(&ev(6, 41101, 2, 12, 0)), "X축 Lag 반복 억제 · 12 건");
        assert_eq!(r.render(&ev(18, 10101, 0, 5984, 0)), "X 축 Jog Speed 전진 발생 · 시작 위치 598.4 mm");
        assert_eq!(r.render(&ev(19, 30301, 0x50, 101, 55)), "작업 수락 (Buff) · Type PICK · Cell 101 · WorkId 55");
        assert_eq!(en(ev(19, 30999, 0, 0, 7)), "I0999 (not in the ErrorList) · B 7");
        // the old catalog rows are unchanged, the English text only swaps the alarm text
        assert_eq!(r.render(&ev(6, 601, 1, 594, 400)), "F3119 스테이션 인터록 타임아웃 발생 (step 400)");
        assert_eq!(en(ev(6, 601, 1, 594, 400)), "F3119 Station interlock timeout 발생 (step 400)");
        assert_eq!(fmt_ms(850), "850 ms");
        assert_eq!(fmt_ms(3_723_000), "1 h 02 m");
    }

    #[test]
    fn classify_v2_old_alarm_rows_and_catalog_refs() {
        let r = v2_renderer();
        let c = |e: Ev| r.classify(&e).map(|c| (c.ty, c.trans, c.code));
        assert_eq!(c(ev(6, 13119, 1, 400, 0)), Some((Ty::Alarm, Some(Trans::Raise), Some("F3119".into()))));
        assert_eq!(c(ev(6, 21101, 2, 0, 0)), Some((Ty::Warn, Some(Trans::Clear), Some("W1101".into()))));
        assert_eq!(c(ev(19, 30301, 1, 0, 0)), Some((Ty::Task, Some(Trans::Momentary), Some("I0301".into()))), "HMI class TASK");
        assert_eq!(c(ev(19, 30999, 1, 0, 0)), Some((Ty::Info, Some(Trans::Momentary), Some("I0999".into()))), "unknown Info code");
        assert_eq!(c(ev(6, 601, 1, 594, 0)), Some((Ty::Alarm, Some(Trans::Raise), Some("F3119".into()))), "old row: code from alarms.json");
        assert_eq!(c(ev(6, 602, 3, 3, 0)), Some((Ty::Operator, Some(Trans::Clear), None)), "EVENT bits have no code");
        assert_eq!(c(ev(6, 601, 4, 9, 0)), Some((Ty::Task, Some(Trans::Raise), None)), "old TASK area");
        assert_eq!(c(ev(5, 513, 1, 1, 0)), Some((Ty::Operator, None, Some("O0101".into()))), "CMD_JOG Src=1");
        assert_eq!(c(ev(3, 300, 20, 900, 200)), Some((Ty::Info, None, Some("I5101".into()))), "STEP (\"*\") by name");
        assert_eq!(c(ev(4, 401, 1, 101, 7)), Some((Ty::Task, None, Some("I0301".into()))));
        assert_eq!(c(ev(7, 701, 1, 0, 0)), None, "not in the ErrorList");
        assert_eq!(c(ev(6, 603, 1, 2, 0)), None);
    }

    #[test]
    fn console_rows_use_plc_and_detail() {
        let c = crate::tests::repo_catalog();
        let r = Renderer::new(Arc::new(c), &HashMap::new(), AlarmTable::default());
        let mut e = ev(14, 9001, 0, 1, 0);
        e.plc = "GRM";
        assert_eq!(r.render(&e), "콘솔 S7 GRM ON");
        let mut e = ev(5, 9010, 0, 0, 0);
        e.detail = Some("GR2 PLC 끊김");
        assert_eq!(r.render(&e), "제출 거부 : GR2 PLC 끊김");
    }

    #[test]
    fn template_parser_keeps_non_placeholders() {
        assert_eq!(parse("a {x} {a:hex} }{"), vec![Tok::Lit("a {x} ".into()), Tok::Val(Field::A, Fmt::Hex), Tok::Lit(" }{".into())]);
        assert_eq!(scaled(5984, 10), "598.4");
        assert_eq!(scaled(7, 1000), "0.007");
        assert_eq!(scaled(12, 4), "3.0");
        assert_eq!(hex(0x63F8_3AC3), "16#63F83AC3");
    }
}
