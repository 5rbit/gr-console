//! Ledger operations: submit / cancel / force-complete / resubmit (M3-B refines; M1 provides working versions).

use std::sync::Arc;

use gr_proto::{TaskData, TaskKey};

use super::{Actor, LedgerEntry, Origin, TaskState};
use crate::cmd::{CommandPort, TaskOp};
use crate::error::ApiError;
use crate::state::{AppState, RobotCtx};

/// Gate for a new submission.
pub struct Gate {
    pub can_submit: bool,
    pub reasons: Vec<String>,
    /// 이 게이트가 말하는 로봇과 그 상태 PLC — 화면이 사유를 다시 꾸미지 않고 그대로 읽게 같이 낸다.
    pub robot: String,
    pub plc: String,
}

/// 메시지 앞에 로봇 이름을 단다(이미 달려 있으면 그대로).
///
/// 왜 여기서 다는가: 사이드바 선택과 화면이 어긋난 채 거부 문구만 읽으면 "왜 막혔나"는 알아도
/// **누가 막혔나**를 모른다. 현장에서 GR2 앞에 선 사람이 GR1 의 `Accept = FALSE` 를 읽고
/// 로봇이 고장 난 줄 알았다 — 사유를 내는 자리에서 이름을 붙여 화면이 빠뜨릴 수 없게 한다.
pub fn with_robot(robot: &str, msg: &str) -> String {
    match msg.strip_prefix(robot) {
        Some(rest) if rest.starts_with(':') => msg.to_string(),
        _ => format!("{robot}: {msg}"),
    }
}

/// 게이트 사유 전부에 로봇 이름을 단다.
pub fn label_reasons(robot: &str, reasons: Vec<String>) -> Vec<String> {
    reasons.iter().map(|r| with_robot(robot, r)).collect()
}

/// 제출 거부 409 의 본문 — 사유가 비어 있어도 어느 로봇이 거부했는지는 남는다.
pub fn submit_refusal(robot: &str, reasons: &[String]) -> String {
    if reasons.is_empty() {
        return with_robot(robot, "제출 거부 — 게이트 사유를 확인할 수 없습니다");
    }
    label_reasons(robot, reasons.to_vec()).join("; ")
}

pub fn gate(st: &AppState, r: &RobotCtx) -> Gate {
    let mut reasons = Vec::new();
    if let Some(why) = r.cmd.not_ready_reason() {
        reasons.push(why);
    }
    match st.robot_plc(r) {
        Ok(h) => {
            let hl = h.health_now();
            if !hl.connected {
                reasons.push(format!("{} S7 연결 없음", h.name()));
            } else if !hl.db_ok("OPCUA") {
                reasons.push(format!("{} OPCUA DB 레이아웃 불일치", h.name()));
            } else if let Some(stat) = h.decode_path("OPCUA", "STAT")
                && let Ok(v) = gr_proto::StatusView::from_json(&stat)
            {
                if !v.task.status.accept {
                    reasons.push("Task.Status.Accept = FALSE".into());
                }
                if !v.task.queue.iter().any(|t| t.is_zero()) {
                    reasons.push("태스크 버퍼 가득 참".into());
                }
                if !v.mode.auto && !v.mode.auto_ready && !st.cfg.demo {
                    reasons.push("AUTO 모드가 아님".into());
                }
            }
        }
        Err(_) => reasons.push("상태 PLC 미설정".into()),
    }
    if r.ledger.list().iter().any(|e| e.state == TaskState::Submitted) {
        reasons.push("에코 대기 중인 제출이 있음".into());
    }
    if let Some(grm) = st.grm_plc() {
        let path = format!("{}.Header.CMD", r.opcua_root);
        for (path, label) in [(path.as_str(), "GRM CMD 헤더가 비어 있지 않음 (이전 명령 미소진)")] {
            if let Some(v) = grm.decode_path("OPCUA", path)
                && v.as_u64().unwrap_or(0) != 0
            {
                reasons.push(label.into());
            }
        }
    }
    Gate { can_submit: reasons.is_empty(), reasons: label_reasons(&r.name, reasons), robot: r.name.clone(), plc: r.plc.clone() }
}

