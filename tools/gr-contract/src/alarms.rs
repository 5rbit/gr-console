//! `gr-contract alarms`: `plc/contract/<PLC>/alarms.json` — (alarm area, bit index) → alarm code, class and texts.
//!
//! Truth rules
//! * code ↔ bit: literal alarm calls in the PLC SCL, e.g. `"SetAlarm"(Class := "ALARM_FAULT", Code := 3119, …,
//!   Alarm := "ALARM".FAULT[74].%X2)` (any call with an `Alarm` bit argument and a `Code`/`FaultCode`/`FaultID`
//!   argument, so `"isProcTimeout"(FaultCode := 6001, Alarm := …)` counts too). Commented-out calls and calls in
//!   blocks not reachable from an OB (old `_V2`/`_V3` copies still in the project) are ignored.
//!   Two different codes on one bit → conflict, the first by file order wins.
//! * area arrays: `Array[..] of Byte` members named FAULT / WARN / EVENT / TASK of the alarm DBs (robot PLCs: one DB
//!   `ALARM`; GRM: DBs `FAULT`, `WARN`, `EVENT`). Bit index = byte * 8 + bit.
//! * texts: the Unified HMI discrete alarms (`export/<HMI>/alarms/discrete.json`). Raised tag → HMI tag table →
//!   (PLC, PLC tag `HMI.Alarm.Fault`) → PLC area via the PLC's copy statement `"HMI".Alarm.Fault := "ALARM".FAULT;`.
//!   The HMI text carries the code as `[F0101] …`; an HMI code that differs from the SCL code is a conflict (SCL wins).
//!   Bits the SCL does not set by a literal call (direct writes, EVENT / TASK) come from the HMI alone.
//! * Korean texts (the HMI has none) and missing English texts: the ErrorList workbook, by code.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};

#[derive(clap::Args)]
pub struct Args {
    /// PLC folder name (GR1_PLC / GR2_PLC / GRM_PLC); omit for all three
    #[arg(long)]
    pub plc: Option<String>,
    /// TIA export root
    #[arg(long, default_value = "../siemens/export")]
    pub export: PathBuf,
    /// Contract root (output goes to <contract>/<PLC>/alarms.json)
    #[arg(long, default_value = "plc/contract")]
    pub contract: PathBuf,
    /// ErrorList workbook (code → Korean / English text); skipped with a note when missing
    #[arg(long, default_value = "../siemens/E13398_GR_V1.5.2_ErrorList_260926.xlsx")]
    pub xlsx: PathBuf,
}

const PLCS: [&str; 3] = ["GR1_PLC", "GR2_PLC", "GRM_PLC"];
const CODE_ARGS: [&str; 7] = ["code", "faultcode", "faultid", "warncode", "warnid", "alarmcode", "alarmid"];

