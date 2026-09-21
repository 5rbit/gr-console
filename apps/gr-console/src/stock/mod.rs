//! Per-cell stock (화물 재고). The PLC keeps no inventory table, so the console owns it:
//! - edited by hand (`PUT /api/stock/{cell}`), or
//! - folded automatically from **completed** PICK / DROP tasks on the ledger (idempotent per task id).
//!
//! PICK/DROP 은 늘 한 짝이다 — 그 사이 화물은 로봇 그리퍼(**Hand**, 로봇 상태 PLC 별 한 줄)에 있다.
//! PICK 완료: 셀 − n, Hand + n · DROP 완료: Hand − n, 셀 + n. 예상 재고(`project`/`project_hand`)도 같은 규칙.
//!
//! `compose` reads it to place Z on the stack:
//! - PICK/MEASURE: `floor + H * (n - c) + grip` (top tire of the `c` being taken)
//! - DROP:         `floor + H * n + grip`       (first tire landing on `n` already there)
//!
//! `grip` 은 기준(`mid` = H/2 — 기본, `pick_bead` = 상부 비드 − PickBeadOffset)에 따라 다르고,
//! 눌림양(`ItemSpec.compression`)이 있으면 **타이어 위에 얹힌 개수**만큼 단 높이와 비드가 내려간다
//! (`pressed_height`, 전체 규칙은 `registry::spec::stack_z_with` 와 `docs/item-spec-z.md`).

pub mod routes;
pub mod sync;
pub mod transfer;

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
    Snapshot {
        stock: Vec<StockEntry>,
    },
    Upsert {
        entry: StockEntry,
        reason: String,
    },
    Remove {
        cell_id: u16,
    },
    /// 로봇 Hand 가 바뀜.
    Hand {
        hand: HandEntry,
        reason: String,
    },
    /// 로봇 실제 상태와의 동기화 경고 목록이 바뀜(`sync.rs`).
    Sync {
        plc: String,
        issues: Vec<sync::SyncIssue>,
    },
}

/// 로봇 그리퍼에 든 화물. `plc` = 로봇 상태 PLC 이름(원장 `plc_name`).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct HandEntry {
    pub plc: String,
    /// 0 = 빈 손 / 모르는 품목.
    pub item_code: u32,
    pub count: u32,
    /// 손에 든 화물의 이송 지시(PICK 완료로 열리고 DROP 완료로 닫힌다).
    #[serde(default)]
    pub transfer_order_id: Option<String>,
    pub updated_at: String,
}

