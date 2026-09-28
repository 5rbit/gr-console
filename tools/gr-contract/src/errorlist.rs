//! `gr-contract errorlist`: the ErrorList workbook (the master registry) → `plc/contract/<PLC>/errorlist.json`
//! (Alarm / Warn / Operator / Info rows per PLC, read by the console) + HMI discrete-alarm drafts for the Operator /
//! Info / Task classes (`plc/generated/hmi/<unit>-{operator,info,task}.json`).
//!
//! Sheets (found by name): `GRM…Fault`, `GR1,2…Fault` (Alarm + Warn), `…Operator`, `…Info` (GRM and GR1,2), `알람 통합`
//! (values attached to alarms). Fault rows carry no FAULT/WARN column: the `[F…]` / `[W…]` prefix of the HMI text
//! column (GRM), else 프로그램 확인 `사용 (FAULT)` / `(WARN)` / `(FAULT+WARN)`, else the PLC's alarms.json, else FAULT.
//! Rows without an English and a Korean text are spares and are skipped. `[GR2]` / `GR2 전용` rows go to GR2 only.
//!
//! Checks: unique code per PLC and level, sheet row = 3 + Byte × 8 + Bit, Byte/Bit follow the code rule
//! (Operator / Info: Byte = 4 × (XX − 1) + (YY − 1) div 8; Alarm / Warn: the group base table of
//! `siemens/tools/lib/alarmrule.cjs`). `--check` writes nothing, fails when a generated file differs, and reports
//! where the HMI discrete alarms (FAULT / WARN) differ from the ErrorList texts.
//!
//! HMI draft Ids: 200000 + 20000 × (GR1 0, GR2 1, GRM 2) + (Operator 10000 | Info/Task 20000) + number,
//! i.e. GR1 O0105 = 210105, GR1 I0301 = 220301, GR2 = 23xxxx / 24xxxx, GRM = 25xxxx / 26xxxx — above every Id of
//! HMI_RT_1 (≤ 120072).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Context;
use evt_catalog::errorlist::{ElEntry, ErrorListFile, Kind, Level, label};
use serde::Serialize;

#[derive(clap::Args)]
pub struct Args {
    /// ErrorList workbook
    #[arg(long, default_value = "../siemens/E13398_GR_V1.6.2_ErrorList_260928.xlsx")]
    pub xlsx: PathBuf,
    /// Contract root (output <contract>/<PLC>/errorlist.json; alarms.json read as a level fallback)
    #[arg(long, default_value = "plc/contract")]
    pub contract: PathBuf,
    /// HMI draft output dir
    #[arg(long, default_value = "plc/generated/hmi")]
    pub hmi_out: PathBuf,
    /// TIA export root (the HMI discrete alarms for the drift report)
    #[arg(long, default_value = "../siemens/export")]
    pub export: PathBuf,
    /// Write nothing; exit 1 when a generated file differs. Also reports ErrorList ↔ HMI text drift.
    #[arg(long)]
    pub check: bool,
    /// Also write the drift report as JSON
    #[arg(long)]
    pub drift_json: Option<PathBuf>,
}

pub const PLCS: [&str; 3] = ["GR1_PLC", "GR2_PLC", "GRM_PLC"];

// ---------------------------------------------------------------- workbook → sheets

pub type Grid = Vec<Vec<String>>;

pub struct Sheet {
    pub name: String,
    /// 0-based sheet row of `grid[0]`.
    pub row0: u32,
    pub grid: Grid,
}