pub fn run(a: &Args) -> anyhow::Result<()> {
    let plcs: Vec<String> = match &a.plc {
        Some(p) => vec![p.clone()],
        None => PLCS.iter().map(|s| s.to_string()).collect(),
    };
    let hmi = load_hmi(&a.export)?;
    for n in &hmi.notes {
        println!("note: {n}");
    }
    let el = if a.xlsx.is_file() {
        Some(ErrorList::load(&a.xlsx)?)
    } else {
        println!("note: ErrorList {} not found, Korean texts skipped", a.xlsx.display());
        None
    };
    for plc in &plcs {
        let input = PlcInput::load(&a.export, plc)?;
        let sheet = el.as_ref().and_then(|e| e.sheet_for(plc));
        let hmi_plc: Vec<&HmiAlarm> = hmi.alarms.iter().filter(|h| h.plc.eq_ignore_ascii_case(plc)).collect();
        let mut sources = input.sources.clone();
        if !hmi_plc.is_empty() {
            sources.extend(hmi.sources.iter().cloned());
        }
        if let (Some(e), Some(s)) = (&el, sheet) {
            sources.push(format!("{}#{}", e.file_name, s.name));
        }
        let out = build(plc, &input, &hmi_plc, sheet, sources);
        let dir = a.contract.join(plc);
        std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        let path = dir.join("alarms.json");
        let mut json = serde_json::to_string_pretty(&out.file)?;
        json.push('\n');
        std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
        println!("{plc}: {} entries, {} conflicts -> {}", out.file.entries.len(), out.file.conflicts.len(), path.display());
        for (area, c) in &out.file.coverage.areas {
            println!(
                "  {:<5} scl {:>4}  hmi-only {:>4}  code {:>4}  text {:>4} (en: hmi {}, errorlist {}; ko {})",
                area.name(),
                c.scl_bits,
                c.hmi_only,
                c.bits_with_code,
                c.with_text,
                c.text_en_hmi,
                c.text_en_errorlist,
                c.text_ko
            );
        }
        for n in input.notes.iter().chain(&out.notes) {
            println!("  note: {n}");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- areas

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Area {
    Fault,
    Warn,
    Event,
    Task,
}

impl Area {
    fn name(self) -> &'static str {
        match self {
            Area::Fault => "FAULT",
            Area::Warn => "WARN",
            Area::Event => "EVENT",
            Area::Task => "TASK",
        }
    }
    fn class(self) -> &'static str {
        match self {
            Area::Fault => "Fault",
            Area::Warn => "Warning",
            Area::Event => "Event",
            Area::Task => "Task",
        }
    }
    fn from_name(s: &str) -> Option<Area> {
        match s.trim().to_ascii_uppercase().as_str() {
            "FAULT" => Some(Area::Fault),
            "WARN" | "WARNING" => Some(Area::Warn),
            "EVENT" => Some(Area::Event),
            "TASK" => Some(Area::Task),
            _ => None,
        }
    }
    /// `"ALARM_FAULT"` / `ALARM_WARN` / `1`..`4` (SetAlarm `Class`).
    fn from_class_expr(e: &str) -> Option<Area> {
        let s = e.trim().trim_matches('"');
        match s {
            "1" => return Some(Area::Fault),
            "2" => return Some(Area::Warn),
            "3" => return Some(Area::Event),
            "4" => return Some(Area::Task),
            _ => {}
        }
        s.to_ascii_uppercase().strip_prefix("ALARM_").and_then(Area::from_name)
    }
}

/// One `Array[lo..hi] of Byte` area member of an alarm DB.
#[derive(Clone, Debug)]
struct AreaArray {
    db: String,
    member: String,
    area: Area,
    lo: u32,
    hi: u32,
}

/// `DATA_BLOCK "ALARM" … FAULT : Array[0..399] of Byte; …` → area arrays (only DBs named ALARM / FAULT / WARN / EVENT / TASK);
/// a non-Byte area member is skipped with a note.
fn parse_area_db(text: &str) -> (Vec<AreaArray>, Vec<String>) {
    let mut out = Vec::new();
    let mut notes = Vec::new();
    let Some(db) = text.lines().find_map(|l| l.trim_start_matches('\u{feff}').trim().strip_prefix("DATA_BLOCK")).map(|r| r.trim().trim_matches('"').to_string()) else {
        return (out, notes);
    };
    if Area::from_name(&db).is_none() && !db.eq_ignore_ascii_case("ALARM") {
        return (out, notes);
    }
    for line in text.lines() {
        let Some((name, ty)) = line.split_once(':') else { continue };
        let name = name.trim().trim_matches('"');
        let Some(area) = Area::from_name(name) else { continue };
        let ty = ty.trim().trim_end_matches(';').trim();
        let Some(rest) = ty.strip_prefix("Array[").or_else(|| ty.strip_prefix("Array [")) else { continue };
        let Some((bounds, elem)) = rest.split_once(']') else { continue };
        let elem = elem.trim().strip_prefix("of").unwrap_or(elem).trim();
        let Some((lo, hi)) = bounds.split_once("..") else { continue };
        let (Ok(lo), Ok(hi)) = (lo.trim().parse::<u32>(), hi.trim().parse::<u32>()) else { continue };
        if !elem.eq_ignore_ascii_case("Byte") {
            notes.push(format!("{db}.{name} is Array of {elem}, not Byte: skipped (bit index = byte*8+bit needs Byte)"));
            continue;
        }
        out.push(AreaArray { db: db.clone(), member: name.to_string(), area, lo, hi });
    }
    (out, notes)
}

// ---------------------------------------------------------------- SCL scan

/// Blanks comments and string contents (keeps quoted identifiers, byte length and line breaks).
fn strip_code(text: &str) -> String {
    let b = text.as_bytes();
    let n = b.len();
    let blank = |c: u8| if c == b'\n' || c == b'\r' { c } else { b' ' };
    let mut out = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        let c = b[i];
        if c == b'/' && i + 1 < n && b[i + 1] == b'/' {
            while i < n && b[i] != b'\n' && b[i] != b'\r' {
                out.push(b' ');
                i += 1;
            }
        } else if c == b'(' && i + 1 < n && b[i + 1] == b'*' {
            out.extend_from_slice(b"  ");
            i += 2;
            while i < n && !(b[i] == b'*' && i + 1 < n && b[i + 1] == b')') {
                out.push(blank(b[i]));
                i += 1;
            }
            if i < n {
                out.extend_from_slice(b"  ");
                i += 2;
            }
        } else if c == b'\'' {
            out.push(c);
            i += 1;
            while i < n && b[i] != b'\'' {
                if b[i] == b'$' && i + 1 < n {
                    out.push(b' ');
                    i += 1;
                }
                out.push(blank(b[i]));
                i += 1;
            }
            if i < n {
                out.push(b'\'');
                i += 1;
            }
        } else if c == b'"' {
            out.push(c);
            i += 1;
            while i < n && b[i] != b'"' && b[i] != b'\n' && b[i] != b'\r' {
                out.push(b[i]);
                i += 1;
            }
            if i < n && b[i] == b'"' {
                out.push(b'"');
                i += 1;
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn match_paren(b: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (i, &c) in b.iter().enumerate().skip(open) {
        match c {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Splits call arguments at depth-0 commas → `(lowercase name, expr)`; positional args have an empty name.
fn split_args(s: &str) -> Vec<(String, String)> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
        .into_iter()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once(":=") {
            Some((name, expr)) => {
                let name = name.trim().trim_matches('"');
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') { (name.to_ascii_lowercase(), expr.trim().to_string()) } else { (String::new(), p.to_string()) }
            }
            None => (String::new(), p.to_string()),
        })
        .collect()
}

/// Name of the callee right before `(` (`"SetAlarm"`, `#inst`, `Foo`); empty for a plain parenthesis.
fn callee_before(b: &[u8], open: usize) -> String {
    let mut i = open;
    while i > 0 && (b[i - 1] == b' ' || b[i - 1] == b'\t' || b[i - 1] == b'\r' || b[i - 1] == b'\n') {
        i -= 1;
    }
    if i > 0 && b[i - 1] == b'"' {
        let end = i - 1;
        let mut j = end;
        while j > 0 && b[j - 1] != b'"' {
            j -= 1;
        }
        return String::from_utf8_lossy(&b[j..end]).into_owned();
    }
    let end = i;
    while i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_' || b[i - 1] == b'#') {
        i -= 1;
    }
    String::from_utf8_lossy(&b[i..end]).into_owned()
}

/// `"ALARM".FAULT[74].%X2` → (db, member, index, bit).
fn parse_bit_ref(e: &str) -> Option<(String, String, u32, u8)> {
    let s: String = e.chars().filter(|c| !c.is_whitespace()).collect();
    let rest = s.strip_prefix('"')?;
    let (db, rest) = rest.split_once('"')?;
    let rest = rest.strip_prefix('.')?;
    let (member, rest) = rest.split_once('[')?;
    let member = member.trim_matches('"');
    let (idx, rest) = rest.split_once(']')?;
    let idx: u32 = idx.parse().ok()?;
    let bit = rest.strip_prefix(".%X").or_else(|| rest.strip_prefix(".%x"))?;
    if bit.len() != 1 {
        return None;
    }
    let bit: u8 = bit.parse().ok().filter(|b| *b < 8)?;
    Some((db.to_string(), member.to_string(), idx, bit))
}

/// `3119`, `0102`, `UDINT#75000` → code; expressions → None.
fn parse_code(e: &str) -> Option<u32> {
    let s: String = e.chars().filter(|c| !c.is_whitespace()).collect();
    let digits = match s.rsplit_once('#') {
        Some((prefix, v)) if !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_alphabetic()) => v,
        Some(_) => return None,
        None => s.as_str(),
    };
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

#[derive(Clone, Debug)]
struct SclSite {
    area: Area,
    byte: u32,
    bit: u8,
    code: u32,
    class: Option<Area>,
    callee: String,
    at: String,
    block: String,
    /// block reachable from an OB (dead copies such as `CL_AntiCollision_V2` do not decide a bit)
    live: bool,
}

#[derive(Clone, Debug)]
struct Unresolved {
    at: String,
    callee: String,
    code: String,
    alarm: String,
}

#[derive(Default, Debug)]
struct SclScan {
    sites: Vec<SclSite>,
    unresolved: Vec<Unresolved>,
    /// HMI-side copy target (`HMI.Alarm.Fault`) → area, from `"HMI".Alarm.Fault := "ALARM".FAULT;`
    copies: Vec<(String, Area)>,
    /// literal bits outside alarm calls (`"ALARM".WARN[220].%X0 := …`)
    direct: Vec<(Area, u32, u8, String)>,
}

fn find_array<'a>(arrays: &'a [AreaArray], db: &str, member: &str) -> Option<&'a AreaArray> {
    arrays.iter().find(|a| a.db.eq_ignore_ascii_case(db) && a.member.eq_ignore_ascii_case(member))
}

fn line_of(starts: &[usize], off: usize) -> usize {
    match starts.binary_search(&off) {
        Ok(i) => i + 1,
        Err(i) => i,
    }
}

fn scan_scl(text: &str, rel: &str, arrays: &[AreaArray], live: &dyn Fn(&str) -> bool, scan: &mut SclScan) {
    let code = strip_code(text);
    let block = scl_block_info(&code).map(|b| b.name).unwrap_or_else(|| rel.rsplit('/').next().unwrap_or(rel).trim_end_matches(".scl").to_string());
    let is_live = live(&block);
    let b = code.as_bytes();
    let mut starts = vec![0usize];
    starts.extend(b.iter().enumerate().filter(|(_, c)| **c == b'\n').map(|(i, _)| i + 1));
    let at = |off: usize| format!("{rel}:{}", line_of(&starts, off));

    for open in b.iter().enumerate().filter(|(_, c)| **c == b'(').map(|(i, _)| i) {
        let Some(close) = match_paren(b, open) else { continue };
        let args = split_args(&code[open + 1..close]);
        let Some((_, alarm)) = args.iter().find(|(n, _)| n == "alarm") else { continue };
        let code_arg = args.iter().find(|(n, _)| CODE_ARGS.contains(&n.as_str())).map(|(_, e)| e.as_str());
        let class = args.iter().find(|(n, _)| n == "class").and_then(|(_, e)| Area::from_class_expr(e));
        let callee = callee_before(b, open);
        let arr = parse_bit_ref(alarm).and_then(|(db, member, idx, bit)| find_array(arrays, &db, &member).map(|a| (a.area, idx, bit)));
        match (arr, code_arg.and_then(parse_code)) {
            (Some((area, byte, bit)), Some(c)) => scan.sites.push(SclSite { area, byte, bit, code: c, class, callee, at: at(open), block: block.clone(), live: is_live }),
            _ if code_arg.is_some() => scan.unresolved.push(Unresolved {
                at: at(open),
                callee,
                code: code_arg.unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" "),
                alarm: alarm.split_whitespace().collect::<Vec<_>>().join(" "),
            }),
            _ => {}
        }
    }

    // statements: copies into the HMI arrays and direct bit writes
    let mut off = 0;
    for stmt in code.split(';') {
        let here = off;
        off += stmt.len() + 1;
        let Some((lhs, rhs)) = stmt.split_once(":=") else { continue };
        if rhs.contains(":=") {
            continue;
        }
        let lhs_tok = lhs.split_whitespace().last().unwrap_or("");
        let rhs = rhs.trim();
        if let Some((db, member)) = rhs.split_once('.').map(|(d, m)| (d.trim_matches('"'), m.trim_matches('"')))
            && rhs.starts_with('"')
            && !member.contains(['.', '['])
            && let Some(a) = find_array(arrays, db, member)
        {
            let target: String = lhs_tok.chars().filter(|c| *c != '"').collect();
            if !target.is_empty() {
                scan.copies.push((target, a.area));
            }
        }
        if let Some((db, member, idx, bit)) = parse_bit_ref(lhs_tok)
            && let Some(a) = find_array(arrays, &db, &member)
        {
            let pos = here + stmt.find(lhs_tok).unwrap_or(0);
            scan.direct.push((a.area, idx, bit, at(pos)));
        }
    }
}

// ---------------------------------------------------------------- reachability (OB → callees)

#[derive(Clone, Debug, Default)]
struct BlockInfo {
    name: String,
    is_ob: bool,
    callees: Vec<String>,
    instance_of: Option<String>,
}

/// Header name + callees of a stripped SCL block: every `"X"(` call and every declared `: "X"` / `of "X"` type
/// (multi-instances; over-approximates, which only keeps more blocks alive).
fn scl_block_info(code: &str) -> Option<BlockInfo> {
    let mut info = None;
    for line in code.lines() {
        let t = line.trim_start_matches('\u{feff}').trim_start();
        let Some((kw, rest)) =
            ["ORGANIZATION_BLOCK", "FUNCTION_BLOCK", "FUNCTION", "DATA_BLOCK", "TYPE"].iter().find_map(|k| t.strip_prefix(k).filter(|r| r.starts_with([' ', '\t'])).map(|r| (*k, r.trim_start())))
        else {
            continue;
        };
        let name = match rest.strip_prefix('"') {
            Some(r) => r.split('"').next().unwrap_or("").to_string(),
            None => rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect(),
        };
        info = Some(BlockInfo { name, is_ob: kw == "ORGANIZATION_BLOCK", ..Default::default() });
        break;
    }
    let mut info = info?;
    let b = code.as_bytes();
    let begin = code.lines().scan(0usize, |off, l| {
        let here = *off;
        *off += l.len() + 1;
        Some((here, l))
    });
    let decl_end = begin.filter(|(_, l)| l.trim() == "BEGIN").map(|(o, _)| o).next().unwrap_or(code.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'"' {
            i += 1;
            continue;
        }
        let start = i;
        let Some(len) = b[i + 1..].iter().position(|c| *c == b'"' || *c == b'\n') else { break };
        let end = i + 1 + len;
        if b[end] != b'"' {
            i = end + 1;
            continue;
        }
        let name = &code[start + 1..end];
        i = end + 1;
        let mut k = i;
        while k < b.len() && b[k].is_ascii_whitespace() {
            k += 1;
        }
        let call = k < b.len() && b[k] == b'(';
        let typed = start < decl_end && {
            let before = code[..start].trim_end();
            before.ends_with(':') || before.to_ascii_lowercase().ends_with(" of")
        };
        if (call || typed) && !name.eq_ignore_ascii_case(&info.name) {
            info.callees.push(name.to_string());
        }
    }
    Some(info)
}

/// LAD/FBD/instance-DB SimaticML: `<Name>`, `SW.Blocks.OB`, `<CallInfo Name="X"`, `<InstanceOfName>`.
fn xml_block_info(text: &str) -> Option<BlockInfo> {
    let between = |s: &str, a: &str, b: &str| s.split_once(a).and_then(|(_, r)| r.split_once(b)).map(|(v, _)| v.to_string());
    let name = between(text, "<Name>", "</Name>")?;
    let callees = text.split("<CallInfo").skip(1).filter_map(|s| between(s, "Name=\"", "\"")).collect();
    Some(BlockInfo { name, is_ob: text.contains("<SW.Blocks.OB"), callees, instance_of: between(text, "<InstanceOfName>", "</InstanceOfName>") })
}

/// Lowercase names of the blocks reachable from any OB (instance DB calls resolve to their FB).
fn reachable(blocks: &[BlockInfo]) -> std::collections::HashSet<String> {
    let by: HashMap<String, &BlockInfo> = blocks.iter().map(|b| (b.name.to_ascii_lowercase(), b)).collect();
    let mut seen: std::collections::HashSet<String> = blocks.iter().filter(|b| b.is_ob).map(|b| b.name.to_ascii_lowercase()).collect();
    let mut queue: Vec<String> = seen.iter().cloned().collect();
    while let Some(n) = queue.pop() {
        let Some(b) = by.get(&n) else { continue };
        let next = b.callees.iter().map(|c| c.to_ascii_lowercase()).chain(b.instance_of.iter().map(|c| c.to_ascii_lowercase()));
        for c in next {
            let fb = by.get(&c).and_then(|x| x.instance_of.as_ref()).map(|f| f.to_ascii_lowercase());
            for t in std::iter::once(c).chain(fb) {
                if seen.insert(t.clone()) {
                    queue.push(t);
                }
            }
        }
    }
    seen
}

fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut items: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    items.sort();
    for p in items {
        if p.is_dir() {
            walk(&p, ext, out);
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)) {
            out.push(p);
        }
    }
}

