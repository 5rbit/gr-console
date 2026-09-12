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
pub async fn create_and_submit(
    st: &AppState,
    r: &RobotCtx,
    origin: Origin,
    request: Option<super::TaskRequest>,
    resolved: Option<gr_proto::TaskParams>,
    task: TaskData,
    submit_now: bool,
) -> Result<LedgerEntry, ApiError> {
    let e = r.ledger.create(origin, request, resolved, task)?;
    if !submit_now {
        return Ok(e);
    }
    submit(st, r, e).await
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

pub async fn cancel(st: &AppState, id: &str) -> Result<LedgerEntry, ApiError> {
    let (r, e) = st.find_task(id).ok_or_else(|| ApiError::NotFound(format!("task {id}")))?;
    match e.state {
        TaskState::Draft => return r.ledger.transition(e, TaskState::Canceled, Actor::Ui, Some("draft discarded".into())),
        TaskState::Submitted | TaskState::Accepted | TaskState::Queued | TaskState::Running | TaskState::Lost => {}
        s => return Err(ApiError::Conflict(format!("cannot cancel a {} task", s.as_str()))),
    }
    // GR2 only processes `Command.Task.Delete` for the running task outside AUTO mode.
    if e.state == TaskState::Running && !st.cfg.demo {
        match status_view(st, r) {
            Some(v) if v.mode.auto => {
                return Err(ApiError::Conflict("AUTO 모드에서는 실행 중인 태스크를 취소(Delete)할 수 없습니다 — 로봇을 AUTO에서 내린 뒤 다시 시도하세요".into()));
            }
            Some(_) => {}
            None => return Err(ApiError::PlcUnavailable("상태 PLC 스냅샷 없음 — AUTO 모드 여부를 확인할 수 없습니다".into())),
        }
    }
    // Note the request *before* the op: `task_op` sleeps 600 ms for the re-arm, during which the sync
    // engine may already record `Canceled` — upserting a pre-op snapshot afterwards would clobber it.
    let id = e.id.clone();
    let mut e2 = e.clone();
    e2.history.push(super::Transition { from: Some(e2.state), to: e2.state, at: crate::util::now_str(), by: Actor::Ui, note: Some("delete requested".into()) });
    r.ledger.upsert(e2)?;
    task_op(&r.cmd, TaskOp::Delete, e.key()).await?;
    Ok(r.ledger.get(&id).unwrap_or(e))
}

pub async fn force_complete(st: &AppState, id: &str) -> Result<LedgerEntry, ApiError> {
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
    e2.history.push(super::Transition { from: Some(e2.state), to: e2.state, at: crate::util::now_str(), by: Actor::Ui, note: Some("complete requested".into()) });
    r.ledger.upsert(e2)?;
    task_op(&r.cmd, TaskOp::Complete, e.key()).await?;
    Ok(r.ledger.get(&id).unwrap_or(e))
}

pub async fn resubmit(st: &AppState, id: &str) -> Result<LedgerEntry, ApiError> {
    let (r, e) = st.find_task(id).ok_or_else(|| ApiError::NotFound(format!("task {id}")))?;
    let mut task = e.plc_task.clone();
    task.work_id = 0;
    task.task_id = 0;
    create_and_submit(st, r, e.origin, e.request.clone(), e.resolved.clone(), task, true).await
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