pub struct Stock {
    db: Db,
    pub events: broadcast::Sender<StockEvent>,
    lock: std::sync::Mutex<()>,
    /// 동기화 판정 이력(로봇별 경고).
    pub sync: std::sync::Mutex<sync::Tracker>,
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
        Arc::new(Stock { db, events: tx, lock: std::sync::Mutex::new(()), sync: Default::default() })
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
        self.set_logged(cell_id, item_code, count, note, reason, None, None)
    }

    /// `set` + 재고 변경 기록(`stock_log`) — 작업 완료면 Task·이송 지시를, 손 정정이면 지시(있으면)를 단다.
    #[allow(clippy::too_many_arguments)]
    pub fn set_logged(&self, cell_id: u16, item_code: u32, count: u32, note: &str, reason: &str, task_id: Option<&str>, order: Option<&str>) -> Result<StockEntry, ApiError> {
        let before = self.get(cell_id)?.map(|s| (s.item_code, s.count)).unwrap_or((0, 0));
        let now = now_str();
        let item_code = if count == 0 { 0 } else { item_code };
        transfer::log_change(&self.db, "cell", &cell_id.to_string(), before, (item_code, count), reason, task_id, order)?;
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

    /// 모든 Hand(한 번이라도 기록된 로봇).
    pub fn hands(&self) -> Result<Vec<HandEntry>, ApiError> {
        let rows: Vec<HandEntry> = self.db.with(|c| {
            let mut st = c.prepare("SELECT plc, item_code, count, updated_at, transfer_order_id FROM hand ORDER BY plc")?;
            let it =
                st.query_map([], |r| Ok(HandEntry { plc: r.get(0)?, item_code: r.get::<_, i64>(1)? as u32, count: r.get::<_, i64>(2)? as u32, updated_at: r.get(3)?, transfer_order_id: r.get(4)? }))?;
            it.collect()
        })?;
        Ok(rows)
    }

    /// 로봇 `plc` 의 Hand(기록 없으면 빈 손).
    pub fn hand(&self, plc: &str) -> Result<HandEntry, ApiError> {
        Ok(self.hands()?.into_iter().find(|h| h.plc == plc).unwrap_or(HandEntry { plc: plc.into(), ..Default::default() }))
    }

    /// Hand 를 적는다(손 정정 · 접기). 0 개면 품목도 지시도 비운다. 모든 변경은 `stock_log` 에 남는다.
    pub fn set_hand(&self, plc: &str, item_code: u32, count: u32, reason: &str, task_id: Option<&str>, order: Option<&str>) -> Result<HandEntry, ApiError> {
        let now = now_str();
        let item_code = if count == 0 { 0 } else { item_code };
        let order = if count == 0 { None } else { order };
        let before = self.hand(plc)?;
        self.db.with(|c| {
            c.execute(
                "INSERT INTO hand (plc, item_code, count, updated_at, transfer_order_id) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(plc) DO UPDATE SET item_code=excluded.item_code, count=excluded.count, updated_at=excluded.updated_at, transfer_order_id=excluded.transfer_order_id",
                (plc, item_code, count, &now, order),
            )
        })?;
        transfer::log_change(&self.db, "hand", plc, (before.item_code, before.count), (item_code, count), reason, task_id, order.or(before.transfer_order_id.as_deref()))?;
        let h = HandEntry { plc: plc.into(), item_code, count, updated_at: now, transfer_order_id: order.map(str::to_string) };
        let _ = self.events.send(StockEvent::Hand { hand: h.clone(), reason: reason.into() });
        Ok(h)
    }

    /// Folds one completed task into the stock. Returns the new entry when something changed.
    /// Idempotent: the task id is remembered in `stock_applied`.
    pub fn apply_task(&self, e: &LedgerEntry) -> Result<Option<StockEntry>, ApiError> {
        if e.state != TaskState::Completed || task_delta(&e.plc_task).is_none() {
            return Ok(None);
        }
        let _g = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        self.apply_locked(e, None)
    }

    /// 동기화가 찾은 "완료됐는데 안 접힌" Task 를 접는다 — 재고 기록 사유 `plc-sync`.
    pub fn apply_task_sync(&self, e: &LedgerEntry) -> Result<Option<StockEntry>, ApiError> {
        let _g = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        self.apply_locked(e, Some("plc-sync"))
    }

    /// 완료됐는데 아직 재고에 안 접힌 PICK/DROP(동기화 판정용).
    pub fn unfolded<'a>(&self, entries: &'a [LedgerEntry]) -> Result<Vec<(&'a LedgerEntry, Option<String>)>, ApiError> {
        let mut out = Vec::new();
        for e in entries {
            if e.state == TaskState::Completed && task_delta(&e.plc_task).is_some() && !self.is_applied(&e.id)? {
                out.push((e, self.held_reason(e)?));
            }
        }
        Ok(out)
    }

    /// 이송 지시 이력에 한 줄(상태는 그대로) — 동기화 경고 등.
    pub fn note_order(&self, id: &str, note: &str) -> Result<(), ApiError> {
        let _g = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(mut o) = transfer::get(&self.db, id)? {
            transfer::add_note(&mut o, note);
            transfer::save(&self.db, &o)?;
        }
        Ok(())
    }

    /// 사람이 고친 Hand 의 새 지시(`adopt_plc`) — `in_hand` 로 연다.
    pub fn open_in_hand_order(&self, n: transfer::NewOrder, note: &str) -> Result<transfer::TransferOrder, ApiError> {
        let _g = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut o = transfer::create(&self.db, n, transfer::OrderState::Planned)?;
        transfer::set_state(&mut o, transfer::OrderState::InHand, note);
        transfer::save(&self.db, &o)?;
        Ok(o)
    }

    /// `apply_task` 본체 — 호출자가 `lock` 을 잡고 있어야 한다.
    /// 완료 반영 본체(정확히 한 번 — `stock_applied` 가 Task id 로 막는다). `force` 가 아니면 손 정정 가드를 본다:
    /// 대상 셀이나 Hand 가 이 Task 완료 **뒤에** 손으로 고쳐졌으면 반영하지 않는다(`held_reason`, 동기화 경고로 사람이 정한다).
    fn apply_locked(&self, e: &LedgerEntry, prefix: Option<&str>) -> Result<Option<StockEntry>, ApiError> {
        self.apply_locked_with(e, prefix, false)
    }

    fn apply_locked_with(&self, e: &LedgerEntry, prefix: Option<&str>, force: bool) -> Result<Option<StockEntry>, ApiError> {
        let Some(delta) = task_delta(&e.plc_task) else { return Ok(None) };
        if e.state != TaskState::Completed || self.is_applied(&e.id)? {
            return Ok(None);
        }
        if !force && self.held_reason(e)?.is_some() {
            return Ok(None);
        }
        let cur = self.get(delta.cell_id)?.unwrap_or(StockEntry { cell_id: delta.cell_id, ..Default::default() });
        let (item_code, count) = fold(cur.item_code, cur.count, &delta);
        let base = format!("{} #{}:{} {}", if delta.count >= 0 { "DROP" } else { "PICK" }, e.work_id, e.task_id, delta.count);
        let reason = match prefix {
            Some(p) => format!("{p}: {base}"),
            None => base,
        };
        let h = self.hand(&e.plc_name)?;
        // 이 이동의 이송 지시 — Task 에 달려 있으면 그것, DROP 은 손에 든 화물의 지시, 둘 다 없으면(외부 Task) 새로 연다.
        let mut order = match e.transfer_order_id.as_deref().or(if delta.count > 0 { h.transfer_order_id.as_deref() } else { None }) {
            Some(id) => transfer::get(&self.db, id)?,
            None => None,
        };
        if order.is_none() {
            let target = e.request.as_ref().and_then(|r| r.target.clone()).unwrap_or(crate::ledger::Target { kind: "cell".into(), id: delta.cell_id });
            let (from, to) = if delta.count < 0 { (Some(target), None) } else { (None, Some(target)) };
            let note = format!("{} #{}:{} 에 지시 없음 — 완료 때 자동 생성", if delta.count < 0 { "PICK" } else { "DROP" }, e.work_id, e.task_id);
            let n = transfer::NewOrder { robot: None, plc: e.plc_name.clone(), item_code: delta.item_code, count: delta.count.unsigned_abs(), from, to, source: "external".into(), note };
            order = Some(transfer::create(&self.db, n, transfer::OrderState::Planned)?);
        }
        let mut order = order.expect("order set above");
        let out = self.set_logged(delta.cell_id, item_code, count, &cur.note, &reason, Some(&e.id), Some(&order.id))?;
        // 같은 작업으로 Hand 도 옮긴다(PICK 은 손으로, DROP 은 손에서).
        let (hi, hn) = fold_hand(h.item_code, h.count, &delta);
        self.set_hand(&e.plc_name, hi, hn, &reason, Some(&e.id), Some(&order.id))?;
        if let Some(tt) = TaskType::from_code(e.plc_task.task_type) {
            transfer::apply_task_state(&mut order, tt, &e.id, TaskState::Completed);
            transfer::save(&self.db, &order)?;
        }
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
    /// 1. `Completed` 인데 아직 안 접힌 작업은 **여기서 접는다**(Task id 로 정확히 한 번, 원장 순서, 손 정정 가드),
    /// 2. 표를 읽고,
    /// 3. 진행 중(`Submitted`/`Accepted`/`Queued`/`Running`) 작업만 원장 순서(`seq`)로 더한다.
    ///
    /// 원장은 **잠금을 잡은 뒤** 읽는다(`entries`). 원장 캐시는 완료 이벤트를 보내기 전에 바뀌고 접기는 그 이벤트 뒤에
    /// 잠금 안에서만 일어나므로, 잠금 안에서 본 `Running` 작업은 아직 표에 없다 — 한 작업은 표 또는 대기 목록 한쪽에만 있다.
    pub fn projected(&self, cell_id: u16, entries: impl FnOnce() -> Vec<LedgerEntry>) -> Result<Projection, ApiError> {
        let _g = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let entries = entries();
        self.fold_fresh(&entries)?;
        let base = self.get(cell_id)?;
        Ok(project(cell_id, base, &entries))
    }

    /// 원장 Task 의 상태를 그 이송 지시에 반영(완료는 `apply_task` 가 재고와 함께 한다). 바뀐 지시를 알린다.
    pub fn on_task(&self, e: &LedgerEntry) -> Result<Option<transfer::TransferOrder>, ApiError> {
        let (Some(id), Some(tt)) = (e.transfer_order_id.as_deref(), TaskType::from_code(e.plc_task.task_type)) else { return Ok(None) };
        let _g = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(mut o) = transfer::get(&self.db, id)? else { return Ok(None) };
        if e.state == TaskState::Completed && !self.is_applied(&e.id)? {
            return Ok(None); // 접기가 재고와 함께 옮긴다
        }
        let picked = o.pick_state == Some(TaskState::Completed);
        if transfer::apply_task_state(&mut o, tt, &e.id, e.state) {
            transfer::save(&self.db, &o)?;
            // PLC 에서 DROP 을 지우면 PLC 가 그리퍼 화물 데이터(GRIPPER.Item)도 지운다(CL_Common_Command) — 콘솔 Hand 도 비운다.
            if tt == TaskType::Drop && e.state == TaskState::Canceled && picked {
                let h = self.hand(&e.plc_name)?;
                if h.count > 0 && h.transfer_order_id.as_deref().is_none_or(|x| x == id) {
                    let reason = format!("task-delete: DROP #{}:{} 취소 — PLC 가 그리퍼 화물 데이터를 지움", e.work_id, e.task_id);
                    self.set_hand(&e.plc_name, 0, 0, &reason, Some(&e.id), Some(id))?;
                }
            }
            return Ok(Some(o));
        }
        Ok(None)
    }

    /// 새 이송 지시(짝을 보내기 직전, 실행기).
    pub fn open_order(&self, n: transfer::NewOrder) -> Result<transfer::TransferOrder, ApiError> {
        transfer::create(&self.db, n, transfer::OrderState::Planned)
    }

    pub fn order(&self, id: &str) -> Result<Option<transfer::TransferOrder>, ApiError> {
        transfer::get(&self.db, id)
    }

    pub fn orders(&self, q: &transfer::OrderQuery) -> Result<(Vec<transfer::TransferOrder>, i64), ApiError> {
        transfer::list(&self.db, q)
    }

    pub fn order_changes(&self, id: &str) -> Result<Vec<transfer::StockChange>, ApiError> {
        transfer::changes_of(&self.db, id)
    }

    /// 사람이 지시를 끝낸다(보내지 못한 채 실행이 멈춤 · Hand 손 정정). 열린 지시만.
    pub fn abort_order(&self, id: &str, note: &str, only_planned: bool) -> Result<(), ApiError> {
        let _g = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(mut o) = transfer::get(&self.db, id)?
            && o.state.is_open()
            && (!only_planned || o.state == transfer::OrderState::Planned)
        {
            transfer::set_state(&mut o, transfer::OrderState::Aborted, note);
            transfer::save(&self.db, &o)?;
        }
        Ok(())
    }

    /// 로봇 `plc` 의 **예상** Hand — `projected` 와 같은 잠금·접기 규칙.
    pub fn projected_hand(&self, plc: &str, entries: impl FnOnce() -> Vec<LedgerEntry>) -> Result<HandProjection, ApiError> {
        let _g = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let entries = entries();
        self.fold_fresh(&entries)?;
        let base = self.hand(plc)?;
        Ok(project_hand(base, &entries))
    }

    /// 모든 셀·Hand 의 예상 값(계획 미리보기 출발점). 대기 작업이 닿는 셀은 표에 없어도 나온다.
    pub fn projected_all(&self, entries: impl FnOnce() -> Vec<LedgerEntry>) -> Result<(Vec<StockEntry>, Vec<HandEntry>), ApiError> {
        let _g = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let entries = entries();
        self.fold_fresh(&entries)?;
        let cells = self.list()?.into_iter().map(|s| (s.cell_id, s)).collect();
        let hands = self.hands()?.into_iter().map(|h| (h.plc.clone(), h)).collect();
        Ok(project_all(cells, hands, &entries))
    }

    /// `Completed` 인데 아직 안 접힌 작업을 원장 순서(`seq`)로 접는다 — Task id 로 정확히 한 번, 손 정정 가드. 잠금을 잡고 부른다.
    fn fold_fresh(&self, entries: &[LedgerEntry]) -> Result<usize, ApiError> {
        let mut done: Vec<&LedgerEntry> = entries.iter().filter(|e| e.state == TaskState::Completed && task_delta(&e.plc_task).is_some()).collect();
        done.sort_by_key(|e| e.seq);
        let mut n = 0;
        for e in done {
            if self.apply_locked(e, None)?.is_some() {
                n += 1;
            }
        }
        Ok(n)
    }

    /// 기동 때 따라잡기 — 콘솔이 꺼진 동안 끝난(원장 완료, 재고 미반영) Task 를 원장 순서로 반영한다(같은 가드). 반영한 수.
    pub fn catch_up(&self, entries: &[LedgerEntry]) -> Result<usize, ApiError> {
        let _g = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        self.fold_fresh(entries)
    }

    /// 손 정정 가드 — 대상 셀·Hand 의 마지막 손 정정(`manual` · `sync-resolve`)이 이 Task 완료 시각 뒤면 그 사유.
    pub fn held_reason(&self, e: &LedgerEntry) -> Result<Option<String>, ApiError> {
        let Some(delta) = task_delta(&e.plc_task) else { return Ok(None) };
        let Some(ended) = e.ended_at.as_deref().and_then(crate::ledger::parse_rfc3339) else { return Ok(None) };
        for (kind, key, what) in [("cell", delta.cell_id.to_string(), format!("셀 {}", delta.cell_id)), ("hand", e.plc_name.clone(), format!("Hand {}", e.plc_name))] {
            let last: Option<String> = self.db.with(|c| {
                c.query_row("SELECT at FROM stock_log WHERE kind = ?1 AND key = ?2 AND (reason LIKE 'manual%' OR reason LIKE 'sync-resolve%') ORDER BY id DESC LIMIT 1", (kind, &key), |r| r.get(0))
                    .map(Some)
                    .or_else(|x| if x == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(x) })
            })?;
            if let Some(at) = last
                && crate::ledger::parse_rfc3339(&at).is_some_and(|t| t > ended)
            {
                return Ok(Some(format!("{what} 을 완료({}) 뒤 {at} 에 손으로 고침", e.ended_at.as_deref().unwrap_or(""))));
            }
        }
        Ok(None)
    }

    /// 가드로 멈춘 Task 를 사람이 정한다 — `apply` 면 그대로 반영, 아니면 반영 없이 "무시" 로 한 번 기록.
    pub fn decide_held(&self, e: &LedgerEntry, apply: bool) -> Result<(), ApiError> {
        let _g = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        if self.is_applied(&e.id)? {
            return Ok(());
        }
        if apply {
            self.apply_locked_with(e, Some("sync-resolve: 반영"), true)?;
            return Ok(());
        }
        let Some(delta) = task_delta(&e.plc_task) else { return Ok(()) };
        let cur = self.get(delta.cell_id)?.map(|s| (s.item_code, s.count)).unwrap_or((0, 0));
        let reason = format!("sync-resolve: 무시 — {} #{}:{} (손 정정 뒤 완료)", if delta.count >= 0 { "DROP" } else { "PICK" }, e.work_id, e.task_id);
        transfer::log_change(&self.db, "cell", &delta.cell_id.to_string(), cur, cur, &reason, Some(&e.id), e.transfer_order_id.as_deref())?;
        self.db.with(|c| c.execute("INSERT OR IGNORE INTO stock_applied (task_id, applied_at, outcome) VALUES (?1, ?2, 'ignored')", (&e.id, now_str())))?;
        Ok(())
    }
}

