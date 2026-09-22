//! 재고 스냅샷 — 한꺼번에 바꾸는 조작의 되돌리기 장치(2026-09-22).
//!
//! 전체 비우기(`clear`) · Excel 가져오기(`apply_ops`, merge·replace 둘 다) · 되돌리기(`restore`) 는
//! **바꾸기 직전의 재고 표 전체**를 `stock_snapshots` 에 한 줄로 떠 두고, 그 저장과 변경을 **한 트랜잭션**으로
//! 묶는다(스냅샷 없이 지워지는 일이 없다). 되돌리기도 먼저 지금 상태를 `restore-before` 로 떠 두므로
//! 되돌리기 자체도 되돌릴 수 있다.
//!
//! - 되돌리기 = 지금 재고를 스냅샷 내용으로 **통째로 바꾼다**(스냅샷 뒤에 완료된 PICK/DROP 반영분도 사라진다).
//!   `stock_applied`(이미 접은 작업 id)는 건드리지 않는다 — 같은 작업이 두 번 접히지 않게.
//! - 보관: 최근 [`SNAPSHOT_KEEP`] 개, 무슨 일이 있어도 최근 [`SNAPSHOT_KEEP_MIN`] 개는 지우지 않는다.
//! - 끝나면 `StockEvent::Snapshot`(표 전체) 한 번 — 화면 표·맵이 바로 바뀐다.

use std::sync::PoisonError;

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

use super::{Stock, StockEntry, StockEvent};
use crate::error::ApiError;
use crate::util::now_str;

/// 보관할 스냅샷 수(오래된 것부터 지운다).
pub const SNAPSHOT_KEEP: usize = 50;
/// 보관 수를 얼마로 줄이든 남기는 최근 스냅샷 수.
pub const SNAPSHOT_KEEP_MIN: usize = 5;

pub const REASON_CLEAR: &str = "clear";
pub const REASON_IMPORT_MERGE: &str = "import-merge";
pub const REASON_IMPORT_REPLACE: &str = "import-replace";
pub const REASON_RESTORE_BEFORE: &str = "restore-before";

/// 목록 한 줄(줄 내용 없이).
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SnapshotInfo {
    pub id: i64,
    pub created_at: String,
    pub reason: String,
    /// 재고가 있는(count > 0) 칸 수.
    pub row_count: u32,
    pub total: u32,
    pub restored_at: Option<String>,
    pub restored_from: Option<i64>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Snapshot {
    #[serde(flatten)]
    pub info: SnapshotInfo,
    pub rows: Vec<StockEntry>,
}

/// 되돌리기 결과.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Restored {
    /// 되돌린 스냅샷 id.
    pub restored: i64,
    /// 되돌리기 직전 상태를 떠 둔 스냅샷 id(이것으로 되돌리기를 되돌린다).
    pub snapshot_id: i64,
    pub row_count: u32,
    pub total: u32,
}

/// 가져오기 한 자리의 할 일(`stock::io::Op` 를 적용할 때의 모양).
#[derive(Clone, Debug, PartialEq)]
pub enum BulkOp {
    Set { cell_id: u16, item_code: u32, count: u32, note: String },
    Remove(u16),
}

fn read_rows(c: &Connection) -> rusqlite::Result<Vec<StockEntry>> {
    let mut st = c.prepare("SELECT cell_id, item_code, count, note, updated_at FROM stock ORDER BY cell_id")?;
    let it =
        st.query_map([], |r| Ok(StockEntry { cell_id: r.get::<_, i64>(0)? as u16, item_code: r.get::<_, i64>(1)? as u32, count: r.get::<_, i64>(2)? as u32, note: r.get(3)?, updated_at: r.get(4)? }))?;
    it.collect()
}

fn tally(rows: &[StockEntry]) -> (u32, u32) {
    (rows.iter().filter(|e| e.count > 0).count() as u32, rows.iter().map(|e| e.count).sum())
}

/// 지금 재고 표 전체를 한 줄로 떠 두고 id 를 돌려준다(호출자의 트랜잭션 안에서).
fn save_current(c: &Connection, reason: &str, restored_from: Option<i64>) -> rusqlite::Result<i64> {
    let rows = read_rows(c)?;
    let (n, total) = tally(&rows);
    let json = serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into());
    c.execute("INSERT INTO stock_snapshots (created_at, reason, rows_json, row_count, total, restored_from) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", (now_str(), reason, json, n, total, restored_from))?;
    let id = c.last_insert_rowid();
    prune(c, SNAPSHOT_KEEP)?;
    Ok(id)
}

/// 최근 `keep` 개(최소 [`SNAPSHOT_KEEP_MIN`])만 남긴다. 지운 수.
fn prune(c: &Connection, keep: usize) -> rusqlite::Result<usize> {
    let keep = keep.max(SNAPSHOT_KEEP_MIN) as i64;
    c.execute("DELETE FROM stock_snapshots WHERE id NOT IN (SELECT id FROM stock_snapshots ORDER BY id DESC LIMIT ?1)", [keep])
}

