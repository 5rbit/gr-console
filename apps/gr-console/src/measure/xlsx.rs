//! 측정 이력 Excel (`GET /api/measlog/export.xlsx`) — 규격 `docs/measure-export.md`.
//!
//! 시트와 열은 **데이터가 없어도 항상 같다**: 받은 사람이 피벗·수식·매크로를 한 번 만들어 두면 다음 파일에도
//! 그대로 걸린다. `Data[0..19]` 는 종류마다 뜻이 달라 한 표에 `data0..` 로 두면 Excel 에서 쓸 수 없었다 →
//! 종류별 시트에 PLC 이름 그대로의 열로 푼다. 칸 정의는 GR2 `MeasureLogAdd` 머리 주석과 같고, 화면의
//! `apps/gr-web/src/lib/meas/dataFields.ts` 와 같아야 한다(바꾸면 둘 다, 그리고 FORMAT 을 올린다).

use rust_xlsxwriter::{ExcelDateTime, Format, Workbook, Worksheet, XlsxError};
use serde_json::Value as Json;

/// 규격 판. 열을 빼거나 뜻을 바꾸면 올린다(끝에 열을 더하는 것은 그대로).
pub const FORMAT: &str = "measlog-xlsx/1";

pub const KIND_SHEETS: [(u8, &str); 5] = [(1, "Item"), (2, "Sku"), (3, "Floor"), (4, "Pick"), (5, "Manual")];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fmt {
    Mm,
    Count,
    SkuDiag,
    Bool,
    Factor,
    Code,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub idx: usize,
    pub name: String,
    pub fmt: Fmt,
}

fn f(idx: usize, name: &str, fmt: Fmt) -> Field {
    Field { idx, name: name.to_string(), fmt }
}

/// 종류별 `Data` 칸(예비 칸은 뺀다). 0 번(Status)은 기록 머리 `Status` 와 같아 뺀다.
pub fn data_fields(kind: u8) -> Vec<Field> {
    use Fmt::*;
    match kind {
        1 => vec![
            f(1, "InnerDia", Mm),
            f(2, "UpperBeadHeight", Mm),
            f(3, "TireHeight", Mm),
            f(7, "Torq_InnerDia", Mm),
            f(8, "Torq_Factor", Factor),
            f(10, "In.InnerDia", Mm),
            f(11, "In.UpperBeadHeight", Mm),
            f(12, "In.TireHeight", Mm),
            f(13, "In.OffsetX", Mm),
            f(14, "In.OffsetY", Mm),
            f(15, "Out.InnerDia", Mm),
            f(16, "Out.UpperBeadHeight", Mm),
            f(17, "Out.TireHeight", Mm),
            f(18, "Out.OffsetX", Mm),
            f(19, "Out.OffsetY", Mm),
        ],
        2 => {
            let mut v = Vec::new();
            for n in 1..=7 {
                v.push(f(2 * n - 1, &format!("Stack[{n}].LowerBead"), Mm));
                v.push(f(2 * n, &format!("Stack[{n}].UpperBead"), Mm));
            }
            v.extend([f(15, "DiagFlags", SkuDiag), f(16, "LayerOffsetMax", Mm), f(17, "StackCount", Count), f(18, "EachHeight", Mm), f(19, "StackHeight", Mm)]);
            v
        }
        3 => vec![f(1, "FloorHeight", Mm), f(2, "Z.Position", Mm), f(3, "FLD.Distance", Mm)],
        4 => vec![
            f(1, "LastBeadPos-Target", Mm),
            f(2, "PickZTarget", Mm),
            f(3, "Valid", Bool),
            f(4, "FitError", Code),
            f(5, "InnerDia", Mm),
            f(6, "OffsetX", Mm),
            f(7, "OffsetY", Mm),
            f(8, "UpperBeadHeight", Mm),
            f(9, "TireHeight", Mm),
        ],
        5 => vec![f(1, "LaserInnerDia", Mm), f(2, "TorqueInnerDia", Mm), f(3, "TireHeight", Mm), f(4, "OffsetX", Mm), f(5, "OffsetY", Mm), f(6, "UpperBeadHeight", Mm), f(7, "FitError", Code)],
        _ => Vec::new(),
    }
}

