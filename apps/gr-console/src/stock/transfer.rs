//! 이송 지시(Transfer Order, `TO-YYMMDD-NNNN`) — PICK/DROP 한 짝 = 한 지시. 재고 이동은 모두 여기에 묶여
//! 나중에 추적된다: 지시 문서(`transfer_orders`) + 재고 변경 기록(`stock_log`, 지시 id 참조).
//!
//! 상태는 두 Task 의 **지금 상태에서 유도**한다(`derive`) — 미리 넣기에서는 PICK 이 도는 중에 DROP 이 대기열에
//! 들어가므로 이벤트 순서로 상태를 옮기면 뒤집힌다.
//!
//! | PICK            | DROP              | 지시        |
//! |-----------------|-------------------|-------------|
//! | 없음            | —                 | planned     |
//! | 진행 중         | 무엇이든          | picking     |
//! | 취소            | —                 | aborted     |
//! | 실패·거부·Lost  | —                 | failed      |
//! | 완료            | 없음 / 실패·Lost  | in_hand     |
//! | 완료            | 취소(PLC 삭제)    | aborted — PLC 가 그리퍼 화물 데이터를 지운다, Hand 도 비움 |
//! | 완료            | 진행 중           | dropping    |
//! | 완료            | 완료              | done        |
//!
//! `aborted` 는 사람이 끝낸 지시(보내지 못한 채 실행이 멈춤, Hand 손 정정)이고 더는 바뀌지 않는다.

use gr_proto::TaskType;
use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::ApiError;
use crate::ledger::{Target, TaskState};
use crate::util::now_str;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderState {
    Planned,
    Picking,
    InHand,
    Dropping,
    Done,
    Failed,
    Aborted,
}

impl OrderState {
    pub fn as_str(self) -> &'static str {
        match self {
            OrderState::Planned => "planned",
            OrderState::Picking => "picking",
            OrderState::InHand => "in_hand",
            OrderState::Dropping => "dropping",
            OrderState::Done => "done",
            OrderState::Failed => "failed",
            OrderState::Aborted => "aborted",
        }
    }
    pub fn parse(s: &str) -> Option<OrderState> {
        Some(match s {
            "planned" => OrderState::Planned,
            "picking" => OrderState::Picking,
            "in_hand" => OrderState::InHand,
            "dropping" => OrderState::Dropping,
            "done" => OrderState::Done,
            "failed" => OrderState::Failed,
            "aborted" => OrderState::Aborted,
            _ => return None,
        })
    }
    /// 화물이 로봇에 걸려 있거나 걸릴 수 있는(열린) 지시.
    pub fn is_open(self) -> bool {
        matches!(self, OrderState::Planned | OrderState::Picking | OrderState::InHand | OrderState::Dropping)
    }
}

fn bad(s: TaskState) -> bool {
    matches!(s, TaskState::Rejected | TaskState::Failed | TaskState::Canceled | TaskState::Lost)
}