fn info_of(r: &rusqlite::Row) -> rusqlite::Result<SnapshotInfo> {
    Ok(SnapshotInfo {
        id: r.get(0)?,
        created_at: r.get(1)?,
        reason: r.get(2)?,
        row_count: r.get::<_, i64>(3)? as u32,
        total: r.get::<_, i64>(4)? as u32,
        restored_at: r.get(5)?,
        restored_from: r.get(6)?,
    })
}

impl Stock {
    /// 전체 비우기 — `(지운 줄 수, 스냅샷 id)`.
    pub fn clear(&self) -> Result<(usize, i64), ApiError> {
        let _g = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let (n, id) = self.db.with(|c| {
            let tx = c.unchecked_transaction()?;
            let id = save_current(&tx, REASON_CLEAR, None)?;
            let n = tx.execute("DELETE FROM stock", [])?;
            tx.commit()?;
            Ok((n, id))
        })?;
        let _ = self.events.send(StockEvent::Snapshot { stock: vec![] });
        Ok((n, id))
    }

    /// 가져오기 등 여러 자리를 한꺼번에 — 할 일이 없으면 스냅샷도 만들지 않는다(`None`).
    pub fn apply_ops(&self, ops: &[BulkOp], reason: &str) -> Result<Option<i64>, ApiError> {
        if ops.is_empty() {
            return Ok(None);
        }
        let _g = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let now = now_str();
        let id = self.db.with(|c| {
            let tx = c.unchecked_transaction()?;
            let id = save_current(&tx, reason, None)?;
            for op in ops {
                match op {
                    BulkOp::Set { cell_id, item_code, count, note } => {
                        let item_code = if *count == 0 { 0 } else { *item_code };
                        tx.execute(
                            "INSERT INTO stock (cell_id, item_code, count, note, updated_at) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(cell_id) DO UPDATE SET item_code=excluded.item_code, count=excluded.count, note=excluded.note, updated_at=excluded.updated_at",
                            (cell_id, item_code, count, note, &now),
                        )?;
                    }
                    BulkOp::Remove(cell_id) => {
                        tx.execute("DELETE FROM stock WHERE cell_id = ?1", [cell_id])?;
                    }
                }
            }
            tx.commit()?;
            Ok(id)
        })?;
        self.broadcast_all()?;
        Ok(Some(id))
    }

    /// 최근 것부터(줄 내용 없이).
    pub fn snapshots(&self, limit: usize) -> Result<Vec<SnapshotInfo>, ApiError> {
        let limit = limit.clamp(1, SNAPSHOT_KEEP) as i64;
        Ok(self.db.with(|c| {
            let mut st = c.prepare("SELECT id, created_at, reason, row_count, total, restored_at, restored_from FROM stock_snapshots ORDER BY id DESC LIMIT ?1")?;
            let it = st.query_map([limit], info_of)?;
            it.collect()
        })?)
    }

    pub fn snapshot(&self, id: i64) -> Result<Option<Snapshot>, ApiError> {
        let row = self.db.with(|c| {
            c.query_row("SELECT id, created_at, reason, row_count, total, restored_at, restored_from, rows_json FROM stock_snapshots WHERE id = ?1", [id], |r| {
                Ok((info_of(r)?, r.get::<_, String>(7)?))
            })
            .optional()
        })?;
        let Some((info, json)) = row else { return Ok(None) };
        let rows: Vec<StockEntry> = serde_json::from_str(&json).map_err(|e| ApiError::Internal(format!("stock snapshot {id}: {e}")))?;
        Ok(Some(Snapshot { info, rows }))
    }

    /// 스냅샷 `id` 로 되돌린다 — 지금 재고를 먼저 `restore-before` 로 떠 두고 표를 통째로 바꾼다.
    pub fn restore(&self, id: i64) -> Result<Restored, ApiError> {
        let snap = self.snapshot(id)?.ok_or_else(|| ApiError::NotFound(format!("stock snapshot {id}")))?;
        let _g = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let before = self.db.with(|c| {
            let tx = c.unchecked_transaction()?;
            let before = save_current(&tx, REASON_RESTORE_BEFORE, Some(id))?;
            tx.execute("DELETE FROM stock", [])?;
            for e in &snap.rows {
                tx.execute("INSERT INTO stock (cell_id, item_code, count, note, updated_at) VALUES (?1,?2,?3,?4,?5)", (e.cell_id, e.item_code, e.count, &e.note, &e.updated_at))?;
            }
            tx.execute("UPDATE stock_snapshots SET restored_at = ?1 WHERE id = ?2", (now_str(), id))?;
            tx.commit()?;
            Ok(before)
        })?;
        self.broadcast_all()?;
        let (row_count, total) = tally(&snap.rows);
        Ok(Restored { restored: id, snapshot_id: before, row_count, total })
    }

