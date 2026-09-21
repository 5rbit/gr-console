//! 로봇 단위 운전 명령 — 사이드바 로봇 행의 우클릭 메뉴(`POST /api/robots/{id}/command/{action}`).
//!
//! - `start` / `stop` / `reset` / `buzzerstop` : `GR[n].CMD.Command.Common.Start` / `.Stop.Normal` / `.Common.Reset` /
//!   `.Common.BuzzerStop` 펄스.
//!   GRM 은 Command 가 바뀌면 헤더 없이 GR 로 중계하고, GR 에코 후 스스로 지운다.
//! - `complete` : PLC 가 지금 실행 중인 Task(`STAT.Task.Now`) 강제 완료 — Task 관리의 Complete 와 같은 경로.
//! - Complete / Clear 는 AUTO 에서 거부한다(`ops::auto_refusal`, Task 관리의 개별 완료·삭제와 같은 규칙).
//! - `clear` : 이 로봇의 살아 있는 Task(원장 + PLC 대기열) 전부 Delete. WorkId 마다 첫 TaskId 만 부르면
//!   `ops::cancel` 이 뒤따르는 TaskId 를 같이 지운다.

use std::collections::BTreeMap;

use gr_proto::TaskKey;
use serde::Deserialize;
use serde_json::{Value as Json, json};

use super::ops::{self, auto_refusal, status_view, task_op, with_robot};
use super::{LedgerEntry, TaskState};
use crate::cmd::{CommandBit, TaskOp};
use crate::error::ApiError;
use crate::state::{AppState, RobotCtx};

/// 비트를 올려 두는 시간. GRM 이 바뀐 Command 를 BSEND 로 넘기고 GR 이 한 사이클 보는 데 충분하다
/// (Task Complete/Delete 재무장 600 ms 와 같은 성격, 여유를 더 둔다).
const PULSE_MS: u64 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RobotAction {
    Start,
    Stop,
    Reset,
    BuzzerStop,
    Complete,
    Clear,
}

impl RobotAction {
    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(Json::String(s.to_ascii_lowercase())).ok()
    }
}

pub async fn run(st: &AppState, r: &RobotCtx, action: RobotAction) -> Result<Json, ApiError> {
    let bit = match action {
        RobotAction::Start => CommandBit::Start,
        RobotAction::Stop => CommandBit::Stop,
        RobotAction::Reset => CommandBit::Reset,
        RobotAction::BuzzerStop => CommandBit::BuzzerStop,
        RobotAction::Complete => return complete_now(st, r).await,
        RobotAction::Clear => return clear(st, r).await,
    };
    // 펄스 도중 프로세스가 사라지면 비트가 걸린 채 남는다 — 종료 절차가 끝날 때까지 기다리게 등록.
    let _busy = st.shutdown.enter(format!("{} {:?}", r.name, bit))?;
    if let Some(why) = r.cmd.not_ready_reason() {
        return Err(ApiError::OpcNotReady(with_robot(&r.name, &why)));
    }
    r.cmd.write_command_bit(bit, true).await?;
    tokio::time::sleep(std::time::Duration::from_millis(PULSE_MS)).await;
    r.cmd.write_command_bit(bit, false).await?;
    tracing::info!(robot = %r.name, path = bit.path(), "robot command pulse");
    Ok(json!({ "robot": r.name, "action": action_name(action), "path": bit.path() }))
}

fn action_name(a: RobotAction) -> &'static str {
    match a {
        RobotAction::Start => "start",
        RobotAction::Stop => "stop",
        RobotAction::Reset => "reset",
        RobotAction::BuzzerStop => "buzzerstop",
        RobotAction::Complete => "complete",
        RobotAction::Clear => "clear",
    }
}

fn key_str(k: TaskKey) -> String {
    format!("{}/{}", k.work_id, k.task_id)
}

async fn complete_now(st: &AppState, r: &RobotCtx) -> Result<Json, ApiError> {
    let v = status_view(st, r).ok_or_else(|| ApiError::PlcUnavailable(with_robot(&r.name, "상태 PLC 스냅샷 없음 — 실행 중 Task 를 알 수 없습니다")))?;
    let key = v.task.now.key();
    if !st.cfg.demo && v.mode.auto {
        return Err(auto_refusal(&r.name, "완료(Complete)"));
    }
    if key.work_id == 0 {
        return Err(ApiError::Conflict(with_robot(&r.name, "실행 중인 Task 가 없습니다")));
    }
    match r.ledger.find_by_key(key) {
        Some(e) => {
            ops::force_complete(st, &e.id).await?;
        }
        // 원장에 아직 안 잡힌 Task(콘솔 기동 직후 등) — 같은 검사를 하고 바로 쓴다.
        None => {
            if !st.cfg.demo && !v.reject_info().complete_allowed {
                return Err(ApiError::Conflict(with_robot(&r.name, "PLC가 강제 완료를 허용하지 않습니다 (STAT.RES.Data[5] = 0)")));
            }
            let _busy = st.shutdown.enter(format!("{} Task 강제 완료 {}", r.name, key_str(key)))?;
            task_op(&r.cmd, TaskOp::Complete, key).await?;
        }
    }
    Ok(json!({ "robot": r.name, "action": "complete", "task": key_str(key) }))
}

