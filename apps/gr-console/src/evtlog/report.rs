//! Daily event report (xlsx): Summary · Alarms · Steps · Events (WARN and above) · Tasks.
//! `GET /api/events/report.xlsx?date=` and, with `[evtlog] daily_report = true`, `data/reports/events-<date>.xlsx`
//! for the previous day shortly after midnight (pruned with `keep_days`).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use rust_xlsxwriter::{Format, Workbook, Worksheet, XlsxError};
use serde::Deserialize;

use super::catalog::{EventRow, Texts, now_ms};
use super::routes::{log, opt};
use super::stats::{self, AlarmStats, StepStats};
use super::store::Filter;
use super::{EvtLog, stats::CAT_SYS};
use crate::error::ApiError;
use crate::state::AppState;

pub const SHEETS: [&str; 5] = ["Summary", "Alarms", "Steps", "Events", "Tasks"];
const DAY_MS: i64 = 86_400_000;
const MAX_ROWS: usize = 200_000;
const CAT_TASK: u8 = 4;
const TASK_COMPLETED: u32 = 408;
const TASK_CYCLE: u32 = 416;
const CON_EVT_GAP: u32 = 9020;
const LVL_WARN: u8 = 3;
/// Minutes after midnight before the previous day is written (late PLC rows of 23:59 are in by then).
const AFTER_MIDNIGHT_MS: i64 = 5 * 60_000;

/// Logger health of one PLC over the period.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoggerRow {
    pub plc: String,
    /// EVTLOG.Dropped as last read (since the PLC's boot, not per period).
    pub dropped: Option<u32>,
    pub gaps: i64,
    pub lost: i64,
}

pub struct ReportData {
    pub from: String,
    pub to: String,
    pub levels: Vec<String>,
    /// PLC → count per level id (index = level id).
    pub counts: BTreeMap<String, Vec<i64>>,
    pub tasks_completed: i64,
    pub alarms_raised: i64,
    pub loggers: Vec<LoggerRow>,
    pub alarms: AlarmStats,
    pub steps: StepStats,
    pub events: Vec<EventRow>,
    pub tasks: Vec<EventRow>,
}

/// Blocking: every table of the report for `[from, to]`.
pub fn gather(log: &EvtLog, from: i64, to: i64) -> rusqlite::Result<ReportData> {
    let t = &log.texts;
    let levels: Vec<String> = (0..=t.catalog.levels.iter().map(|l| l.id).max().unwrap_or(4)).map(|i| t.catalog.level_name(i).map(str::to_string).unwrap_or_else(|| format!("L{i}"))).collect();
    let n_lvl = levels.len();
    let mut counts: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    let mut gaps: HashMap<String, (i64, i64)> = HashMap::new();
    let (mut tasks_completed, mut alarms_raised) = (0, 0);
    log.store.db().with(|c| {
        let mut st = c.prepare("SELECT plc, lvl, COUNT(*) FROM events WHERE plc_ts >= ?1 AND plc_ts <= ?2 GROUP BY plc, lvl")?;
        let it = st.query_map([from, to], |r| Ok((r.get::<_, String>(0)?, r.get::<_, usize>(1)?, r.get::<_, i64>(2)?)))?;
        for x in it {
            let (plc, lvl, n) = x?;
            let v = counts.entry(plc).or_insert_with(|| vec![0; n_lvl]);
            if let Some(s) = v.get_mut(lvl) {
                *s += n;
            }
        }
        let count = |cat: u8, code: u32| -> rusqlite::Result<i64> {
            c.query_row("SELECT COUNT(*) FROM events WHERE cat = ?1 AND code = ?2 AND plc_ts >= ?3 AND plc_ts <= ?4", rusqlite::params![cat, code, from, to], |r| r.get(0))
        };
        tasks_completed = count(CAT_TASK, TASK_COMPLETED)?;
        alarms_raised = count(stats::CAT_ALARM, stats::ALM_RAISED)?;
        let mut st = c.prepare("SELECT plc, COUNT(*), COALESCE(SUM(a), 0) FROM events WHERE cat = ?1 AND code = ?2 AND plc_ts >= ?3 AND plc_ts <= ?4 GROUP BY plc")?;
        let it = st.query_map(rusqlite::params![CAT_SYS, CON_EVT_GAP, from, to], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))?;
        for x in it {
            let (plc, n, lost) = x?;
            gaps.insert(plc, (n, lost));
        }
        Ok(())
    })?;
    let mut loggers: Vec<LoggerRow> = log.statuses().into_iter().map(|(plc, s)| LoggerRow { dropped: s.dropped, gaps: 0, lost: 0, plc }).collect();
    for (plc, (n, lost)) in gaps {
        match loggers.iter_mut().find(|l| l.plc == plc) {
            Some(l) => (l.gaps, l.lost) = (n, lost),
            None => loggers.push(LoggerRow { plc, dropped: None, gaps: n, lost }),
        }
    }
    loggers.sort_by(|a, b| a.plc.cmp(&b.plc));
    let alarms = stats::alarm_stats(t, &stats::alarm_rows(&log.store, from, to, &[])?, from, to, now_ms());
    let steps = stats::step_stats(t, stats::step_rows(&log.store, from, to, &[], None)?, None, from, to);
    let render = |f: Filter| -> rusqlite::Result<Vec<EventRow>> { Ok(log.store.query(&f, None, MAX_ROWS, true)?.iter().map(|(id, r)| t.row(*id, r)).collect()) };
    let events = render(Filter { min_lvl: Some(LVL_WARN), from: Some(from), to: Some(to), ..Default::default() })?;
    let tasks = render(Filter { codes: vec![(Some(CAT_TASK), Some(TASK_COMPLETED)), (Some(CAT_TASK), Some(TASK_CYCLE))], from: Some(from), to: Some(to), ..Default::default() })?;
    Ok(ReportData { from: t.fmt_ms(from), to: t.fmt_ms(to), levels, counts, tasks_completed, alarms_raised, loggers, alarms, steps, events, tasks })
}