    fn broadcast_all(&self) -> Result<(), ApiError> {
        let stock = self.list()?;
        let _ = self.events.send(StockEvent::Snapshot { stock });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn seeded() -> std::sync::Arc<Stock> {
        let s = Stock::new(Db::open_memory().unwrap());
        s.set(101, 1001, 3, "a", "seed").unwrap();
        s.set(102, 1002, 2, "", "seed").unwrap();
        s.set(103, 0, 0, "empty", "seed").unwrap();
        s
    }

    #[test]
    fn clear_snapshots_then_restore_brings_every_row_back() {
        let s = seeded();
        let before = s.list().unwrap();
        let mut rx = s.events.subscribe();
        let (n, id) = s.clear().unwrap();
        assert_eq!(n, 3);
        assert!(s.list().unwrap().is_empty());
        assert!(matches!(rx.try_recv().unwrap(), StockEvent::Snapshot { stock } if stock.is_empty()));

        let list = s.snapshots(10).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].id, list[0].reason.as_str(), list[0].row_count, list[0].total), (id, REASON_CLEAR, 2, 5), "count 0 줄은 칸 수에서 빠진다");
        assert_eq!(s.snapshot(id).unwrap().unwrap().rows, before, "0 개 줄·메모·시각까지 그대로");

        let r = s.restore(id).unwrap();
        assert_eq!((r.restored, r.row_count, r.total), (id, 2, 5));
        assert_eq!(s.list().unwrap(), before);
        assert!(matches!(rx.try_recv().unwrap(), StockEvent::Snapshot { stock } if stock == before), "화면이 새 표를 받는다");
        assert!(s.snapshot(id).unwrap().unwrap().info.restored_at.is_some());
    }

    #[test]
    fn restore_saves_the_state_it_replaces_so_it_can_be_undone() {
        let s = seeded();
        let seeded_rows = s.list().unwrap();
        let (_, cleared) = s.clear().unwrap();
        s.set(105, 1001, 1, "", "manual").unwrap();
        let after_clear = s.list().unwrap();

        let r = s.restore(cleared).unwrap();
        let undo = s.snapshot(r.snapshot_id).unwrap().unwrap();
        assert_eq!(undo.info.reason, REASON_RESTORE_BEFORE);
        assert_eq!(undo.info.restored_from, Some(cleared));
        assert_eq!(undo.rows, after_clear);
        assert_eq!(s.list().unwrap(), seeded_rows);

        // 되돌리기를 되돌린다
        s.restore(r.snapshot_id).unwrap();
        assert_eq!(s.list().unwrap(), after_clear);
        assert_eq!(s.snapshots(10).unwrap().len(), 3, "clear · restore-before · restore-before");
        assert!(matches!(s.restore(9999), Err(ApiError::NotFound(_))));
    }

    #[test]
    fn retention_keeps_the_newest_and_never_fewer_than_the_minimum() {
        let s = seeded();
        let ids: Vec<i64> = (0..SNAPSHOT_KEEP + 7).map(|_| s.apply_ops(&[BulkOp::Remove(999)], REASON_IMPORT_MERGE).unwrap().unwrap()).collect();
        let kept = s.snapshots(SNAPSHOT_KEEP).unwrap();
        assert_eq!(kept.len(), SNAPSHOT_KEEP);
        assert_eq!(kept[0].id, *ids.last().unwrap(), "최근 것부터");
        assert_eq!(kept.last().unwrap().id, ids[7]);
        // 보관 수를 0 으로 줄여도 최근 SNAPSHOT_KEEP_MIN 개는 남는다
        let left = s.db.with(|c| prune(c, 0).and_then(|_| c.query_row("SELECT COUNT(*) FROM stock_snapshots", [], |r| r.get::<_, i64>(0)))).unwrap();
        assert_eq!(left as usize, SNAPSHOT_KEEP_MIN);
        assert_eq!(s.snapshots(50).unwrap()[0].id, *ids.last().unwrap());
    }

    #[test]
    fn apply_ops_snapshots_first_and_skips_when_nothing_changes() {
        let s = seeded();
        let before = s.list().unwrap();
        assert_eq!(s.apply_ops(&[], REASON_IMPORT_REPLACE).unwrap(), None);
        assert!(s.snapshots(10).unwrap().is_empty());
        let id = s.apply_ops(&[BulkOp::Set { cell_id: 104, item_code: 1001, count: 2, note: String::new() }, BulkOp::Remove(101), BulkOp::Remove(102)], REASON_IMPORT_REPLACE).unwrap().unwrap();
        let now: Vec<(u16, u32)> = s.list().unwrap().iter().map(|e| (e.cell_id, e.count)).collect();
        assert_eq!(now, vec![(103, 0), (104, 2)]);
        assert_eq!(s.snapshot(id).unwrap().unwrap().info.reason, REASON_IMPORT_REPLACE);
        s.restore(id).unwrap();
        assert_eq!(s.list().unwrap(), before);
    }
}
