//! Per-cell stock (화물 재고). The PLC keeps no inventory table, so the console owns it:
//! - edited by hand (`PUT /api/stock/{cell}`), or
//! - folded automatically from **completed** PICK / DROP tasks on the ledger (idempotent per task id).
//!
//! `compose` reads it to place Z on the stack:
//! - PICK/MEASURE: `floor + H * (n - c) + grip` (top tire of the `c` being taken)
//! - DROP:         `floor + H * n + grip`       (first tire landing on `n` already there)
//!
//! `grip` 은 기준(`mid` = H/2 — 기본, `pick_bead` = 상부 비드 − PickBeadOffset)에 따라 다르고,
//! 눌림양(`ItemSpec.compression`)이 있으면 **타이어 위에 얹힌 개수**만큼 단 높이와 비드가 내려간다
//! (`pressed_height`, 전체 규칙은 `registry::spec::stack_z_with` 와 `docs/item-spec-z.md`).

pub mod conveyor;
pub mod io;
pub mod routes;
pub mod snapshot;

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

/// Z offset (from the tire bottom) where the gripper takes the tire: `mid` = Height/2,
/// `pick_bead` = 공칭 UpperBidHeight (mid when the item has no bead height).
/// 눌림·PickBeadOffset·"측정 없으면 mid" 까지 태운 판은 `registry::spec::grip_point`.
pub fn grip_offset(grip_ref: &str, item: &gr_proto::StockItem) -> f32 {
    let mid = item.height.max(0.0) / 2.0;
    match crate::registry::spec::normalize_grip_ref(grip_ref) {
        "pick_bead" if item.upper_bid_height > 0.0 => item.upper_bid_height,
        _ => mid,
    }
}

/// 위에 `above` 개가 얹힌 타이어의 눌린 높이 — `Height − Compression·above`, 최소 `MIN_PITCH`.
pub fn pressed_height(h: f32, compression: f32, above: u32) -> f32 {
    let h = h.max(0.0);
    if compression <= 0.0 || above == 0 {
        return h;
    }
    (h - compression * above as f32).max(crate::registry::spec::MIN_PITCH)
}

/// 잡는 타이어 아래에 깔리는 개수 — PICK/MEASURE: `n − c`, DROP: `n`, 그 밖: 0(바닥).
pub fn below_count(tt: TaskType, n: u32, c: u32) -> u32 {
    match tt {
        TaskType::Pick | TaskType::Measure => n.saturating_sub(c.max(1)),
        TaskType::Drop => n,
        _ => 0,
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

/// Over-limit note for a folded entry: the cell now holds more than the item's `spec.stack_max`.
/// The ledger path never refuses (the tires are physically there) — it only logs.
pub fn over_limit_note(e: &StockEntry, spec: &crate::registry::spec::ItemSpec) -> Option<String> {
    spec.exceeds(e.count).then(|| format!("cell {} holds {} of item {} > stack_max {}", e.cell_id, e.count, e.item_code, spec.stack_max))
}

/// Follows the ledger: every entry that becomes `Completed` is folded in once.
pub fn spawn(stock: Arc<Stock>, events: broadcast::Sender<LedgerEvent>, registry: Arc<crate::registry::Registry>) {
    tokio::spawn(async move {
        let mut rx = events.subscribe();
        loop {
            match rx.recv().await {
                Ok(LedgerEvent::Upsert { task }) => match stock.apply_task(&task) {
                    Ok(Some(e)) if e.item_code != 0 => {
                        if let Ok(Some(item)) = registry.item(e.item_code)
                            && let Some(note) = over_limit_note(&e, &item.spec)
                        {
                            tracing::warn!("stock: {note} (task {})", task.id);
                        }
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!("stock: apply {}: {e}", task.id),
                },
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
    fn the_scalar_fallback_matches_the_agreed_formulas() {
        // pick: the tire being taken sits on n − c below it; drop: on all n
        assert_eq!(below_count(TaskType::Pick, 3, 1), 2);
        assert_eq!(below_count(TaskType::Drop, 3, 1), 3);
        assert_eq!(below_count(TaskType::Pick, 3, 2), 1, "taking 2 of 3 grips the middle tire");
        assert_eq!(below_count(TaskType::Pick, 0, 1), 0);
        assert_eq!(below_count(TaskType::Move, 9, 1), 0);
        // compression: tire j of n carries n − j above it (the top one is not pressed)
        assert_eq!(pressed_height(240.0, 8.0, 0), 240.0);
        assert_eq!(pressed_height(240.0, 8.0, 3), 240.0 - 24.0);
        assert_eq!(pressed_height(240.0, 300.0, 2), crate::registry::spec::MIN_PITCH, "clamped, never negative");
        // grip reference: pick_bead uses UpperBidHeight, mid = H/2, no bead height falls back to mid.
        // 옛 `bead` 는 `pick_bead` 로 읽힌다(`spec::normalize_grip_ref`).
        let it = gr_proto::StockItem { height: 240.0, upper_bid_height: 200.0, ..Default::default() };
        assert_eq!(grip_offset("mid", &it), 120.0);
        assert_eq!(grip_offset("pick_bead", &it), 200.0, "the PickBeadOffset and the 측정 없음 → mid rule live in spec::grip_point");
        assert_eq!(grip_offset("bead", &it), 200.0, "legacy value reads as pick_bead");
        assert_eq!(grip_offset("bead", &gr_proto::StockItem { height: 240.0, ..Default::default() }), 120.0);
    }

    #[test]
    fn over_limit_note_only_past_stack_max() {
        let spec = crate::registry::spec::ItemSpec { stack_max: 4, ..Default::default() };
        let e = |count| StockEntry { cell_id: 101, item_code: 1001, count, ..Default::default() };
        assert!(over_limit_note(&e(4), &spec).is_none());
        assert!(over_limit_note(&e(5), &spec).unwrap().contains("stack_max 4"));
        assert!(over_limit_note(&e(50), &Default::default()).is_none(), "0 = unlimited");
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
            station_offset: None,
            pallet: None,
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