pub const STATUS: [(u16, &str, &str); 11] = [
    (0, "None", "없음"),
    (1, "Busy", "측정 중"),
    (2, "Done", "정상"),
    (3, "Error", "오류"),
    (4, "Mismatch", "레이저 · 토크 내경 차이 큼"),
    (5, "SkuCountMismatch", "방향별 단수 불일치 (무효)"),
    (6, "SkuHeightSpread", "방향별 이탈 높이 편차 초과 (무효)"),
    (7, "SkuLayerOffset", "단별 편심 초과 - 삐뚤게 쌓임 (무효)"),
    (8, "SkuLiftEnd", "다 빠져나가기 전 리프트 끝 (무효)"),
    (9, "SkuNoEdge", "이탈 경계 없음 (무효)"),
    (10, "SkuCmdCount", "명령 단수와 측정 단수 다름 (무효)"),
];

/// SKU `DiagFlags` 비트(`FB_MeasureSku_V2`). 세 번째 = 무효 원인(아니면 경고).
pub const SKU_DIAG: [(u8, &str, bool); 9] = [
    (0, "방향별 단수 불일치", true),
    (1, "방향별 이탈 높이 편차", true),
    (2, "단별 비드 높이 편차", false),
    (3, "TBR 비평면", false),
    (4, "단별 편심 (삐뚤게 쌓임)", true),
    (5, "비드 구간 원 맞춤 실패", false),
    (6, "다 빠져나가기 전 리프트 끝", true),
    (7, "이탈 경계 없음", true),
    (8, "명령 단수 불일치", true),
];

pub fn status_name(s: u16) -> String {
    STATUS.iter().find(|x| x.0 == s).map(|x| x.1.to_string()).unwrap_or_else(|| s.to_string())
}

fn status_text(s: u16) -> &'static str {
    STATUS.iter().find(|x| x.0 == s).map(|x| x.2).unwrap_or("")
}

pub fn kind_name(k: u8) -> String {
    KIND_SHEETS.iter().find(|x| x.0 == k).map(|x| x.1.to_string()).unwrap_or_else(|| k.to_string())
}

fn task_type(t: u64) -> String {
    match t {
        0x40 => "UP".into(),
        0x41 => "PICK".into(),
        0x42 => "DROP".into(),
        0x43 => "MOVE".into(),
        0x44 => "MEASURE".into(),
        0 => String::new(),
        n => format!("0x{n:02X}"),
    }
}

/// 작업 플래그(F 바닥 · I 품목 · S SKU · C 중심 · A 회피 · L 완료 후 상승 · O 출고) — 화면 `flagStr` 과 같다.
fn flags(c: &Json) -> String {
    [("MeasureFloor", 'F'), ("MeasureItem", 'I'), ("MeasureSku", 'S'), ("AdjustCenter", 'C'), ("Avoid", 'A'), ("LiftUpAfterComplete", 'L'), ("Outbound", 'O')]
        .iter()
        .filter(|(k, _)| c[*k].as_bool().unwrap_or(false))
        .map(|(_, ch)| *ch)
        .collect()
}

pub fn sku_diag_text(flags: u32) -> String {
    SKU_DIAG.iter().filter(|(b, _, _)| flags & (1 << b) != 0).map(|(b, t, bad)| format!("X{b} {t}{}", if *bad { "" } else { " (경고)" })).collect::<Vec<_>>().join(" / ")
}

fn num(v: &Json) -> Option<f64> {
    v.as_f64()
}

/// 한 칸 값. 문자열 · 숫자 · 날짜 · 빈칸.
enum V {
    S(String),
    N(Option<f64>, u8),
    T(String),
}

/// 열 정의(머리글 · 단위 · 뜻 · 값). 머리글 = PLC 이름 그대로 + 단위.
struct Col {
    head: String,
    source: String,
    unit: &'static str,
    note: String,
    get: Box<dyn Fn(&Json) -> V>,
}

fn col(head: &str, source: &str, unit: &'static str, note: &str, get: impl Fn(&Json) -> V + 'static) -> Col {
    let head = if unit.is_empty() { head.to_string() } else { format!("{head} ({unit})") };
    Col { head, source: source.to_string(), unit, note: note.to_string(), get: Box::new(get) }
}

