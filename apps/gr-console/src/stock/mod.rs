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

    pub fn clear(&self) -> Result<usize, ApiError> {
        let n = self.db.with(|c| c.execute("DELETE FROM stock", []))?;
        let _ = self.events.send(StockEvent::Snapshot { stock: vec![] });
        Ok(n)
    }

    /// Folds one completed task into the stock. Returns the new entry when something changed.
    /// Idempotent: the task id is remembered in `stock_applied`.
    pub fn apply_task(&self, e: &LedgerEntry) -> Result<Option<StockEntry>, ApiError> {
        if e.state != TaskState::Completed || task_delta(&e.plc_task).is_none() {
            return Ok(None);
        }
        let _g = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        self.apply_locked(e)
    }

    /// `apply_task` 본체 — 호출자가 `lock` 을 잡고 있어야 한다.
    fn apply_locked(&self, e: &LedgerEntry) -> Result<Option<StockEntry>, ApiError> {
        let Some(delta) = task_delta(&e.plc_task) else { return Ok(None) };
        if e.state != TaskState::Completed || self.is_applied(&e.id)? {
            return Ok(None);
        }
        let cur = self.get(delta.cell_id)?.unwrap_or(StockEntry { cell_id: delta.cell_id, ..Default::default() });
        let (item_code, count) = fold(cur.item_code, cur.count, &delta);
        let reason = format!("{} #{}:{} {}", if delta.count >= 0 { "DROP" } else { "PICK" }, e.work_id, e.task_id, delta.count);
        let out = self.set(delta.cell_id, item_code, count, &cur.note, &reason)?;
        self.db.with(|c| c.execute("INSERT OR IGNORE INTO stock_applied (task_id, applied_at) VALUES (?1, ?2)", (&e.id, now_str())))?;
        Ok(Some(out))
    }

    fn is_applied(&self, task_id: &str) -> Result<bool, ApiError> {
        Ok(self.db.with(|c| c.query_row("SELECT COUNT(*) FROM stock_applied WHERE task_id = ?1", [task_id], |r| r.get::<_, i64>(0)))? > 0)
    }

    /// 셀 `cell_id` 의 **예상** 재고 — 재고 표 값 + 아직 접히지 않은 원장 작업(`entries`, 모든 로봇).
    ///
    /// 미리 넣기(pre-queue)로 앞 작업이 끝나기 전에 다음 작업을 작성하면 재고 표는 아직 옛 값이다.
    /// 같은 셀을 다시 만지는 작업(DROP → PICK)의 Z 가 틀리지 않게, 완료 때 `apply_task` 가 할 일을 미리 계산한다.
    ///
    /// 한 번만 세기(이중 계산 없음): `lock` 을 잡은 채
    /// 1. 방금 `Completed` 가 됐는데 구독 태스크(`spawn`)가 아직 접지 않은 작업은 **여기서 접는다**(멱등, `FRESH_FOLD_MS` 안만),
    /// 2. 표를 읽고,
    /// 3. 진행 중(`Submitted`/`Accepted`/`Queued`/`Running`) 작업만 원장 순서(`seq`)로 더한다.
    ///
    /// 원장은 **잠금을 잡은 뒤** 읽는다(`entries`). 원장 캐시는 완료 이벤트를 보내기 전에 바뀌고 접기는 그 이벤트 뒤에
    /// 잠금 안에서만 일어나므로, 잠금 안에서 본 `Running` 작업은 아직 표에 없다 — 한 작업은 표 또는 대기 목록 한쪽에만 있다.
    pub fn projected(&self, cell_id: u16, entries: impl FnOnce() -> Vec<LedgerEntry>) -> Result<Projection, ApiError> {
        let _g = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let entries = entries();
        let now = time::OffsetDateTime::now_utc();
        for e in &entries {
            let fresh = e.ended_at.as_deref().and_then(crate::ledger::parse_rfc3339).map(|t| (now - t).whole_milliseconds() <= FRESH_FOLD_MS as i128).unwrap_or(false);
            if e.state == TaskState::Completed && fresh && task_delta(&e.plc_task).is_some_and(|d| d.cell_id == cell_id) {
                self.apply_locked(e)?;
            }
        }
        let base = self.get(cell_id)?;
        Ok(project(cell_id, base, &entries))
    }
}

/// 방금 끝난 작업을 `projected` 가 대신 접어 주는 창(ms). 그보다 오래된 미적용 `Completed` 는
/// 운전자가 재고를 손으로 고쳤을 수 있으므로 건드리지 않는다.
pub const FRESH_FOLD_MS: u64 = 60_000;