/// 두 Task 상태 → 지시 상태(표는 모듈 머리). `aborted` 는 그대로 둔다.
pub fn derive(cur: OrderState, pick: Option<TaskState>, drop: Option<TaskState>) -> OrderState {
    if cur == OrderState::Aborted {
        return cur;
    }
    // 손에 든 채 연 지시(화물 지정 DROP · PLC 기준 Hand 맞춤)는 PICK 이 없다 — 끝난 PICK 으로 본다(예전에는 DROP 이 끝나도 in_hand 에 남았다).
    let pick = match (pick, cur) {
        (None, OrderState::InHand | OrderState::Dropping) => Some(TaskState::Completed),
        (p, _) => p,
    };
    match pick {
        None | Some(TaskState::Draft) => cur,
        Some(TaskState::Completed) => match drop {
            Some(TaskState::Completed) => OrderState::Done,
            // PLC 에서 DROP 을 지우면 그리퍼 화물 데이터도 지워진다 — 사람이 끝낸 지시(Hand 도 비운다).
            Some(TaskState::Canceled) => OrderState::Aborted,
            Some(d) if !bad(d) && d != TaskState::Draft => OrderState::Dropping,
            _ => OrderState::InHand,
        },
        // 시작 전 PICK 을 취소하면 짝 DROP 도 같이 취소된다 — 사람이 끝낸 지시.
        Some(TaskState::Canceled) => OrderState::Aborted,
        Some(p) if bad(p) => OrderState::Failed,
        Some(_) => OrderState::Picking,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OrderStep {
    pub at: String,
    pub from: Option<OrderState>,
    pub to: OrderState,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransferOrder {
    pub id: String,
    pub seq: i64,
    /// 로봇 id(설정) — 외부 Task 로 생긴 지시는 모를 수 있다.
    pub robot: Option<u8>,
    /// 로봇 상태 PLC(원장 `plc_name`, Hand 키).
    pub plc: String,
    pub item_code: u32,
    pub count: u32,
    pub from: Option<Target>,
    pub to: Option<Target>,
    pub pick_task: Option<String>,
    pub drop_task: Option<String>,
    pub pick_state: Option<TaskState>,
    pub drop_state: Option<TaskState>,
    pub state: OrderState,
    /// `scenario:<run id>` · `console` · `external`(PLC 에서 처음 본 Task) · `manual`.
    pub source: String,
    pub note: String,
    pub created_at: String,
    pub updated_at: String,
    pub ended_at: Option<String>,
    pub history: Vec<OrderStep>,
}

/// 재고 변경 한 줄(셀 또는 Hand).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StockChange {
    pub id: i64,
    pub at: String,
    /// `cell` | `hand`
    pub kind: String,
    /// 셀 id 또는 Hand 의 PLC 이름.
    pub key: String,
    pub item_before: u32,
    pub count_before: u32,
    pub item_after: u32,
    pub count_after: u32,
    pub reason: String,
    pub task_id: Option<String>,
    pub transfer_order_id: Option<String>,
}

/// 새 지시에 넣을 값.
#[derive(Clone, Debug, Default)]
pub struct NewOrder {
    pub robot: Option<u8>,
    pub plc: String,
    pub item_code: u32,
    pub count: u32,
    pub from: Option<Target>,
    pub to: Option<Target>,
    pub source: String,
    pub note: String,
}

/// 목록 조건(`GET /api/transfer-orders`).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct OrderQuery {
    pub plc: Option<String>,
    pub state: Option<String>,
    pub cell: Option<u16>,
    pub since: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// `TO-YYMMDD-NNNN` — 날짜(로컬) + 전역 일련번호(4 자리 이상, 날이 바뀌어도 이어진다 → 전역 유일).
pub fn format_id(date: time::Date, n: i64) -> String {
    format!("TO-{:02}{:02}{:02}-{n:04}", date.year() % 100, date.month() as u8, date.day())
}

pub fn create(db: &Db, n: NewOrder, state: OrderState) -> Result<TransferOrder, ApiError> {
    let seq = db.next_counter("transfer_order", 1)?;
    let today = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc()).date();
    let now = now_str();
    let o = TransferOrder {
        id: format_id(today, seq),
        seq,
        robot: n.robot,
        plc: n.plc,
        item_code: n.item_code,
        count: n.count,
        from: n.from,
        to: n.to,
        pick_task: None,
        drop_task: None,
        pick_state: None,
        drop_state: None,
        state,
        source: n.source,
        note: n.note.clone(),
        created_at: now.clone(),
        updated_at: now.clone(),
        ended_at: None,
        history: vec![OrderStep { at: now, from: None, to: state, note: n.note }],
    };
    save(db, &o)?;
    Ok(o)
}

pub fn save(db: &Db, o: &TransferOrder) -> Result<(), ApiError> {
    let doc = serde_json::to_string(o)?;
    db.with(|c| {
        c.execute(
            "INSERT INTO transfer_orders (id, seq, plc, state, from_id, to_id, doc_json, created_at, updated_at, source) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(id) DO UPDATE SET state = excluded.state, from_id = excluded.from_id, to_id = excluded.to_id, doc_json = excluded.doc_json, updated_at = excluded.updated_at, source = excluded.source",
            (&o.id, o.seq, &o.plc, o.state.as_str(), o.from.as_ref().map(|t| t.id), o.to.as_ref().map(|t| t.id), &doc, &o.created_at, &o.updated_at, &o.source),
        )
    })?;
    Ok(())
}

pub fn get(db: &Db, id: &str) -> Result<Option<TransferOrder>, ApiError> {
    let doc: Option<String> = db.with(|c| {
        c.query_row("SELECT doc_json FROM transfer_orders WHERE id = ?1", [id], |r| r.get(0)).map(Some).or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })
    })?;
    Ok(doc.and_then(|d| serde_json::from_str(&d).ok()))
}

