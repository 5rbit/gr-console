//! Per-cell stock (화물 재고). The PLC keeps no inventory table, so the console owns it:
//! - edited by hand (`PUT /api/stock/{cell}`), or
//! - folded automatically from **completed** PICK / DROP tasks on the ledger (idempotent per task id).
//!
//! `compose` reads it to place Z on the stack:
//! - PICK/MEASURE: `floor + H * (n - c) + H/2` (centre of the top tire of the `c` being taken)
//! - DROP:         `floor + H * n + H/2`       (centre of the first tire landing on `n` already there)

pub mod routes;

use std::sync::{Arc, PoisonError};

use gr_proto::{TaskData, TaskType};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::db::Db;
use crate::error::ApiError;
use crate::ledger::{LedgerEntry, LedgerEvent, TaskState};
use crate::util::now_str;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct StockEntry {
    pub cell_id: u16,
    /// 0 = empty / unknown item.
    pub item_code: u32,
    pub count: u32,
    pub note: String,
    pub updated_at: String,
}

/// Broadcast on every change (whole table — it is small: ≤ 500 cells).
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum StockEvent {
    Snapshot { stock: Vec<StockEntry> },
    Upsert { entry: StockEntry, reason: String },
    Remove { cell_id: u16 },
}

pub struct Stock {
    db: Db,
    pub events: broadcast::Sender<StockEvent>,
    lock: std::sync::Mutex<()>,
}

/// Z offset (from the tire bottom) where the gripper takes the tire: `mid` = Height/2, `bead` = UpperBidHeight
/// (mid when the item has no bead height).
pub fn grip_offset(grip_ref: &str, item: &gr_proto::StockItem) -> f32 {
    let mid = item.height.max(0.0) / 2.0;
    match grip_ref.trim().to_ascii_lowercase().as_str() {
        "bead" if item.upper_bid_height > 0.0 => item.upper_bid_height,
        _ => mid,
    }
}

/// Z of the gripper for a stack of `n` tires of height `h` on `floor`, taking/leaving `c`; `grip` = offset
/// from the bottom of the tire being gripped (see `grip_offset`).
pub fn stack_z(tt: TaskType, floor: f32, h: f32, grip: f32, n: u32, c: u32) -> f32 {
    match tt {
        TaskType::Pick | TaskType::Measure => floor + h * (n.saturating_sub(c.max(1)) as f32) + grip,
        TaskType::Drop => floor + h * (n as f32) + grip,
        _ => floor,
    }
}

impl Stock {
    pub fn new(db: Db) -> Arc<Stock> {
        let (tx, _) = broadcast::channel(256);
        Arc::new(Stock { db, events: tx, lock: std::sync::Mutex::new(()) })
    }

    pub fn list(&self) -> Result<Vec<StockEntry>, ApiError> {
        let rows: Vec<StockEntry> = self.db.with(|c| {
            let mut st = c.prepare("SELECT cell_id, item_code, count, note, updated_at FROM stock ORDER BY cell_id")?;
            let it = st.query_map([], |r| {
                Ok(StockEntry { cell_id: r.get::<_, i64>(0)? as u16, item_code: r.get::<_, i64>(1)? as u32, count: r.get::<_, i64>(2)? as u32, note: r.get(3)?, updated_at: r.get(4)? })
            })?;
            it.collect()
        })?;
        Ok(rows)
    }

    pub fn get(&self, cell_id: u16) -> Result<Option<StockEntry>, ApiError> {
        Ok(self.list()?.into_iter().find(|s| s.cell_id == cell_id))
    }

    pub fn set(&self, cell_id: u16, item_code: u32, count: u32, note: &str, reason: &str) -> Result<StockEntry, ApiError> {
        let now = now_str();
        let item_code = if count == 0 { 0 } else { item_code };
        self.db.with(|c| {
            c.execute(
                "INSERT INTO stock (cell_id, item_code, count, note, updated_at) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(cell_id) DO UPDATE SET item_code=excluded.item_code, count=excluded.count, note=excluded.note, updated_at=excluded.updated_at",
                (cell_id, item_code, count, note, &now),
            )
        })?;
        let e = StockEntry { cell_id, item_code, count, note: note.into(), updated_at: now };
        let _ = self.events.send(StockEvent::Upsert { entry: e.clone(), reason: reason.into() });
        Ok(e)
    }

    pub fn remove(&self, cell_id: u16) -> Result<bool, ApiError> {
        let n = self.db.with(|c| c.execute("DELETE FROM stock WHERE cell_id = ?1", [cell_id]))?;
        if n > 0 {
            let _ = self.events.send(StockEvent::Remove { cell_id });
        }
        Ok(n > 0)
    }

    pub fn clear(&self) -> Result<usize, ApiError> {
        let n = self.db.with(|c| c.execute("DELETE FROM stock", []))?;
        let _ = self.events.send(StockEvent::Snapshot { stock: vec![] });
        Ok(n)
    }