/// Submits a Draft entry: writes the command, records the header and moves to Submitted.
pub async fn submit(st: &AppState, r: &RobotCtx, entry: LedgerEntry) -> Result<LedgerEntry, ApiError> {
    if entry.state != TaskState::Draft {
        return Err(ApiError::Conflict(format!("task {} is {}", entry.id, entry.state.as_str())));
    }
    // 초안은 작성 때 위치를 들고 있다 — 스테이션 트래킹은 그 사이에 바뀌므로 보낼 때 다시 계산한다.
    let mut entry = entry;
    if let Some(req) = entry.request.clone() {
        // 재고는 초안 작성 뒤에도 바뀐다 — 단수 Max 는 보낼 때의 재고로 본다.
        crate::issue::enforce_stack_limit(st, &req, &entry.plc_task)?;
        crate::issue::enforce_hand(st, &req, &entry.plc_task)?;
        let mut task = entry.plc_task.clone();
        if let Some(audit) = crate::issue::refresh_station_offset(st, &req, &mut task, true)? {
            entry.position = task.position;
            entry.plc_task = task;
            entry.station_offset = Some(audit);
        }
    }
    submit_prepared(st, r, entry).await
}

/// 보정 재계산이 끝난 초안을 게이트 → 쓰기 → Submitted 로.
async fn submit_prepared(st: &AppState, r: &RobotCtx, entry: LedgerEntry) -> Result<LedgerEntry, ApiError> {
    // 명령 쓰기 + 원장 전이는 종료가 기다려 주는 한 구간이다.
    let _busy = st.shutdown.enter(format!("Task 제출 (로봇 {})", r.id))?;
    let g = gate(st, r);
    if !g.can_submit {
        return Err(ApiError::Conflict(submit_refusal(&r.name, &g.reasons)));
    }
    let header = r.cmd.write_task(&entry.plc_task).await?;
    let mut e = entry;
    e.header = Some(header);
    e.submitted_at = Some(crate::util::now_str());
    r.ledger.transition(e, TaskState::Submitted, Actor::Ui, None)
}

/// Creates + submits in one go (used by the task-issue slice and the scenario runner).
/// `pallet` = compose 가 정한 팔렛 슬롯 근거 — 원장에 같이 남긴다(auto 로 고른 seq/단이 여기서 고정된다).
#[allow(clippy::too_many_arguments)]
pub async fn create_and_submit(
    st: &AppState,
    r: &RobotCtx,
    origin: Origin,
    request: Option<super::TaskRequest>,
    resolved: Option<gr_proto::TaskParams>,
    task: TaskData,
    pallet: Option<crate::pallet::compose::PalletAudit>,
    submit_now: bool,
) -> Result<LedgerEntry, ApiError> {
    // 스테이션 보정은 호출자(작성 라우트·시나리오 게이트 대기·재제출)가 들고 온 위치가 아니라 지금 스냅샷으로.
    // 바로 제출이면 거부 사유가 있을 때 원장에 초안을 남기지 않고 여기서 멈춘다.
    let mut task = task;
    if submit_now && let Some(req) = &request {
        crate::issue::enforce_stack_limit(st, req, &task)?;
        crate::issue::enforce_hand(st, req, &task)?;
        // 두 로봇 영역 — 단일 명령은 기다리지 않고 거부한다(시나리오 실행기는 게이트에서 기다린다).
        if req.source.is_none() && task.cell.id != 0 {
            let cfg = crate::area::load(&st.db);
            if let Some((b, mine)) = crate::area::check(st, &cfg, r, task.position[0], None, &Default::default()) {
                return Err(ApiError::Conflict(with_robot(&r.name, &crate::area::wait_reason(&b, mine, cfg.separation_mm))));
            }
        }
    }
    let audit = match &request {
        Some(req) => crate::issue::refresh_station_offset(st, req, &mut task, submit_now)?,
        None => None,
    };
    let mut e = r.ledger.create(origin, request, resolved, task)?;
    if audit.is_some() || pallet.is_some() {
        e.station_offset = audit;
        e.pallet = pallet;
        e = r.ledger.upsert(e)?;
    }
    if !submit_now {
        return Ok(e);
    }
    submit_prepared(st, r, e).await
}