fn at<'a>(e: &'a Json, path: &[&str]) -> &'a Json {
    path.iter().fold(e, |j, k| &j[*k])
}

fn idx(e: &Json, path: &[&str], i: usize) -> Option<f64> {
    at(e, path).as_array().and_then(|a| a.get(i)).and_then(num)
}

const MM: u8 = 2;
const INT: u8 = 0;
const FACTOR: u8 = 3;

/// 모든 시트가 앞에 두는 열(기록 머리 · 명령). `with_kind` = 전체 시트에만 Kind 열.
fn head_cols(with_kind: bool) -> Vec<Col> {
    let mut v = vec![
        col("Seq", "Seq", "", "PLC 누적 번호(로봇 PLC 마다 1 부터)", |e| V::N(num(&e["Seq"]), INT)),
        col("TimeStamp", "TimeStamp", "", "PLC 시각(DTL, 로컬)", |e| V::T(e["TimeStamp"].as_str().unwrap_or("").to_string())),
    ];
    if with_kind {
        v.push(col("Kind", "Kind", "", "1 Item · 2 Sku · 3 Floor · 4 Pick · 5 Manual", |e| V::S(kind_name(e["Kind"].as_u64().unwrap_or(0) as u8))));
    }
    v.extend([
        col("Status", "Status", "", "Legend 시트의 Status 표", |e| V::N(num(&e["Status"]), INT)),
        col("StatusName", "Status", "", "", |e| V::S(status_name(e["Status"].as_u64().unwrap_or(0) as u16))),
        col("StatusText", "Status", "", "", |e| V::S(status_text(e["Status"].as_u64().unwrap_or(0) as u16).to_string())),
        col("WorkId", "Cmd.WorkId", "", "", |e| V::N(num(at(e, &["Cmd", "WorkId"])), INT)),
        col("TaskId", "Cmd.TaskId", "", "", |e| V::N(num(at(e, &["Cmd", "TaskId"])), INT)),
        col("TaskType", "Cmd.TaskType", "", "UP · PICK · DROP · MOVE · MEASURE", |e| V::S(task_type(at(e, &["Cmd", "TaskType"]).as_u64().unwrap_or(0)))),
        col("CellId", "Cmd.Cell.Id", "", "", |e| V::N(num(at(e, &["Cmd", "Cell", "Id"])), INT)),
        col("Code", "Cmd.Item.Code", "", "품목 코드", |e| V::N(num(at(e, &["Cmd", "Item", "Code"])), INT)),
        col("Cmd.Count", "Cmd.Item.Count", "", "명령 단수", |e| V::N(num(at(e, &["Cmd", "Item", "Count"])), INT)),
        col("Cmd.InnerDiameter", "Cmd.Item.InnerDiameter", "mm", "", |e| V::N(num(at(e, &["Cmd", "Item", "InnerDiameter"])), MM)),
        col("Cmd.OuterDiameter", "Cmd.Item.OuterDiameter", "mm", "", |e| V::N(num(at(e, &["Cmd", "Item", "OuterDiameter"])), MM)),
        col("Cmd.Height", "Cmd.Item.Height", "mm", "", |e| V::N(num(at(e, &["Cmd", "Item", "Height"])), MM)),
        col("Cmd.X", "Cmd.Position[0]", "mm", "", |e| V::N(idx(e, &["Cmd", "Position"], 0), MM)),
        col("Cmd.Y", "Cmd.Position[1]", "mm", "", |e| V::N(idx(e, &["Cmd", "Position"], 1), MM)),
        col("Cmd.Z", "Cmd.Position[2]", "mm", "", |e| V::N(idx(e, &["Cmd", "Position"], 2), MM)),
        col("Cmd.G", "Cmd.Position[3]", "mm", "그리퍼 명령", |e| V::N(idx(e, &["Cmd", "Position"], 3), MM)),
        col("Cell.Z", "Cmd.Cell.Position[2]", "mm", "셀 바닥 Z", |e| V::N(idx(e, &["Cmd", "Cell", "Position"], 2), MM)),
        col("Cmd.ZRel", "Cmd.Position[2] - Cmd.Cell.Position[2]", "mm", "명령 Z - 셀 바닥 Z", |e| {
            V::N(idx(e, &["Cmd", "Position"], 2).map(|z| z - idx(e, &["Cmd", "Cell", "Position"], 2).unwrap_or(0.0)), MM)
        }),
        col("Flags", "Cmd.MeasureFloor..Outbound", "", "F 바닥 · I 품목 · S SKU · C 중심 · A 회피 · L 완료 후 상승 · O 출고", |e| V::S(flags(&e["Cmd"]))),
    ]);
    v
}

