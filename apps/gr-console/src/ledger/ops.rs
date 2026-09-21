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
}

pub fn gate(st: &AppState, r: &RobotCtx) -> Gate {
    let mut reasons = Vec::new();
    if !r.cmd.is_ready() {
        reasons.push("OPC UA 명령 경로가 준비되지 않음".into());
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
                    reasons.push("PLC Task.Status.Accept = FALSE".into());
                }
                if !v.task.queue.iter().any(|t| t.is_zero()) {
                    reasons.push("PLC 태스크 버퍼 가득 참".into());
                }
                if !v.mode.auto && !v.mode.auto_ready && !st.cfg.demo {
                    reasons.push("로봇이 AUTO 모드가 아님".into());
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
    Gate { can_submit: reasons.is_empty(), reasons }
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
        return Err(ApiError::Conflict(g.reasons.join("; ")));
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

pub async fn cancel(st: &AppState, id: &str) -> Result<LedgerEntry, ApiError> {
    // Delete 는 600 ms 뒤 0 으로 되돌려야 끝난다 — 그 사이에 프로세스가 사라지면 PLC 에 명령이 걸린 채 남는다.
    let _busy = st.shutdown.enter(format!("Task 취소 {id}"))?;
    let (r, e) = st.find_task(id).ok_or_else(|| ApiError::NotFound(format!("task {id}")))?;
    match e.state {
        TaskState::Draft => return r.ledger.transition(e, TaskState::Canceled, Actor::Ui, Some("draft discarded".into())),
        TaskState::Submitted | TaskState::Accepted | TaskState::Queued | TaskState::Running | TaskState::Lost => {}
        s => return Err(ApiError::Conflict(format!("cannot cancel a {} task", s.as_str()))),
    }
    // `STAT.RES.Data[6]` says whether the PLC accepts a Delete right now (the mirror of Data[5] for
    // Complete); on top of that GR2 only processes `Command.Task.Delete` for the running task outside
    // AUTO mode. Both are checked here so the operator gets the reason instead of a silent no-op.
    if !st.cfg.demo {
        match status_view(st, r) {
            Some(v) if !v.reject_info().delete_allowed => {
                return Err(ApiError::Conflict("PLC가 취소(Delete)를 허용하지 않습니다 (STAT.RES.Data[6] = 0)".into()));
            }
            Some(v) if e.state == TaskState::Running && v.mode.auto => {
                return Err(ApiError::Conflict("AUTO 모드에서는 실행 중인 태스크를 취소(Delete)할 수 없습니다 — 로봇을 AUTO에서 내린 뒤 다시 시도하세요".into()));
            }
            Some(_) => {}
            None => return Err(ApiError::PlcUnavailable("상태 PLC 스냅샷 없음 — 취소 허용 여부를 확인할 수 없습니다".into())),
        }
    }
    // Note the request *before* the op: `task_op` sleeps 600 ms for the re-arm, during which the sync
    // engine may already record `Canceled` — upserting a pre-op snapshot afterwards would clobber it.
    let id = e.id.clone();
    let tail = if e.key().work_id != 0 { cascade_after(&r.ledger.list(), e.key()) } else { Vec::new() };
    for t in std::iter::once(&e).chain(tail.iter()) {
        let mut t2 = t.clone();
        let note = if t.id == e.id { super::sync::DELETE_REQUESTED.to_string() } else { format!("{} (cascade from task {})", super::sync::DELETE_REQUESTED, e.key().task_id) };
        t2.history.push(super::Transition { from: Some(t2.state), to: t2.state, at: crate::util::now_str(), by: Actor::Ui, note: Some(note) });
        r.ledger.upsert(t2)?;
    }
    // The PLC takes one Delete at a time (re-armed with zeros in between): the requested task first,
    // then its tail in TaskId order.
    for t in std::iter::once(&e).chain(tail.iter()) {
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
                return Err(ApiError::Conflict("PLC가 강제 완료를 허용하지 않습니다 (STAT.RES.Data[5] = 0)".into()));
            }
            Some(_) => {}
            None => return Err(ApiError::PlcUnavailable("상태 PLC 스냅샷 없음 — 완료 허용 여부를 확인할 수 없습니다".into())),
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
}