/// Latest decoded `OPCUA.STAT` of the status PLC (None when there is no snapshot yet).
fn status_view(st: &AppState, r: &RobotCtx) -> Option<gr_proto::StatusView> {
    let h = st.robot_plc(r).ok()?;
    let stat = h.decode_path("OPCUA", "STAT")?;
    gr_proto::StatusView::from_json(&stat).ok()
}

async fn task_op(cmd: &Arc<CommandPort>, op: TaskOp, key: TaskKey) -> Result<(), ApiError> {
    cmd.write_task_op(op, key.work_id, key.task_id).await?;
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    cmd.write_task_op(op, 0, 0).await // re-arm
}

/// How long the PLC gets to answer a Delete/Complete (status bit or ring) before the request is
/// recorded as unanswered. The op itself is fire-and-forget on the wire — there is no ack field —
/// so "the PLC ignored it" is only observable as *nothing happening*.
pub const OP_ANSWER_MS: u64 = 5000;

/// Watches a Delete/Complete request: if the entry is still in the same non-terminal state after
/// `OP_ANSWER_MS`, a `System` history line says so. Without it an ignored Delete looked exactly like
/// a Delete in flight until the task went `Lost` minutes later with an unrelated note.
fn watch_op_answer(ledger: Arc<super::Ledger>, id: String, state: TaskState, what: &'static str) {
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(OP_ANSWER_MS)).await;
        let Some(e) = ledger.get(&id) else { return };
        if e.state != state || e.state.is_terminal() {
            return;
        }
        let mut e2 = e;
        e2.history.push(super::Transition { from: Some(state), to: state, at: crate::util::now_str(), by: Actor::System, note: Some(format!("{what} not answered by PLC within {OP_ANSWER_MS} ms")) });
        if let Err(err) = ledger.upsert(e2) {
            tracing::warn!(id, "op watch: {err}");
        }
    });
}

/// Later tasks of the same WorkId that are still alive. A WorkId is one job split into TaskIds
/// that the PLC runs in order; canceling task N while N+1.. are queued leaves the job half-done
/// with the tail still waiting to run on a state the canceled task never produced. So a cancel
/// takes the tail with it — the dialog says so before the operator confirms.
pub fn cascade_after(entries: &[LedgerEntry], key: TaskKey) -> Vec<LedgerEntry> {
    let mut v: Vec<LedgerEntry> =
        entries.iter().filter(|e| e.key().work_id == key.work_id && e.key().task_id > key.task_id && !e.state.is_terminal() && e.state != TaskState::Draft).cloned().collect();
    v.sort_by_key(|e| e.key().task_id);
    v
}

/// 취소(Delete) 거부 사유 — GR2 `UL_ParseOpcUaCommand` 의 `RES.Data[6]`(삭제 가능)과 `CL_Common_Command` 규칙:
/// AUTO 모드에서는 OPC UA Delete 를 **소비만 하고 처리하지 않는다**, 화물을 잡은 채 그리퍼가 닫혀 있으면 6020 으로 거부.
pub fn cancel_refusal(delete_allowed: bool, auto: bool, item_detect: bool, running: bool) -> Option<String> {
    if auto {
        return Some(if running {
            "AUTO 모드에서는 실행 중인 태스크를 취소(Delete)할 수 없습니다 — 로봇을 AUTO에서 내린 뒤 다시 시도하세요".into()
        } else {
            "AUTO 모드에서는 PLC 가 취소(Delete)를 처리하지 않습니다 (RES.Data[6] = 0) — AUTO 에서 내린 뒤 다시 시도하세요".into()
        });
    }
    if !delete_allowed {
        return Some(if item_detect {
            "화물을 잡은 채 그리퍼가 닫혀 있어 취소할 수 없습니다 (RES.Data[6] = 0, 알람 6020) — 그리퍼를 연 뒤 다시 시도하세요".into()
        } else {
            "PLC가 취소(Delete)를 허용하지 않습니다 (STAT.RES.Data[6] = 0)".into()
        });
    }
    None
}