fn delta_cols() -> Vec<Col> {
    let d = |name: &'static str, unit: &'static str, dig: u8| col(&format!("Delta.{name}"), &format!("Delta.{name}"), unit, "측정 - 명령", move |e| V::N(num(at(e, &["Delta", name])), dig));
    vec![d("InnerDia", "mm", MM), d("Height", "mm", MM), d("Z", "mm", MM), d("Offset", "mm", MM), d("Count", "", INT)]
}

fn kind_cols(kind: u8) -> Vec<Col> {
    let mut v = head_cols(false);
    for fd in data_fields(kind) {
        let i = fd.idx;
        let src = format!("Data[{i}]");
        match fd.fmt {
            Fmt::Mm => v.push(col(&fd.name, &src, "mm", "", move |e| V::N(idx(e, &["Data"], i), MM))),
            Fmt::Factor => v.push(col(&fd.name, &src, "", "", move |e| V::N(idx(e, &["Data"], i), FACTOR))),
            Fmt::Count | Fmt::Code => v.push(col(&fd.name, &src, "", if fd.fmt == Fmt::Code { "0 = 정상" } else { "" }, move |e| V::N(idx(e, &["Data"], i).map(f64::round), INT))),
            Fmt::Bool => v.push(col(&fd.name, &src, "", "1 = 유효", move |e| V::N(idx(e, &["Data"], i).map(|x| if x != 0.0 { 1.0 } else { 0.0 }), INT))),
            Fmt::SkuDiag => {
                v.push(col(&fd.name, &src, "", "비트 합(Legend 시트)", move |e| V::N(idx(e, &["Data"], i).map(f64::round), INT)));
                v.push(col("DiagText", &src, "", "켜진 비트 이름", move |e| V::S(sku_diag_text(idx(e, &["Data"], i).unwrap_or(0.0).round() as u32))));
            }
        }
    }
    v.extend(delta_cols());
    v
}

/// 요약 시트 조건(파일에 남긴다 — 같은 조건으로 다시 받을 수 있게).
pub struct Meta {
    pub plc: String,
    pub robot: String,
    pub exported_at: String,
    pub kind: Option<u8>,
    pub code: Option<u32>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub truncated: bool,
}

fn datetime(s: &str) -> Option<ExcelDateTime> {
    if s.is_empty() {
        return None;
    }
    ExcelDateTime::parse_from_str(&s.replace('T', " ")).ok()
}

fn write_rows(ws: &mut Worksheet, cols: &[Col], rows: &[&Json], fmts: &Fmts) -> Result<(), XlsxError> {
    for (c, cd) in cols.iter().enumerate() {
        ws.write_with_format(0, c as u16, &cd.head, &fmts.bold)?;
    }
    for (r, e) in rows.iter().enumerate() {
        let r = r as u32 + 1;
        for (c, cd) in cols.iter().enumerate() {
            let c = c as u16;
            match (cd.get)(e) {
                V::S(s) if !s.is_empty() => {
                    ws.write(r, c, s)?;
                }
                V::S(_) | V::N(None, _) => {}
                V::N(Some(x), dig) => {
                    let fm = match dig {
                        INT => &fmts.int,
                        FACTOR => &fmts.factor,
                        _ => &fmts.mm,
                    };
                    ws.write_with_format(r, c, x, fm)?;
                }
                V::T(s) => match datetime(&s) {
                    Some(dt) => {
                        ws.write_with_format(r, c, &dt, &fmts.time)?;
                    }
                    None if !s.is_empty() => {
                        ws.write(r, c, s)?;
                    }
                    None => {}
                },
            }
        }
    }
    let last_col = cols.len().saturating_sub(1) as u16;
    ws.autofilter(0, 0, rows.len() as u32, last_col)?;
    ws.set_freeze_panes(1, 2)?;
    ws.autofit();
    ws.set_column_width(1, 24)?;
    Ok(())
}