/// 순수 — 표(셀·Hand) 위에 진행 중 PICK/DROP 을 `seq` 순서로 접는다(`fold` + `fold_hand`).
pub fn project_all(mut cells: std::collections::BTreeMap<u16, StockEntry>, mut hands: std::collections::BTreeMap<String, HandEntry>, entries: &[LedgerEntry]) -> (Vec<StockEntry>, Vec<HandEntry>) {
    let mut mine: Vec<(&LedgerEntry, Delta)> = entries.iter().filter(|e| is_pending(e.state)).filter_map(|e| task_delta(&e.plc_task).map(|d| (e, d))).collect();
    mine.sort_by_key(|(e, _)| e.seq);
    for (e, d) in mine {
        let c = cells.entry(d.cell_id).or_insert_with(|| StockEntry { cell_id: d.cell_id, ..Default::default() });
        let (code, n) = fold(c.item_code, c.count, &d);
        c.item_code = if n == 0 { 0 } else { code };
        c.count = n;
        let h = hands.entry(e.plc_name.clone()).or_insert_with(|| HandEntry { plc: e.plc_name.clone(), ..Default::default() });
        let (hi, hn) = fold_hand(h.item_code, h.count, &d);
        h.item_code = if hn == 0 { 0 } else { hi };
        h.count = hn;
    }
    (cells.into_values().collect(), hands.into_values().collect())
}

