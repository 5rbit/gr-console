//! Ledger operations: submit / cancel / force-complete / resubmit (M3-B refines; M1 provides working versions).

use std::sync::Arc;

use gr_proto::{TaskData, TaskKey};

use super::{Actor, LedgerEntry, Origin, TaskState};
use crate::cmd::{CommandPort, TaskOp};
use crate::error::ApiError;
use crate::state::AppState;

/// Gate for a new submission.
pub struct Gate {
    pub can_submit: bool,
    pub reasons: Vec<String>,
}

pub fn gate(st: &AppState) -> Gate {
    let mut reasons = Vec::new();
    if !st.cmd.is_ready() {
        reasons.push("OPC UA 명령 경로가 준비되지 않음".into());
    }
    match st.status_plc() {
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
    if st.ledger.list().iter().any(|e| e.state == TaskState::Submitted) {
        reasons.push("에코 대기 중인 제출이 있음".into());
    }
    if let Some(grm) = st.grm_plc() {
        for (path, label) in [("GR[2].CMD.Header.CMD", "GRM CMD 헤더가 비어 있지 않음 (이전 명령 미소진)")] {
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
pub async fn submit(st: &AppState, entry: LedgerEntry) -> Result<LedgerEntry, ApiError> {
    if entry.state != TaskState::Draft {
        return Err(ApiError::Conflict(format!("task {} is {}", entry.id, entry.state.as_str())));
    }
    let g = gate(st);
    if !g.can_submit {
        return Err(ApiError::Conflict(g.reasons.join("; ")));
    }
    let header = st.cmd.write_task(&entry.plc_task).await?;
    let mut e = entry;
    e.header = Some(header);
    e.submitted_at = Some(crate::util::now_str());
    st.ledger.transition(e, TaskState::Submitted, Actor::Ui, None)
}

/// Creates + submits in one go (used by the task-issue slice and the scenario runner).
pub async fn create_and_submit(st: &AppState, origin: Origin, request: Option<super::TaskRequest>, resolved: Option<gr_proto::TaskParams>, task: TaskData, submit_now: bool) -> Result<LedgerEntry, ApiError> {
    let e = st.ledger.create(origin, request, resolved, task)?;
    if !submit_now {
        return Ok(e);
    }
    submit(st, e).await
}

async fn task_op(cmd: &Arc<CommandPort>, op: TaskOp, key: TaskKey) -> Result<(), ApiError> {
    cmd.write_task_op(op, key.work_id, key.task_id).await?;
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    cmd.write_task_op(op, 0, 0).await // re-arm
}

pub async fn cancel(st: &AppState, id: &str) -> Result<LedgerEntry, ApiError> {
    let e = st.ledger.get(id).ok_or_else(|| ApiError::NotFound(format!("task {id}")))?;
    match e.state {
        TaskState::Draft => return st.ledger.transition(e, TaskState::Canceled, Actor::Ui, Some("draft discarded".into())),
        TaskState::Submitted | TaskState::Accepted | TaskState::Queued | TaskState::Running | TaskState::Lost => {}
        s => return Err(ApiError::Conflict(format!("cannot cancel a {} task", s.as_str()))),
    }
    task_op(&st.cmd, TaskOp::Delete, e.key()).await?;
    let mut e2 = e;
    e2.history.push(super::Transition { from: Some(e2.state), to: e2.state, at: crate::util::now_str(), by: Actor::Ui, note: Some("delete requested".into()) });
    st.ledger.upsert(e2)
}

pub async fn force_complete(st: &AppState, id: &str) -> Result<LedgerEntry, ApiError> {
    let e = st.ledger.get(id).ok_or_else(|| ApiError::NotFound(format!("task {id}")))?;
    if !matches!(e.state, TaskState::Queued | TaskState::Running | TaskState::Accepted | TaskState::Lost) {
        return Err(ApiError::Conflict(format!("cannot complete a {} task", e.state.as_str())));
    }
    task_op(&st.cmd, TaskOp::Complete, e.key()).await?;
    let mut e2 = e;
    e2.history.push(super::Transition { from: Some(e2.state), to: e2.state, at: crate::util::now_str(), by: Actor::Ui, note: Some("complete requested".into()) });
    st.ledger.upsert(e2)
}

pub async fn resubmit(st: &AppState, id: &str) -> Result<LedgerEntry, ApiError> {
    let e = st.ledger.get(id).ok_or_else(|| ApiError::NotFound(format!("task {id}")))?;
    let mut task = e.plc_task.clone();
    task.work_id = 0;
    task.task_id = 0;
    create_and_submit(st, e.origin, e.request.clone(), e.resolved.clone(), task, true).await
}

pub fn mark_failed(st: &AppState, id: &str, note: Option<String>) -> Result<LedgerEntry, ApiError> {
    let e = st.ledger.get(id).ok_or_else(|| ApiError::NotFound(format!("task {id}")))?;
    st.ledger.transition(e, TaskState::Failed, Actor::Ui, note)
}