enum Cell<'a> {
    S(&'a str),
    N(f64),
    E,
}

fn row(ws: &mut Worksheet, r: u32, cells: &[Cell], fmt: Option<&Format>) -> Result<(), XlsxError> {
    for (c, v) in cells.iter().enumerate() {
        let c = c as u16;
        match (v, fmt) {
            (Cell::S(s), Some(f)) => ws.write_with_format(r, c, *s, f).map(|_| ())?,
            (Cell::S(s), None) => ws.write(r, c, *s).map(|_| ())?,
            (Cell::N(n), _) => ws.write(r, c, *n).map(|_| ())?,
            (Cell::E, _) => {}
        }
    }
    Ok(())
}

fn header(ws: &mut Worksheet, r: u32, cols: &[&str], bold: &Format) -> Result<(), XlsxError> {
    row(ws, r, &cols.iter().map(|c| Cell::S(c)).collect::<Vec<_>>(), Some(bold))
}

fn secs(ms: i64) -> f64 {
    (ms as f64 / 100.0).round() / 10.0
}

fn event_sheet(ws: &mut Worksheet, rows: &[EventRow], bold: &Format) -> Result<(), XlsxError> {
    header(ws, 0, &["Time", "PLC", "Level", "Cat", "Code", "Name", "Src", "A", "B", "Ctx", "Text", "Detail"], bold)?;
    for (i, e) in rows.iter().enumerate() {
        let name = e.name.as_deref().unwrap_or("");
        let detail = e.detail.as_deref().unwrap_or("");
        let cells = [
            Cell::S(&e.ts),
            Cell::S(&e.plc),
            Cell::S(&e.lvl_name),
            Cell::S(&e.cat_name),
            Cell::N(f64::from(e.code)),
            Cell::S(name),
            Cell::N(f64::from(e.src)),
            Cell::N(e.a as f64),
            Cell::N(e.b as f64),
            Cell::N(f64::from(e.ctx)),
            Cell::S(&e.text),
            Cell::S(detail),
        ];
        row(ws, i as u32 + 1, &cells, None)?;
    }
    Ok(())
}