/// 짝 취소 — 같은 이송 지시의 다른 Task 를 어떻게 할지. `(같이 지울 Task, 경고)`.
/// - PICK 취소: 짝 DROP(끝나지 않은)도 같이 지운다(DROP 이 PICK 없이 돌면 빈 그리퍼로 내려간다).
/// - DROP 취소: 짝 PICK 이 아직 시작 전이면 같이 지운다. PICK 이 도는 중이면 DROP 만 지우고 경고(PICK 이 끝나면 타이어가
///   그리퍼에 남는다). PICK 이 끝났으면 PLC 가 그리퍼 화물 데이터를 지우므로 콘솔 Hand 도 비우고 지시는 aborted(`Stock::on_task`).
pub fn pair_cancel(entries: &[LedgerEntry], e: &LedgerEntry) -> (Vec<LedgerEntry>, Option<String>) {
    let Some(to) = e.transfer_order_id.as_deref() else { return (Vec::new(), None) };
    let tt = gr_proto::TaskType::from_code(e.plc_task.task_type);
    let partner = entries.iter().filter(|p| p.id != e.id && p.transfer_order_id.as_deref() == Some(to) && !p.state.is_terminal()).max_by_key(|p| p.seq);
    let done_pick = || entries.iter().any(|p| p.id != e.id && p.transfer_order_id.as_deref() == Some(to) && p.state == TaskState::Completed);
    match tt {
        Some(gr_proto::TaskType::Pick) => (partner.cloned().into_iter().collect(), None),
        Some(gr_proto::TaskType::Drop) => match partner {
            Some(p) if matches!(p.state, TaskState::Draft | TaskState::Submitted | TaskState::Accepted | TaskState::Queued) => (vec![p.clone()], None),
            Some(p) => (Vec::new(), Some(format!("짝 PICK #{} 이 {} — DROP 만 취소하면 타이어가 그리퍼(Hand)에 남습니다 (이송 지시 {to})", p.seq, p.state.as_str()))),
            None if done_pick() => (Vec::new(), Some(format!("짝 PICK 은 이미 완료 — PLC 가 DROP 삭제와 함께 그리퍼 화물 데이터를 지우므로 콘솔 Hand 도 비우고 이송 지시 {to} 를 중단합니다"))),
            None => (Vec::new(), None),
        },
        _ => (Vec::new(), None),
    }
}