fn rel_path(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

fn read_text(p: &Path) -> anyhow::Result<String> {
    let bytes = std::fs::read(p).with_context(|| format!("read {}", p.display()))?;
    let s = String::from_utf8_lossy(&bytes).into_owned();
    Ok(s.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(s))
}

struct PlcInput {
    arrays: Vec<AreaArray>,
    scan: SclScan,
    sources: Vec<String>,
    notes: Vec<String>,
}

impl PlcInput {
    fn load(export: &Path, plc: &str) -> anyhow::Result<PlcInput> {
        let blocks = export.join(plc).join("blocks");
        anyhow::ensure!(blocks.is_dir(), "{} not found", blocks.display());
        let mut sources = vec![format!("export/{plc}/blocks/**/*.scl")];
        let mut notes = Vec::new();
        let mut arrays = Vec::new();
        let mut dbs = Vec::new();
        walk(&blocks, "db", &mut dbs);
        for p in &dbs {
            let (found, n) = parse_area_db(&read_text(p)?);
            notes.extend(n);
            if !found.is_empty() {
                sources.push(format!("export/{}", rel_path(export, p)));
                arrays.extend(found);
            }
        }
        anyhow::ensure!(!arrays.is_empty(), "{plc}: no alarm area arrays (FAULT/WARN/EVENT/TASK : Array of Byte) in any DB");
        notes.push(format!("area arrays: {}", arrays.iter().map(|a| format!("\"{}\".{} Array[{}..{}] of Byte", a.db, a.member, a.lo, a.hi)).collect::<Vec<_>>().join(", ")));
        let mut scl = Vec::new();
        walk(&blocks, "scl", &mut scl);
        let texts: Vec<(String, String)> = scl.iter().map(|p| Ok((rel_path(export, p), read_text(p)?))).collect::<anyhow::Result<_>>()?;
        // call graph: SCL blocks + XML-only blocks (LAD/FBD OBs, instance DBs)
        let mut infos: Vec<BlockInfo> = texts.iter().filter_map(|(_, t)| scl_block_info(&strip_code(t))).collect();
        let mut xml = Vec::new();
        walk(&blocks, "xml", &mut xml);
        for p in xml.iter().filter(|p| !p.with_extension("scl").is_file()) {
            if let Some(i) = xml_block_info(&read_text(p)?) {
                infos.push(i);
            }
        }
        let live = reachable(&infos);
        let is_live = |b: &str| live.contains(&b.to_ascii_lowercase());
        let mut scan = SclScan::default();
        for (rel, text) in &texts {
            scan_scl(text, rel, &arrays, &is_live, &mut scan);
        }
        let mut dead: Vec<&str> = scan.sites.iter().filter(|s| !s.live).map(|s| s.block.as_str()).collect();
        dead.sort_unstable();
        dead.dedup();
        if !dead.is_empty() {
            let n = scan.sites.iter().filter(|s| !s.live).count();
            notes.push(format!("{n} alarm calls in {} blocks not reachable from an OB (ignored): {}", dead.len(), dead.join(", ")));
        }
        Ok(PlcInput { arrays, scan, sources, notes })
    }
}

// ---------------------------------------------------------------- HMI

#[derive(Clone, Debug)]
struct HmiAlarm {
    hmi: String,
    name: String,
    plc: String,
    plc_tag: String,
    index: u32,
    bit: u8,
    code: Option<u32>,
    text_en: String,
    text_ko: String,
    class: String,
}

#[derive(Default)]
struct HmiData {
    alarms: Vec<HmiAlarm>,
    sources: Vec<String>,
    notes: Vec<String>,
}

/// `<body><p>[F0101] PLC CPU Fault</p></body>` → plain text.
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
    let out = out.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&#39;", "'").replace("&nbsp;", " ").replace("&amp;", "&");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `[F0101] PLC CPU Fault` → (Some(101), "PLC CPU Fault").
fn split_code_prefix(s: &str) -> (Option<u32>, String) {
    if let Some(rest) = s.strip_prefix('[')
        && let Some((tag, text)) = rest.split_once(']')
        && tag.len() >= 2
        && tag.as_bytes()[0].is_ascii_alphabetic()
        && tag[1..].bytes().all(|c| c.is_ascii_digit())
        && let Ok(code) = tag[1..].parse()
    {
        return (Some(code), text.trim().to_string());
    }
    (None, s.trim().to_string())
}

fn lang_text(v: &serde_json::Value, lang: &str) -> String {
    v.get(lang).and_then(|t| t.as_str()).map(html_text).unwrap_or_default()
}

fn load_hmi(export: &Path) -> anyhow::Result<HmiData> {
    let mut data = HmiData::default();
    let Ok(rd) = std::fs::read_dir(export) else { return Ok(data) };
    let mut dirs: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.join("alarms").join("discrete.json").is_file()).collect();
    dirs.sort();
    for dir in dirs {
        let hmi = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // HMI tag name → (PLC, PLC tag)
        let mut tags: HashMap<String, (String, String)> = HashMap::new();
        let mut files = Vec::new();
        walk(&dir.join("tags"), "json", &mut files);
        for f in &files {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&read_text(f)?) else { continue };
            for t in v.get("Tags").and_then(|t| t.as_array()).into_iter().flatten() {
                let s = |k: &str| t.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
                if !s("PlcName").is_empty() {
                    tags.insert(s("Name"), (s("PlcName"), s("PlcTag")));
                }
            }
        }
        let path = dir.join("alarms").join("discrete.json");
        let list: Vec<serde_json::Value> = serde_json::from_str(&read_text(&path)?).with_context(|| format!("parse {}", path.display()))?;
        let mut skipped = 0;
        for a in &list {
            let s = |k: &str| a.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
            let raised = s("RaisedStateTag");
            let bit = a.get("RaisedStateTagBitNumber").and_then(|x| x.as_u64()).unwrap_or(0);
            let parsed = raised.split_once('[').and_then(|(n, r)| r.strip_suffix(']').and_then(|i| i.parse::<u32>().ok()).map(|i| (n.to_string(), i)));
            let Some((tag, index)) = parsed else {
                skipped += 1;
                continue;
            };
            let Some((plc, plc_tag)) = tags.get(&tag).cloned() else {
                skipped += 1;
                continue;
            };
            let Ok(bit) = u8::try_from(bit) else {
                skipped += 1;
                continue;
            };
            let text = a.get("EventText").cloned().unwrap_or_default();
            let (code, text_en) = split_code_prefix(&lang_text(&text, "en-US"));
            let (_, text_ko) = split_code_prefix(&lang_text(&text, "ko-KR"));
            data.alarms.push(HmiAlarm { hmi: hmi.clone(), name: s("Name"), plc, plc_tag, index, bit, code, text_en, text_ko, class: s("AlarmClass") });
        }
        if skipped > 0 {
            data.notes.push(format!("{hmi}: {skipped} discrete alarms without a PLC array tag (not mapped)"));
        }
        data.sources.push(format!("export/{hmi}/alarms/discrete.json"));
        data.sources.push(format!("export/{hmi}/tags/**/*.json"));
    }
    Ok(data)
}