/// 상태를 바꾸고 이력에 한 줄 남긴다(같은 상태면 메모만 바뀐 것 — 이력은 남기지 않는다).
pub fn set_state(o: &mut TransferOrder, to: OrderState, note: &str) {
    let now = now_str();
    if o.state != to {
        o.history.push(OrderStep { at: now.clone(), from: Some(o.state), to, note: note.into() });
        o.state = to;
        if !to.is_open() {
            o.ended_at = Some(now.clone());
        } else {
            o.ended_at = None;
        }
    }
    o.updated_at = now;
}

/// 이력에 메모 한 줄(상태는 그대로).
pub fn add_note(o: &mut TransferOrder, note: &str) {
    let now = now_str();
    o.history.push(OrderStep { at: now.clone(), from: Some(o.state), to: o.state, note: note.into() });
    o.updated_at = now;
}

/// 짝 DROP 을 **보낼 때** 대상이 바뀌었으면(계획에서 고친 자리로 초안을 다시 작성) 지시의 도착지를 그 값으로 맞춘다.
/// 초안(보내기 전)은 건드리지 않는다 — 지시의 도착지는 로봇에 실제로 나간 DROP 의 자리다. 바뀌었으면 true.
pub fn sync_drop_target(o: &mut TransferOrder, state: TaskState, target: Option<&Target>) -> bool {
    let Some(t) = target else { return false };
    if state == TaskState::Draft || o.to.as_ref() == Some(t) {
        return false;
    }
    let before = o.to.as_ref().map(|x| format!("{} #{}", x.kind, x.id)).unwrap_or_else(|| "-".into());
    o.to = Some(t.clone());
    add_note(o, &format!("DROP 대상 변경 {before} → {} #{}", t.kind, t.id));
    true
}

/// Task 하나의 상태를 지시에 반영한다(PICK/DROP 이 아니면 아무것도 안 한다). 바뀐 지시를 돌려준다.
/// 재시도로 새 PICK 이 오면(앞 PICK 이 실패) 그 Task 로 바꿔 단다.
pub fn apply_task_state(o: &mut TransferOrder, tt: TaskType, task_id: &str, state: TaskState) -> bool {
    let before = (o.state, o.pick_task.clone(), o.drop_task.clone(), o.pick_state, o.drop_state);
    match tt {
        TaskType::Pick => {
            let replace = match (&o.pick_task, o.pick_state) {
                (None, _) => true,
                (Some(id), _) if id == task_id => true,
                (Some(_), Some(prev)) => bad(prev) && !bad(state),
                (Some(_), None) => true,
            };
            if replace {
                o.pick_task = Some(task_id.into());
                o.pick_state = Some(state);
            }
        }
        TaskType::Drop => {
            let replace = match (&o.drop_task, o.drop_state) {
                (None, _) => true,
                (Some(id), _) if id == task_id => true,
                (Some(_), Some(prev)) => bad(prev) && !bad(state),
                (Some(_), None) => true,
            };
            if replace {
                o.drop_task = Some(task_id.into());
                o.drop_state = Some(state);
            }
        }
        _ => return false,
    }
    let to = derive(o.state, o.pick_state, o.drop_state);
    let note = match (tt, state) {
        (TaskType::Drop, TaskState::Canceled) if o.pick_state == Some(TaskState::Completed) => "DROP 취소 — PLC 가 그리퍼 화물 데이터를 지움 (Hand 비움)".to_string(),
        (TaskType::Drop, s) if bad(s) && to == OrderState::InHand => format!("DROP {} — 타이어는 Hand 에 남음", s.as_str()),
        (t, s) => format!("{} {}", t.name(), s.as_str()),
    };
    set_state(o, to, &note);
    before != (o.state, o.pick_task.clone(), o.drop_task.clone(), o.pick_state, o.drop_state)
}