pub async fn cancel(st: &AppState, id: &str) -> Result<LedgerEntry, ApiError> {
    // Delete 는 600 ms 뒤 0 으로 되돌려야 끝난다 — 그 사이에 프로세스가 사라지면 PLC 에 명령이 걸린 채 남는다.
    let _busy = st.shutdown.enter(format!("Task 취소 {id}"))?;
    let (r, e) = st.find_task(id).ok_or_else(|| ApiError::NotFound(format!("task {id}")))?;
    match e.state {
        TaskState::Draft => {
            // 콘솔만 들고 있는 초안 — PLC 와 무관, 어느 모드에서나 지운다. 짝 초안도 같이.
            let (pair, _) = pair_cancel(&r.ledger.list(), &e);
            for p in pair.into_iter().filter(|p| p.state == TaskState::Draft) {
                r.ledger.transition(p, TaskState::Canceled, Actor::Ui, Some("draft discarded (짝 초안)".into()))?;
            }
            return r.ledger.transition(e, TaskState::Canceled, Actor::Ui, Some("draft discarded".into()));
        }
        TaskState::Submitted | TaskState::Accepted | TaskState::Queued | TaskState::Running | TaskState::Lost => {}
        s => return Err(ApiError::Conflict(format!("cannot cancel a {} task", s.as_str()))),
    }
    // `STAT.RES.Data[6]` says whether the PLC accepts a Delete right now (the mirror of Data[5] for
    // Complete); on top of that GR2 only processes `Command.Task.Delete` for the running task outside
    // AUTO mode. Both are checked here so the operator gets the reason instead of a silent no-op.
    if !st.cfg.demo {
        match status_view(st, r) {
            Some(v) => {
                if let Some(why) = cancel_refusal(v.reject_info().delete_allowed, v.mode.auto, v.status.item_detect, e.state == TaskState::Running) {
                    return Err(ApiError::Conflict(with_robot(&r.name, &why)));
                }
            }
            None => return Err(ApiError::PlcUnavailable(with_robot(&r.name, "상태 PLC 스냅샷 없음 — 취소 허용 여부를 확인할 수 없습니다"))),
        }
    }
    // Note the request *before* the op: `task_op` sleeps 600 ms for the re-arm, during which the sync
    // engine may already record `Canceled` — upserting a pre-op snapshot afterwards would clobber it.
    let id = e.id.clone();
    let mut tail = if e.key().work_id != 0 { cascade_after(&r.ledger.list(), e.key()) } else { Vec::new() };
    // 짝(같은 이송 지시) — 뒤에 도는 쪽(DROP)을 먼저 지우도록 앞에 세운다: PICK 만 지워지고 DROP 이 남으면 빈 그리퍼로 내려간다.
    let (pair, pair_warn) = pair_cancel(&r.ledger.list(), &e);
    let pair: Vec<LedgerEntry> = pair.into_iter().filter(|p| p.state != TaskState::Draft && !tail.iter().any(|t| t.id == p.id)).collect();
    let is_pick = gr_proto::TaskType::from_code(e.plc_task.task_type) == Some(gr_proto::TaskType::Pick);
    let mut order: Vec<LedgerEntry> = Vec::new();
    if is_pick {
        order.extend(pair.iter().cloned());
        order.push(e.clone());
    } else {
        order.push(e.clone());
        order.extend(pair.iter().cloned());
    }
    order.append(&mut tail);
    for t in order.iter() {
        let mut t2 = t.clone();
        let note = if t.id == e.id {
            match &pair_warn {
                Some(w) => format!("{} — {w}", super::sync::DELETE_REQUESTED),
                None => super::sync::DELETE_REQUESTED.to_string(),
            }
        } else if pair.iter().any(|p| p.id == t.id) {
            format!("{} (짝 Task {} 취소에 따라 — 이송 지시 {})", super::sync::DELETE_REQUESTED, e.key().task_id, e.transfer_order_id.as_deref().unwrap_or("-"))
        } else {
            format!("{} (cascade from task {})", super::sync::DELETE_REQUESTED, e.key().task_id)
        };
        t2.history.push(super::Transition { from: Some(t2.state), to: t2.state, at: crate::util::now_str(), by: Actor::Ui, note: Some(note) });
        r.ledger.upsert(t2)?;
    }
    // The PLC takes one Delete at a time (re-armed with zeros in between): the requested task first,
    // then its tail in TaskId order.
    for t in order.iter() {
        task_op(&r.cmd, TaskOp::Delete, t.key()).await?;
        watch_op_answer(r.ledger.clone(), t.id.clone(), t.state, "Delete");
    }
    Ok(r.ledger.get(&id).unwrap_or(e))
}

pub async fn force_complete(st: &AppState, id: &str) -> Result<LedgerEntry, ApiError> {
    let _busy = st.shutdown.enter(format!("Task 강제 완료 {id}"))?;
    let (r, e) = st.find_task(id).ok_or_else(|| ApiError::NotFound(format!("task {id}")))?;
    if !matches!(e.state, TaskState::Queued | TaskState::Running | TaskState::Accepted | TaskState::Lost) {
        return Err(ApiError::Conflict(format!("cannot complete a {} task", e.state.as_str())));
    }
    // `STAT.RES.Data[5]` says whether the PLC accepts a forced Complete right now.
    if !st.cfg.demo {
        match status_view(st, r) {
            Some(v) if !v.reject_info().complete_allowed => {
                return Err(ApiError::Conflict(with_robot(&r.name, "PLC가 강제 완료를 허용하지 않습니다 (STAT.RES.Data[5] = 0)")));
            }
            Some(_) => {}
            None => return Err(ApiError::PlcUnavailable(with_robot(&r.name, "상태 PLC 스냅샷 없음 — 완료 허용 여부를 확인할 수 없습니다"))),
        }
    }
    // Same ordering as `cancel`: note first, op second, fresh read last.
    let id = e.id.clone();
    let mut e2 = e.clone();
    e2.history.push(super::Transition { from: Some(e2.state), to: e2.state, at: crate::util::now_str(), by: Actor::Ui, note: Some(super::sync::COMPLETE_REQUESTED.into()) });
    r.ledger.upsert(e2)?;
    task_op(&r.cmd, TaskOp::Complete, e.key()).await?;
    watch_op_answer(r.ledger.clone(), id.clone(), e.state, "Complete");
    Ok(r.ledger.get(&id).unwrap_or(e))
}

