//! `events.db` — PLC and console events as raw numbers (text is rendered when read).

use std::path::Path;

use rusqlite::types::Value;

use crate::db::{Db, Migrations};

pub const MIGRATIONS: Migrations = &[("0001_events", include_str!("migrations/0001_events.sql"))];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Plc,
    Console,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Plc => "plc",
            Origin::Console => "console",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub plc: String,
    pub epoch: i64,
    /// `None` for console rows.
    pub seq: Option<i64>,
    pub plc_ts: i64,
    pub rx_ts: i64,
    pub cat: u8,
    pub lvl: u8,
    pub src: u32,
    pub code: u32,
    pub a: i64,
    pub b: i64,
    pub ctx: u32,
    pub origin: Origin,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollState {
    pub epoch: i64,
    pub boot_id: Option<u16>,
    pub last_seq: u32,
}

/// Newest-first page position: rows strictly older than `(ts, id)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub ts: i64,
    pub id: i64,
}

impl Cursor {
    pub fn parse(s: &str) -> Option<Cursor> {
        let (t, i) = s.split_once(':')?;
        Some(Cursor { ts: t.trim().parse().ok()?, id: i.trim().parse().ok()? })
    }
    pub fn text(&self) -> String {
        format!("{}:{}", self.ts, self.id)
    }
}

/// SQL-side filter (the text search `q` is applied after rendering).
#[derive(Clone, Debug, Default)]
pub struct Filter {
    pub plcs: Vec<String>,
    pub cats: Vec<u8>,
    pub min_lvl: Option<u8>,
    /// (cat, code), OR-ed; `None` = any (a number without its category, STEP `"*"` = any code of STEP).
    pub codes: Vec<(Option<u8>, Option<u32>)>,
    pub src: Option<u32>,
    pub ctx: Option<u32>,
    pub origin: Option<Origin>,
    pub from: Option<i64>,
    pub to: Option<i64>,
}

const COLS: &str = "id, plc, epoch, seq, plc_ts, rx_ts, cat, lvl, src, code, a, b, ctx, origin, detail";

fn row_of(r: &rusqlite::Row) -> rusqlite::Result<(i64, Row)> {
    let origin: String = r.get(13)?;
    Ok((
        r.get(0)?,
        Row {
            plc: r.get(1)?,
            epoch: r.get(2)?,
            seq: r.get(3)?,
            plc_ts: r.get(4)?,
            rx_ts: r.get(5)?,
            cat: r.get(6)?,
            lvl: r.get(7)?,
            src: r.get(8)?,
            code: r.get(9)?,
            a: r.get(10)?,
            b: r.get(11)?,
            ctx: r.get(12)?,
            origin: if origin == "console" { Origin::Console } else { Origin::Plc },
            detail: r.get(14)?,
        },
    ))
}

#[derive(Clone)]
pub struct Store {
    db: Db,
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Store> {
        Ok(Store { db: Db::open_with(path, MIGRATIONS)? })
    }

    #[cfg(test)]
    pub fn memory() -> Store {
        Store { db: Db::open_memory_with(MIGRATIONS).unwrap() }
    }