pub fn build(d: &ReportData) -> Result<Vec<u8>, XlsxError> {
    let mut wb = Workbook::new();
    let bold = Format::new().set_bold();

    let ws = wb.add_worksheet().set_name(SHEETS[0])?;
    row(ws, 0, &[Cell::S("Report"), Cell::S("events")], None)?;
    row(ws, 1, &[Cell::S("From"), Cell::S(&d.from)], None)?;
    row(ws, 2, &[Cell::S("To"), Cell::S(&d.to)], None)?;
    let mut r = 4;
    let mut cols: Vec<&str> = vec!["PLC"];
    cols.extend(d.levels.iter().map(String::as_str));
    cols.push("Total");
    header(ws, r, &cols, &bold)?;
    for (plc, v) in &d.counts {
        r += 1;
        let mut cells = vec![Cell::S(plc)];
        cells.extend(v.iter().map(|n| Cell::N(*n as f64)));
        cells.push(Cell::N(v.iter().sum::<i64>() as f64));
        row(ws, r, &cells, None)?;
    }
    r += 2;
    header(ws, r, &["Key", "Value"], &bold)?;
    for (k, v) in [
        ("TasksCompleted", d.tasks_completed),
        ("AlarmsRaised", d.alarms_raised),
        ("AlarmSeconds", d.alarms.total_ms / 1000),
        ("StepSamples", i64::from(d.steps.samples)),
        ("StepOutliers", i64::from(d.steps.outliers_total)),
        ("WarnOrAbove", d.events.len() as i64),
    ] {
        r += 1;
        row(ws, r, &[Cell::S(k), Cell::N(v as f64)], None)?;
    }
    r += 2;
    header(ws, r, &["PLC", "Dropped", "Gaps", "Lost"], &bold)?;
    for l in &d.loggers {
        r += 1;
        row(ws, r, &[Cell::S(&l.plc), l.dropped.map(|n| Cell::N(f64::from(n))).unwrap_or(Cell::E), Cell::N(l.gaps as f64), Cell::N(l.lost as f64)], None)?;
    }
    ws.autofit();

    let ws = wb.add_worksheet().set_name(SHEETS[1])?;
    header(ws, 0, &["PLC", "Area", "Bit", "Code", "Label", "Text", "Count", "TotalSec", "MeanSec", "MaxSec", "Open", "Last"], &bold)?;
    for (i, a) in d.alarms.rows.iter().enumerate() {
        let cells = [
            Cell::S(&a.plc),
            Cell::S(&a.area_name),
            Cell::N(f64::from(a.bit)),
            Cell::N(f64::from(a.code)),
            Cell::S(&a.label),
            Cell::S(&a.text),
            Cell::N(f64::from(a.count)),
            Cell::N(secs(a.total_ms)),
            Cell::N(secs(a.mean_ms)),
            Cell::N(secs(a.max_ms)),
            Cell::N(f64::from(a.open)),
            Cell::S(&a.last),
        ];
        row(ws, i as u32 + 1, &cells, None)?;
    }
    ws.autofit();
    ws.set_freeze_panes(1, 0)?;

    let ws = wb.add_worksheet().set_name(SHEETS[2])?;
    header(ws, 0, &["PLC", "Proc", "Step", "StepName", "Count", "MeanMs", "P50Ms", "P95Ms", "MaxMs", "Outliers"], &bold)?;
    let mut r = 0;
    for s in &d.steps.rows {
        r += 1;
        let cells = [
            Cell::S(&s.plc),
            Cell::S(&s.proc_name),
            Cell::N(s.step as f64),
            Cell::S(&s.step_name),
            Cell::N(f64::from(s.count)),
            Cell::N(s.mean_ms as f64),
            Cell::N(s.p50_ms as f64),
            Cell::N(s.p95_ms as f64),
            Cell::N(s.max_ms as f64),
            Cell::N(f64::from(s.outliers)),
        ];
        row(ws, r, &cells, None)?;
    }
    r += 2;
    header(ws, r, &["Time", "PLC", "Proc", "Step", "StepName", "DwellMs", "P95Ms", "LimitMs", "WorkId"], &bold)?;
    for o in &d.steps.outliers {
        r += 1;
        let cells = [
            Cell::S(&o.ts),
            Cell::S(&o.plc),
            Cell::S(&o.proc_name),
            Cell::N(o.step as f64),
            Cell::S(&o.step_name),
            Cell::N(o.dwell_ms as f64),
            Cell::N(o.p95_ms as f64),
            Cell::N(o.limit_ms as f64),
            Cell::N(f64::from(o.ctx)),
        ];
        row(ws, r, &cells, None)?;
    }
    ws.autofit();

    let ws = wb.add_worksheet().set_name(SHEETS[3])?;
    event_sheet(ws, &d.events, &bold)?;
    ws.autofit();
    ws.set_freeze_panes(1, 0)?;

    let ws = wb.add_worksheet().set_name(SHEETS[4])?;
    header(ws, 0, &["Time", "PLC", "Name", "TaskType", "Cell", "WorkId", "ProcessTimeMs", "TravelMm", "Text"], &bold)?;
    for (i, e) in d.tasks.iter().enumerate() {
        let done = e.code == TASK_COMPLETED;
        let cells = [
            Cell::S(&e.ts),
            Cell::S(&e.plc),
            Cell::S(e.name.as_deref().unwrap_or("")),
            if done { Cell::N(f64::from(e.src)) } else { Cell::E },
            if done { Cell::N(e.a as f64) } else { Cell::E },
            Cell::N(if done { e.b as f64 } else { f64::from(e.ctx) }),
            if done { Cell::E } else { Cell::N(e.a as f64) },
            if done { Cell::E } else { Cell::N(e.b as f64) },
            Cell::S(&e.text),
        ];
        row(ws, i as u32 + 1, &cells, None)?;
    }
    ws.autofit();
    ws.set_freeze_panes(1, 0)?;

    wb.save_to_buffer()
}