pub async fn resubmit(st: &AppState, id: &str) -> Result<LedgerEntry, ApiError> {
    let (r, e) = st.find_task(id).ok_or_else(|| ApiError::NotFound(format!("task {id}")))?;
    let mut task = e.plc_task.clone();
    task.work_id = 0;
    task.task_id = 0;
    create_and_submit(st, r, e.origin, e.request.clone(), e.resolved.clone(), task, e.pallet.clone(), true).await
}

pub fn mark_failed(st: &AppState, id: &str, note: Option<String>) -> Result<LedgerEntry, ApiError> {
    let (r, e) = st.find_task(id).ok_or_else(|| ApiError::NotFound(format!("task {id}")))?;
    if e.state.is_terminal() {
        return Err(ApiError::Conflict(format!("cannot fail a {} task", e.state.as_str())));
    }
    let mut e2 = e;
    if e2.error.is_none() {
        e2.error = Some(note.clone().unwrap_or_else(|| "marked failed by operator".into()));
    }
    r.ledger.transition(e2, TaskState::Failed, Actor::Ui, note)
}

/// Deletes a terminal entry from the ledger (sqlite + cache) and broadcasts `Remove`.
pub fn remove(st: &AppState, id: &str) -> Result<(), ApiError> {
    let (r, _) = st.find_task(id).ok_or_else(|| ApiError::NotFound(format!("task {id}")))?;
    r.ledger.remove(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::ledger::{Ledger, Origin};

    fn entry(ledger: &Ledger, w: u32, t: u32, state: TaskState) -> LedgerEntry {
        let task = TaskData { work_id: w, task_id: t, task_type: 0x41, ..Default::default() };
        // `create` keys a console entry 0:0 until submission assigns the ids — set them as submit would.
        let mut e = ledger.create(Origin::Console, None, None, task).unwrap();
        e.work_id = w;
        e.task_id = t;
        ledger.upsert(e.clone()).unwrap();
        if state == TaskState::Draft { e } else { ledger.transition(e, state, Actor::Plc, None).unwrap() }
    }

    #[test]
    fn gate_reasons_name_the_robot() {
        let reasons = label_reasons("GR1", vec!["Task.Status.Accept = FALSE".into(), "AUTO 모드가 아님".into()]);
        assert_eq!(reasons, vec!["GR1: Task.Status.Accept = FALSE", "GR1: AUTO 모드가 아님"]);
        // 이미 이름이 붙은 사유는 두 번 붙지 않는다(게이트 → 409 로 두 번 지난다).
        assert_eq!(label_reasons("GR1", reasons.clone()), reasons);
        // 다른 로봇 이름으로 시작하는 사유는 그 로봇 것이 아니다 — 앞에 제 이름을 단다.
        assert_eq!(with_robot("GR1", "GR1_PLC S7 연결 없음"), "GR1: GR1_PLC S7 연결 없음");
    }

    #[test]
    fn submit_refusal_names_the_robot() {
        let d = submit_refusal("GR1", &label_reasons("GR1", vec!["Task.Status.Accept = FALSE".into(), "AUTO 모드가 아님".into()]));
        assert_eq!(d, "GR1: Task.Status.Accept = FALSE; GR1: AUTO 모드가 아님");
        // 사유가 없어도(게이트가 그 사이에 열렸어도) 어느 로봇이 거부했는지는 남는다.
        assert!(submit_refusal("GR2", &[]).starts_with("GR2: "));
        // 라벨 없이 들어온 사유도 이름을 얻는다.
        assert!(submit_refusal("GR2", &["태스크 버퍼 가득 참".to_string()]).starts_with("GR2: "));
    }

    #[test]
    fn cascade_takes_later_alive_tasks_of_the_same_work_only() {
        let db = Db::open_memory().unwrap();
        let ledger = Ledger::new(db, "GR2", None).unwrap();
        entry(&ledger, 7, 1, TaskState::Completed); // earlier, done — untouched
        let me = entry(&ledger, 7, 2, TaskState::Running);
        entry(&ledger, 7, 4, TaskState::Queued); // later — goes
        entry(&ledger, 7, 3, TaskState::Accepted); // later — goes, sorted before 4
        entry(&ledger, 7, 5, TaskState::Draft); // never submitted — stays a draft
        entry(&ledger, 7, 6, TaskState::Canceled); // already terminal
        entry(&ledger, 8, 3, TaskState::Queued); // other work
        let tail: Vec<u32> = cascade_after(&ledger.list(), me.key()).iter().map(|e| e.key().task_id).collect();
        assert_eq!(tail, vec![3, 4]);
        // the requested task itself is never in its own tail
        assert!(cascade_after(&ledger.list(), TaskKey { work_id: 7, task_id: 4 }).is_empty());
    }

    fn pair_entry(ledger: &Ledger, tt: u8, t: u32, state: TaskState, to: &str) -> LedgerEntry {
        let task = TaskData { work_id: 900 + t, task_id: t, task_type: tt, ..Default::default() };
        let mut e = ledger.create(Origin::Scenario, None, None, task).unwrap();
        e.work_id = 900 + t;
        e.task_id = t;
        e.transfer_order_id = Some(to.into());
        ledger.upsert(e.clone()).unwrap();
        if state == TaskState::Draft { e } else { ledger.transition(e, state, Actor::Plc, None).unwrap() }
    }

    /// 짝 취소: PICK 을 지우면 DROP 도, 시작 전 PICK 의 DROP 을 지우면 PICK 도. PICK 이 돌거나 끝났으면 DROP 만 + 경고.
    #[test]
    fn pair_cancel_keeps_the_pair_together() {
        use gr_proto::{CMD_TASK_DROP as D, CMD_TASK_PICK as P};
        let ledger = Ledger::new(Db::open_memory().unwrap(), "GR2", None).unwrap();
        let pick = pair_entry(&ledger, P, 1, TaskState::Running, "TO-1");
        let drop = pair_entry(&ledger, D, 2, TaskState::Queued, "TO-1");
        let other = pair_entry(&ledger, D, 3, TaskState::Queued, "TO-2");
        let all = ledger.list();
        let (with, warn) = pair_cancel(&all, &pick);
        assert_eq!((with.iter().map(|e| e.task_id).collect::<Vec<_>>(), warn), (vec![2], None), "PICK takes its DROP along");
        let (with, warn) = pair_cancel(&all, &drop);
        assert!(with.is_empty() && warn.unwrap().contains("running"), "running PICK: DROP only, tires stay on the gripper");
        assert!(pair_cancel(&all, &other).0.is_empty());
        // PICK 시작 전 → DROP 을 지우면 PICK 도
        let ledger = Ledger::new(Db::open_memory().unwrap(), "GR2", None).unwrap();
        pair_entry(&ledger, P, 1, TaskState::Queued, "TO-1");
        let drop = pair_entry(&ledger, D, 2, TaskState::Queued, "TO-1");
        assert_eq!(pair_cancel(&ledger.list(), &drop).0.iter().map(|e| e.task_id).collect::<Vec<_>>(), vec![1]);
        // PICK 완료 뒤 DROP → Hand 경고
        let ledger = Ledger::new(Db::open_memory().unwrap(), "GR2", None).unwrap();
        pair_entry(&ledger, P, 1, TaskState::Completed, "TO-1");
        let drop = pair_entry(&ledger, D, 2, TaskState::Queued, "TO-1");
        let (with, warn) = pair_cancel(&ledger.list(), &drop);
        assert!(with.is_empty() && warn.unwrap().contains("Hand"));
    }

    #[test]
    fn cancel_refusal_follows_the_plc_rule() {
        assert!(cancel_refusal(true, false, false, true).is_none());
        assert!(cancel_refusal(false, true, false, false).unwrap().contains("AUTO"), "AUTO: PLC ignores Delete even for queued tasks");
        assert!(cancel_refusal(false, true, false, true).unwrap().contains("실행 중"));
        assert!(cancel_refusal(false, false, true, true).unwrap().contains("6020"));
        assert!(cancel_refusal(false, false, false, false).unwrap().contains("RES.Data[6]"));
    }
}