/// `Stock::projected_hand` 결과.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct HandProjection {
    pub base: HandEntry,
    /// 이 로봇의 진행 중 PICK/DROP 을 다 접은 값.
    pub projected: HandEntry,
    pub pending: Vec<PendingDelta>,
    pub lost: u32,
}

/// 순수 예상 Hand — `base.plc` 로봇의 진행 중 PICK/DROP 을 `seq` 순서로 `fold_hand`.
pub fn project_hand(base: HandEntry, entries: &[LedgerEntry]) -> HandProjection {
    let mut mine: Vec<(&LedgerEntry, Delta)> = entries.iter().filter(|e| e.plc_name == base.plc).filter_map(|e| task_delta(&e.plc_task).map(|d| (e, d))).collect();
    let lost = mine.iter().filter(|(e, _)| e.state == TaskState::Lost).count() as u32;
    mine.retain(|(e, _)| is_pending(e.state));
    mine.sort_by_key(|(e, _)| e.seq);
    let mut cur = base.clone();
    let mut pending = Vec::new();
    for (e, d) in mine {
        let (i, n) = fold_hand(cur.item_code, cur.count, &d);
        cur.item_code = if n == 0 { 0 } else { i };
        cur.count = n;
        pending.push(PendingDelta { id: e.id.clone(), work_id: e.work_id, task_id: e.task_id, plc: e.plc_name.clone(), state: e.state, item_code: d.item_code, count: d.count });
    }
    HandProjection { base, projected: cur, pending, lost }
}