struct Fmts {
    bold: Format,
    int: Format,
    mm: Format,
    factor: Format,
    time: Format,
}

/// 기록(최신순 JSON, `MEASLOG_HIST.Entry` 모양) → xlsx. 시트: Info · All · Item · Sku · Floor · Pick · Manual · Legend.
pub fn build(entries: &[Json], meta: &Meta) -> Result<Vec<u8>, XlsxError> {
    let fmts = Fmts {
        bold: Format::new().set_bold(),
        int: Format::new().set_num_format("0"),
        mm: Format::new().set_num_format("0.00"),
        factor: Format::new().set_num_format("0.000"),
        time: Format::new().set_num_format("yyyy-mm-dd hh:mm:ss.000"),
    };
    // 옛것부터 — Excel 에서 시간 축으로 그리거나 이어 붙일 때 정렬이 필요 없게.
    let mut rows: Vec<&Json> = entries.iter().collect();
    rows.sort_by_key(|e| e["Seq"].as_u64().unwrap_or(0));
    let mut wb = Workbook::new();

    let ws = wb.add_worksheet().set_name("Info")?;
    let opt = |s: Option<String>| s.unwrap_or_else(|| "전체".into());
    let info: Vec<(&str, String)> = vec![
        ("Format", FORMAT.into()),
        ("PLC", meta.plc.clone()),
        ("Robot", meta.robot.clone()),
        ("ExportedAt", meta.exported_at.clone()),
        ("Filter.Kind", opt(meta.kind.map(kind_name))),
        ("Filter.Code", opt(meta.code.map(|c| c.to_string()))),
        ("Filter.From", opt(meta.from.clone())),
        ("Filter.To", opt(meta.to.clone())),
        ("Rows", rows.len().to_string()),
        ("SeqFirst", rows.first().map(|e| e["Seq"].to_string()).unwrap_or_default()),
        ("SeqLast", rows.last().map(|e| e["Seq"].to_string()).unwrap_or_default()),
        ("Truncated", if meta.truncated { "예 - 행 한도에 걸림, 기간을 줄여 다시 받을 것".into() } else { "아니오".into() }),
    ];
    ws.write_with_format(0, 0, "Key", &fmts.bold)?;
    ws.write_with_format(0, 1, "Value", &fmts.bold)?;
    for (i, (k, v)) in info.iter().enumerate() {
        ws.write(i as u32 + 1, 0, *k)?;
        ws.write(i as u32 + 1, 1, v)?;
    }
    let mut r = info.len() as u32 + 2;
    ws.write_with_format(r, 0, "Sheet", &fmts.bold)?;
    ws.write_with_format(r, 1, "Rows", &fmts.bold)?;
    for (k, name) in KIND_SHEETS {
        r += 1;
        ws.write(r, 0, name)?;
        ws.write(r, 1, rows.iter().filter(|e| e["Kind"].as_u64() == Some(u64::from(k))).count() as f64)?;
    }
    ws.autofit();

    let all = {
        let mut v = head_cols(true);
        v.extend(delta_cols());
        v
    };
    write_rows(wb.add_worksheet().set_name("All")?, &all, &rows, &fmts)?;
    let mut legend: Vec<(String, Vec<Col>)> = vec![("All".into(), all)];
    for (k, name) in KIND_SHEETS {
        let cols = kind_cols(k);
        let mine: Vec<&Json> = rows.iter().copied().filter(|e| e["Kind"].as_u64() == Some(u64::from(k))).collect();
        write_rows(wb.add_worksheet().set_name(name)?, &cols, &mine, &fmts)?;
        legend.push((name.into(), cols));
    }

    let ws = wb.add_worksheet().set_name("Legend")?;
    let mut r = 0;
    for (c, h) in ["Sheet", "Column", "Source (MEASLOG_HIST.Entry)", "Unit", "Note"].iter().enumerate() {
        ws.write_with_format(r, c as u16, *h, &fmts.bold)?;
    }
    for (sheet, cols) in &legend {
        for cd in cols {
            r += 1;
            ws.write(r, 0, sheet)?;
            ws.write(r, 1, &cd.head)?;
            ws.write(r, 2, &cd.source)?;
            ws.write(r, 3, cd.unit)?;
            ws.write(r, 4, &cd.note)?;
        }
    }
    r += 2;
    for (c, h) in ["Status", "StatusName", "StatusText"].iter().enumerate() {
        ws.write_with_format(r, c as u16, *h, &fmts.bold)?;
    }
    for (n, name, text) in STATUS {
        r += 1;
        ws.write(r, 0, f64::from(n))?;
        ws.write(r, 1, name)?;
        ws.write(r, 2, text)?;
    }
    r += 2;
    for (c, h) in ["DiagFlags bit", "Value", "Text", "Invalid"].iter().enumerate() {
        ws.write_with_format(r, c as u16, *h, &fmts.bold)?;
    }
    for (b, text, bad) in SKU_DIAG {
        r += 1;
        ws.write(r, 0, format!("X{b}"))?;
        ws.write(r, 1, f64::from(1u32 << b))?;
        ws.write(r, 2, text)?;
        ws.write(r, 3, if bad { "무효" } else { "경고" })?;
    }
    ws.autofit();
    ws.set_freeze_panes(1, 0)?;

    wb.save_to_buffer()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(seq: u32, kind: u8, data: &[f64]) -> Json {
        json!({
            "Seq": seq, "Kind": kind, "Status": 2, "TimeStamp": "2026-09-30 10:11:12.345",
            "Cmd": { "WorkId": 7, "TaskId": 3, "TaskType": 0x44, "MeasureItem": true,
                     "Item": { "Code": 1500, "Count": 1, "InnerDiameter": 508.0, "OuterDiameter": 1000.0, "Height": 250.0 },
                     "Position": [100.0, 200.0, 350.5, 508.0], "Cell": { "Id": 410, "Position": [0.0, 0.0, 100.0] } },
            "Data": data, "Delta": { "InnerDia": 1.25, "Height": -0.5, "Z": 0.0, "Offset": 2.0, "Count": 0.0 },
        })
    }

    #[test]
    fn builds_all_sheets_even_when_empty() {
        let meta = Meta { plc: "GR2".into(), robot: "GR2".into(), exported_at: "t".into(), kind: None, code: None, from: None, to: None, truncated: false };
        let bytes = build(&[], &meta).unwrap();
        assert!(bytes.starts_with(b"PK"));
        let mut data = vec![0.0; 20];
        data[15] = f64::from(1u32 | 4);
        data[17] = 3.0;
        let bytes = build(&[entry(2, 2, &data), entry(1, 1, &[2.0, 509.1])], &meta).unwrap();
        assert!(bytes.len() > 1000);
    }

    #[test]
    fn columns_are_named_not_numbered() {
        let heads: Vec<String> = kind_cols(1).iter().map(|c| c.head.clone()).collect();
        assert!(heads.contains(&"InnerDia (mm)".to_string()));
        assert!(heads.contains(&"Torq_Factor".to_string()));
        assert!(!heads.iter().any(|h| h.starts_with("data")));
        let sku: Vec<String> = kind_cols(2).iter().map(|c| c.head.clone()).collect();
        assert!(sku.contains(&"Stack[7].UpperBead (mm)".to_string()));
        assert!(sku.contains(&"DiagText".to_string()));
    }

    #[test]
    fn values_come_from_the_right_slots() {
        let e = entry(1, 1, &[2.0, 509.1, 0.0, 251.0]);
        let cols = kind_cols(1);
        let get = |h: &str| match (cols.iter().find(|c| c.head == h).unwrap().get)(&e) {
            V::N(v, _) => v.map(|x| format!("{x}")).unwrap_or_default(),
            V::S(s) | V::T(s) => s,
        };
        assert_eq!(get("InnerDia (mm)"), "509.1");
        assert_eq!(get("TireHeight (mm)"), "251");
        assert_eq!(get("Cmd.ZRel (mm)"), "250.5");
        assert_eq!(get("TaskType"), "MEASURE");
        assert_eq!(get("Flags"), "I");
        assert_eq!(get("StatusName"), "Done");
        assert_eq!(sku_diag_text(1 | 4), "X0 방향별 단수 불일치 / X2 단별 비드 높이 편차 (경고)");
        assert!(datetime("2026-09-30 10:11:12.345").is_some());
    }
}