    /// Inserts in one transaction; a PLC row already stored (same plc, epoch, seq) is skipped. Returns the new rows
    /// with their ids, and applies the collector states after the rows.
    pub fn write(&self, rows: Vec<Row>, states: &[(String, CollState)]) -> rusqlite::Result<Vec<(i64, Row)>> {
        self.db.with_mut(|c| {
            let tx = c.transaction()?;
            let mut out = Vec::with_capacity(rows.len());
            {
                let mut st = tx.prepare_cached(
                    "INSERT OR IGNORE INTO events (plc, epoch, seq, plc_ts, rx_ts, cat, lvl, src, code, a, b, ctx, origin, detail) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                )?;
                for r in rows {
                    let n = st.execute(rusqlite::params![r.plc, r.epoch, r.seq, r.plc_ts, r.rx_ts, r.cat, r.lvl, r.src, r.code, r.a, r.b, r.ctx, r.origin.as_str(), r.detail])?;
                    if n == 1 {
                        out.push((tx.last_insert_rowid(), r));
                    }
                }
            }
            for (plc, s) in states {
                tx.execute(
                    "INSERT INTO evt_state (plc, epoch, boot_id, last_seq, updated_at) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(plc) DO UPDATE SET epoch = excluded.epoch, boot_id = excluded.boot_id, last_seq = excluded.last_seq, updated_at = excluded.updated_at",
                    rusqlite::params![plc, s.epoch, s.boot_id, s.last_seq, crate::util::now_str()],
                )?;
            }
            tx.commit()?;
            Ok(out)
        })
    }

    pub fn state(&self, plc: &str) -> rusqlite::Result<Option<CollState>> {
        self.db.with(|c| {
            let mut st = c.prepare("SELECT epoch, boot_id, last_seq FROM evt_state WHERE plc = ?1")?;
            let mut it = st.query_map([plc], |r| Ok(CollState { epoch: r.get(0)?, boot_id: r.get(1)?, last_seq: r.get(2)? }))?;
            it.next().transpose()
        })
    }

    /// Newest first. `ascending` flips to oldest first (timeline windows); the cursor then means "newer than".
    pub fn query(&self, f: &Filter, cursor: Option<Cursor>, limit: usize, ascending: bool) -> rusqlite::Result<Vec<(i64, Row)>> {
        let mut sql = format!("SELECT {COLS} FROM events WHERE 1=1");
        let mut args: Vec<Value> = Vec::new();
        let mut list = |sql: &mut String, col: &str, vals: Vec<Value>| {
            if vals.is_empty() {
                return;
            }
            sql.push_str(&format!(" AND {col} IN ({})", vec!["?"; vals.len()].join(",")));
            args.extend(vals);
        };
        list(&mut sql, "plc", f.plcs.iter().map(|p| Value::Text(p.clone())).collect());
        list(&mut sql, "cat", f.cats.iter().map(|c| Value::Integer(i64::from(*c))).collect());
        if !f.codes.is_empty() {
            let parts: Vec<String> = f
                .codes
                .iter()
                .map(|(cat, code)| match (cat, code) {
                    (Some(k), Some(c)) => format!("(cat = {k} AND code = {c})"),
                    (Some(k), None) => format!("(cat = {k})"),
                    (None, Some(c)) => format!("(code = {c})"),
                    (None, None) => "1=1".to_string(),
                })
                .collect();
            sql.push_str(&format!(" AND ({})", parts.join(" OR ")));
        }
        let mut push = |cond: &str, v: Value| {
            sql.push_str(cond);
            args.push(v);
        };
        if let Some(l) = f.min_lvl {
            push(" AND lvl >= ?", Value::Integer(i64::from(l)));
        }
        if let Some(s) = f.src {
            push(" AND src = ?", Value::Integer(i64::from(s)));
        }
        if let Some(c) = f.ctx {
            push(" AND ctx = ?", Value::Integer(i64::from(c)));
        }
        if let Some(o) = f.origin {
            push(" AND origin = ?", Value::Text(o.as_str().into()));
        }
        if let Some(t) = f.from {
            push(" AND plc_ts >= ?", Value::Integer(t));
        }
        if let Some(t) = f.to {
            push(" AND plc_ts <= ?", Value::Integer(t));
        }
        if let Some(c) = cursor {
            let op = if ascending { ">" } else { "<" };
            sql.push_str(&format!(" AND (plc_ts {op} ? OR (plc_ts = ? AND id {op} ?))"));
            args.extend([Value::Integer(c.ts), Value::Integer(c.ts), Value::Integer(c.id)]);
        }
        let dir = if ascending { "ASC" } else { "DESC" };
        sql.push_str(&format!(" ORDER BY plc_ts {dir}, id {dir} LIMIT ?"));
        args.push(Value::Integer(limit as i64));
        self.db.with(|c| {
            let mut st = c.prepare(&sql)?;
            let it = st.query_map(rusqlite::params_from_iter(args), row_of)?;
            it.collect()
        })
    }

    /// Bytes the rows occupy (free pages excluded — deleting frees pages that later inserts reuse).
    pub fn used_bytes(&self) -> rusqlite::Result<i64> {
        self.db.with(|c| {
            let pages: i64 = c.query_row("PRAGMA page_count", [], |r| r.get(0))?;
            let free: i64 = c.query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
            let size: i64 = c.query_row("PRAGMA page_size", [], |r| r.get(0))?;
            Ok((pages - free) * size)
        })
    }

    /// Retention: rows older than `keep_days`, then the oldest rows while the store uses more than `max_bytes`.
    /// Returns (deleted by age, deleted by size).
    pub fn prune(&self, now_ms: i64, keep_days: u32, max_bytes: i64) -> rusqlite::Result<(usize, usize)> {
        let cutoff = now_ms - i64::from(keep_days) * 86_400_000;
        let by_age = self.db.with(|c| c.execute("DELETE FROM events WHERE plc_ts < ?1", [cutoff]))?;
        let mut by_size = 0usize;
        for _ in 0..50 {
            let used = self.used_bytes()?;
            if used <= max_bytes {
                break;
            }
            let rows: i64 = self.db.with(|c| c.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0)))?;
            if rows == 0 {
                break;
            }
            // the share over the limit plus 5 %, at least 1000 rows per round
            let over = (used - max_bytes) as f64 / used as f64;
            let n = ((rows as f64 * (over + 0.05)).ceil() as i64).clamp(1000.min(rows), rows);
            by_size += self.db.with(|c| c.execute("DELETE FROM events WHERE id IN (SELECT id FROM events ORDER BY plc_ts ASC, id ASC LIMIT ?1)", [n]))?;
        }
        Ok((by_age, by_size))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn row(plc: &str, seq: Option<i64>, ts: i64, cat: u8, code: u32) -> Row {
        Row { plc: plc.into(), epoch: 1, seq, plc_ts: ts, rx_ts: ts, cat, lvl: 2, src: 0, code, a: 0, b: 0, ctx: 0, origin: if seq.is_some() { Origin::Plc } else { Origin::Console }, detail: None }
    }