fn alive(e: &LedgerEntry) -> bool {
    matches!(e.state, TaskState::Submitted | TaskState::Accepted | TaskState::Queued | TaskState::Running | TaskState::Lost)
}

async fn clear(st: &AppState, r: &RobotCtx) -> Result<Json, ApiError> {
    let view = status_view(st, r);
    if !st.cfg.demo {
        let v = view.as_ref().ok_or_else(|| ApiError::PlcUnavailable(with_robot(&r.name, "상태 PLC 스냅샷 없음 — 삭제 허용 여부를 확인할 수 없습니다")))?;
        // 부분 삭제로 끝나지 않게 막히는 조건은 먼저 본다 (`ops::cancel` 과 같은 규칙).
        if v.mode.auto {
            return Err(auto_refusal(&r.name, "Clear"));
        }
        if !v.reject_info().delete_allowed {
            return Err(ApiError::Conflict(with_robot(&r.name, "PLC가 취소(Delete)를 허용하지 않습니다 (STAT.RES.Data[6] = 0)")));
        }
    }
    // WorkId → (가장 앞 TaskId, 원장 id). 원장에 없는 PLC 대기열·실행 Task 도 포함한다.
    let mut heads: BTreeMap<u32, (u32, Option<String>)> = BTreeMap::new();
    let mut put = |k: TaskKey, id: Option<String>| {
        if k.work_id == 0 {
            return;
        }
        let slot = heads.entry(k.work_id).or_insert((k.task_id, id.clone()));
        if k.task_id < slot.0 || (k.task_id == slot.0 && slot.1.is_none()) {
            *slot = (k.task_id, id);
        }
    };
    for e in r.ledger.list().iter().filter(|e| alive(e)) {
        put(e.key(), Some(e.id.clone()));
    }
    if let Some(v) = &view {
        for t in std::iter::once(&v.task.now).chain(v.task.queue.iter()) {
            let k = t.key();
            put(k, r.ledger.find_by_key(k).filter(alive).map(|e| e.id));
        }
    }
    if heads.is_empty() {
        return Err(ApiError::Conflict(with_robot(&r.name, "지울 Task 가 없습니다")));
    }
    let mut deleted = Vec::new();
    for (work_id, (task_id, id)) in heads {
        let key = TaskKey { work_id, task_id };
        match id {
            Some(id) => {
                // 앞 Delete 의 600 ms 재무장 사이에 PLC 가 이 Task 를 끝냈을 수 있다 — 차례가 오면 다시 본다.
                if !r.ledger.get(&id).is_some_and(|e| alive(&e)) {
                    continue;
                }
                let e = ops::cancel(st, &id).await.map_err(|err| partial(r, &deleted, err))?;
                deleted.push(key_str(e.key()));
            }
            None => {
                let _busy = st.shutdown.enter(format!("{} Task 삭제 {}", r.name, key_str(key)))?;
                task_op(&r.cmd, TaskOp::Delete, key).await.map_err(|err| partial(r, &deleted, err))?;
                deleted.push(key_str(key));
            }
        }
    }
    Ok(json!({ "robot": r.name, "action": "clear", "deleted": deleted }))
}

/// 중간에 실패하면 이미 보낸 Delete 를 사유에 붙인다 — 화면이 "아무것도 안 됐다"로 읽지 않게.
fn partial(r: &RobotCtx, done: &[String], err: ApiError) -> ApiError {
    if done.is_empty() {
        return err;
    }
    ApiError::Conflict(with_robot(&r.name, &format!("{err} (이미 삭제 요청: {})", done.join(", "))))
}

#[cfg(test)]
mod tests {
    use super::RobotAction;

    #[test]
    fn parses_actions_case_insensitively() {
        assert_eq!(RobotAction::parse("Start"), Some(RobotAction::Start));
        assert_eq!(RobotAction::parse("CLEAR"), Some(RobotAction::Clear));
        assert_eq!(RobotAction::parse("complete"), Some(RobotAction::Complete));
        assert_eq!(RobotAction::parse("BuzzerStop"), Some(RobotAction::BuzzerStop));
        assert_eq!(RobotAction::parse("init"), None);
    }
}