pub fn list(db: &Db, q: &OrderQuery) -> Result<(Vec<TransferOrder>, i64), ApiError> {
    let mut wh: Vec<String> = Vec::new();
    let mut args: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(p) = &q.plc {
        args.push(p.clone().into());
        wh.push(format!("plc = ?{}", args.len()));
    }
    if let Some(s) = q.state.as_deref().filter(|s| !s.is_empty()) {
        let states: Vec<&str> = s.split(',').map(str::trim).filter(|s| OrderState::parse(s).is_some()).collect();
        if states.is_empty() {
            return Err(ApiError::BadRequest(format!("unknown state {s}")));
        }
        let mut ph = Vec::new();
        for st in states {
            args.push(st.to_string().into());
            ph.push(format!("?{}", args.len()));
        }
        wh.push(format!("state IN ({})", ph.join(",")));
    }
    if let Some(c) = q.cell {
        args.push((c as i64).into());
        wh.push(format!("(from_id = ?{0} OR to_id = ?{0})", args.len()));
    }
    if let Some(s) = &q.since {
        args.push(s.clone().into());
        wh.push(format!("created_at >= ?{}", args.len()));
    }
    let wh = if wh.is_empty() { String::new() } else { format!("WHERE {}", wh.join(" AND ")) };
    let limit = q.limit.unwrap_or(50).clamp(1, 500) as i64;
    let offset = q.offset.unwrap_or(0) as i64;
    let (rows, total) = db.with(|c| {
        let total: i64 = c.query_row(&format!("SELECT COUNT(*) FROM transfer_orders {wh}"), rusqlite::params_from_iter(args.iter()), |r| r.get(0))?;
        let mut st = c.prepare(&format!("SELECT doc_json FROM transfer_orders {wh} ORDER BY seq DESC LIMIT {limit} OFFSET {offset}"))?;
        let rows = st.query_map(rusqlite::params_from_iter(args.iter()), |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        Ok((rows, total))
    })?;
    Ok((rows.into_iter().filter_map(|d| serde_json::from_str(&d).ok()).collect(), total))
}

/// 출처가 `prefix` 로 시작하는 지시의 상태별 **화물 수**(지시의 count 합) — 요청 진행(`req:<id>`)을 센다(2개 한 번에 집은 지시는 2).
pub fn count_by_source(db: &Db, prefix: &str) -> Result<std::collections::BTreeMap<String, u32>, ApiError> {
    // 출처 문자열(`req:RQ-YYMMDD-NNNN@2101`)은 접두 범위로 센다.
    let hi = format!("{prefix}\u{10FFFF}");
    Ok(db.with(|c| {
        let mut st = c.prepare("SELECT state, SUM(COALESCE(json_extract(doc_json, '$.count'), 1)) FROM transfer_orders WHERE source >= ?1 AND source < ?2 GROUP BY state")?;
        let rows = st.query_map([prefix, hi.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u32)))?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().collect())
    })?)
}