fn cell_str(d: &calamine::Data) -> String {
    use calamine::Data;
    match d {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Float(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", *f as i64),
        Data::Int(i) => i.to_string(),
        other => other.to_string(),
    }
}

pub fn read_workbook(path: &Path) -> anyhow::Result<Vec<Sheet>> {
    use calamine::Reader;
    let mut wb = calamine::open_workbook_auto(path).with_context(|| format!("open {}", path.display()))?;
    let mut out = Vec::new();
    for name in wb.sheet_names() {
        let Ok(range) = wb.worksheet_range(&name) else { continue };
        let row0 = range.start().map(|s| s.0).unwrap_or(0);
        let col0 = range.start().map(|s| s.1).unwrap_or(0) as usize;
        let grid = range
            .rows()
            .map(|r| {
                let mut v = vec![String::new(); col0];
                v.extend(r.iter().map(cell_str));
                v
            })
            .collect();
        out.push(Sheet { name, row0, grid });
    }
    Ok(out)
}

/// `_x000D_` and runs of whitespace → one space.
fn one_line(s: &str) -> String {
    s.replace("_x000D_", " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Keeps line breaks (cause / action lists), drops `_x000D_` and trailing blanks.
fn multi_line(s: &str) -> String {
    s.replace("_x000D_", "").replace("\r\n", "\n").lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim().to_string()
}

struct Cols {
    head: usize,
    map: Vec<String>,
}

impl Cols {
    /// Header row = the first of the top 10 rows that has a cell equal to `key`.
    fn find(grid: &Grid, key: &str) -> Option<Cols> {
        let (head, r) = grid.iter().enumerate().take(10).find(|(_, r)| r.iter().any(|c| one_line(c) == key))?;
        Some(Cols { head, map: r.iter().map(|c| one_line(c)).collect() })
    }
    fn col(&self, pred: impl Fn(&str) -> bool) -> Option<usize> {
        self.map.iter().position(|c| pred(c))
    }
}

fn get(r: &[String], c: Option<usize>) -> String {
    c.and_then(|c| r.get(c)).map(|s| one_line(s)).unwrap_or_default()
}

fn get_ml(r: &[String], c: Option<usize>) -> String {
    c.and_then(|c| r.get(c)).map(|s| multi_line(s)).unwrap_or_default()
}

// ---------------------------------------------------------------- Fault sheets

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FaultRow {
    pub row: u32,
    pub num: u32,
    pub title: String,
    pub en: String,
    pub ko: String,
    pub cause_ko: String,
    pub action_ko: String,
    pub cause_en: String,
    pub action_en: String,
    pub byte: Option<u32>,
    pub bit: Option<u8>,
    /// Column after Trigger Bit (`[F0101] …`, GRM sheet only).
    pub hmi_text: String,
    pub program: String,
    pub condition: String,
}

pub fn parse_fault(grid: &Grid, row0: u32) -> Option<Vec<FaultRow>> {
    let h = Cols::find(grid, "HMI Code")?;
    let code_c = h.col(|c| c == "HMI Code")?;
    let en_c = h.col(|c| c.starts_with("이상내역 (영문)"));
    let ko_c = h.col(|c| c.starts_with("이상내역 (한국어)"));
    let title_c = h.col(|c| c == "이상내역");
    let cause_c = h.col(|c| c == "발생 원인");
    let action_c = h.col(|c| c == "조치방법");
    let byte_c = h.col(|c| c.contains("Trigger Byte"));
    let bit_c = h.col(|c| c.contains("Trigger Bit"));
    let cause_en_c = h.col(|c| c.starts_with("발생 원인 (EN)"));
    let action_en_c = h.col(|c| c.starts_with("조치방법 (EN)"));
    let prog_c = h.col(|c| c.starts_with("프로그램 확인"));
    let cond_c = h.col(|c| c.starts_with("발생 조건"));
    let hmi_c = bit_c.map(|b| b + 1).filter(|c| h.map.get(*c).is_none_or(|s| s.is_empty()));
    let mut out = Vec::new();
    for (i, r) in grid.iter().enumerate().skip(h.head + 1) {
        let mut code = get(r, Some(code_c));
        if code.len() < 4 && !code.is_empty() && code.bytes().all(|c| c.is_ascii_digit()) {
            code = format!("{code:0>4}");
        }
        if code.len() != 4 || !code.bytes().all(|c| c.is_ascii_digit()) {
            continue;
        }
        out.push(FaultRow {
            row: row0 + i as u32 + 1,
            num: code.parse().ok()?,
            title: get(r, title_c),
            en: get(r, en_c),
            ko: get(r, ko_c),
            cause_ko: get_ml(r, cause_c),
            action_ko: get_ml(r, action_c),
            cause_en: get_ml(r, cause_en_c),
            action_en: get_ml(r, action_en_c),
            byte: get(r, byte_c).parse().ok(),
            bit: get(r, bit_c).parse().ok(),
            hmi_text: get(r, hmi_c),
            program: get(r, prog_c),
            condition: get_ml(r, cond_c),
        });
    }
    Some(out)
}

/// Levels a fault row stands for, and how that was decided.
pub fn fault_levels(r: &FaultRow, alarms: Option<&evt_catalog::AlarmTable>) -> (Vec<Level>, &'static str) {
    if let Some(l) = r.hmi_text.strip_prefix('[').and_then(|s| s.chars().next()).and_then(Level::from_letter).filter(|l| matches!(l, Level::Alarm | Level::Warn)) {
        return (vec![l], "hmi text");
    }
    let p = r.program.to_ascii_uppercase();
    if p.contains("FAULT+WARN") {
        return (vec![Level::Alarm, Level::Warn], "program");
    }
    if p.contains("(FAULT") {
        return (vec![Level::Alarm], "program");
    }
    if p.contains("(WARN") {
        return (vec![Level::Warn], "program");
    }
    if let Some(t) = alarms {
        let mut v = Vec::new();
        if t.entries.iter().any(|e| e.code == r.num && e.area.eq_ignore_ascii_case("FAULT")) {
            v.push(Level::Alarm);
        }
        if t.entries.iter().any(|e| e.code == r.num && e.area.eq_ignore_ascii_case("WARN")) {
            v.push(Level::Warn);
        }
        if !v.is_empty() {
            return (v, "alarms.json");
        }
    }
    (vec![Level::Alarm], "assumed")
}

// ---------------------------------------------------------------- Operator / Info sheets

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EvRow {
    pub row: u32,
    pub level: Option<Level>,
    pub num: u32,
    pub group: String,
    pub item: String,
    pub en: String,
    pub ko: String,
    pub kind: Kind,
    pub hmi: bool,
    pub byte: Option<u32>,
    pub bit: Option<u8>,
    pub hmi_text: String,
    pub a: String,
    pub b: String,
    pub at: String,
    pub condition: String,
    pub change: String,
    pub class: String,
    pub gr2_only: bool,
}

pub fn parse_events(grid: &Grid, row0: u32) -> Option<Vec<EvRow>> {
    let h = Cols::find(grid, "기록 방식")?;
    let c = |k: &str| h.col(|x| x == k);
    let s = |k: &str| h.col(|x| x.starts_with(k));
    let (group_c, item_c, code_c, en_c, ko_c, kind_c, hmi_c) = (c("구분"), c("항목"), c("Code")?, s("문구 (영문)"), s("문구 (한국어)"), c("기록 방식"), s("HMI 표시"));
    let (byte_c, bit_c, text_c, a_c, b_c, at_c, cond_c, change_c, class_c) =
        (c("Byte"), c("Bit"), c("HMI 문구"), c("값 A"), c("값 B"), c("선언 위치"), c("발생 조건"), s("변경 내역"), c("HMI 클래스"));
    let mut out = Vec::new();
    let mut group = String::new();
    for (i, r) in grid.iter().enumerate().skip(h.head + 1) {
        let g = get(r, group_c);
        if !g.is_empty() {
            group = g;
        }
        let code = get(r, Some(code_c));
        let Some((level, num)) = evt_catalog::errorlist::parse_label(&code) else { continue };
        let item = get(r, item_c);
        let kind = match get(r, kind_c).as_str() {
            "상태형" => Kind::State,
            "로그전용" => Kind::Log,
            _ => Kind::Momentary,
        };
        out.push(EvRow {
            row: row0 + i as u32 + 1,
            level: Some(level),
            num,
            group: group.clone(),
            gr2_only: item.starts_with("[GR2]"),
            item: item.trim_start_matches("[GR2]").trim().to_string(),
            en: get(r, en_c),
            ko: get(r, ko_c),
            kind,
            hmi: get(r, hmi_c).eq_ignore_ascii_case("Y"),
            byte: get(r, byte_c).parse().ok(),
            bit: get(r, bit_c).parse().ok(),
            hmi_text: get(r, text_c),
            a: get(r, a_c),
            b: get(r, b_c),
            at: get(r, at_c),
            condition: get_ml(r, cond_c),
            change: get_ml(r, change_c),
            class: get(r, class_c).to_ascii_uppercase(),
        });
    }
    Some(out)
}

/// `…\ncatalog : CMD_JOG (DEBUG, Src=1)` → (`CMD_JOG`, Some(1)).
pub fn catalog_ref(change: &str) -> Option<(String, Option<u32>)> {
    let line = change.lines().find(|l| l.trim_start().to_ascii_lowercase().starts_with("catalog"))?;
    let rest = line.split_once(':')?.1.trim();
    let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
    if name.is_empty() || !name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let src = rest.find("Src=").and_then(|i| rest[i + 4..].chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok());
    Some((name, src))
}

// ---------------------------------------------------------------- 알람 통합

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Merge {
    pub event: String,
    pub plcs: Vec<&'static str>,
    pub codes: Vec<(Level, u32)>,
    pub a: String,
    pub b: String,
}

/// `F3118 (…), F3119`, `F·W 1001 / 2001`, `F1010~1013`, `F6201~F6330`, `GR F0209` → (level, number); a range
/// expands to every number in it (the caller keeps the ones that exist).
pub fn parse_codes(s: &str) -> Vec<(Level, u32)> {
    let b: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut prefix: Vec<Level> = Vec::new();
    let mut last: Option<u32> = None;
    let mut range = false;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let letter_here = matches!(c, 'F' | 'W') && (i == 0 || !b[i - 1].is_ascii_alphanumeric());
        if letter_here {
            let mut j = i;
            let mut ls = Vec::new();
            while j < b.len() && matches!(b[j], 'F' | 'W') {
                ls.push(Level::from_letter(b[j]).unwrap_or(Level::Alarm));
                j += 1;
                if j < b.len() && b[j] == '·' {
                    j += 1;
                }
            }
            if j < b.len() && (b[j].is_ascii_digit() || b[j] == ' ') {
                prefix = ls;
                i = j;
                continue;
            }
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let before_ok = start == 0 || !b[start - 1].is_ascii_alphanumeric() || matches!(b[start - 1], 'F' | 'W');
            if i - start == 4 && before_ok && !prefix.is_empty() {
                let n: u32 = b[start..i].iter().collect::<String>().parse().unwrap_or(0);
                if range && let Some(lo) = last {
                    for m in lo + 1..=n {
                        out.extend(prefix.iter().map(|l| (*l, m)));
                    }
                } else {
                    out.extend(prefix.iter().map(|l| (*l, n)));
                }
                last = Some(n);
            }
            range = false;
            continue;
        }
        if c == '~' {
            range = true;
        } else if !c.is_whitespace() && c != 'F' && c != 'W' {
            range = false;
        }
        i += 1;
    }
    out.sort();
    out.dedup();
    out
}

/// `A = step (…), B = WorkId` → (A, B); `제안 : …` wins over the text before it; `없음` alone → nothing.
pub fn parse_values(s: &str) -> (String, String) {
    let proposal = s.split_once("제안 :").or_else(|| s.split_once("제안:")).map(|(_, r)| r);
    if proposal.is_none() && (s.trim_start().starts_with("없음") || s.trim_start().starts_with("현재 없음")) {
        return (String::new(), String::new());
    }
    let s = proposal.unwrap_or(s).trim();
    let find = |key: &str| -> Option<usize> {
        let mut from = 0;
        while let Some(i) = s[from..].find(key) {
            let at = from + i;
            if at == 0 || !s[..at].chars().last().is_some_and(|c| c.is_alphanumeric() || c == '.') {
                return Some(at);
            }
            from = at + key.len();
        }
        None
    };
    let (ai, bi) = (find("A = "), find("B = "));
    let clean = |t: &str| t.trim().trim_end_matches([',', '.', ';']).trim().to_string();
    let a = ai.map(|i| clean(&s[i + 4..bi.filter(|b| *b > i).unwrap_or(s.len())])).unwrap_or_default();
    let b = bi.map(|i| clean(&s[i + 4..ai.filter(|a| *a > i).unwrap_or(s.len())])).unwrap_or_default();
    (a, b)
}

pub fn parse_merge(grid: &Grid) -> Option<Vec<Merge>> {
    let h = Cols::find(grid, "Event (catalog)")?;
    let (ev_c, plc_c, code_c, val_c, dec_c) =
        (h.col(|c| c == "Event (catalog)")?, h.col(|c| c == "PLC"), h.col(|c| c == "대상 알람 코드")?, h.col(|c| c.starts_with("알람에 붙일 값")), h.col(|c| c == "결정"));
    let mut out = Vec::new();
    for r in grid.iter().skip(h.head + 1) {
        if dec_c.is_some() && get(r, dec_c) != "결정" {
            continue;
        }
        let codes = parse_codes(&get(r, Some(code_c)));
        if codes.is_empty() {
            continue;
        }
        let who = get(r, plc_c);
        let target = who.rsplit('→').next().unwrap_or(&who).to_ascii_uppercase();
        let plcs: Vec<&'static str> = if target.contains("전체") { PLCS.to_vec() } else { PLCS.iter().copied().filter(|p| target.contains(&p[..3])).collect() };
        let (a, b) = parse_values(&get(r, val_c));
        out.push(Merge { event: get(r, Some(ev_c)).split_whitespace().next().unwrap_or("").to_string(), plcs, codes, a, b });
    }
    Some(out)
}

// ---------------------------------------------------------------- code rules

/// Alarm / Warn byte base per group (siemens `tools/lib/alarmrule.cjs` SEED).
const SEED: [(u32, u32, u32, u32); 6] = [(1, 13, 0, 4), (20, 23, 52, 4), (30, 33, 68, 4), (40, 43, 84, 4), (50, 59, 100, 4), (60, 99, 140, 4)];

/// Expected (byte, bit) of a code; `None` = no rule for that group.
pub fn expected_pos(level: Level, num: u32) -> Option<(u32, u8)> {
    let (xx, yy) = (num / 100, num % 100);
    if yy == 0 {
        return None;
    }
    let base = match level {
        Level::Operator | Level::Info => (xx >= 1).then(|| 4 * (xx - 1))?,
        Level::Alarm | Level::Warn => SEED.iter().find(|(a, b, _, _)| (*a..=*b).contains(&xx)).map(|(a, _, s, w)| s + (xx - a) * w)?,
    };
    Some((base + (yy - 1) / 8, ((yy - 1) % 8) as u8))
}

// ---------------------------------------------------------------- build

#[derive(Clone, Debug, Default, Serialize)]
pub struct Issue {
    pub plc: String,
    pub code: String,
    pub row: u32,
    pub what: String,
}

#[derive(Default)]
pub struct Built {
    pub files: BTreeMap<String, ErrorListFile>,
    pub errors: Vec<Issue>,
    pub notes: Vec<String>,
    pub spares: usize,
    /// how the fault level was decided → count
    pub level_src: BTreeMap<&'static str, usize>,
}

pub struct Input {
    pub gr_fault: Vec<FaultRow>,
    pub grm_fault: Vec<FaultRow>,
    pub gr_events: Vec<EvRow>,
    pub grm_events: Vec<EvRow>,
    pub merge: Vec<Merge>,
    pub source: String,
}

pub fn input_from(sheets: &[Sheet], source: &str) -> anyhow::Result<Input> {
    let find = |pred: &dyn Fn(&str) -> bool| sheets.iter().filter(|s| pred(&s.name)).collect::<Vec<_>>();
    let one = |what: &str, pred: &dyn Fn(&str) -> bool| -> anyhow::Result<&Sheet> {
        let v = find(pred);
        anyhow::ensure!(v.len() == 1, "{source}: expected one sheet for {what}, found {}", v.len());
        Ok(v[0])
    };
    let lower = |s: &str| s.to_ascii_lowercase();
    let gr_fault = one("GR1,2 Fault", &|n| n.contains("GR1") && lower(n).contains("fault"))?;
    let grm_fault = one("GRM Fault", &|n| n.contains("GRM") && lower(n).contains("fault"))?;
    let mut gr_events = Vec::new();
    let mut grm_events = Vec::new();
    for s in sheets.iter().filter(|s| lower(&s.name).contains("operator") || lower(&s.name).ends_with("info")) {
        let rows = parse_events(&s.grid, s.row0).with_context(|| format!("{source}#{}: no header row with 기록 방식", s.name))?;
        if s.name.contains("GRM") {
            grm_events.extend(rows);
        } else {
            gr_events.extend(rows);
        }
    }
    let merge = sheets.iter().find(|s| s.name.contains("알람 통합")).and_then(|s| parse_merge(&s.grid)).unwrap_or_default();
    Ok(Input {
        gr_fault: parse_fault(&gr_fault.grid, gr_fault.row0).with_context(|| format!("{source}#{}: no HMI Code header", gr_fault.name))?,
        grm_fault: parse_fault(&grm_fault.grid, grm_fault.row0).with_context(|| format!("{source}#{}: no HMI Code header", grm_fault.name))?,
        gr_events,
        grm_events,
        merge,
        source: source.to_string(),
    })
}

fn where_of(condition: &str) -> String {
    let mut v = Vec::new();
    let mut rest = condition;
    while let Some(i) = rest.find('[') {
        let Some(j) = rest[i..].find(']') else { break };
        let t = &rest[i + 1..i + j];
        if t.contains(':') && !t.contains(' ') {
            v.push(t.to_string());
        }
        rest = &rest[i + j + 1..];
    }
    v.join(", ")
}

fn fault_entry(r: &FaultRow, level: Level) -> ElEntry {
    let code = label(level, r.num);
    let hmi_text = if r.hmi_text.is_empty() { format!("[{code}] {}", r.en) } else { r.hmi_text.clone() };
    ElEntry {
        level: Some(level),
        hmi_class: if level == Level::Alarm { "FAULT" } else { "WARN" }.into(),
        code,
        num: r.num,
        area: level.area().into(),
        byte: r.byte,
        bit: r.bit,
        kind: Kind::State,
        hmi: true,
        text_en: if r.en.is_empty() { r.title.clone() } else { r.en.clone() },
        text_ko: r.ko.clone(),
        hmi_text,
        cause_ko: r.cause_ko.clone(),
        cause_en: r.cause_en.clone(),
        action_ko: r.action_ko.clone(),
        action_en: r.action_en.clone(),
        at: where_of(&r.condition),
        condition: r.condition.clone(),
        program: r.program.clone(),
        row: r.row,
        ..Default::default()
    }
}

fn event_entry(r: &EvRow) -> ElEntry {
    let level = r.level.unwrap_or(Level::Info);
    let (catalog, catalog_src) = catalog_ref(&r.change).map(|(n, s)| (Some(n), s)).unwrap_or((None, None));
    let class = if r.class.is_empty() || r.class == "HMI" { if level == Level::Operator { "OPERATOR" } else { "INFO" }.to_string() } else { r.class.clone() };
    ElEntry {
        level: Some(level),
        hmi_class: class,
        code: label(level, r.num),
        num: r.num,
        area: level.area().into(),
        byte: r.byte.filter(|_| r.kind != Kind::Log),
        bit: r.bit.filter(|_| r.kind != Kind::Log),
        kind: r.kind,
        hmi: r.hmi,
        text_en: r.en.clone(),
        text_ko: r.ko.clone(),
        hmi_text: r.hmi_text.clone(),
        a_meaning: r.a.clone(),
        b_meaning: r.b.clone(),
        at: r.at.clone(),
        condition: r.condition.clone(),
        catalog,
        catalog_src,
        group: r.group.clone(),
        row: r.row,
        ..Default::default()
    }
}

pub fn build(inp: &Input, alarms: &HashMap<String, evt_catalog::AlarmTable>) -> Built {
    let mut b = Built::default();
    for plc in PLCS {
        let grm = plc == "GRM_PLC";
        let gr2 = plc == "GR2_PLC";
        let mut entries: Vec<ElEntry> = Vec::new();
        for r in if grm { &inp.grm_fault } else { &inp.gr_fault } {
            if r.en.is_empty() && r.ko.is_empty() {
                if plc == PLCS[0] || grm {
                    b.spares += 1;
                }
                continue;
            }
            if !gr2 && r.program.contains("GR2 전용") {
                continue;
            }
            let (levels, how) = fault_levels(r, alarms.get(plc));
            if !gr2 {
                *b.level_src.entry(how).or_default() += 1;
            }
            entries.extend(levels.into_iter().map(|l| fault_entry(r, l)));
        }
        for r in if grm { &inp.grm_events } else { &inp.gr_events } {
            if r.en.is_empty() && r.ko.is_empty() {
                if plc == PLCS[0] || grm {
                    b.spares += 1;
                }
                continue;
            }
            if r.gr2_only && !gr2 {
                continue;
            }
            entries.push(event_entry(r));
        }
        let have: HashSet<(Level, u32)> = entries.iter().map(|e| (e.level(), e.num)).collect();
        for m in inp.merge.iter().filter(|m| m.plcs.contains(&plc)) {
            for e in entries.iter_mut().filter(|e| m.codes.contains(&(e.level(), e.num))) {
                e.merged_event = Some(m.event.clone());
                e.a_meaning = m.a.clone();
                e.b_meaning = m.b.clone();
            }
            let missing: Vec<String> = m.codes.iter().filter(|c| !have.contains(c)).map(|(l, n)| label(*l, *n)).collect();
            // a range (F6201~F6330) names numbers that were never assigned — only short lists are worth a note
            if !missing.is_empty() && m.codes.len() <= 12 {
                b.notes.push(format!("{plc}: 알람 통합 {} names {} not in the Fault sheet", m.event, missing.join(", ")));
            }
        }
        entries.sort_by_key(|e| (e.level(), e.num));
        // checks
        let mut seen: HashMap<(Level, u32), u32> = HashMap::new();
        let mut no_rule: Vec<u32> = Vec::new();
        for e in &entries {
            let issue = |what: String| Issue { plc: plc.into(), code: e.code.clone(), row: e.row, what };
            if let Some(first) = seen.insert((e.level(), e.num), e.row) {
                b.errors.push(issue(format!("duplicate code (also row {first})")));
            }
            let (Some(byte), Some(bit)) = (e.byte, e.bit) else {
                if e.kind != Kind::Log {
                    b.errors.push(issue("no Byte/Bit".into()));
                }
                continue;
            };
            if bit > 7 {
                b.errors.push(issue(format!("Bit {bit} > 7")));
            }
            if e.row != 3 + byte * 8 + u32::from(bit) {
                b.errors.push(issue(format!("row {} ≠ 3 + Byte {byte} × 8 + Bit {bit}", e.row)));
            }
            match expected_pos(e.level(), e.num) {
                Some((eb, ebit)) if (eb, ebit) != (byte, bit) => b.errors.push(issue(format!("Byte/Bit {byte}.{bit} ≠ code rule {eb}.{ebit}"))),
                Some(_) => {}
                None => no_rule.push(e.num / 100),
            }
        }
        no_rule.sort_unstable();
        no_rule.dedup();
        if !no_rule.is_empty() {
            b.notes.push(format!("{plc}: no Byte base for alarm groups {no_rule:?} (row rule only)"));
        }
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for e in &entries {
            *counts.entry(e.level().name().to_string()).or_default() += 1;
            if e.ty() == evt_catalog::Ty::Task {
                *counts.entry("Task".into()).or_default() += 1;
            }
            if e.kind == Kind::Log {
                *counts.entry("log".into()).or_default() += 1;
            }
            if e.hmi && e.byte.is_some() && matches!(e.level(), Level::Operator | Level::Info) {
                *counts.entry("hmi_drafts".into()).or_default() += 1;
            }
        }
        b.files.insert(plc.to_string(), ErrorListFile { plc: plc.into(), generated_by: "gr-contract errorlist".into(), source: inp.source.clone(), counts, entries });
    }
    b.notes.sort();
    b.notes.dedup();
    b
}

// ---------------------------------------------------------------- HMI drafts

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Texts3 {
    #[serde(rename = "en-US")]
    en: String,
    #[serde(rename = "ko-KR")]
    ko: String,
    #[serde(rename = "hu-HU")]
    hu: String,
}

const EMPTY_P: &str = "<body><p/></body>";

fn html(s: &str) -> String {
    let t = s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    if t.trim().is_empty() { EMPTY_P.into() } else { format!("<body><p>{}</p></body>", t.split_whitespace().collect::<Vec<_>>().join(" ")) }
}

impl Texts3 {
    fn new(en: &str, ko: &str) -> Texts3 {
        Texts3 { en: html(en), ko: html(ko), hu: EMPTY_P.into() }
    }
    fn empty() -> Texts3 {
        Texts3::new("", "")
    }
}

/// The `export/HMI_RT_1/alarms/discrete.json` object shape without `AlarmParameterTags` (import strips it anyway).
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Discrete {
    name: String,
    acknowledgment_control_tag: &'static str,
    acknowledgment_control_tag_bit_number: u8,
    acknowledgment_state_tag: &'static str,
    acknowledgment_state_tag_bit_number: u8,
    alarm_class: String,
    area: &'static str,
    id: u32,
    origin: &'static str,
    priority: u8,
    raised_state_tag: String,
    raised_state_tag_bit_number: u8,
    trigger_bit_address: String,
    trigger_mode: &'static str,
    event_text: Texts3,
    event_text1: Texts3,
    event_text2: Texts3,
    event_text3: Texts3,
    event_text4: Texts3,
    event_text5: Texts3,
    event_text6: Texts3,
    event_text7: Texts3,
    event_text8: Texts3,
    event_text9: Texts3,
}

pub fn unit_of(plc: &str) -> &'static str {
    match plc {
        "GR1_PLC" => "GR1",
        "GR2_PLC" => "GR2",
        _ => "GRM",
    }
}

pub fn draft_id(unit: &str, level: Level, num: u32) -> u32 {
    let u = match unit {
        "GR1" => 0,
        "GR2" => 1,
        _ => 2,
    };
    200_000 + 20_000 * u + if level == Level::Operator { 10_000 } else { 20_000 } + num
}

/// `A: … · B: …`, else the condition (one line, 200 chars).
fn hint(e: &ElEntry) -> String {
    let mut parts = Vec::new();
    if !e.a_meaning.is_empty() {
        parts.push(format!("A: {}", e.a_meaning));
    }
    if !e.b_meaning.is_empty() {
        parts.push(format!("B: {}", e.b_meaning));
    }
    if parts.is_empty() {
        let c = one_line(&e.condition);
        return c.chars().take(200).collect();
    }
    parts.join(" · ")
}

/// (file stem → alarms) for one PLC: `<unit>-operator`, `<unit>-info`, `<unit>-task`.
pub fn drafts(plc: &str, f: &ErrorListFile) -> BTreeMap<String, Vec<Discrete>> {
    let unit = unit_of(plc);
    let (ev_tag, info_tag) = if unit == "GRM" { ("HMI_Alarm_Event".to_string(), "HMI_Alarm_Info".to_string()) } else { (format!("{unit}_Alarm_Event"), format!("{unit}_Alarm_Info")) };
    let mut out: BTreeMap<String, Vec<Discrete>> = BTreeMap::new();
    for e in f.entries.iter().filter(|e| e.hmi && matches!(e.level(), Level::Operator | Level::Info)) {
        let (Some(byte), Some(bit)) = (e.byte, e.bit) else { continue };
        let class = e.hmi_class.to_ascii_uppercase();
        let (tag, member) = if e.level() == Level::Operator { (&ev_tag, "Event") } else { (&info_tag, "Info") };
        let text_en = if e.hmi_text.is_empty() { format!("[{}] {}", e.code, e.text_en) } else { e.hmi_text.clone() };
        let h = hint(e);
        out.entry(format!("{}-{}", unit.to_ascii_lowercase(), class.to_ascii_lowercase())).or_default().push(Discrete {
            name: format!("{unit}_{}", e.code),
            acknowledgment_control_tag: "<No tag>",
            acknowledgment_control_tag_bit_number: 0,
            acknowledgment_state_tag: "<No tag>",
            acknowledgment_state_tag_bit_number: 0,
            alarm_class: format!("{unit} {class}"),
            area: "HMI_RT_1::Alarming",
            id: draft_id(unit, e.level(), e.num),
            origin: "",
            priority: 0,
            raised_state_tag: format!("{tag}[{byte}]"),
            raised_state_tag_bit_number: bit,
            trigger_bit_address: format!("HMI.Alarm.{member}[{byte}].x{bit}"),
            trigger_mode: "OnRisingEdge",
            event_text: Texts3::new(&text_en, &format!("[{}] {}", e.code, e.text_ko)),
            event_text1: Texts3::new(&h, &h),
            event_text2: Texts3::empty(),
            event_text3: Texts3::empty(),
            event_text4: Texts3::empty(),
            event_text5: Texts3::empty(),
            event_text6: Texts3::empty(),
            event_text7: Texts3::empty(),
            event_text8: Texts3::empty(),
            event_text9: Texts3::empty(),
        });
    }
    out
}

// ---------------------------------------------------------------- drift vs the HMI (FAULT / WARN)

#[derive(Clone, Debug, Default, Serialize)]
pub struct DriftItem {
    pub unit: String,
    pub area: String,
    pub byte: u32,
    pub bit: u8,
    pub code: String,
    pub kind: &'static str,
    pub errorlist: String,
    pub hmi: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Drift {
    /// unit/area → kind → count
    pub counts: BTreeMap<String, BTreeMap<&'static str, usize>>,
    pub items: Vec<DriftItem>,
}

fn html_text(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    one_line(&out.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&#39;", "'").replace("&nbsp;", " ").replace("&amp;", "&"))
}

fn norm(s: &str) -> String {
    one_line(s)
}

/// `(1351PLC01)` → `(2351PLC01)`: device tags of the GR1 panel as the GR2 panel names them.
fn gr2_tags(s: &str) -> String {
    let b: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, c) in b.iter().enumerate() {
        let tag =
            *c == '1' && i > 0 && matches!(b[i - 1], '(' | ' ' | ',') && b.get(i + 1..i + 4).is_some_and(|d| d.iter().all(char::is_ascii_digit)) && b.get(i + 4).is_some_and(char::is_ascii_uppercase);
        out.push(if tag { '2' } else { *c });
    }
    out
}

/// HMI discrete alarms of classes `<unit> FAULT|WARN` vs the ErrorList Alarm / Warn rows at the same bit.
pub fn drift(files: &BTreeMap<String, ErrorListFile>, discrete: &[serde_json::Value]) -> Drift {
    let mut d = Drift::default();
    let mut hmi_at: HashMap<(String, &'static str, u32, u8), &serde_json::Value> = HashMap::new();
    for a in discrete {
        let class = a.get("AlarmClass").and_then(|v| v.as_str()).unwrap_or("");
        let Some((unit, area)) = class.split_once(' ') else { continue };
        let area: &'static str = match area {
            "FAULT" => "FAULT",
            "WARN" => "WARN",
            _ => continue,
        };
        let tag = a.get("TriggerBitAddress").and_then(|v| v.as_str()).unwrap_or("");
        let Some((idx, bit)) = tag.split_once('[').and_then(|(_, r)| r.split_once("].x")).and_then(|(i, b)| Some((i.parse::<u32>().ok()?, b.parse::<u8>().ok()?))) else { continue };
        hmi_at.insert((unit.to_string(), area, idx, bit), a);
    }
    let lang = |a: &serde_json::Value, k: &str| a.get(k).and_then(|t| t.get("en-US")).and_then(|v| v.as_str()).map(html_text).unwrap_or_default();
    let mut matched: HashSet<(String, &'static str, u32, u8)> = HashSet::new();
    for (plc, f) in files {
        let unit = unit_of(plc).to_string();
        for e in f.entries.iter().filter(|e| matches!(e.level(), Level::Alarm | Level::Warn)) {
            let (Some(byte), Some(bit)) = (e.byte, e.bit) else { continue };
            let area: &'static str = if e.level() == Level::Alarm { "FAULT" } else { "WARN" };
            let key = (unit.clone(), area, byte, bit);
            let mut push = |kind: &'static str, el: String, hmi: String| {
                *d.counts.entry(format!("{unit} {area}")).or_default().entry(kind).or_default() += 1;
                d.items.push(DriftItem { unit: unit.clone(), area: area.into(), byte, bit, code: e.code.clone(), kind, errorlist: el, hmi });
            };
            let Some(a) = hmi_at.get(&key) else {
                if !e.program.starts_with("미사용") {
                    push("missing_in_hmi", e.hmi_text.clone(), String::new());
                }
                continue;
            };
            matched.insert(key);
            let text = lang(a, "EventText");
            let want = norm(&e.hmi_text);
            let hmi_code = text.strip_prefix('[').and_then(|r| r.split_once(']')).map(|(c, _)| c.to_string()).unwrap_or_default();
            if hmi_code.is_empty() {
                push("no_code_prefix", want.clone(), text.clone());
            } else if hmi_code != e.code {
                push("code", want.clone(), text.clone());
            } else if text != want {
                // the robot sheet is shared: GR2 panels carry 2xxx device tags where the sheet says 1xxx
                let kind = if unit == "GR2" && gr2_tags(&want) == text { "device_tag_only" } else { "text" };
                push(kind, want.clone(), text.clone());
            }
            let cause = lang(a, "EventText1");
            if !e.cause_en.is_empty() && cause != norm(&e.cause_en) {
                push("cause", norm(&e.cause_en), cause);
            }
            let action = lang(a, "EventText2");
            if !e.action_en.is_empty() && action != norm(&e.action_en) {
                push("action", norm(&e.action_en), action);
            }
        }
    }
    let units: HashSet<String> = files.keys().map(|p| unit_of(p).to_string()).collect();
    let mut rest: Vec<_> = hmi_at.iter().filter(|(k, _)| units.contains(&k.0) && !matched.contains(*k)).collect();
    rest.sort_by(|a, b| a.0.cmp(b.0));
    for ((unit, area, byte, bit), a) in rest {
        *d.counts.entry(format!("{unit} {area}")).or_default().entry("hmi_only").or_default() += 1;
        d.items.push(DriftItem { unit: unit.clone(), area: area.to_string(), byte: *byte, bit: *bit, code: String::new(), kind: "hmi_only", errorlist: String::new(), hmi: lang(a, "EventText") });
    }
    d
}

fn load_discrete(export: &Path) -> Option<(PathBuf, Vec<serde_json::Value>)> {
    let rd = std::fs::read_dir(export).ok()?;
    let mut dirs: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.join("alarms").join("discrete.json").is_file()).collect();
    dirs.sort();
    let p = dirs.first()?.join("alarms").join("discrete.json");
    let text = std::fs::read_to_string(&p).ok()?;
    let v = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    Some((p, v))
}

// ---------------------------------------------------------------- run

fn pretty<T: Serialize>(v: &T) -> anyhow::Result<String> {
    let mut s = serde_json::to_string_pretty(v)?;
    s.push('\n');
    Ok(s)
}

pub fn run(a: &Args) -> anyhow::Result<()> {
    let sheets = read_workbook(&a.xlsx)?;
    let source = a.xlsx.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let inp = input_from(&sheets, &source)?;
    let mut alarms = HashMap::new();
    for plc in PLCS {
        if let Ok(t) = std::fs::read_to_string(a.contract.join(plc).join("alarms.json"))
            && let Ok(t) = evt_catalog::AlarmTable::parse(&t)
        {
            alarms.insert(plc.to_string(), t);
        }
    }
    let b = build(&inp, &alarms);
    println!("{source}: {} coded rows without text (spares) skipped; fault level from {:?}", b.spares, b.level_src);
    let mut outputs: Vec<(PathBuf, String)> = Vec::new();
    for (plc, f) in &b.files {
        println!("{plc}: {}", f.counts.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", "));
        outputs.push((a.contract.join(plc).join("errorlist.json"), pretty(f)?));
        for (stem, list) in drafts(plc, f) {
            outputs.push((a.hmi_out.join(format!("{stem}.json")), pretty(&list)?));
        }
    }
    for n in &b.notes {
        println!("  note: {n}");
    }
    for e in &b.errors {
        println!("  error: {} {} row {}: {}", e.plc, e.code, e.row, e.what);
    }
    let mut ids = HashSet::new();
    for f in b.files.values() {
        for e in &f.entries {
            if e.hmi && e.byte.is_some() && matches!(e.level(), Level::Operator | Level::Info) {
                anyhow::ensure!(ids.insert(draft_id(unit_of(&f.plc), e.level(), e.num)), "draft id collision at {} {}", f.plc, e.code);
            }
        }
    }

    if a.check {
        let mut differ = Vec::new();
        for (p, text) in &outputs {
            let old = std::fs::read_to_string(p).unwrap_or_default();
            if old.replace("\r\n", "\n") != *text {
                differ.push(p.display().to_string());
            }
        }
        match load_discrete(&a.export) {
            Some((path, list)) => {
                let d = drift(&b.files, &list);
                println!("drift vs {} ({} alarms):", path.display(), list.len());
                for (k, m) in &d.counts {
                    println!("  {k:<10} {}", m.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", "));
                }
                let mut shown: HashMap<&str, usize> = HashMap::new();
                for it in &d.items {
                    let n = shown.entry(it.kind).or_default();
                    if *n < 3 {
                        println!("  e.g. {} {} {}.{} {} [{}]\n       ErrorList: {}\n       HMI      : {}", it.kind, it.unit, it.byte, it.bit, it.code, it.area, it.errorlist, it.hmi);
                    }
                    *n += 1;
                }
                if let Some(p) = &a.drift_json {
                    std::fs::write(p, pretty(&d)?).with_context(|| format!("write {}", p.display()))?;
                    println!("drift report -> {}", p.display());
                }
            }
            None => println!("note: no <export>/*/alarms/discrete.json under {} — drift not checked", a.export.display()),
        }
        if !differ.is_empty() {
            for p in &differ {
                println!("differs: {p}");
            }
            anyhow::bail!("{} generated files differ (run without --check)", differ.len());
        }
        anyhow::ensure!(b.errors.is_empty(), "{} ErrorList errors", b.errors.len());
        println!("up to date");
        return Ok(());
    }
    anyhow::ensure!(b.errors.is_empty(), "{} ErrorList errors — nothing written", b.errors.len());
    // drafts of a unit/class that no longer exist are removed, the rest rewritten
    if a.hmi_out.is_dir() {
        let keep: HashSet<PathBuf> = outputs.iter().map(|(p, _)| p.clone()).collect();
        for e in std::fs::read_dir(&a.hmi_out)?.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json") && !keep.contains(&p) && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(['g', 'G'])) {
                std::fs::remove_file(&p)?;
            }
        }
    }
    for (p, text) in &outputs {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(p, text).with_context(|| format!("write {}", p.display()))?;
        println!("  -> {}", p.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_xlsxwriter::{Formula, Workbook};

    /// A tiny workbook with the real sheet names and header rows (codes as cached formula results, like the file).
    fn fixture() -> Vec<u8> {
        let mut wb = Workbook::new();
        let fault_head = [
            "",
            "ID",
            "Group",
            "sub",
            "이상내역",
            "HMI Code",
            "이상내역 (영문) - FAULT",
            "이상내역 (한국어)",
            "발생 원인",
            "조치방법",
            "HMI Alarm\nTrigger Byte",
            "HMI Alarm\nTrigger Bit",
            "",
            "발생 원인 (EN)\nCause",
            "조치방법 (EN)\nAction",
            "프로그램 확인\n(V1.4.2)",
            "발생 조건 (프로그램)",
            "변경 내역",
        ];
        type FaultFix<'a> = (u32, &'a str, &'a str, &'a str, u32, u32, &'a str, &'a str);
        let fault = |ws: &mut rust_xlsxwriter::Worksheet, rows: &[FaultFix]| {
            ws.write_row(1, 0, fault_head).unwrap();
            for (row, code, en, ko, byte, bit, m, prog) in rows {
                let r = row - 1;
                ws.write_formula(r, 5, Formula::new("=TEXT(1,\"0000\")").set_result(*code)).unwrap();
                ws.write(r, 6, *en).unwrap();
                ws.write(r, 7, *ko).unwrap();
                ws.write(r, 8, "1. 원인\n2. 둘").unwrap();
                ws.write(r, 10, *byte).unwrap();
                ws.write(r, 11, *bit).unwrap();
                if !m.is_empty() {
                    ws.write(r, 12, *m).unwrap();
                }
                ws.write(r, 13, "1. cause").unwrap();
                ws.write(r, 15, *prog).unwrap();
                ws.write(r, 16, "조건 [FL_Alarm:52]").unwrap();
            }
        };
        let ws = wb.add_worksheet().set_name("GRM(지상반)Fault").unwrap();
        fault(
            ws,
            &[
                (3, "0101", "PLC CPU Fault", "PLC CPU 이상", 0, 0, "[F0101] PLC CPU Fault", ""),
                (323, "1101", "Station 01 - Measuring Error", "측정 오류", 40, 0, "[W1101] Station 01 - Measuring Error", "사용 (WARN, V1.6.1 신규)"),
            ],
        );
        let ws = wb.add_worksheet().set_name("GR1,2(기상반)Fault").unwrap();
        fault(
            ws,
            &[
                (3, "0101", "PLC CPU Fault", "PLC CPU 이상", 0, 0, "", "사용 (FAULT)"),
                (4, "0102", "", "", 0, 1, "", ""),
                (323, "1101", "X Axis - Lag Error", "X축 Lag", 40, 0, "", "사용 (FAULT+WARN)"),
                (597, "3119", "Station Interlock Timeout", "인터록 타임아웃", 74, 2, "", "사용 (FAULT, GR2 전용)"),
                (5, "0103", "Wrong Place", "틀린 자리", 9, 0, "", "사용 (FAULT)"),
            ],
        );
        let ev_head = [
            "구분",
            "ID",
            "Group",
            "sub",
            "항목",
            "Code",
            "문구 (영문)",
            "문구 (한국어)",
            "기록 방식",
            "HMI 표시\n(Y/N)",
            "Byte",
            "Bit",
            "HMI 문구",
            "값 A",
            "값 B",
            "선언 위치",
            "발생 조건",
            "변경 내역\n(V1.6.2 초안)",
            "HMI 클래스",
        ];
        type EvFix<'a> = (u32, &'a str, &'a str, &'a str, &'a str, Option<(u32, u32)>, &'a str, &'a str, &'a str);
        let events = |ws: &mut rust_xlsxwriter::Worksheet, rows: &[EvFix]| {
            ws.write(0, 0, "title").unwrap();
            ws.write_row(1, 0, ev_head).unwrap();
            for (row, item, code, en, kind, pos, a, change, class) in rows {
                let r = row - 1;
                ws.write(r, 4, *item).unwrap();
                ws.write(r, 5, *code).unwrap();
                ws.write(r, 6, *en).unwrap();
                ws.write(r, 7, if en.is_empty() { "" } else { "한글" }).unwrap();
                ws.write(r, 8, *kind).unwrap();
                ws.write(r, 9, if pos.is_some() { "Y" } else { "N" }).unwrap();
                if let Some((byte, bit)) = pos {
                    ws.write(r, 10, *byte).unwrap();
                    ws.write(r, 11, *bit).unwrap();
                    ws.write(r, 12, format!("[{code}] {en}")).unwrap();
                }
                ws.write(r, 13, *a).unwrap();
                ws.write(r, 17, *change).unwrap();
                ws.write(r, 18, *class).unwrap();
            }
        };
        let ws = wb.add_worksheet().set_name("GRM(지상반)Operator").unwrap();
        events(ws, &[(7, "Homing", "O0105", "X Axis - Homing", "순간형", Some((0, 4)), "시작 위치 (mm×10)", "", "OPERATOR")]);
        let ws = wb.add_worksheet().set_name("GRM(지상반)Info").unwrap();
        events(ws, &[(3, "Mode", "I0101", "Mode - BOOT", "순간형", Some((0, 0)), "", "", "INFO")]);
        let ws = wb.add_worksheet().set_name("GR1,2(기상반)Operator").unwrap();
        events(
            ws,
            &[
                (3, "Jog", "O0101", "X Axis - Jog", "상태형", Some((0, 0)), "시작 위치 (mm×10)", "기존 …\ncatalog : CMD_JOG (DEBUG, Src=1)", "OPERATOR"),
                (4, "spare", "O0102", "", "순간형", Some((0, 1)), "", "", ""),
            ],
        );
        let ws = wb.add_worksheet().set_name("GR1,2(기상반)Info").unwrap();
        events(
            ws,
            &[
                (67, "Task Accepted", "I0301", "Task - Accepted", "순간형", Some((8, 0)), "Cell.Id (Src = task type)", "catalog : TASK_ACCEPTED", "TASK"),
                (68, "[GR2] Learn", "I0302", "Gripper - Learn", "순간형", Some((8, 1)), "", "", "INFO"),
                (300, "Step", "I5101", "Step - Changed", "로그전용", None, "체류 ms", "신규\ncatalog : STEP_CHANGED", "INFO"),
            ],
        );
        let ws = wb.add_worksheet().set_name("알람 통합").unwrap();
        ws.write_row(1, 0, ["No", "Event (catalog)", "PLC", "대상 알람 코드", "알람 주소", "알람에 붙일 값 (SetAlarmV)", "현재 훅", "결정", "비고"]).unwrap();
        ws.write_row(2, 0, ["1", "ILK_TIMEOUT", "GR1, GR2", "F3119 (step 400 인터락)", "FAULT[74].X2", "A = step (400 / 600, 현재 Src), B = WorkId", "", "결정", ""]).unwrap();
        ws.write_row(3, 0, ["2", "DRV_LAG", "GR1, GR2", "F·W 1101", "", "현재 없음. 제안 : A = LagError (mm×10)", "", "결정", ""]).unwrap();
        wb.save_to_buffer().unwrap()
    }

    fn built() -> Built {
        // tests run in parallel: one file per call
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("gr-errorlist-{}-{n}.xlsx", std::process::id()));
        std::fs::write(&p, fixture()).unwrap();
        let sheets = read_workbook(&p).unwrap();
        std::fs::remove_file(&p).ok();
        let inp = input_from(&sheets, "fixture.xlsx").unwrap();
        build(&inp, &HashMap::new())
    }

    #[test]
    fn xlsx_rows_to_entries_per_plc() {
        let b = built();
        let codes = |plc: &str| b.files[plc].entries.iter().map(|e| e.code.as_str()).collect::<Vec<_>>();
        assert_eq!(codes("GR1_PLC"), vec!["F0101", "F0103", "F1101", "W1101", "O0101", "I0301", "I5101"]);
        assert_eq!(codes("GR2_PLC"), vec!["F0101", "F0103", "F1101", "F3119", "W1101", "O0101", "I0301", "I0302", "I5101"]);
        assert_eq!(codes("GRM_PLC"), vec!["F0101", "W1101", "O0105", "I0101"]);
        assert_eq!(b.spares, 2, "O0102 and F0102 have no text");
        let gr2 = &b.files["GR2_PLC"];
        let f = &gr2.entries[0];
        assert_eq!((f.num, f.area.as_str(), f.byte, f.bit, f.hmi_text.as_str(), f.at.as_str()), (101, "FAULT", Some(0), Some(0), "[F0101] PLC CPU Fault", "FL_Alarm:52"));
        assert_eq!((f.cause_ko.as_str(), f.cause_en.as_str()), ("1. 원인\n2. 둘", "1. cause"));
        let ilk = gr2.entries.iter().find(|e| e.code == "F3119").unwrap();
        assert_eq!((ilk.a_meaning.as_str(), ilk.b_meaning.as_str(), ilk.merged_event.as_deref()), ("step (400 / 600, 현재 Src)", "WorkId", Some("ILK_TIMEOUT")));
        let lag = gr2.entries.iter().filter(|e| e.num == 1101).map(|e| (e.code.as_str(), e.a_meaning.as_str())).collect::<Vec<_>>();
        assert_eq!(lag, vec![("F1101", "LagError (mm×10)"), ("W1101", "LagError (mm×10)")]);
        let jog = gr2.entries.iter().find(|e| e.code == "O0101").unwrap();
        assert_eq!((jog.kind, jog.catalog.as_deref(), jog.catalog_src, jog.hmi_class.as_str()), (Kind::State, Some("CMD_JOG"), Some(1), "OPERATOR"));
        let task = gr2.entries.iter().find(|e| e.code == "I0301").unwrap();
        assert_eq!((task.ty(), task.byte, task.bit), (evt_catalog::Ty::Task, Some(8), Some(0)));
        let log = gr2.entries.iter().find(|e| e.code == "I5101").unwrap();
        assert_eq!((log.kind, log.byte, log.hmi, log.catalog.as_deref()), (Kind::Log, None, false, Some("STEP_CHANGED")));
        assert_eq!(gr2.counts["Task"], 1);
        assert_eq!(b.files["GRM_PLC"].entries[1].level(), Level::Warn, "GRM: the [W…] HMI text decides");
        // F0103 sits at row 5 but claims byte 9: both rules report it; nothing else does
        let errs: Vec<(&str, &str)> = b.errors.iter().map(|e| (e.plc.as_str(), e.what.as_str())).collect();
        assert_eq!(errs.len(), 4, "{errs:?}");
        assert!(b.errors.iter().all(|e| e.code == "F0103"));
    }

    #[test]
    fn code_rules_and_parsers() {
        assert_eq!(expected_pos(Level::Alarm, 101), Some((0, 0)));
        assert_eq!(expected_pos(Level::Alarm, 3128), Some((75, 3)));
        assert_eq!(expected_pos(Level::Warn, 1101), Some((40, 0)));
        assert_eq!(expected_pos(Level::Alarm, 8001), Some((220, 0)));
        assert_eq!(expected_pos(Level::Operator, 518), Some((18, 1)));
        assert_eq!(expected_pos(Level::Info, 902), Some((32, 1)));
        assert_eq!(expected_pos(Level::Alarm, 1501), None, "group 15 has no base");
        assert_eq!(parse_codes("F3118 (step 600), F3119 (step 400)"), vec![(Level::Alarm, 3118), (Level::Alarm, 3119)]);
        assert_eq!(parse_codes("F·W 1001 / 2001"), vec![(Level::Alarm, 1001), (Level::Alarm, 2001), (Level::Warn, 1001), (Level::Warn, 2001)]);
        assert_eq!(parse_codes("F1010~1013, F2010/2011").len(), 6);
        assert_eq!(parse_codes("F6201~F6203 (스텝)").len(), 3);
        assert_eq!(parse_codes("GR F0209 (Door)"), vec![(Level::Alarm, 209)]);
        assert_eq!(parse_codes("모든 FAULT (FaultCode[0])"), vec![]);
        assert_eq!(parse_values("A = MainCode, B = SubCode"), ("MainCode".into(), "SubCode".into()));
        assert_eq!(gr2_tags("MC Off (1302MC01), 1101MCCB03 at 1000 mm"), "MC Off (2302MC01), 2101MCCB03 at 1000 mm");
        assert_eq!(parse_values("현재 없음 (A = 코드). 제안 : A = 상대 X (mm×10), B = 내 X (mm×10)"), ("상대 X (mm×10)".into(), "내 X (mm×10)".into()));
        assert_eq!(parse_values("B = Skew.SyncDiff (mm×10)"), (String::new(), "Skew.SyncDiff (mm×10)".into()));
        assert_eq!(parse_values("없음 (Src = 축은 알람 코드에 이미 포함)"), (String::new(), String::new()));
        assert_eq!(catalog_ref("x\ncatalog : CMD_FUNCTION (Src=5, 미사용)"), Some(("CMD_FUNCTION".into(), Some(5))));
        assert_eq!(catalog_ref("기존 로그전용 → HMI 표시"), None);
    }

    #[test]
    fn hmi_drafts_and_drift() {
        let b = built();
        let d = drafts("GR2_PLC", &b.files["GR2_PLC"]);
        assert_eq!(d.keys().collect::<Vec<_>>(), vec!["gr2-info", "gr2-operator", "gr2-task"]);
        let v = serde_json::to_value(&d["gr2-task"][0]).unwrap();
        assert_eq!(v["AlarmClass"], "GR2 TASK");
        assert_eq!(v["Id"], 240301);
        assert_eq!(v["RaisedStateTag"], "GR2_Alarm_Info[8]");
        assert_eq!(v["TriggerBitAddress"], "HMI.Alarm.Info[8].x0");
        assert_eq!(v["EventText"]["en-US"], "<body><p>[I0301] Task - Accepted</p></body>");
        assert_eq!(v["EventText1"]["en-US"], "<body><p>A: Cell.Id (Src = task type)</p></body>");
        assert!(v.get("AlarmParameterTags").is_none());
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys.len(), 24);
        let grm = drafts("GRM_PLC", &b.files["GRM_PLC"]);
        assert_eq!(serde_json::to_value(&grm["grm-operator"][0]).unwrap()["RaisedStateTag"], "HMI_Alarm_Event[0]");
        assert_eq!(draft_id("GRM", Level::Info, 101), 260101);
        // drift: same text, other text, missing, HMI-only
        let hmi = |class: &str, addr: &str, text: &str, cause: &str| serde_json::json!({ "AlarmClass": class, "TriggerBitAddress": addr, "EventText": { "en-US": format!("<body><p>{text}</p></body>") }, "EventText1": { "en-US": format!("<body><p>{cause}</p></body>") } });
        let list = vec![
            hmi("GRM FAULT", "HMI.Alarm.Fault[0].x0", "[F0101] PLC CPU Fault", "1. cause"),
            hmi("GRM WARN", "HMI.Alarm.Warn[40].x0", "[W1101] Station 01 - Measuring Err", "1. cause"),
            hmi("GRM FAULT", "HMI.Alarm.Fault[9].x1", "[F0999] Old", ""),
            hmi("GRM EVENT", "HMI.Alarm.Event[0].x0", "ignored", ""),
        ];
        let grm_only: BTreeMap<String, ErrorListFile> = [("GRM_PLC".to_string(), b.files["GRM_PLC"].clone())].into_iter().collect();
        let dr = drift(&grm_only, &list);
        let kinds: Vec<(&str, &str)> = dr.items.iter().map(|i| (i.code.as_str(), i.kind)).collect();
        assert_eq!(kinds, vec![("W1101", "text"), ("", "hmi_only")]);
        assert_eq!(dr.counts["GRM WARN"]["text"], 1);
    }
}