    /// Folds one completed task into the stock. Returns the new entry when something changed.
    /// Idempotent: the task id is remembered in `stock_applied`.
    pub fn apply_task(&self, e: &LedgerEntry) -> Result<Option<StockEntry>, ApiError> {
        if e.state != TaskState::Completed {
            return Ok(None);
        }
        let Some(delta) = task_delta(&e.plc_task) else { return Ok(None) };
        let _g = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let seen: bool = self.db.with(|c| c.query_row("SELECT COUNT(*) FROM stock_applied WHERE task_id = ?1", [&e.id], |r| r.get::<_, i64>(0)))? > 0;
        if seen {
            return Ok(None);
        }
        let cur = self.get(delta.cell_id)?.unwrap_or(StockEntry { cell_id: delta.cell_id, ..Default::default() });
        let (item_code, count) = if delta.count >= 0 {
            (delta.item_code, cur.count + delta.count as u32)
        } else {
            let left = cur.count.saturating_sub((-delta.count) as u32);
            (
                if left == 0 {
                    0
                } else if cur.item_code != 0 {
                    cur.item_code
                } else {
                    delta.item_code
                },
                left,
            )
        };
        let reason = format!("{} #{}:{} {}", if delta.count >= 0 { "DROP" } else { "PICK" }, e.work_id, e.task_id, delta.count);
        let out = self.set(delta.cell_id, item_code, count, &cur.note, &reason)?;
        self.db.with(|c| c.execute("INSERT OR IGNORE INTO stock_applied (task_id, applied_at) VALUES (?1, ?2)", (&e.id, now_str())))?;
        Ok(Some(out))
    }
}

struct Delta {
    cell_id: u16,
    item_code: u32,
    /// +n for DROP, −n for PICK.
    count: i32,
}

/// Stock change implied by a task as sent to the PLC (works for external tasks too).
fn task_delta(t: &TaskData) -> Option<Delta> {
    if t.cell.id == 0 {
        return None;
    }
    let n = t.item.count.max(1) as i32;
    match TaskType::from_code(t.task_type)? {
        TaskType::Pick => Some(Delta { cell_id: t.cell.id, item_code: t.item.code, count: -n }),
        TaskType::Drop => Some(Delta { cell_id: t.cell.id, item_code: t.item.code, count: n }),
        _ => None,
    }
}

/// Follows the ledger: every entry that becomes `Completed` is folded in once.
pub fn spawn(stock: Arc<Stock>, events: broadcast::Sender<LedgerEvent>) {
    tokio::spawn(async move {
        let mut rx = events.subscribe();
        loop {
            match rx.recv().await {
                Ok(LedgerEvent::Upsert { task }) => {
                    if let Err(e) = stock.apply_task(&task) {
                        tracing::warn!("stock: apply {}: {e}", task.id);
                    }
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_z_matches_the_agreed_formulas() {
        // pick: H*(n-1) + H/2 for one tire; drop: H*n + H/2
        assert_eq!(stack_z(TaskType::Pick, 1000.0, 240.0, 120.0, 3, 1), 1000.0 + 240.0 * 2.0 + 120.0);
        assert_eq!(stack_z(TaskType::Drop, 1000.0, 240.0, 120.0, 3, 1), 1000.0 + 240.0 * 3.0 + 120.0);
        // taking 2 of 3 grips the middle tire; empty cell pick sits on the floor mid-tire
        assert_eq!(stack_z(TaskType::Pick, 0.0, 240.0, 120.0, 3, 2), 240.0 + 120.0);
        assert_eq!(stack_z(TaskType::Pick, 0.0, 240.0, 120.0, 0, 1), 120.0);
        assert_eq!(stack_z(TaskType::Drop, 0.0, 240.0, 120.0, 0, 1), 120.0);
        assert_eq!(stack_z(TaskType::Move, 50.0, 240.0, 120.0, 9, 1), 50.0);
        // grip reference: bead uses UpperBidHeight, mid = H/2, bead without a bead height falls back
        let it = gr_proto::StockItem { height: 240.0, upper_bid_height: 200.0, ..Default::default() };
        assert_eq!(grip_offset("mid", &it), 120.0);
        assert_eq!(grip_offset("bead", &it), 200.0);
        assert_eq!(grip_offset("bead", &gr_proto::StockItem { height: 240.0, ..Default::default() }), 120.0);
        assert_eq!(stack_z(TaskType::Pick, 1000.0, 240.0, 200.0, 3, 1), 1000.0 + 480.0 + 200.0);
    }

    #[test]
    fn apply_task_folds_pick_and_drop_once() {
        let db = Db::open_memory().unwrap();
        let s = Stock::new(db);
        s.set(101, 1001, 3, "", "seed").unwrap();
        let mut t = TaskData { task_type: TaskType::Pick.code(), ..Default::default() };
        t.cell.id = 101;
        t.item.code = 1001;
        t.item.count = 1;
        let mk = |id: &str, t: &TaskData| LedgerEntry {
            id: id.into(),
            seq: 0,
            work_id: 1,
            task_id: 1,
            origin: crate::ledger::Origin::Console,
            plc_name: "GR2".into(),
            request: None,
            resolved: None,
            position: t.position,
            plc_task: t.clone(),
            state: TaskState::Completed,
            state_at: String::new(),
            created_at: String::new(),
            submitted_at: None,
            ended_at: None,
            ack: None,
            header: None,
            plc: None,
            error: None,
            history: vec![],
        };
        assert_eq!(s.apply_task(&mk("a", &t)).unwrap().unwrap().count, 2);
        assert!(s.apply_task(&mk("a", &t)).unwrap().is_none(), "second fold of the same task is ignored");
        t.task_type = TaskType::Drop.code();
        t.cell.id = 105;
        t.item.count = 2;
        let e = s.apply_task(&mk("b", &t)).unwrap().unwrap();
        assert_eq!((e.cell_id, e.item_code, e.count), (105, 1001, 2));
        // picking the last tire clears the item code
        t.task_type = TaskType::Pick.code();
        t.item.count = 2;
        let e = s.apply_task(&mk("c", &t)).unwrap().unwrap();
        assert_eq!((e.item_code, e.count), (0, 0));
        // not completed → ignored
        let mut r = mk("d", &t);
        r.state = TaskState::Running;
        assert!(s.apply_task(&r).unwrap().is_none());
    }
}