/// 재고에 아직 접히지 않은 작업 하나(예상 재고의 근거).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PendingDelta {
    pub id: String,
    pub work_id: u32,
    pub task_id: u32,
    pub plc: String,
    pub state: TaskState,
    pub item_code: u32,
    /// +n DROP, −n PICK.
    pub count: i32,
}

/// `Stock::projected` 결과.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Projection {
    pub cell_id: u16,
    /// 재고 표 값.
    pub base: Option<StockEntry>,
    /// 대기 작업을 반영한 값 — 대기 작업이 없으면 `base` 그대로.
    pub projected: Option<StockEntry>,
    pub pending: Vec<PendingDelta>,
    /// 이 셀을 겨냥한 `Lost` 작업 수 — 끝났는지 모르므로 반영하지 않는다.
    pub lost: u32,
}

impl Projection {
    pub fn base_count(&self) -> u32 {
        self.base.as_ref().map(|s| s.count).unwrap_or(0)
    }
    pub fn count(&self) -> u32 {
        self.projected.as_ref().map(|s| s.count).unwrap_or(0)
    }
    /// compose 경고 한 줄(반영한 것이 없고 Lost 도 없으면 `None`).
    pub fn note(&self, what: &str) -> Option<String> {
        let mut parts = Vec::new();
        if !self.pending.is_empty() {
            parts.push(format!("대기 중 Task {} 건 반영: {what} {} 재고 {} → {}", self.pending.len(), self.cell_id, self.base_count(), self.count()));
        }
        if self.lost > 0 {
            parts.push(format!("{what} {} 을 겨냥한 Lost Task {} 건은 재고에 반영하지 않음 — 확인 필요", self.cell_id, self.lost));
        }
        (!parts.is_empty()).then(|| parts.join(" · "))
    }
}

/// 재고에 아직 없는, 끝나면 접힐 상태.
pub fn is_pending(s: TaskState) -> bool {
    matches!(s, TaskState::Submitted | TaskState::Accepted | TaskState::Queued | TaskState::Running)
}

/// 순수 예상 — `base` 위에 `entries` 중 이 셀의 진행 중 작업을 `seq` 순서로 `fold`.
pub fn project(cell_id: u16, base: Option<StockEntry>, entries: &[LedgerEntry]) -> Projection {
    let mut mine: Vec<(&LedgerEntry, Delta)> = entries.iter().filter_map(|e| task_delta(&e.plc_task).filter(|d| d.cell_id == cell_id).map(|d| (e, d))).collect();
    let lost = mine.iter().filter(|(e, _)| e.state == TaskState::Lost).count() as u32;
    mine.retain(|(e, _)| is_pending(e.state));
    mine.sort_by_key(|(e, _)| e.seq);
    let mut out = Projection { cell_id, base: base.clone(), projected: base.clone(), pending: Vec::new(), lost };
    if mine.is_empty() {
        return out;
    }
    let mut cur = base.unwrap_or(StockEntry { cell_id, ..Default::default() });
    for (e, d) in mine {
        let (code, n) = fold(cur.item_code, cur.count, &d);
        // `Stock::set` 과 같이 0 개면 품목도 0.
        cur.item_code = if n == 0 { 0 } else { code };
        cur.count = n;
        out.pending.push(PendingDelta { id: e.id.clone(), work_id: e.work_id, task_id: e.task_id, plc: e.plc_name.clone(), state: e.state, item_code: d.item_code, count: d.count });
    }
    cur.note = "projected".into();
    out.projected = Some(cur);
    out
}

struct Delta {
    cell_id: u16,
    item_code: u32,
    /// +n for DROP, −n for PICK.
    count: i32,
}

