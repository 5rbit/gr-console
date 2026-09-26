//! Template → text (syntax in the catalog header). A [`Renderer`] is built once per PLC: `from_const` enums are
//! resolved against that PLC's contract constants and `{alarm}` against its alarm table.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::Catalog;

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
        Renderer { cat, enums, alarms, templates }
    }

    pub fn enum_label(&self, name: &str, v: i64) -> Option<&str> {
        self.enums.get(name)?.get(&v).map(String::as_str)
    }

    /// Resolved values of one enum (catalog API).
    pub fn enum_values(&self, name: &str) -> Option<&BTreeMap<i64, String>> {
        self.enums.get(name)
    }

    pub fn alarm(&self, area: u32, bit: u32) -> Option<&AlarmEntry> {
        let area = self.enum_label("alarm_area", i64::from(area))?;
        self.alarms.find(area, bit)
    }

    /// Text of one event; an unknown (cat, code) renders as `CAT code src=.. a=.. b=..`.
    pub fn render(&self, ev: &Ev) -> String {
        let Some(def) = self.cat.event(ev.cat, ev.code) else { return self.fallback(ev) };
        let Some(toks) = self.templates.get(&def.name) else { return self.fallback(ev) };
        let mut s = String::new();
        for t in toks {
            match t {
                Tok::Lit(l) => s.push_str(l),
                Tok::Plc => s.push_str(ev.plc),
                Tok::Detail => s.push_str(ev.detail.unwrap_or("")),
                Tok::Alarm => s.push_str(&self.alarm_text(ev)),
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
    fn alarm_text(&self, ev: &Ev) -> String {
        let bit = u32::try_from(ev.a).unwrap_or(u32::MAX);
        let area = self.enum_label("alarm_area", i64::from(ev.src)).unwrap_or("area?").to_string();
        match self.alarm(ev.src, bit) {
            Some(a) => {
                // HMI / ErrorList spelling: F0101. EVENT / TASK bits have no code.
                let text = if a.text_ko.is_empty() { &a.text_en } else { &a.text_ko };
                let head = if a.code == 0 { area.clone() } else { format!("{}{:04}", area.chars().next().unwrap_or('?'), a.code) };
                format!("{head} {text}").trim_end().to_string()
            }
            None => format!("{area} bit {} ({}.{})", ev.a, ev.a.div_euclid(8), ev.a.rem_euclid(8)),
        }
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
        assert_eq!(r.render(&ev(6, 601, 3, 3, 0)), "EVENT Robot auto allowed 발생 (step 0)", "code 0 is not shown");
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