    #[test]
    fn plc_rows_are_unique_per_epoch_and_console_rows_are_not() {
        let s = Store::memory();
        let got = s.write(vec![row("GR2", Some(1), 10, 3, 300), row("GR2", Some(1), 10, 3, 300), row("GR2", None, 11, 14, 9001), row("GR2", None, 11, 14, 9001)], &[]).unwrap();
        assert_eq!(got.len(), 3, "the duplicate PLC row is ignored");
        let mut again = row("GR2", Some(1), 10, 3, 300);
        again.epoch = 2;
        assert_eq!(s.write(vec![again], &[("GR2".into(), CollState { epoch: 2, boot_id: Some(7), last_seq: 1 })]).unwrap().len(), 1, "a new epoch may repeat seq");
        assert_eq!(s.state("GR2").unwrap(), Some(CollState { epoch: 2, boot_id: Some(7), last_seq: 1 }));
        assert_eq!(s.state("GRM").unwrap(), None);
    }

    #[test]
    fn query_filters_and_pages_newest_first() {
        let s = Store::memory();
        let mut rows = Vec::new();
        for i in 0..10 {
            let mut r = row(if i % 2 == 0 { "GR2" } else { "GRM" }, Some(i), 1000 + i, if i < 5 { 3 } else { 7 }, 700 + i as u32);
            r.lvl = (i % 5) as u8;
            r.ctx = if i >= 8 { 42 } else { 0 };
            rows.push(r);
        }
        s.write(rows, &[]).unwrap();
        let all = s.query(&Filter::default(), None, 4, false).unwrap();
        assert_eq!(all.iter().map(|(_, r)| r.plc_ts).collect::<Vec<_>>(), vec![1009, 1008, 1007, 1006]);
        let (last_id, last) = all.last().unwrap();
        let next = s.query(&Filter::default(), Some(Cursor { ts: last.plc_ts, id: *last_id }), 4, false).unwrap();
        assert_eq!(next.iter().map(|(_, r)| r.plc_ts).collect::<Vec<_>>(), vec![1005, 1004, 1003, 1002]);
        let f = Filter { plcs: vec!["GR2".into()], cats: vec![7], ..Default::default() };
        assert_eq!(s.query(&f, None, 50, false).unwrap().iter().map(|(_, r)| r.plc_ts).collect::<Vec<_>>(), vec![1008, 1006]);
        let f = Filter { min_lvl: Some(4), ..Default::default() };
        assert_eq!(s.query(&f, None, 50, false).unwrap().len(), 2);
        let f = Filter { ctx: Some(42), ..Default::default() };
        assert_eq!(s.query(&f, None, 50, false).unwrap().len(), 2);
        let f = Filter { codes: vec![(Some(3), Some(702)), (Some(7), None)], ..Default::default() };
        assert_eq!(s.query(&f, None, 50, false).unwrap().len(), 6);
        let f = Filter { codes: vec![(None, Some(702)), (None, Some(707))], ..Default::default() };
        assert_eq!(s.query(&f, None, 50, false).unwrap().len(), 2);
        let f = Filter { codes: vec![(Some(3), Some(702)), (Some(7), None)], ..Default::default() };
        assert_eq!(s.query(&f, None, 50, false).unwrap().len(), 6);
        let f = Filter { from: Some(1002), to: Some(1004), ..Default::default() };
        assert_eq!(s.query(&f, None, 50, true).unwrap().iter().map(|(_, r)| r.plc_ts).collect::<Vec<_>>(), vec![1002, 1003, 1004]);
        assert_eq!(Cursor::parse("12:3"), Some(Cursor { ts: 12, id: 3 }));
        assert_eq!(Cursor::parse(&Cursor { ts: -5, id: 9 }.text()), Some(Cursor { ts: -5, id: 9 }));
    }

    #[test]
    fn retention_drops_old_rows_then_oldest_over_the_size_cap() {
        let s = Store::memory();
        let day = 86_400_000i64;
        let now = 100 * day;
        let mut rows = vec![row("GR2", Some(0), now - 91 * day, 1, 101)];
        for i in 1..=3000 {
            let mut r = row("GR2", Some(i), now - 10 * day + i, 1, 101);
            r.detail = Some("x".repeat(200));
            rows.push(r);
        }
        s.write(rows, &[]).unwrap();
        let (age, size) = s.prune(now, 90, i64::MAX).unwrap();
        assert_eq!((age, size), (1, 0));
        let used = s.used_bytes().unwrap();
        let (_, size) = s.prune(now, 90, used / 2).unwrap();
        assert!(size > 0);
        assert!(s.used_bytes().unwrap() <= used / 2);
        let left = s.query(&Filter::default(), None, 5000, true).unwrap();
        assert!(!left.is_empty());
        assert!(left[0].1.seq.unwrap() > 1, "the oldest went first");
    }
}
