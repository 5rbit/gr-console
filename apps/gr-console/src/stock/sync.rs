//! 로봇 실제 상태와 콘솔 재고(Hand · 이송 지시)의 상시 동기화.
//!
//! GR2 가 내보내는 것(읽기 전용, `OPCUA.STAT`):
//! - `Task.Status.HoldItem` = `"GRIPPER".Item.Code <> 0` (PLC 가 기억하는 "들고 있음", `UL_TaskStatus.scl:48`)
//! - `Status.ItemDectect` = 그리퍼 감지 AND 그리퍼가 열린 끝이 아님 (`UL_MachineStatus.scl:75`)
//! - `Task.Now` / 링(Completed·Canceled·Rejected) — 원장 동기화(`ledger/sync.rs`)가 Task 상태로 옮긴다.
//!
//! 들고 있는 **품목 코드·개수**는 STAT 에 없다(`GRIPPER.Item` 은 OPC UA 로 안 나온다) — 그래서 "PLC 기준 Hand 맞춤"은
//! PLC Completed 링의 마지막 PICK 을 후보로 쓴다.
//!
//! 규칙(`reconcile`, 순수):
//! 1. 로봇이 바쁘면(Now 에 Task, 또는 진행 중 PICK/DROP) 손을 비교하지 않는다 — 작업 중의 과도 상태는 불일치가 아니다.
//! 2. 원장은 완료인데 재고에 안 접힌 PICK/DROP(구독 누락 등)은 **자동으로 접는다**(`plc-sync`) — PLC 링이 근거라 모호하지 않다.
//!    단 완료 뒤 그 셀·Hand 를 손으로 고쳤으면(`Stock::held_reason`) 접지 않고 경고 + "반영"/"무시".
//! 3. 콘솔 Hand 에 화물이 있는데 PLC 는 HoldItem=0 · 감지 없음 → 경고 + "Hand 비움".
//! 4. 콘솔 Hand 는 비었는데 PLC HoldItem=1 → 경고 + "PLC 기준 Hand 맞춤".
//! 5. HoldItem 과 그리퍼 감지가 다름 → 알림(동작 없음). Lost 된 PICK/DROP → 경고.
//!
//! 경고·자동 처리는 `DEBOUNCE_MS` 동안 이어져야 선다(스냅샷 한 번의 흔들림은 무시). 콘솔 DB 만 고치고 PLC 에는 쓰지 않는다.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gr_proto::{StatusView, TaskType};
use serde::Serialize;

use super::{HandEntry, is_pending};
use crate::ledger::{LedgerEntry, TaskState};

pub const DEBOUNCE_MS: u64 = 3000;
pub const POLL_MS: u64 = 1000;

/// 동기화에 쓰는 PLC 값.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlcView {
    pub hold_item: bool,
    pub item_detect: bool,
    /// `Task.Now` 에 Task 가 있다.
    pub busy: bool,
    /// Completed 링(최신 먼저)의 마지막 PICK 의 (품목, 개수) — "PLC 기준 Hand 맞춤" 후보.
    pub last_pick: Option<(u32, u32)>,
}