/// 한 작업을 재고에 접는 규칙 — `apply_task`(완료 시)와 `project`(예상)가 같이 쓴다.
/// DROP: 품목 = 작업 품목, 개수 + n · PICK: 개수 − n(0 에서 멈춤), 남으면 기존 품목(없으면 작업 품목), 다 비면 0.
fn fold(cur_item: u32, cur_count: u32, delta: &Delta) -> (u32, u32) {
    if delta.count >= 0 {
        (delta.item_code, cur_count + delta.count as u32)
    } else {
        let left = cur_count.saturating_sub((-delta.count) as u32);
        (
            if left == 0 {
                0
            } else if cur_item != 0 {
                cur_item
            } else {
                delta.item_code
            },
            left,
        )
    }
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

    fn entry(id: &str, seq: i64, tt: TaskType, cell: u16, code: u32, n: u8, state: TaskState) -> LedgerEntry {
        let mut t = TaskData { task_type: tt.code(), ..Default::default() };
        t.cell.id = cell;
        t.item.code = code;
        t.item.count = n;
        LedgerEntry {
            id: id.into(),
            seq,
            work_id: 1,
            task_id: seq as u32,
            origin: crate::ledger::Origin::Scenario,
            plc_name: "GR2".into(),
            request: None,
            resolved: None,
            position: t.position,
            plc_task: t,
            state,
            state_at: String::new(),
            created_at: String::new(),
            submitted_at: None,
            ended_at: (state == TaskState::Completed).then(now_str),
            ack: None,
            header: None,
            plc: None,
            error: None,
            history: vec![],
            station_offset: None,
            pallet: None,
        }
    }

    /// 진행 중 DROP/PICK 은 seq 순서로 접히고, MOVE/MEASURE·다른 셀·끝난 작업·초안은 빠진다. Lost 는 세기만.
    #[test]
    fn project_folds_pending_in_ledger_order() {
        let base = Some(StockEntry { cell_id: 401, item_code: 1001, count: 3, ..Default::default() });
        let es = vec![
            entry("p", 5, TaskType::Pick, 401, 1001, 2, TaskState::Queued),
            entry("d", 4, TaskType::Drop, 401, 1001, 1, TaskState::Running),
            entry("m", 6, TaskType::Measure, 401, 1001, 1, TaskState::Queued),
            entry("v", 7, TaskType::Move, 401, 0, 1, TaskState::Accepted),
            entry("x", 8, TaskType::Drop, 402, 1001, 1, TaskState::Submitted),
            entry("c", 3, TaskType::Drop, 401, 1001, 1, TaskState::Completed),
            entry("f", 9, TaskType::Drop, 401, 1001, 1, TaskState::Failed),
            entry("r", 10, TaskType::Drop, 401, 1001, 1, TaskState::Draft),
            entry("l", 11, TaskType::Pick, 401, 1001, 1, TaskState::Lost),
        ];
        let p = project(401, base.clone(), &es);
        assert_eq!(p.pending.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), vec!["d", "p"], "seq order, only pending PICK/DROP of this cell");
        assert_eq!((p.base_count(), p.count(), p.lost), (3, 2, 1));
        let note = p.note("셀").unwrap();
        assert!(note.contains("대기 중 Task 2 건 반영: 셀 401 재고 3 → 2") && note.contains("Lost Task 1 건"), "{note}");
        // 비어 가는 셀: 품목 0, 빈 셀에 DROP 하면 그 품목
        let p = project(401, base.clone(), &[entry("p", 1, TaskType::Pick, 401, 1001, 3, TaskState::Running), entry("d", 2, TaskType::Drop, 401, 2002, 2, TaskState::Queued)]);
        let e = p.projected.unwrap();
        assert_eq!((e.item_code, e.count), (2002, 2));
        // 대기 작업이 없으면 표 그대로, 경고 없음
        let p = project(401, base.clone(), &[]);
        assert_eq!((p.projected.clone(), p.note("셀")), (base, None));
        // 모르는 셀에 DROP 두 번
        let p = project(9, None, &[entry("a", 1, TaskType::Drop, 9, 7, 1, TaskState::Submitted), entry("b", 2, TaskType::Drop, 9, 7, 1, TaskState::Accepted)]);
        assert_eq!((p.base_count(), p.count()), (0, 2));
    }

    /// 한 작업은 표나 대기 목록 한쪽에서만 센다 — 진행 중 → 완료(구독 태스크가 아직 안 접음) → 접힘 어디서도 같은 값.
    #[test]
    fn projected_counts_each_task_once_across_completion() {
        let s = Stock::new(Db::open_memory().unwrap());
        s.set(401, 1001, 3, "", "seed").unwrap();
        let running = entry("d", 1, TaskType::Drop, 401, 1001, 1, TaskState::Running);
        assert_eq!(s.projected(401, || vec![running.clone()]).unwrap().count(), 4);
        assert_eq!(s.get(401).unwrap().unwrap().count, 3, "running tasks are not written to the table");
        // 원장은 Completed, 구독 태스크는 아직 — projected 가 대신 접는다(멱등)
        let done = entry("d", 1, TaskType::Drop, 401, 1001, 1, TaskState::Completed);
        let p = s.projected(401, || vec![done.clone()]).unwrap();
        assert_eq!((p.base_count(), p.count(), p.pending.len()), (4, 4, 0));
        // 늦게 온 구독 태스크의 접기는 무시된다
        assert!(s.apply_task(&done).unwrap().is_none());
        assert_eq!(s.projected(401, || vec![done.clone()]).unwrap().count(), 4);
        // 오래된 미적용 Completed 는 건드리지 않는다(운전자가 재고를 고쳤을 수 있음)
        let mut old = entry("o", 2, TaskType::Drop, 401, 1001, 1, TaskState::Completed);
        old.ended_at = Some("2020-01-01T00:00:00Z".into());
        assert_eq!(s.projected(401, || vec![old]).unwrap().count(), 4);
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