// ---------------------------------------------------------------- ErrorList

#[derive(Clone, Debug, Default)]
struct ElRow {
    row: u32,
    code: u32,
    en: String,
    ko: String,
    byte: Option<u32>,
    bit: Option<u8>,
}

struct ElSheet {
    name: String,
    rows: Vec<ElRow>,
}

impl ElSheet {
    fn by_code(&self, code: u32) -> Option<&ElRow> {
        self.rows.iter().find(|r| r.code == code && (!r.en.is_empty() || !r.ko.is_empty())).or_else(|| self.rows.iter().find(|r| r.code == code))
    }
    fn by_bit(&self, byte: u32, bit: u8) -> Option<&ElRow> {
        self.rows.iter().find(|r| r.byte == Some(byte) && r.bit == Some(bit) && (!r.en.is_empty() || !r.ko.is_empty()))
    }
}

struct ErrorList {
    file_name: String,
    sheets: Vec<ElSheet>,
}

fn cell_str(d: &calamine::Data) -> String {
    use calamine::Data;
    match d {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Float(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", *f as i64),
        other => other.to_string(),
    }
}

fn one_line(s: &str) -> String {
    s.replace("_x000D_", " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

impl ErrorList {
    fn load(path: &Path) -> anyhow::Result<ErrorList> {
        use calamine::Reader;
        let mut wb = calamine::open_workbook_auto(path).with_context(|| format!("open {}", path.display()))?;
        let mut sheets = Vec::new();
        for name in wb.sheet_names() {
            let Ok(range) = wb.worksheet_range(&name) else { continue };
            let row0 = range.start().map(|s| s.0).unwrap_or(0);
            let grid: Vec<Vec<String>> = range.rows().map(|r| r.iter().map(cell_str).collect()).collect();
            if let Some(rows) = Self::parse_sheet(&grid, row0) {
                sheets.push(ElSheet { name, rows });
            }
        }
        let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(ErrorList { file_name, sheets })
    }

    /// Header row = the row with a `HMI Code` cell; columns by header text.
    fn parse_sheet(grid: &[Vec<String>], row0: u32) -> Option<Vec<ElRow>> {
        let (h, head) = grid.iter().enumerate().take(10).find(|(_, r)| r.iter().any(|c| c.trim() == "HMI Code"))?;
        let col = |pred: &dyn Fn(&str) -> bool| head.iter().position(|c| pred(&one_line(c)));
        let code_c = col(&|c| c == "HMI Code")?;
        let en_c = col(&|c| c.starts_with("이상내역 (영문)"));
        let ko_c = col(&|c| c.starts_with("이상내역 (한국어)"));
        let title_c = col(&|c| c == "이상내역");
        let byte_c = col(&|c| c.contains("Trigger Byte"));
        let bit_c = col(&|c| c.contains("Trigger Bit"));
        let get = |r: &Vec<String>, c: Option<usize>| c.and_then(|c| r.get(c)).map(|s| one_line(s)).unwrap_or_default();
        let mut rows = Vec::new();
        for (i, r) in grid.iter().enumerate().skip(h + 1) {
            let code = get(r, Some(code_c));
            if !(3..=5).contains(&code.len()) || !code.bytes().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let mut en = get(r, en_c);
            if en.is_empty() {
                en = get(r, title_c);
            }
            rows.push(ElRow { row: row0 + i as u32 + 1, code: code.parse().ok()?, en, ko: get(r, ko_c), byte: get(r, byte_c).parse().ok(), bit: get(r, bit_c).parse().ok() });
        }
        Some(rows)
    }

    /// GRM → the `GRM…Fault` sheet, robot PLCs → the shared `GR1,2…Fault` sheet.
    fn sheet_for(&self, plc: &str) -> Option<&ElSheet> {
        let key = if plc.to_ascii_uppercase().starts_with("GRM") { "GRM" } else { "GR1" };
        self.sheets.iter().find(|s| s.name.contains(key) && s.name.to_ascii_lowercase().contains("fault"))
    }
}

// ---------------------------------------------------------------- build

#[derive(Serialize, Debug, Clone)]
struct Entry {
    area: &'static str,
    bit: u32,
    byte: u32,
    bit_in_byte: u8,
    code: u32,
    class: String,
    text_en: String,
    text_ko: String,
    /// `scl` (literal alarm call) / `hmi` (HMI alarm list only)
    origin: &'static str,
    #[serde(skip_serializing_if = "str::is_empty")]
    en_src: &'static str,
    #[serde(skip_serializing_if = "str::is_empty")]
    ko_src: &'static str,
    #[serde(rename = "where", skip_serializing_if = "String::is_empty")]
    at: String,
}

#[derive(Serialize, Debug, Clone)]
struct Conflict {
    area: &'static str,
    bit: u32,
    /// `scl` (two codes in the SCL) / `hmi` (HMI text code ≠ SCL) / `errorlist` (ErrorList Byte/Bit row ≠ SCL)
    kind: &'static str,
    /// kept code first
    codes: Vec<u32>,
    #[serde(rename = "where")]
    at: Vec<String>,
}

#[derive(Serialize, Debug, Default, Clone)]
struct AreaCov {
    entries: usize,
    scl_bits: usize,
    hmi_only: usize,
    bits_with_code: usize,
    with_text: usize,
    text_en_hmi: usize,
    text_en_errorlist: usize,
    text_ko: usize,
}

#[derive(Debug, Default)]
struct Coverage {
    areas: Vec<(Area, AreaCov)>,
    conflicts: usize,
}

impl Serialize for Coverage {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(self.areas.len() + 1))?;
        for (a, c) in &self.areas {
            m.serialize_entry(a.name(), c)?;
        }
        m.serialize_entry("conflicts", &self.conflicts)?;
        m.end()
    }
}

#[derive(Serialize, Debug)]
struct AlarmsFile {
    plc: String,
    generated_by: &'static str,
    sources: Vec<String>,
    coverage: Coverage,
    entries: Vec<Entry>,
    conflicts: Vec<Conflict>,
}

struct Built {
    file: AlarmsFile,
    notes: Vec<String>,
}

fn hmi_class(h: &HmiAlarm, area: Area) -> String {
    h.class.split_whitespace().last().and_then(Area::from_name).unwrap_or(area).class().to_string()
}

fn build(plc: &str, input: &PlcInput, hmi: &[&HmiAlarm], el: Option<&ElSheet>, sources: Vec<String>) -> Built {
    let mut notes = Vec::new();
    let scan = &input.scan;
    let mut entries: BTreeMap<(Area, u32), Entry> = BTreeMap::new();
    let mut conflicts: BTreeMap<(Area, u32, &'static str), Conflict> = BTreeMap::new();
    let mut add_conflict = |area: Area, bit: u32, kind: &'static str, kept: (u32, &str), other: (u32, String)| {
        let c = conflicts.entry((area, bit, kind)).or_insert_with(|| Conflict { area: area.name(), bit, kind, codes: vec![kept.0], at: vec![kept.1.to_string()] });
        if !c.codes.contains(&other.0) {
            c.codes.push(other.0);
        }
        if !c.at.contains(&other.1) {
            c.at.push(other.1);
        }
    };

    // 1. SCL literal alarm calls
    let mut class_mismatch = 0;
    for s in scan.sites.iter().filter(|s| s.live) {
        let bit = s.byte * 8 + u32::from(s.bit);
        if let Some(arr) = input.arrays.iter().find(|a| a.area == s.area)
            && (s.byte < arr.lo || s.byte > arr.hi)
        {
            notes.push(format!("{} {}[{}].%X{} outside Array[{}..{}] ({})", s.callee, s.area.name(), s.byte, s.bit, arr.lo, arr.hi, s.at));
        }
        if s.class.is_some_and(|c| c != s.area) {
            class_mismatch += 1;
        }
        match entries.get(&(s.area, bit)) {
            Some(e) if e.code != s.code => {
                let kept = (e.code, e.at.clone());
                add_conflict(s.area, bit, "scl", (kept.0, &kept.1), (s.code, s.at.clone()));
            }
            Some(_) => {}
            None => {
                entries.insert(
                    (s.area, bit),
                    Entry {
                        area: s.area.name(),
                        bit,
                        byte: s.byte,
                        bit_in_byte: s.bit,
                        code: s.code,
                        class: s.class.unwrap_or(s.area).class().to_string(),
                        text_en: String::new(),
                        text_ko: String::new(),
                        origin: "scl",
                        en_src: "",
                        ko_src: "",
                        at: s.at.clone(),
                    },
                );
            }
        }
    }
    if class_mismatch > 0 {
        notes.push(format!("{class_mismatch} alarm calls whose Class differs from the array area (class kept from the call)"));
    }
    if !scan.unresolved.is_empty() {
        let list: Vec<String> = scan.unresolved.iter().map(|u| format!("{} {}(code {}, alarm {})", u.at, u.callee, u.code, u.alarm)).collect();
        notes.push(format!("{} alarm calls with a non-literal code or bit (HMI fills them if it can): {}", list.len(), list.join("; ")));
    }
    let direct_only: Vec<&(Area, u32, u8, String)> = scan.direct.iter().filter(|(a, byte, bit, _)| !entries.contains_key(&(*a, byte * 8 + u32::from(*bit)))).collect();
    if !direct_only.is_empty() {
        notes.push(format!("{} direct bit writes without an alarm call (code from the HMI only)", direct_only.len()));
    }

    // 2. HMI alarms of this PLC → (area, bit)
    let copy_area = |plc_tag: &str| -> (Option<Area>, bool) {
        let norm: String = plc_tag.chars().filter(|c| *c != '"').collect();
        if let Some((_, a)) = scan.copies.iter().find(|(t, _)| t.eq_ignore_ascii_case(&norm)) {
            return (Some(*a), false);
        }
        (norm.rsplit('.').next().and_then(Area::from_name), true)
    };
    let mut hmi_at: BTreeMap<(Area, u32), &HmiAlarm> = BTreeMap::new();
    let mut hmi_code: HashMap<(Area, u32), &HmiAlarm> = HashMap::new();
    let mut by_name: BTreeMap<String, (usize, bool)> = BTreeMap::new();
    let mut hmi_dups = 0;
    for h in hmi {
        let (area, fallback) = copy_area(&h.plc_tag);
        let Some(area) = area else {
            by_name.entry(format!("{} (no area)", h.plc_tag)).or_insert((0, true)).0 += 1;
            continue;
        };
        by_name.entry(format!("{} -> {}", h.plc_tag, area.name())).or_insert((0, fallback)).0 += 1;
        let bit = h.index * 8 + u32::from(h.bit);
        if hmi_at.insert((area, bit), h).is_some() {
            hmi_dups += 1;
        }
        if let Some(c) = h.code {
            hmi_code.entry((area, c)).or_insert(h);
        }
    }
    for (k, (n, fallback)) in &by_name {
        notes.push(format!("HMI {k}: {n} alarms{}", if *fallback { " (area by tag name; no copy statement found in the PLC)" } else { " (copy statement in the PLC)" }));
    }
    if hmi_dups > 0 {
        notes.push(format!("{hmi_dups} HMI alarms share a bit with another HMI alarm (first kept)"));
    }

    // 3. SCL vs HMI / ErrorList code on the same bit
    for ((area, bit), e) in &entries {
        if let Some(h) = hmi_at.get(&(*area, *bit))
            && let Some(hc) = h.code
            && hc != e.code
        {
            add_conflict(*area, *bit, "hmi", (e.code, &e.at), (hc, format!("{} '{}' [{}]", h.hmi, h.name, h.text_en)));
        }
        if matches!(area, Area::Fault | Area::Warn)
            && let Some(r) = el.and_then(|s| s.by_bit(e.byte, e.bit_in_byte))
            && r.code != e.code
        {
            add_conflict(*area, *bit, "errorlist", (e.code, &e.at), (r.code, format!("ErrorList row {} ({})", r.row, r.en)));
        }
    }

    // 4. HMI-only bits
    for ((area, bit), h) in &hmi_at {
        entries.entry((*area, *bit)).or_insert_with(|| Entry {
            area: area.name(),
            bit: *bit,
            byte: h.index,
            bit_in_byte: h.bit,
            code: h.code.unwrap_or(0),
            class: hmi_class(h, *area),
            text_en: String::new(),
            text_ko: String::new(),
            origin: "hmi",
            en_src: "",
            ko_src: "",
            at: String::new(),
        });
    }

    // 5. texts
    for ((area, bit), e) in entries.iter_mut() {
        let at_bit = hmi_at.get(&(*area, *bit)).copied();
        let row = if e.code != 0 { el.and_then(|s| s.by_code(e.code)) } else { None };
        let same = at_bit.filter(|h| h.code == Some(e.code) && e.code != 0);
        if let Some(h) = same.filter(|h| !h.text_en.is_empty()) {
            e.text_en = h.text_en.clone();
            e.en_src = "hmi";
        } else if let Some(r) = row.filter(|r| !r.en.is_empty()) {
            e.text_en = r.en.clone();
            e.en_src = "errorlist";
        } else if let Some(h) = (e.code != 0).then(|| hmi_code.get(&(*area, e.code)).copied()).flatten().filter(|h| !h.text_en.is_empty()) {
            e.text_en = h.text_en.clone();
            e.en_src = "hmi";
        } else if let Some(h) = at_bit.filter(|h| h.code.is_none() && !h.text_en.is_empty()) {
            e.text_en = h.text_en.clone();
            e.en_src = "hmi";
        }
        if let Some(h) = same.filter(|h| !h.text_ko.is_empty()) {
            e.text_ko = h.text_ko.clone();
            e.ko_src = "hmi";
        } else if let Some(r) = row.filter(|r| !r.ko.is_empty()) {
            e.text_ko = r.ko.clone();
            e.ko_src = "errorlist";
        } else if let Some(h) = at_bit.filter(|h| h.code.is_none() && !h.text_ko.is_empty()) {
            e.text_ko = h.text_ko.clone();
            e.ko_src = "hmi";
        }
        if e.class.is_empty()
            && let Some(h) = at_bit
        {
            e.class = hmi_class(h, *area);
        }
    }

    // 6. coverage
    let mut areas: Vec<Area> = input.arrays.iter().map(|a| a.area).collect();
    areas.sort();
    areas.dedup();
    let mut coverage = Coverage { areas: areas.iter().map(|a| (*a, AreaCov::default())).collect(), conflicts: conflicts.len() };
    for e in entries.values() {
        let Some(area) = Area::from_name(e.area) else { continue };
        let Some((_, c)) = coverage.areas.iter_mut().find(|(a, _)| *a == area) else { continue };
        c.entries += 1;
        if e.origin == "scl" {
            c.scl_bits += 1;
        } else {
            c.hmi_only += 1;
        }
        if e.code != 0 {
            c.bits_with_code += 1;
        }
        if !e.text_en.is_empty() || !e.text_ko.is_empty() {
            c.with_text += 1;
        }
        match e.en_src {
            "hmi" => c.text_en_hmi += 1,
            "errorlist" => c.text_en_errorlist += 1,
            _ => {}
        }
        if !e.text_ko.is_empty() {
            c.text_ko += 1;
        }
    }
    let stray = entries.keys().filter(|(a, _)| !areas.contains(a)).count();
    if stray > 0 {
        notes.push(format!("{stray} entries in an area without a PLC array"));
    }

    Built {
        file: AlarmsFile { plc: plc.to_string(), generated_by: "gr-contract alarms", sources, coverage, entries: entries.into_values().collect(), conflicts: conflicts.into_values().collect() },
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arrays() -> Vec<AreaArray> {
        let db = "\u{feff}DATA_BLOCK \"ALARM\"\n{ S7_Optimized_Access := 'FALSE' }\nVERSION : 0.1\nNON_RETAIN\n   STRUCT \n      FAULT : Array[0..399] of Byte;\n      WARN : Array[0..399] of Byte;\n      EVENT : Array[0..29] of Byte;\n      TASK : Array[0..199] of Byte;\n      X : Array[0..3] of Word;\n   END_STRUCT;\nBEGIN\nEND_DATA_BLOCK\n";
        let (a, notes) = parse_area_db(db);
        assert!(notes.is_empty());
        a
    }

    const SCL: &str = r#"FUNCTION_BLOCK "FL_Alarm"
BEGIN
	//#AlarmRaised := "SetAlarm"(Class:="ALARM_FAULT", Code := 0101, Popup:=FALSE, Condition := TRUE, Alarm := "ALARM".FAULT[0].%X0);
	#AlarmRaised := "SetAlarm"(Class := "ALARM_FAULT", Code := 0102, Popup := FALSE, Condition := NOT "MACHINE".Module.RIO_Ok[1], "Alarm" := "ALARM".FAULT[0].%X1);
	#AlarmRaised := "SetAlarm"(Class := "ALARM_WARN",
	                           Code := UDINT#3119,
	                           Condition := "OnDlyTimer"(Condition := #c, PresetTime := 0.5),
	                           Popup := FALSE,
	                           Alarm := "ALARM".WARN[74] .%X2);
	(* #AlarmRaised := "SetAlarm"(Code := 9999, Alarm := "ALARM".FAULT[1].%X0); *)
	#isTimeOut := "isProcTimeout"(TimeOutTime := 20.0, FaultCode := 6001, Proc := #Proc, Alarm := "ALARM".FAULT[140].%X0);
	#x := 'Code := 1, Alarm := "ALARM".FAULT[2].%X0';
	#AlarmRaised := "SetAlarm"(Class := "ALARM_WARN", Code := UDINT#75000 + UINT_TO_UDINT(#CsvIdx), Condition := TRUE, Alarm := "ALARM".WARN[200].%X1);
	#AlarmRaised := "SetAlarm"(Class := "ALARM_FAULT", Code := 0103, Condition := TRUE, Alarm := "ALARM".FAULT[0].%X1);
	"ALARM".WARN[220].%X0 := #a AND #b;
	"HMI".Alarm.Fault := "ALARM".FAULT;
END_FUNCTION_BLOCK
"#;

    fn scan() -> SclScan {
        let mut s = SclScan::default();
        scan_scl(SCL, "GR2_PLC/blocks/FL_Alarm.scl", &arrays(), &|b| b == "FL_Alarm", &mut s);
        s
    }

    #[test]
    fn reachability_from_obs() {
        let ob = scl_block_info(&strip_code("ORGANIZATION_BLOCK \"Main\"\nBEGIN\n  \"A_DB\"();\n  // \"C\"();\nEND_ORGANIZATION_BLOCK\n")).unwrap();
        let a = scl_block_info(&strip_code("FUNCTION_BLOCK \"A\"\nVAR\n  inst : \"B\";\n  arr : Array[0..1] of \"D\";\nEND_VAR\nBEGIN\n  #inst();\nEND_FUNCTION_BLOCK\n")).unwrap();
        assert_eq!(a.callees, vec!["B", "D"]);
        let idb = xml_block_info("<SW.Blocks.InstanceDB><AttributeList><Name>A_DB</Name></AttributeList><InstanceOfName>A</InstanceOfName>").unwrap();
        let c = scl_block_info(&strip_code("FUNCTION \"C\" : Void\nBEGIN\n  \"B\"();\nEND_FUNCTION\n")).unwrap();
        let lad = xml_block_info("<SW.Blocks.OB ID=\"0\"><AttributeList><Name>Cyclic</Name></AttributeList><CallInfo Name=\"E\" BlockType=\"FC\">").unwrap();
        let live = reachable(&[ob, a, idb, c, lad]);
        let mut v: Vec<&str> = live.iter().map(String::as_str).collect();
        v.sort_unstable();
        assert_eq!(v, vec!["a", "a_db", "b", "cyclic", "d", "e", "main"]);
    }

    #[test]
    fn area_db_needs_byte_arrays() {
        let a = arrays();
        assert_eq!(a.iter().map(|a| (a.member.as_str(), a.hi)).collect::<Vec<_>>(), vec![("FAULT", 399), ("WARN", 399), ("EVENT", 29), ("TASK", 199)]);
        let (_, notes) = parse_area_db("DATA_BLOCK \"FAULT\"\n  FAULT : Array[0..9] of Word;\n");
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn scl_calls_variants() {
        let s = scan();
        let got: Vec<(Area, u32, u8, u32, &str)> = s.sites.iter().map(|x| (x.area, x.byte, x.bit, x.code, x.callee.as_str())).collect();
        assert_eq!(got, vec![(Area::Fault, 0, 1, 102, "SetAlarm"), (Area::Warn, 74, 2, 3119, "SetAlarm"), (Area::Fault, 140, 0, 6001, "isProcTimeout"), (Area::Fault, 0, 1, 103, "SetAlarm"),]);
        assert_eq!(s.sites[0].at, "GR2_PLC/blocks/FL_Alarm.scl:4");
        assert_eq!(s.sites[1].at, "GR2_PLC/blocks/FL_Alarm.scl:5");
        assert_eq!(s.sites[1].class, Some(Area::Warn));
        assert_eq!(s.unresolved.len(), 1);
        assert!(s.unresolved[0].code.starts_with("UDINT#75000"));
        assert_eq!(s.copies, vec![("HMI.Alarm.Fault".to_string(), Area::Fault)]);
        assert_eq!(s.direct.iter().map(|d| (d.0, d.1, d.2)).collect::<Vec<_>>(), vec![(Area::Warn, 220, 0)]);
    }

    #[test]
    fn literals() {
        assert_eq!(parse_code("0102"), Some(102));
        assert_eq!(parse_code("UDINT#75000"), Some(75000));
        assert_eq!(parse_code("#FaultCode"), None);
        assert_eq!(parse_code("16#FF"), None);
        assert_eq!(parse_bit_ref("\"FAULT\".FAULT[3].%X7"), Some(("FAULT".into(), "FAULT".into(), 3, 7)));
        assert_eq!(parse_bit_ref("\"ALARM\".FAULT[3].%X8"), None);
        assert_eq!(parse_bit_ref("#Alarm"), None);
        assert_eq!(Area::from_class_expr("\"ALARM_WARN\""), Some(Area::Warn));
        assert_eq!(Area::from_class_expr("1"), Some(Area::Fault));
        assert_eq!(split_code_prefix(&html_text("<body><p>[F0101] PLC CPU Fault &amp; IO</p></body>")), (Some(101), "PLC CPU Fault & IO".into()));
        assert_eq!(split_code_prefix("GCS REQ FLT : [401] X"), (None, "GCS REQ FLT : [401] X".into()));
    }

    fn hmi(plc_tag: &str, index: u32, bit: u8, code: Option<u32>, text: &str) -> HmiAlarm {
        HmiAlarm {
            hmi: "HMI_RT_1".into(),
            name: format!("a{index}.{bit}"),
            plc: "GR2_PLC".into(),
            plc_tag: plc_tag.into(),
            index,
            bit,
            code,
            text_en: text.into(),
            text_ko: String::new(),
            class: "GR2 FAULT".into(),
        }
    }

    #[test]
    fn build_bits_conflicts_texts_and_shape() {
        let mut sc = scan();
        // a dead copy (block not reachable) never decides a bit
        let mut dead = sc.sites[0].clone();
        (dead.code, dead.live, dead.block) = (7777, false, "FL_Alarm_Old".into());
        sc.sites.insert(0, dead);
        let input = PlcInput { arrays: arrays(), scan: sc, sources: vec!["export/GR2_PLC/blocks/**/*.scl".into()], notes: vec![] };
        let h = [
            hmi("HMI.Alarm.Fault", 0, 1, Some(102), "PLC Remote IO Module Fault"),
            hmi("HMI.Alarm.Warn", 74, 2, Some(3118), "Other"),
            hmi("HMI.Alarm.Fault", 5, 0, Some(209), "Door Opened"),
            hmi("HMI.Alarm.Task", 1, 3, None, "Cell#012 Task Executed"),
        ];
        let hr: Vec<&HmiAlarm> = h.iter().collect();
        let el = ElSheet {
            name: "GR1,2(기상반)Fault".into(),
            rows: vec![
                ElRow { row: 3, code: 102, en: "PLC Remote IO Module Fault".into(), ko: "PLC Remote IO 모듈 이상".into(), byte: Some(0), bit: Some(1) },
                ElRow { row: 4, code: 3119, en: "Station Interlock Timeout".into(), ko: "스테이션 인터록 타임아웃".into(), byte: Some(74), bit: Some(2) },
                ElRow { row: 5, code: 6001, en: String::new(), ko: String::new(), byte: Some(140), bit: Some(0) },
            ],
        };
        let b = build("GR2_PLC", &input, &hr, Some(&el), input.sources.clone());
        let f = &b.file;
        let key: Vec<(&str, u32, u32)> = f.entries.iter().map(|e| (e.area, e.bit, e.code)).collect();
        assert_eq!(key, vec![("FAULT", 1, 102), ("FAULT", 40, 209), ("FAULT", 1120, 6001), ("WARN", 594, 3119), ("TASK", 11, 0)]);
        // first code on a bit wins, HMI code mismatch → conflict, SCL wins
        let c: Vec<(&str, u32, &str, Vec<u32>)> = f.conflicts.iter().map(|c| (c.area, c.bit, c.kind, c.codes.clone())).collect();
        assert_eq!(c, vec![("FAULT", 1, "scl", vec![102, 103]), ("WARN", 594, "hmi", vec![3119, 3118])]);
        let w = &f.entries[3];
        assert_eq!((w.byte, w.bit_in_byte, w.class.as_str(), w.text_en.as_str(), w.en_src, w.origin), (74, 2, "Warning", "Station Interlock Timeout", "errorlist", "scl"));
        assert_eq!(w.text_ko, "스테이션 인터록 타임아웃");
        assert_eq!((f.entries[0].en_src, f.entries[0].ko_src), ("hmi", "errorlist"));
        assert_eq!((f.entries[1].origin, f.entries[1].class.as_str()), ("hmi", "Fault"));
        assert_eq!(f.entries[4].text_en, "Cell#012 Task Executed");
        assert_eq!(f.coverage.conflicts, 2);

        let v = serde_json::to_value(f).unwrap();
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        let mut want = vec!["plc", "generated_by", "sources", "coverage", "entries", "conflicts"];
        want.sort();
        let mut keys = keys;
        keys.sort();
        assert_eq!(keys, want);
        assert_eq!(v["coverage"]["FAULT"]["bits_with_code"], 3);
        assert_eq!(v["coverage"]["FAULT"]["scl_bits"], 2);
        assert_eq!(v["coverage"]["WARN"]["with_text"], 1);
        assert_eq!(v["coverage"]["conflicts"], 2);
        assert_eq!(v["entries"][0]["where"], "GR2_PLC/blocks/FL_Alarm.scl:4");
        assert_eq!(v["conflicts"][0]["where"][1], "GR2_PLC/blocks/FL_Alarm.scl:14");
        // the console reads it back
        let t = evt_catalog::render::AlarmTable::parse(&serde_json::to_string(f).unwrap()).unwrap();
        assert_eq!(t.find("WARN", 594).map(|e| e.code), Some(3119));
    }
}