impl PlcView {
    pub fn from_status(v: &StatusView) -> PlcView {
        PlcView {
            hold_item: v.task.status.hold_item,
            item_detect: v.status.item_detect,
            busy: !v.task.now.is_zero(),
            last_pick: v.task.completed.iter().find(|t| !t.is_zero() && t.task_type() == Some(TaskType::Pick)).map(|t| (t.item.code, t.item.count.max(1) as u32)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SyncIssue {
    /// `hand_stale` · `plc_holds` · `sensor_mismatch` · `task_lost`
    pub code: String,
    pub message: String,
    pub transfer_order_id: Option<String>,
    /// 한 번에 고치는 동작 — `clear_hand` · `adopt_plc`.
    pub actions: Vec<String>,
    /// `apply_task`/`ignore_task` 의 대상 Task(원장 id).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// `adopt_plc` 후보(PLC Completed 링의 마지막 PICK).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate: Option<(u32, u32)>,
    /// 처음 본 시각(경고가 선 뒤 채워진다).
    pub since: String,
}

impl SyncIssue {
    fn key(&self) -> String {
        format!("{}|{}|{}", self.code, self.transfer_order_id.as_deref().unwrap_or(""), self.task_id.as_deref().unwrap_or(""))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Fix {
    /// 원장은 완료인데 재고에 안 접힌 Task(원장 id) — `plc-sync` 로 접는다.
    Fold(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Check {
    pub issues: Vec<SyncIssue>,
    pub fixes: Vec<Fix>,
}

fn pick_or_drop(e: &LedgerEntry) -> bool {
    matches!(TaskType::from_code(e.plc_task.task_type), Some(TaskType::Pick | TaskType::Drop))
}

fn issue(code: &str, message: String, order: Option<String>, actions: &[&str], candidate: Option<(u32, u32)>) -> SyncIssue {
    SyncIssue { code: code.into(), message, transfer_order_id: order, actions: actions.iter().map(|a| a.to_string()).collect(), task_id: None, candidate, since: String::new() }
}

/// 순수 판정 — `entries` 는 이 로봇의 원장, `unfolded` 는 그중 완료됐는데 재고에 안 접힌 PICK/DROP 과 손 정정 가드 사유.
pub fn reconcile(hand: &HandEntry, plc: &PlcView, entries: &[LedgerEntry], unfolded: &[(&LedgerEntry, Option<String>)]) -> Check {
    let mut out = Check::default();
    for e in entries.iter().filter(|e| e.state == TaskState::Lost && pick_or_drop(e)) {
        out.issues.push(issue(
            "task_lost",
            format!("Task #{} {}/{} 가 PLC 배열에서 사라짐(Lost) — PLC 에서 확인 후 완료·취소·실패로 정리", e.seq, e.work_id, e.task_id),
            e.transfer_order_id.clone(),
            &[],
            None,
        ));
    }
    // 손 정정 뒤에 끝난 Task — 자동으로 접지 않고 사람이 정한다(바빠도 알린다).
    for (e, why) in unfolded.iter().filter_map(|(e, w)| w.as_ref().map(|w| (e, w))) {
        let mut i =
            issue("held_completion", format!("Task #{} {}/{} 완료가 재고에 반영되지 않음 — {why}", e.seq, e.work_id, e.task_id), e.transfer_order_id.clone(), &["apply_task", "ignore_task"], None);
        i.task_id = Some(e.id.clone());
        out.issues.push(i);
    }
    let busy = plc.busy || entries.iter().any(|e| is_pending(e.state) && pick_or_drop(e));
    let mut free: Vec<&&LedgerEntry> = unfolded.iter().filter(|(_, w)| w.is_none()).map(|(e, _)| e).collect();
    if !free.is_empty() {
        free.sort_by_key(|e| e.seq);
        out.fixes = free.into_iter().map(|e| Fix::Fold(e.id.clone())).collect();
        return out; // 접은 뒤 다음 판정에서 손을 비교한다
    }
    if busy {
        return out;
    }
    let console = hand.count > 0;
    if console && !plc.hold_item && !plc.item_detect {
        out.issues.push(issue(
            "hand_stale",
            format!("콘솔 Hand 에 {} × {} 이 있는데 PLC 는 빈 그리퍼 (HoldItem=0, ItemDetect=0)", hand.item_code, hand.count),
            hand.transfer_order_id.clone(),
            &["clear_hand"],
            None,
        ));
    } else if !console && plc.hold_item {
        let what = plc.last_pick.map(|(c, n)| format!(" — 마지막 PICK {c} × {n}")).unwrap_or_default();
        out.issues.push(issue("plc_holds", format!("PLC 는 화물을 들고 있음 (HoldItem=1) — 콘솔 Hand 는 비어 있음{what}"), None, &["adopt_plc"], plc.last_pick));
    }
    if plc.hold_item != plc.item_detect {
        out.issues.push(issue(
            "sensor_mismatch",
            format!("PLC HoldItem={} 과 그리퍼 감지 ItemDetect={} 가 다름 — 현장 확인", plc.hold_item as u8, plc.item_detect as u8),
            hand.transfer_order_id.clone(),
            &[],
            None,
        ));
    }
    out
}

/// 로봇별 판정 이력 — 같은 문제가 `DEBOUNCE_MS` 동안 이어져야 경고·자동 처리한다.
#[derive(Default)]
pub struct Tracker {
    first: HashMap<String, HashMap<String, Instant>>,
    raised: HashMap<String, Vec<SyncIssue>>,
}

/// `Tracker::step` 결과.
#[derive(Debug, Default)]
pub struct Step {
    /// 이 로봇의 경고 목록이 바뀌었다(알릴 것).
    pub changed: bool,
    /// 이번에 새로 선 경고(이송 지시 이력에 한 줄 남긴다).
    pub new: Vec<SyncIssue>,
    /// 지금 해도 되는 자동 처리.
    pub fixes: Vec<Fix>,
}

impl Tracker {
    pub fn step(&mut self, plc: &str, check: Check, now: Instant, at: &str) -> Step {
        let debounce = Duration::from_millis(DEBOUNCE_MS);
        let first = self.first.entry(plc.to_string()).or_default();
        let mut keys: Vec<String> = check.issues.iter().map(|i| i.key()).collect();
        keys.extend(check.fixes.iter().map(|Fix::Fold(id)| format!("fold|{id}")));
        first.retain(|k, _| keys.contains(k));
        for k in &keys {
            first.entry(k.clone()).or_insert(now);
        }
        let ripe = |k: &str| first.get(k).is_some_and(|t| now.duration_since(*t) >= debounce);
        let fixes: Vec<Fix> = check.fixes.into_iter().filter(|Fix::Fold(id)| ripe(&format!("fold|{id}"))).collect();
        let prev = self.raised.remove(plc).unwrap_or_default();
        let mut raised = Vec::new();
        let mut new = Vec::new();
        for mut i in check.issues.into_iter().filter(|i| ripe(&i.key())) {
            match prev.iter().find(|p| p.key() == i.key()) {
                Some(p) => i.since = p.since.clone(),
                None => {
                    i.since = at.to_string();
                    new.push(i.clone());
                }
            }
            raised.push(i);
        }
        let changed = raised.len() != prev.len() || raised.iter().zip(prev.iter()).any(|(a, b)| a.key() != b.key() || a.message != b.message);
        self.raised.insert(plc.to_string(), raised);
        Step { changed, new, fixes }
    }

    pub fn issues(&self, plc: &str) -> Vec<SyncIssue> {
        self.raised.get(plc).cloned().unwrap_or_default()
    }

    /// 사람이 고쳤다 — 다음 판정부터 새로 센다.
    pub fn clear(&mut self, plc: &str) {
        self.first.remove(plc);
        self.raised.remove(plc);
    }
}

/// 로봇마다 `POLL_MS` 로 판정 → 자동 접기 → 경고 알림(재고 스트림 `sync`) · 새 경고는 이송 지시 이력에.
pub fn spawn(st: crate::state::AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(POLL_MS));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            for r in st.robots.iter() {
                if let Err(e) = sync_robot(&st, r) {
                    tracing::debug!(robot = %r.name, %e, "stock sync");
                }
            }
        }
    });
}

/// 로봇 하나를 한 번 맞춘다.
pub fn sync_robot(st: &crate::state::AppState, r: &crate::state::RobotCtx) -> Result<(), crate::error::ApiError> {
    let Some(stat) = st.robot_plc(r).ok().and_then(|h| h.decode_path("OPCUA", "STAT")) else { return Ok(()) };
    let Ok(v) = StatusView::from_json(&stat) else { return Ok(()) };
    let plc = PlcView::from_status(&v);
    let entries = r.ledger.list();
    let hand = st.stock.hand(&r.plc)?;
    let unfolded = st.stock.unfolded(&entries)?;
    let check = reconcile(&hand, &plc, &entries, &unfolded);
    let step = st.stock.sync.lock().unwrap_or_else(std::sync::PoisonError::into_inner).step(&r.plc, check, Instant::now(), &crate::util::now_str());
    for Fix::Fold(id) in &step.fixes {
        if let Some(e) = entries.iter().find(|e| &e.id == id) {
            st.stock.apply_task_sync(e)?;
            tracing::info!(robot = %r.name, task = %id, "stock sync: folded a completion the stock had missed (plc-sync)");
        }
    }
    for i in &step.new {
        if let Some(id) = &i.transfer_order_id {
            st.stock.note_order(id, &format!("동기화 경고: {}", i.message))?;
        }
    }
    if step.changed {
        let issues = st.stock.sync.lock().unwrap_or_else(std::sync::PoisonError::into_inner).issues(&r.plc);
        let _ = st.stock.events.send(super::StockEvent::Sync { plc: r.plc.clone(), issues });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gr_proto::TaskData;

    fn e(id: &str, seq: i64, tt: TaskType, state: TaskState) -> LedgerEntry {
        let mut t = TaskData { task_type: tt.code(), ..Default::default() };
        t.cell.id = 401;
        t.item.code = 2011;
        t.item.count = 2;
        serde_json::from_value(serde_json::json!({
            "id": id, "seq": seq, "work_id": 1, "task_id": seq, "origin": "scenario", "plc_name": "GR2", "request": null, "resolved": null,
            "position": [0.0, 0.0, 0.0, 0.0], "plc_task": t, "state": state, "state_at": "", "created_at": "", "submitted_at": null,
            "ended_at": null, "ack": null, "header": null, "plc": null, "error": null, "history": [], "transfer_order_id": "TO-1"
        }))
        .unwrap()
    }
    fn hand(n: u32) -> HandEntry {
        HandEntry { plc: "GR2".into(), item_code: if n > 0 { 2011 } else { 0 }, count: n, transfer_order_id: (n > 0).then(|| "TO-1".into()), ..Default::default() }
    }
    fn idle(hold: bool) -> PlcView {
        PlcView { hold_item: hold, item_detect: hold, busy: false, last_pick: Some((2011, 2)) }
    }

    #[test]
    fn consistent_states_raise_nothing() {
        assert_eq!(reconcile(&hand(0), &idle(false), &[], &[]), Check::default());
        assert_eq!(reconcile(&hand(2), &idle(true), &[], &[]), Check::default());
    }

    #[test]
    fn busy_robot_is_not_compared() {
        let busy = PlcView { busy: true, ..idle(true) };
        assert!(reconcile(&hand(0), &busy, &[], &[]).issues.is_empty());
        // 진행 중 PICK 이 있으면 PLC 가 쉬어도 비교하지 않는다(에코 전)
        let running = [e("p", 1, TaskType::Pick, TaskState::Submitted)];
        assert!(reconcile(&hand(0), &idle(true), &running, &[]).issues.is_empty());
    }

    /// DROP 이 PLC 에서 완료(링 → 원장 완료)됐는데 재고에 안 접힘 + PLC 빈 그리퍼 → 자동 접기(plc-sync), 경고 없음.
    #[test]
    fn missed_completion_is_folded_automatically() {
        let done = e("d", 2, TaskType::Drop, TaskState::Completed);
        let c = reconcile(&hand(2), &idle(false), std::slice::from_ref(&done), &[(&done, None)]);
        assert_eq!(c.fixes, vec![Fix::Fold("d".into())]);
        assert!(c.issues.is_empty());
        // 완료 뒤 손 정정이 있으면 자동으로 접지 않고 반영/무시를 묻는다
        let c = reconcile(&hand(2), &idle(false), std::slice::from_ref(&done), &[(&done, Some("셀 401 손 정정".into()))]);
        assert!(c.fixes.is_empty());
        assert_eq!((c.issues[0].code.as_str(), c.issues[0].task_id.as_deref()), ("held_completion", Some("d")));
        assert_eq!(c.issues[0].actions, vec!["apply_task".to_string(), "ignore_task".to_string()]);
    }

    #[test]
    fn mismatches_raise_warnings_with_actions() {
        // 콘솔은 들고 있다는데 PLC 는 빈 그리퍼(예: PLC 에서 DROP 취소로 GRIPPER.Item 초기화)
        let c = reconcile(&hand(2), &idle(false), &[], &[]);
        assert_eq!((c.issues[0].code.as_str(), c.issues[0].actions.clone(), c.issues[0].transfer_order_id.as_deref()), ("hand_stale", vec!["clear_hand".to_string()], Some("TO-1")));
        // PLC 는 들고 있는데 콘솔은 빈 손 — 후보는 PLC 의 마지막 PICK
        let c = reconcile(&hand(0), &idle(true), &[], &[]);
        assert_eq!((c.issues[0].code.as_str(), c.issues[0].candidate), ("plc_holds", Some((2011, 2))));
        // HoldItem 과 감지가 다름 → 알림
        let c = reconcile(&hand(2), &PlcView { hold_item: true, item_detect: false, ..idle(true) }, &[], &[]);
        assert_eq!(c.issues.iter().map(|i| i.code.as_str()).collect::<Vec<_>>(), vec!["sensor_mismatch"]);
        // Lost 된 PICK/DROP 은 바빠도 알린다
        let lost = [e("l", 3, TaskType::Drop, TaskState::Lost)];
        let c = reconcile(&hand(2), &PlcView { busy: true, ..idle(true) }, &lost, &[]);
        assert_eq!(c.issues[0].code, "task_lost");
    }

    #[test]
    fn tracker_debounces_and_reports_changes_once() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        let check = || reconcile(&hand(2), &idle(false), &[], &[]);
        let s = t.step("GR2", check(), t0, "a");
        assert!(!s.changed && s.new.is_empty(), "not yet ripe");
        let s = t.step("GR2", check(), t0 + Duration::from_millis(DEBOUNCE_MS), "b");
        assert!(s.changed && s.new.len() == 1);
        assert_eq!(t.issues("GR2")[0].since, "b");
        let s = t.step("GR2", check(), t0 + Duration::from_millis(DEBOUNCE_MS + 1000), "c");
        assert!(!s.changed && s.new.is_empty(), "same warning, no repeat");
        // 풀리면 한 번 알린다
        let s = t.step("GR2", Check::default(), t0 + Duration::from_millis(DEBOUNCE_MS + 2000), "d");
        assert!(s.changed && t.issues("GR2").is_empty());
        // 한 번 흔들린 것은 선지 않는다
        let s = t.step("GR2", check(), t0 + Duration::from_millis(DEBOUNCE_MS + 3000), "e");
        assert!(!s.changed);
        let s = t.step("GR2", Check::default(), t0 + Duration::from_millis(DEBOUNCE_MS + 3500), "f");
        assert!(!s.changed);
        // 자동 접기도 이어져야 한다
        let done = e("d", 2, TaskType::Drop, TaskState::Completed);
        let fold = || reconcile(&hand(2), &idle(false), std::slice::from_ref(&done), &[(&done, None)]);
        assert!(t.step("GR2", fold(), t0, "g").fixes.is_empty());
        assert_eq!(t.step("GR2", fold(), t0 + Duration::from_millis(DEBOUNCE_MS), "h").fixes, vec![Fix::Fold("d".into())]);
    }
}