/// `YYYY-MM-DD` → (local midnight, end of that day).
pub fn day_range(texts: &Texts, date: &str) -> Option<(i64, i64)> {
    let d = date.trim();
    if d.len() != 10 {
        return None;
    }
    let from = texts.parse_time(&format!("{d} 00:00"))?;
    Some((from, from + DAY_MS - 1))
}

fn file_name(date: &str) -> String {
    format!("events-{date}.xlsx")
}

#[derive(Deserialize, Default)]
struct ReportQ {
    date: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

async fn report(State(st): State<AppState>, Query(q): Query<ReportQ>) -> Result<impl IntoResponse, ApiError> {
    let log = log(&st)?.clone();
    let (from, to, name) = match opt(&q.date) {
        Some(d) => {
            let (f, t) = day_range(&log.texts, d).ok_or_else(|| ApiError::BadRequest(format!("date: {d} (YYYY-MM-DD)")))?;
            (f, t, file_name(d))
        }
        None => {
            let (f, t) = stats::range(&log.texts, &q.from, &q.to)?;
            let stamp = |ms: i64| log.texts.fmt_ms(ms).chars().filter(char::is_ascii_digit).take(12).collect::<String>();
            (f, t, format!("events-{}-{}.xlsx", stamp(f), stamp(t)))
        }
    };
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, ApiError> {
        let d = gather(&log, from, to)?;
        build(&d).map_err(|e| ApiError::Internal(format!("xlsx: {e}")))
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))??;
    Ok(([(header::CONTENT_TYPE, "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))], bytes))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/events/report.xlsx", get(report))
}

/// Report files older than `keep_days` (by the date in the name) — returns how many were removed.
pub fn prune(dir: &Path, today: &str, keep_days: u32, texts: &Texts) -> usize {
    let Some((today_ms, _)) = day_range(texts, today) else { return 0 };
    let cutoff = today_ms - i64::from(keep_days) * DAY_MS;
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    let mut n = 0;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(date) = name.strip_prefix("events-").and_then(|s| s.strip_suffix(".xlsx")) else { continue };
        if day_range(texts, date).is_some_and(|(t, _)| t < cutoff) && std::fs::remove_file(e.path()).is_ok() {
            n += 1;
        }
    }
    n
}

/// Writes the previous day's report once it is past midnight (+5 min) and the file is missing — also on start, so a
/// console that was off at midnight catches up.
pub fn spawn_daily(log: Arc<EvtLog>, dir: PathBuf) {
    tokio::spawn(async move {
        let mut iv = tokio::time::interval(Duration::from_secs(300));
        loop {
            iv.tick().await;
            let now = now_ms();
            let today = stats::local_midnight(&log.texts, now);
            if now - today < AFTER_MIDNIGHT_MS {
                continue;
            }
            let date = log.texts.fmt_ms(today - 1).chars().take(10).collect::<String>();
            let path = dir.join(file_name(&date));
            let today_s = log.texts.fmt_ms(today).chars().take(10).collect::<String>();
            let l = log.clone();
            let d = dir.clone();
            let res = tokio::task::spawn_blocking(move || -> anyhow::Result<Option<PathBuf>> {
                prune(&d, &today_s, l.cfg.keep_days, &l.texts);
                if path.exists() {
                    return Ok(None);
                }
                std::fs::create_dir_all(&d)?;
                let bytes = build(&gather(&l, today - DAY_MS, today - 1)?)?;
                let tmp = path.with_extension("xlsx.tmp");
                std::fs::write(&tmp, bytes)?;
                std::fs::rename(&tmp, &path)?;
                Ok(Some(path))
            })
            .await;
            match res {
                Ok(Ok(Some(p))) => tracing::info!(path = %p.display(), "evtlog daily report"),
                Ok(Ok(None)) => {}
                Ok(Err(e)) => tracing::warn!("evtlog daily report: {e:#}"),
                Err(e) => tracing::warn!("evtlog daily report task: {e}"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evtlog::stats::tests::raw;
    use calamine::{Reader, Xlsx};

    #[tokio::test]
    async fn report_has_five_sheets_with_the_rows_of_the_period() {
        let cat = Arc::new(crate::evtlog::catalog::embedded());
        let log = EvtLog::memory(cat);
        let rows = vec![
            raw("GR2", 1, 1_000, CAT_TASK, 2, 404, 1, 101, 7, 7),
            raw("GR2", 1, 2_000, 3, 2, 300, 20, 900, 200, 7),
            raw("GR2", 1, 3_000, 6, 3, 601, 2, 320, 300, 7),
            raw("GR2", 1, 4_000, 6, 2, 602, 2, 320, 0, 7),
            raw("GR2", 1, 5_000, 7, 3, 704, 5, 5984, 38, 7),
            raw("GR2", 1, 6_000, CAT_TASK, 2, TASK_COMPLETED, 1, 101, 7, 7),
            raw("GR2", 1, 6_000, CAT_TASK, 2, TASK_CYCLE, 0, 4900, 1400, 7),
            raw("GRM", 1, 6_500, 9, 4, 905, 1, 1, 0, 0),
            raw("GR2", 0, 7_000, CAT_SYS, 3, CON_EVT_GAP, 0, 12, 99, 0),
            raw("GR2", 1, 99_000, 6, 4, 601, 1, 7, 0, 0), // outside the period
        ];
        log.store.write(rows, &[]).unwrap();
        let d = gather(&log, 0, 10_000).unwrap();
        assert_eq!((d.tasks_completed, d.alarms_raised), (1, 1));
        assert_eq!(d.counts["GR2"].iter().sum::<i64>(), 8);
        assert_eq!(d.counts["GRM"][4], 1);
        assert_eq!(d.loggers, vec![LoggerRow { plc: "GR2".into(), dropped: None, gaps: 1, lost: 12 }]);
        assert_eq!(d.events.len(), 4, "WARN and above: raise, GRIP_ERROR, EMS, gap");
        assert_eq!(d.tasks.len(), 2);
        let bytes = build(&d).unwrap();
        let mut x: Xlsx<_> = calamine::open_workbook_from_rs(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(x.sheet_names(), SHEETS.to_vec());
        let height = |x: &mut Xlsx<_>, s: &str| x.worksheet_range(s).unwrap().height();
        assert_eq!(height(&mut x, "Alarms"), 2);
        assert_eq!(height(&mut x, "Events"), 5);
        assert_eq!(height(&mut x, "Tasks"), 3);
        assert_eq!(height(&mut x, "Steps"), 4, "one step row + blank + outlier header");
        let summary = x.worksheet_range("Summary").unwrap();
        assert_eq!(summary.get_value((4, 0)).map(|v| v.to_string()).as_deref(), Some("PLC"));
        assert_eq!(summary.get_value((4, 6)).map(|v| v.to_string()).as_deref(), Some("Total"));
        let tasks = x.worksheet_range("Tasks").unwrap();
        assert_eq!(tasks.get_value((0, 6)).map(|v| v.to_string()).as_deref(), Some("ProcessTimeMs"));
    }

    #[test]
    fn day_range_and_prune_by_file_date() {
        let t = crate::evtlog::stats::tests::texts();
        let (f, to) = day_range(&t, "2026-09-27").unwrap();
        assert_eq!(t.fmt_ms(f), "2026-09-27 00:00:00.000");
        assert_eq!(t.fmt_ms(to), "2026-09-27 23:59:59.999");
        assert!(day_range(&t, "27/09/2026").is_none());
        let dir = std::env::temp_dir().join(format!("gr-evt-report-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for d in ["2026-06-01", "2026-09-20", "2026-09-26"] {
            std::fs::write(dir.join(file_name(d)), b"x").unwrap();
        }
        std::fs::write(dir.join("other.txt"), b"x").unwrap();
        assert_eq!(prune(&dir, "2026-09-27", 90, &t), 1);
        assert_eq!(prune(&dir, "2026-09-27", 3, &t), 1);
        let left: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        assert_eq!(left.len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