#[allow(clippy::too_many_arguments)]
pub fn log_change(db: &Db, kind: &str, key: &str, before: (u32, u32), after: (u32, u32), reason: &str, task_id: Option<&str>, order: Option<&str>) -> Result<(), ApiError> {
    db.with(|c| {
        c.execute(
            "INSERT INTO stock_log (at, kind, key, item_before, count_before, item_after, count_after, reason, task_id, transfer_order_id) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            (now_str(), kind, key, before.0, before.1, after.0, after.1, reason, task_id, order),
        )
    })?;
    Ok(())
}

pub fn changes_of(db: &Db, order: &str) -> Result<Vec<StockChange>, ApiError> {
    let rows = db.with(|c| {
        let mut st =
            c.prepare("SELECT id, at, kind, key, item_before, count_before, item_after, count_after, reason, task_id, transfer_order_id FROM stock_log WHERE transfer_order_id = ?1 ORDER BY id")?;
        let it = st.query_map([order], |r| {
            Ok(StockChange {
                id: r.get(0)?,
                at: r.get(1)?,
                kind: r.get(2)?,
                key: r.get(3)?,
                item_before: r.get::<_, i64>(4)? as u32,
                count_before: r.get::<_, i64>(5)? as u32,
                item_after: r.get::<_, i64>(6)? as u32,
                count_after: r.get::<_, i64>(7)? as u32,
                reason: r.get(8)?,
                task_id: r.get(9)?,
                transfer_order_id: r.get(10)?,
            })
        })?;
        it.collect::<Result<Vec<_>, _>>()
    })?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use TaskState::*;

    #[test]
    fn id_is_dated_and_zero_padded() {
        let d = time::Date::from_calendar_date(2026, time::Month::September, 21).unwrap();
        assert_eq!(format_id(d, 1), "TO-260921-0001");
        assert_eq!(format_id(d, 12345), "TO-260921-12345");
    }

    #[test]
    fn sent_drop_moves_the_destination_but_a_draft_does_not() {
        let cell = |id| Target { kind: "cell".into(), id };
        let mut o = TransferOrder {
            id: "TO-1".into(),
            seq: 1,
            robot: Some(2),
            plc: "GR2".into(),
            item_code: 1001,
            count: 1,
            from: Some(cell(101)),
            to: Some(cell(107)),
            pick_task: None,
            drop_task: None,
            pick_state: None,
            drop_state: None,
            state: OrderState::Planned,
            source: "plan".into(),
            note: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            ended_at: None,
            history: vec![],
        };
        assert!(!sync_drop_target(&mut o, Draft, Some(&cell(108))), "초안은 아직 로봇에 안 나갔다");
        assert!(!sync_drop_target(&mut o, Submitted, Some(&cell(107))), "같은 자리");
        assert!(sync_drop_target(&mut o, Submitted, Some(&cell(108))));
        assert_eq!(o.to, Some(cell(108)));
        assert!(o.history.last().unwrap().note.contains("cell #107 → cell #108"));
    }

    #[test]
    fn derive_follows_both_tasks() {
        use OrderState as O;
        assert_eq!(derive(O::Planned, None, None), O::Planned);
        assert_eq!(derive(O::Planned, Some(Submitted), None), O::Picking);
        // 미리 넣기: PICK 이 도는 동안 DROP 이 대기열에 — 아직 picking
        assert_eq!(derive(O::Picking, Some(Running), Some(Queued)), O::Picking);
        assert_eq!(derive(O::Picking, Some(Completed), Some(Queued)), O::Dropping);
        assert_eq!(derive(O::Picking, Some(Completed), None), O::InHand);
        assert_eq!(derive(O::Dropping, Some(Completed), Some(Completed)), O::Done);
        assert_eq!(derive(O::Dropping, Some(Completed), Some(Canceled)), O::Aborted, "PLC delete clears the gripper item");
        assert_eq!(derive(O::Dropping, Some(Completed), Some(Failed)), O::InHand, "failed DROP: tires stay on the gripper");
        assert_eq!(derive(O::Picking, Some(Rejected), Some(Queued)), O::Failed);
        assert_eq!(derive(O::Picking, Some(Canceled), Some(Canceled)), O::Aborted, "PICK 취소 = 짝 전체 중단");
        assert_eq!(derive(O::Aborted, Some(Completed), Some(Completed)), O::Aborted);
    }

    /// 손에 든 채 연 지시(PICK 없음) — DROP 이 끝나면 done, 도는 중이면 dropping, 실패면 그대로 손에.
    #[test]
    fn in_hand_order_without_pick_finishes_on_drop() {
        use OrderState as O;
        assert_eq!(derive(O::InHand, None, None), O::InHand);
        assert_eq!(derive(O::InHand, None, Some(Running)), O::Dropping);
        assert_eq!(derive(O::Dropping, None, Some(Completed)), O::Done);
        assert_eq!(derive(O::InHand, None, Some(Failed)), O::InHand);
        assert_eq!(derive(O::Planned, None, Some(Completed)), O::Planned, "계획 지시는 PICK 을 기다린다");
    }

    /// 전 과정: planned → picking → (DROP 미리 넣음) → dropping → DROP 취소 → in_hand → 새 DROP → done.
    #[test]
    fn order_lifecycle_with_retry_and_cancel() {
        let db = Db::open_memory().unwrap();
        let mut o = create(&db, NewOrder { plc: "GR2".into(), item_code: 2011, count: 2, source: "scenario:r1".into(), ..Default::default() }, OrderState::Planned).unwrap();
        assert!(o.id.starts_with("TO-") && o.id.ends_with("-0001"), "{}", o.id);
        assert!(apply_task_state(&mut o, TaskType::Pick, "p1", Submitted));
        assert_eq!(o.state, OrderState::Picking);
        apply_task_state(&mut o, TaskType::Drop, "d1", Queued);
        assert_eq!(o.state, OrderState::Picking);
        apply_task_state(&mut o, TaskType::Pick, "p1", Completed);
        assert_eq!(o.state, OrderState::Dropping);
        apply_task_state(&mut o, TaskType::Drop, "d1", Failed);
        assert_eq!(o.state, OrderState::InHand);
        assert!(o.history.last().unwrap().note.contains("Hand"));
        // 같은 지시로 새 DROP — 앞 DROP 이 취소됐으니 바꿔 단다
        apply_task_state(&mut o, TaskType::Drop, "d2", Submitted);
        assert_eq!((o.state, o.drop_task.as_deref()), (OrderState::Dropping, Some("d2")));
        // 늦게 온 옛 DROP 이벤트는 새 DROP 을 덮지 않는다
        assert!(!apply_task_state(&mut o, TaskType::Drop, "d1", Failed));
        apply_task_state(&mut o, TaskType::Drop, "d2", Completed);
        assert_eq!(o.state, OrderState::Done);
        assert!(o.ended_at.is_some());
        save(&db, &o).unwrap();
        let back = get(&db, &o.id).unwrap().unwrap();
        assert_eq!(back, o);
        let states: Vec<_> = back.history.iter().map(|h| h.to.as_str()).collect();
        assert_eq!(states, vec!["planned", "picking", "dropping", "in_hand", "dropping", "done"]);
        // 목록 조건
        let (items, total) = list(&db, &OrderQuery { state: Some("done".into()), ..Default::default() }).unwrap();
        assert_eq!((items.len(), total), (1, 1));
        assert_eq!(list(&db, &OrderQuery { state: Some("in_hand".into()), ..Default::default() }).unwrap().1, 0);
        assert!(list(&db, &OrderQuery { state: Some("nope".into()), ..Default::default() }).is_err());
    }
}