/// PICK/DROP 짝 검사 — `hand` 는 이 작업 **앞까지** 의 예상 Hand.
/// PICK: 손이 비어 있어야 한다. DROP: 손에 든 것과 품목·수량이 같아야 한다(짝 PICK 이 먼저 나가 있어야 한다).
pub fn hand_check(tt: TaskType, hand: &HandEntry, item_code: u32, count: u32) -> Result<(), String> {
    let c = count.max(1);
    match tt {
        TaskType::Pick if hand.count > 0 => Err(format!("Hand 에 품목 {} × {} 이 있음 — 짝 DROP 을 먼저 보내야 PICK 할 수 있음", hand.item_code, hand.count)),
        TaskType::Drop if hand.count == 0 => Err("Hand 가 비어 있음 — DROP 은 짝 PICK 뒤에만 보낼 수 있음".into()),
        TaskType::Drop if item_code != 0 && hand.item_code != 0 && item_code != hand.item_code => Err(format!("DROP 품목 {item_code} ≠ Hand 품목 {}", hand.item_code)),
        TaskType::Drop if c != hand.count => Err(format!("DROP 수량 {c} ≠ Hand {} 개", hand.count)),
        _ => Ok(()),
    }
}

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

/// 한 작업을 Hand 에 접는 규칙 — PICK: 품목 = 작업 품목(없으면 손 품목), + n · DROP: − n(0 에서 멈춤), 비면 품목 0.
fn fold_hand(cur_item: u32, cur_count: u32, delta: &Delta) -> (u32, u32) {
    if delta.count < 0 {
        (if delta.item_code != 0 { delta.item_code } else { cur_item }, cur_count + (-delta.count) as u32)
    } else {
        let left = cur_count.saturating_sub(delta.count as u32);
        (if left == 0 { 0 } else { cur_item }, left)
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
                Ok(LedgerEvent::Upsert { task }) => match stock.apply_task(&task).and_then(|r| stock.on_task(&task).map(|_| r)) {
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
            transfer_order_id: None,
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
        // 오래된 미적용 Completed 도 접는다(시간 창 없음) — 단, 그 뒤 손 정정이 있으면 가드가 멈춘다(아래 시험)
        let mut old = entry("o", 2, TaskType::Drop, 401, 1001, 1, TaskState::Completed);
        old.ended_at = Some("2020-01-01T00:00:00Z".into());
        assert_eq!(s.projected(401, || vec![old]).unwrap().count(), 5);
    }

    /// 짝 접기: PICK 완료 → 셀 − 2, Hand + 2 · DROP 완료 → Hand 0, 대상 셀 + 2. 예상 값도 같은 모양.
    #[test]
    fn pick_drop_pair_moves_through_the_hand() {
        let s = Stock::new(Db::open_memory().unwrap());
        s.set(401, 2011, 5, "", "seed").unwrap();
        let pick = entry("p", 1, TaskType::Pick, 401, 2011, 2, TaskState::Running);
        let drop = entry("d", 2, TaskType::Drop, 402, 2011, 2, TaskState::Queued);
        // 둘 다 진행 중 — 예상: 셀 401 = 3, 402 = 2, 손 = 0(들었다 놓음); DROP 앞까지의 손 = 2
        let (cells, hands) = s.projected_all(|| vec![pick.clone(), drop.clone()]).unwrap();
        let cnt = |id: u16| cells.iter().find(|c| c.cell_id == id).map(|c| c.count);
        assert_eq!((cnt(401), cnt(402)), (Some(3), Some(2)));
        assert_eq!(hands.iter().find(|h| h.plc == "GR2").map(|h| h.count), Some(0));
        let before_drop = s.projected_hand("GR2", || vec![pick.clone()]).unwrap().projected;
        assert_eq!((before_drop.item_code, before_drop.count), (2011, 2));
        assert!(hand_check(TaskType::Drop, &before_drop, 2011, 2).is_ok());
        // PICK 완료 → 표: 셀 3, 손 2011 × 2
        let pick_done = entry("p", 1, TaskType::Pick, 401, 2011, 2, TaskState::Completed);
        s.apply_task(&pick_done).unwrap();
        assert_eq!(s.get(401).unwrap().unwrap().count, 3);
        let h = s.hand("GR2").unwrap();
        assert_eq!((h.item_code, h.count), (2011, 2));
        // DROP 완료 → 손 비고 402 = 2
        s.apply_task(&entry("d", 2, TaskType::Drop, 402, 2011, 2, TaskState::Completed)).unwrap();
        assert_eq!(s.hand("GR2").unwrap().count, 0);
        assert_eq!(s.get(402).unwrap().unwrap().count, 2);
    }

    /// PICK 완료 뒤 DROP 취소 — 타이어는 손에 남고, 셀은 한 번만 빠진다. 다음 PICK 은 막히고 같은 DROP 만 된다.
    #[test]
    fn canceled_drop_leaves_the_tires_in_the_hand() {
        let s = Stock::new(Db::open_memory().unwrap());
        s.set(401, 2011, 5, "", "seed").unwrap();
        let pick_done = entry("p", 1, TaskType::Pick, 401, 2011, 2, TaskState::Completed);
        let drop_canceled = entry("d", 2, TaskType::Drop, 402, 2011, 2, TaskState::Canceled);
        s.apply_task(&pick_done).unwrap();
        s.apply_task(&drop_canceled).unwrap();
        let all = || vec![pick_done.clone(), drop_canceled.clone()];
        assert_eq!(s.projected(401, all).unwrap().count(), 3, "cell not double counted");
        assert_eq!(s.projected(402, all).unwrap().count(), 0);
        let hand = s.projected_hand("GR2", all).unwrap().projected;
        assert_eq!((hand.item_code, hand.count), (2011, 2));
        assert!(hand_check(TaskType::Pick, &hand, 2011, 1).unwrap_err().contains("짝 DROP"));
        assert!(hand_check(TaskType::Drop, &hand, 2011, 2).is_ok());
    }

    /// 이송 지시: 짝의 두 Task 가 같은 TO 를 달고, 모든 재고 변경(셀·Hand)이 그 TO 를 가리킨다.
    /// DROP 취소 → in_hand(타이어는 손에), 새 DROP 완료 → done.
    #[test]
    fn stock_changes_reference_the_transfer_order() {
        use transfer::OrderState as O;
        let s = Stock::new(Db::open_memory().unwrap());
        s.set(401, 2011, 5, "", "seed").unwrap();
        let o = s.open_order(transfer::NewOrder { plc: "GR2".into(), item_code: 2011, count: 2, source: "scenario:r".into(), ..Default::default() }).unwrap();
        let with = |mut e: LedgerEntry| {
            e.transfer_order_id = Some(o.id.clone());
            e
        };
        // PICK 진행 → picking, 완료 → in_hand + 손에 TO
        s.on_task(&with(entry("p", 1, TaskType::Pick, 401, 2011, 2, TaskState::Running))).unwrap();
        assert_eq!(s.order(&o.id).unwrap().unwrap().state, O::Picking);
        let pick = with(entry("p", 1, TaskType::Pick, 401, 2011, 2, TaskState::Completed));
        s.apply_task(&pick).unwrap();
        s.on_task(&pick).unwrap();
        assert_eq!(s.order(&o.id).unwrap().unwrap().state, O::InHand);
        assert_eq!(s.hand("GR2").unwrap().transfer_order_id.as_deref(), Some(o.id.as_str()));
        // DROP 대기 → dropping, 실패 → in_hand(타이어는 손에, 재고 그대로)
        s.on_task(&with(entry("d1", 2, TaskType::Drop, 402, 2011, 2, TaskState::Queued))).unwrap();
        assert_eq!(s.order(&o.id).unwrap().unwrap().state, O::Dropping);
        let failed = with(entry("d1", 2, TaskType::Drop, 402, 2011, 2, TaskState::Failed));
        s.apply_task(&failed).unwrap();
        s.on_task(&failed).unwrap();
        assert_eq!(s.order(&o.id).unwrap().unwrap().state, O::InHand);
        assert_eq!((s.get(401).unwrap().unwrap().count, s.hand("GR2").unwrap().count), (3, 2));
        // 단독 DROP(지시 없이) — 손에 걸린 TO 로 이어져 done
        let drop = entry("d2", 3, TaskType::Drop, 402, 2011, 2, TaskState::Completed);
        s.apply_task(&drop).unwrap();
        let done = s.order(&o.id).unwrap().unwrap();
        assert_eq!((done.state, done.drop_task.as_deref()), (O::Done, Some("d2")));
        assert_eq!(s.hand("GR2").unwrap().transfer_order_id, None);
        let log = s.order_changes(&o.id).unwrap();
        let rows: Vec<(&str, &str, u32, u32)> = log.iter().map(|c| (c.kind.as_str(), c.key.as_str(), c.count_before, c.count_after)).collect();
        assert_eq!(rows, vec![("cell", "401", 5, 3), ("hand", "GR2", 0, 2), ("cell", "402", 0, 2), ("hand", "GR2", 2, 0)]);
        assert!(log.iter().all(|c| c.transfer_order_id.as_deref() == Some(o.id.as_str()) && c.task_id.is_some()));
    }

    /// PLC 에서 DROP 을 지우면 PLC 가 그리퍼 화물 데이터를 지운다 — 콘솔 Hand 도 비우고 지시는 aborted, 기록 사유 task-delete.
    #[test]
    fn deleting_the_drop_clears_the_hand() {
        use transfer::OrderState as O;
        let s = Stock::new(Db::open_memory().unwrap());
        s.set(401, 2011, 5, "", "seed").unwrap();
        let o = s.open_order(transfer::NewOrder { plc: "GR2".into(), item_code: 2011, count: 2, ..Default::default() }).unwrap();
        let with = |mut e: LedgerEntry| {
            e.transfer_order_id = Some(o.id.clone());
            e
        };
        let pick = with(entry("p", 1, TaskType::Pick, 401, 2011, 2, TaskState::Completed));
        s.apply_task(&pick).unwrap();
        s.on_task(&pick).unwrap();
        s.on_task(&with(entry("d", 2, TaskType::Drop, 402, 2011, 2, TaskState::Running))).unwrap();
        s.on_task(&with(entry("d", 2, TaskType::Drop, 402, 2011, 2, TaskState::Canceled))).unwrap();
        assert_eq!(s.order(&o.id).unwrap().unwrap().state, O::Aborted);
        let h = s.hand("GR2").unwrap();
        assert_eq!((h.count, h.transfer_order_id), (0, None));
        assert_eq!(s.get(401).unwrap().unwrap().count, 3, "cell keeps the PICK");
        let last = s.order_changes(&o.id).unwrap().pop().unwrap();
        assert!(last.reason.starts_with("task-delete") && last.kind == "hand" && last.count_after == 0, "{last:?}");
    }

    /// 완료 반영: Task id 로 정확히 한 번, 원장 순서로, 완료 뒤 손 정정이 있으면 멈추고(가드) 사람이 반영/무시.
    #[test]
    fn completion_is_applied_exactly_once_in_order_with_a_manual_guard() {
        let s = Stock::new(Db::open_memory().unwrap());
        s.set(401, 2011, 5, "", "seed").unwrap();
        // 따라잡기(기동): 원장 순서 — DROP(seq 2) 이 PICK(seq 1) 보다 먼저 와도 PICK 부터
        let pick = entry("p", 1, TaskType::Pick, 401, 2011, 2, TaskState::Completed);
        let drop = entry("d", 2, TaskType::Drop, 402, 2011, 2, TaskState::Completed);
        assert_eq!(s.catch_up(&[drop.clone(), pick.clone()]).unwrap(), 2);
        let log: Vec<String> = s.db.with(|c| c.prepare("SELECT reason FROM stock_log WHERE task_id IS NOT NULL ORDER BY id")?.query_map([], |r| r.get(0))?.collect()).unwrap();
        assert!(log[0].starts_with("PICK") && log.last().unwrap().starts_with("DROP"), "{log:?}");
        // 두 번째 따라잡기·이벤트 접기는 아무것도 안 한다
        assert_eq!(s.catch_up(&[drop.clone(), pick.clone()]).unwrap(), 0);
        assert!(s.apply_task(&drop).unwrap().is_none());
        assert_eq!((s.get(401).unwrap().unwrap().count, s.get(402).unwrap().unwrap().count, s.hand("GR2").unwrap().count), (3, 2, 0));
        // 가드: 완료(2020) 뒤에 셀 403 을 손으로 고침 → 자동 반영하지 않고 사유를 낸다
        s.set_logged(403, 2011, 4, "", "manual", None, None).unwrap();
        let mut old = entry("o", 3, TaskType::Drop, 403, 2011, 1, TaskState::Completed);
        old.ended_at = Some("2020-01-01T00:00:00Z".into());
        assert_eq!(s.catch_up(std::slice::from_ref(&old)).unwrap(), 0);
        assert!(s.held_reason(&old).unwrap().unwrap().contains("셀 403"));
        assert_eq!(s.unfolded(std::slice::from_ref(&old)).unwrap().len(), 1);
        // 무시 → 기록만, 다시는 안 뜬다
        s.decide_held(&old, false).unwrap();
        assert!(s.unfolded(std::slice::from_ref(&old)).unwrap().is_empty());
        assert_eq!(s.get(403).unwrap().unwrap().count, 4);
        // 반영 → 가드를 넘어 한 번
        let mut old2 = entry("o2", 4, TaskType::Drop, 403, 2011, 1, TaskState::Completed);
        old2.ended_at = Some("2020-01-01T00:00:00Z".into());
        s.decide_held(&old2, true).unwrap();
        s.decide_held(&old2, true).unwrap();
        assert_eq!(s.get(403).unwrap().unwrap().count, 5);
    }

    /// 지시 없이 PLC 에서 온(외부) PICK 도 완료 때 TO 가 생겨 추적된다. 손 정정(`set`)은 지시 없이 manual 로 남는다.
    #[test]
    fn external_pick_gets_an_order_and_manual_edits_are_logged() {
        let s = Stock::new(Db::open_memory().unwrap());
        s.set(401, 2011, 5, "", "manual").unwrap();
        s.apply_task(&entry("x", 1, TaskType::Pick, 401, 2011, 1, TaskState::Completed)).unwrap();
        let h = s.hand("GR2").unwrap();
        let id = h.transfer_order_id.clone().unwrap();
        let o = s.order(&id).unwrap().unwrap();
        assert_eq!((o.state, o.source.as_str(), o.pick_task.as_deref()), (transfer::OrderState::InHand, "external", Some("x")));
        let manual: i64 = s.db.with(|c| c.query_row("SELECT COUNT(*) FROM stock_log WHERE reason = 'manual' AND transfer_order_id IS NULL", [], |r| r.get(0))).unwrap();
        assert_eq!(manual, 1);
    }

    #[test]
    fn hand_check_refuses_unpaired_tasks() {
        let empty = HandEntry { plc: "GR2".into(), ..Default::default() };
        let held = HandEntry { plc: "GR2".into(), item_code: 2011, count: 2, ..Default::default() };
        assert!(hand_check(TaskType::Pick, &empty, 2011, 2).is_ok());
        assert!(hand_check(TaskType::Drop, &empty, 2011, 2).unwrap_err().contains("비어"));
        assert!(hand_check(TaskType::Drop, &held, 2012, 2).unwrap_err().contains("2012"));
        assert!(hand_check(TaskType::Drop, &held, 2011, 1).unwrap_err().contains("수량 1"));
        assert!(hand_check(TaskType::Move, &held, 0, 1).is_ok());
        assert!(hand_check(TaskType::Measure, &held, 2011, 1).is_ok());
        // 다른 로봇의 진행 중 PICK 은 이 로봇 손에 안 들어간다
        let p = project_hand(
            empty.clone(),
            &[{
                let mut e = entry("x", 1, TaskType::Pick, 401, 2011, 2, TaskState::Running);
                e.plc_name = "GR1".into();
                e
            }],
        );
        assert_eq!(p.projected.count, 0);
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
            transfer_order_id: None,
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
