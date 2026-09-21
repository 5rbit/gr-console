//! Scenario run loop. The planning parts (`Plan`, `Cursor`, `decide`, `reached`) are pure and unit
//! tested; `run_loop` glues them to the issue/ledger slices.
//!
//! Per step: compose → wait for the submission gate (250 ms poll, honours pause/stop) →
//! `create_and_submit` → follow the ledger entry (broadcast events, 500 ms poll fallback) until the
//! step's `wait_for` state is reached or the task ends badly → apply `on_failure` → `wait_after_ms`.
//!
//! 미리 넣기(`wait_for = accepted`): 다음 스텝은 앞 Task 가 끝나기 전에 작성된다. 작성은 진행 중 작업을
//! 반영한 예상 재고(`issue::projected_stock`)로 하고, 게이트를 통과한 뒤 **다시** 작성해 보낸다. 앞 스텝의
//! Task 가 나중에 실패·취소·Lost 가 되면(`earlier_broken`) 그 뒤 스텝은 보내지 않고 실행을 멈춘다.
//!
//! 큐 깊이 1(`queue_depth_reason`): PLC 에는 실행 중 1 건 + 다음 1 건까지만. 로봇 원장에 Submitted/Accepted/Queued 가
//! 하나라도 있으면 다음 스텝은 **예정**으로 기다린다. 미리 넣은 실행은 마지막 Task 가 **완료**돼야 끝난다(`drain`).
//!
//! PICK/DROP 짝(로봇별): 한 로봇의 PICK 이 나가면 그 로봇의 짝 DROP 이 나갈 때까지 정지·일시정지를 미룬다 — 그 사이의
//! 다른 로봇 스텝(회피 MOVE 등)은 그대로 돈다. 짝이 걸려 있는 로봇이 하나라도 있으면 정지는 모든 짝의 DROP 제출 뒤 적용.
//! PICK 이 실패하면 DROP 은 보내지 않고 멈춘다. 짝의 실패는 `skip` 이어도 실행을 멈춘다(`decide_pair`).
//!
//! 이송 지시: PICK 을 보내기 전에 짝마다 `TO-…` 를 열고(`planned`) PICK·DROP 요청에 같은 id 를 단다. 보내지 못한 채
//! 끝나면 `aborted`. 그 뒤 상태(picking/in_hand/dropping/done/failed)는 원장 이벤트가 옮긴다(`stock::transfer`).

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::watch;

use super::{Ctl, OnFailure, Phase, Runner, Scenario, Step, StepResult, WaitFor};
use crate::error::ApiError;
use crate::ledger::{LedgerEvent, Origin, ScenarioSource, TaskRequest, TaskState};
use crate::state::AppState;
use crate::util::now_str;

/// Body of `POST /api/scenarios/{id}/run`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct RunOptions {
    /// Overrides the scenario's `repeat` (0 = infinite).
    pub repeat: Option<u32>,
    /// 0-based step to start the first iteration at.
    pub start_step: Option<u32>,
    /// Robot for steps that name none (`None` = default robot). The operator's sidebar selection.
    pub robot: Option<u8>,
}

pub const MAX_RETRIES: u32 = 3;
pub const GATE_POLL_MS: u64 = 250;
pub const LEDGER_POLL_MS: u64 = 500;

// ---------------------------------------------------------------------------------------------
// Pure planner
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    pub step_count: u32,
    /// `None` = infinite.
    pub total_iterations: Option<u32>,
    pub start_step: u32,
    /// Run robot: steps without their own `robot` go here.
    pub robot: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    /// 1-based.
    pub iteration: u32,
    /// 0-based.
    pub step_index: u32,
}

impl Plan {
    pub fn new(s: &Scenario, o: &RunOptions) -> Result<Plan, ApiError> {
        let step_count = s.steps.len() as u32;
        if step_count == 0 {
            return Err(ApiError::BadRequest("scenario has no steps".into()));
        }
        let repeat = o.repeat.unwrap_or(s.repeat);
        let start_step = o.start_step.unwrap_or(0);
        if start_step >= step_count {
            return Err(ApiError::BadRequest(format!("start_step {start_step} out of range (0..{step_count})")));
        }
        Ok(Plan { step_count, total_iterations: if repeat == 0 { None } else { Some(repeat) }, start_step, robot: o.robot })
    }

    /// Robot a step is sent to: its own, else the run robot, else `None` (= default robot).
    pub fn robot_for(&self, step: &Step) -> Option<u8> {
        step.robot.or(self.robot)
    }

    pub fn first(&self) -> Cursor {
        Cursor { iteration: 1, step_index: self.start_step }
    }

    /// Position after `c`, or `None` when the plan is exhausted. Only the first iteration honours
    /// `start_step`; later iterations start at step 0.
    pub fn advance(&self, c: Cursor) -> Option<Cursor> {
        if c.step_index + 1 < self.step_count {
            return Some(Cursor { iteration: c.iteration, step_index: c.step_index + 1 });
        }
        match self.total_iterations {
            Some(n) if c.iteration >= n => None,
            _ => Some(Cursor { iteration: c.iteration + 1, step_index: 0 }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Move on to the next step.
    Next,
    /// Run the same step again.
    Retry,
    /// End the run as failed.
    Abort,
}

/// `on_failure` policy after a failed attempt. `attempt` is the number of attempts made so far
/// (1 = the first try), so `Retry` allows `max_retries` extra attempts.
pub fn decide(policy: OnFailure, attempt: u32, max_retries: u32) -> Decision {
    match policy {
        OnFailure::Stop => Decision::Abort,
        OnFailure::Skip => Decision::Next,
        OnFailure::Retry => {
            if attempt <= max_retries {
                Decision::Retry
            } else {
                Decision::Abort
            }
        }
    }
}

/// 이번 실행에서 이미 넘어간(도달한) 스텝의 Task 가 나중에 나쁘게 끝났는가 — 미리 넣기에서 앞 Task 가 실패하면
/// 뒤 Task 의 전제(재고·들고 있는 화물)가 틀어진다. `(step_index, work_id, task_id, 지금 상태)` 중 첫 번째.
pub fn earlier_broken(done: &[(u32, u32, u32, TaskState)]) -> Option<String> {
    done.iter()
        .find(|(_, _, _, s)| matches!(s, TaskState::Rejected | TaskState::Failed | TaskState::Canceled | TaskState::Lost))
        .map(|(i, w, t, s)| format!("앞 스텝 {} 의 Task {w}/{t} 가 {} — 이 스텝부터 보내지 않음 (이미 넣은 Task 는 PLC 버퍼에 남아 있을 수 있음)", i + 1, s.as_str()))
}

/// 밖에서(PLC·다른 화면) 취소·실패된 앞 스텝 중 실행을 멈출 것 — PICK/DROP 은 짝이 깨지므로 늘 멈추고,
/// 그 밖의 스텝은 `on_failure = skip` 이면 넘긴다.
pub fn breaks_run(step: &Step) -> bool {
    matches!(step.task_type, gr_proto::TaskType::Pick | gr_proto::TaskType::Drop) || step.on_failure != OnFailure::Skip
}

fn stops_run(scenario: &Scenario, states: Vec<(u32, u32, u32, TaskState)>) -> Vec<(u32, u32, u32, TaskState)> {
    states.into_iter().filter(|(i, ..)| scenario.steps.get(*i as usize).is_none_or(breaks_run)).collect()
}

/// 미리 넣기 검사에 볼 최근 결과 수(버퍼 4 칸보다 넉넉히).
const EARLIER_LOOKBACK: usize = 8;

/// 큐 깊이 1 — 로봇 원장에 아직 실행되지 않은 Task(Submitted/Accepted/Queued)가 있으면 다음 스텝은 기다린다.
/// 실행 중 1 건 + 다음 1 건까지만 PLC 에 있게 한다(중간에 문제가 생겨도 꼬이는 작업이 하나뿐이도록).
pub fn queue_depth_reason(states: impl IntoIterator<Item = TaskState>) -> Option<String> {
    let waiting = states.into_iter().filter(|s| matches!(s, TaskState::Submitted | TaskState::Accepted | TaskState::Queued)).count();
    (waiting > 0).then(|| format!("예정 — 실행 대기 중인 Task {waiting} 건 (실행 중 + 다음 1 건까지만)"))
}

/// 실패한 스텝의 다음 걸음 — PICK/DROP 은 `skip` 해도 짝이 깨지므로 멈춘다(재시도는 그대로).
pub fn decide_pair(tt: gr_proto::TaskType, d: Decision) -> Decision {
    match (tt, d) {
        (gr_proto::TaskType::Pick | gr_proto::TaskType::Drop, Decision::Next) => Decision::Abort,
        (_, d) => d,
    }
}

fn earlier_states(st: &AppState, runner: &Runner) -> Vec<(u32, u32, u32, TaskState)> {
    run_states(st, runner, EARLIER_LOOKBACK)
}

/// 이번 실행에서 도달한 스텝들의 Task 지금 상태(최근 `limit` 개).
fn run_states(st: &AppState, runner: &Runner, limit: usize) -> Vec<(u32, u32, u32, TaskState)> {
    let snap = runner.current();
    snap.results
        .iter()
        .rev()
        .filter(|r| r.error.is_none())
        .take(limit)
        .filter_map(|r| {
            let (_, e) = st.find_task(r.task_id.as_deref()?)?;
            Some((r.step_index, e.work_id, e.task_id, e.state))
        })
        .collect()
}

/// `Some(true)` = the awaited state is reached, `Some(false)` = the task ended badly,
/// `None` = keep waiting.
pub fn reached(wait_for: WaitFor, state: TaskState) -> Option<bool> {
    match state {
        TaskState::Rejected | TaskState::Failed | TaskState::Canceled | TaskState::Lost => Some(false),
        TaskState::Completed => Some(true),
        TaskState::Accepted | TaskState::Queued | TaskState::Running => match wait_for {
            WaitFor::Accepted => Some(true),
            WaitFor::Completed => None,
        },
        TaskState::Draft | TaskState::Submitted => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Async loop
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Go {
    Go,
    Stopped,
}

/// Blocks while paused; returns `Stopped` once a stop is requested (or the controller is gone).
async fn wait_resumed(ctl: &mut watch::Receiver<Ctl>) -> Go {
    loop {
        let c = *ctl.borrow_and_update();
        if c.stop {
            return Go::Stopped;
        }
        if !c.paused {
            return Go::Go;
        }
        if ctl.changed().await.is_err() {
            return Go::Stopped;
        }
    }
}

/// Waits for a stop request (never returns `Go`).
async fn wait_stop(ctl: &mut watch::Receiver<Ctl>) {
    loop {
        if ctl.borrow_and_update().stop {
            return;
        }
        if ctl.changed().await.is_err() {
            return;
        }
    }
}

async fn sleep_interruptible(ms: u64, ctl: &mut watch::Receiver<Ctl>) -> Go {
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_millis(ms)) => Go::Go,
        _ = wait_stop(ctl) => Go::Stopped,
    }
}

enum Outcome {
    Reached,
    Failed(String),
    Stopped,
}

pub async fn run_loop(st: AppState, runner: Arc<Runner>, scenario: Scenario, plan: Plan, mut ctl: watch::Receiver<Ctl>) {
    let run_id = runner.current().run_id;
    // 로봇별: PICK 이 나갔고 짝 DROP 이 아직이다 — 비어 있지 않으면 정지·일시정지를 미룬다.
    let mut holding: std::collections::BTreeSet<u8> = Default::default();
    // 접수까지만 기다린 스텝이 있었다 — 끝에 마지막 Task 완료까지 기다린다.
    let mut prequeued = false;
    // 로봇별 지금 짝의 이송 지시(PICK 전에 열고 DROP 이 도달하면 놓는다).
    let mut pair_order: std::collections::BTreeMap<u8, String> = Default::default();
    tracing::info!(run = %run_id, scenario = %scenario.name, steps = plan.step_count, iterations = ?plan.total_iterations, "scenario run started");
    let mut cur = plan.first();
    let mut error: Option<String> = None;
    let final_phase: Phase = 'outer: loop {
        if holding.is_empty() && wait_resumed(&mut ctl).await == Go::Stopped {
            break Phase::Stopped;
        }
        runner.update(|g| {
            g.iteration = cur.iteration;
            g.step_index = cur.step_index;
            g.current_task_id = None;
            g.note = None;
        });
        let step = &scenario.steps[cur.step_index as usize];
        // 이 스텝이 갈 로봇(설정 id) — 짝·지시는 로봇별이다.
        let rid = st.robot(plan.robot_for(step)).map(|r| r.id).unwrap_or(0);
        // 짝이 걸려 있으면(어느 로봇이든) 정지·일시정지를 미룬 채 보낸다.
        let tail = !holding.is_empty();
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            if step.task_type == gr_proto::TaskType::Pick && !pair_order.contains_key(&rid) {
                match open_pair_order(&st, &scenario, &plan, cur, step, &run_id) {
                    Ok(id) => {
                        pair_order.insert(rid, id);
                    }
                    Err(e) => tracing::warn!(step = cur.step_index, %e, "transfer order: open failed"),
                }
            }
            let order = matches!(step.task_type, gr_proto::TaskType::Pick | gr_proto::TaskType::Drop).then(|| pair_order.get(&rid).cloned()).flatten();
            let (outcome, result) = execute_step(&st, &runner, &scenario, &run_id, step, plan.robot_for(step), cur, attempt, tail, order, &mut ctl).await;
            runner.update(|g| g.push_result(result));
            if !matches!(outcome, Outcome::Reached) && step.task_type == gr_proto::TaskType::Pick {
                // 보내지 못한 PICK 의 지시는 닫는다(보냈다가 실패한 것은 원장이 failed 로 옮긴다). 재시도는 새 지시로.
                close_unsent(&st, pair_order.remove(&rid), "PICK 을 보내지 못함");
            }
            match outcome {
                Outcome::Reached => break,
                Outcome::Stopped => break 'outer Phase::Stopped,
                Outcome::Failed(msg) => match decide_pair(step.task_type, decide(step.on_failure, attempt, MAX_RETRIES)) {
                    Decision::Next => {
                        tracing::warn!(step = cur.step_index, %msg, "step failed — skipped");
                        break;
                    }
                    Decision::Retry => {
                        tracing::warn!(step = cur.step_index, attempt, %msg, "step failed — retrying");
                        if sleep_interruptible(GATE_POLL_MS, &mut ctl).await == Go::Stopped {
                            break 'outer Phase::Stopped;
                        }
                        continue;
                    }
                    Decision::Abort => {
                        let label = if step.label.is_empty() { step.task_type.name().to_string() } else { step.label.clone() };
                        // 다른 로봇의 짝이 걸린 채 멈추면 그 로봇 Hand 에 타이어가 남는다 — 사유에 적는다(동기화 경고로도 뜬다).
                        let open: Vec<String> = holding.iter().filter(|r| **r != rid || step.task_type != gr_proto::TaskType::Drop).map(|r| format!("로봇 {r}")).collect();
                        let open = if open.is_empty() { String::new() } else { format!(" — 짝 DROP 을 보내지 못한 로봇: {} (Hand 확인)", open.join(", ")) };
                        error = Some(format!("스텝 {} ({label}) 실패: {msg}{open}", cur.step_index + 1));
                        break 'outer Phase::Failed;
                    }
                },
            }
        }
        if step.wait_for == WaitFor::Accepted {
            prequeued = true;
        }
        match step.task_type {
            gr_proto::TaskType::Pick => {
                holding.insert(rid);
            }
            gr_proto::TaskType::Drop => {
                holding.remove(&rid);
                pair_order.remove(&rid);
            }
            _ => {}
        }
        if step.wait_after_ms > 0 {
            if !holding.is_empty() {
                tokio::time::sleep(Duration::from_millis(step.wait_after_ms)).await;
            } else if sleep_interruptible(step.wait_after_ms, &mut ctl).await == Go::Stopped {
                break Phase::Stopped;
            }
        }
        match plan.advance(cur) {
            Some(n) => cur = n,
            None => break Phase::Done,
        }
    };
    for (_, id) in std::mem::take(&mut pair_order) {
        close_unsent(&st, Some(id), "실행이 끝남 — 보내지 않은 짝");
    }
    // 미리 넣은 실행은 마지막 Task 가 완료돼야 끝난다.
    let final_phase = if final_phase == Phase::Done && prequeued {
        match drain(&st, &runner, &scenario, &mut ctl).await {
            Drain::Done => Phase::Done,
            Drain::Stopped => Phase::Stopped,
            Drain::Failed(msg) => {
                error = Some(msg);
                Phase::Failed
            }
        }
    } else {
        final_phase
    };
    // 콘솔 종료로 멈춘 실행은 그렇게 남긴다 — "왜 끊겼지" 를 원장에서 바로 알 수 있게.
    let (final_phase, error) = match runner.is_shutting_down() && final_phase != Phase::Done {
        true => (Phase::Stopped, Some(error.unwrap_or_else(|| super::SHUTDOWN_NOTE.to_string()))),
        false => (final_phase, error),
    };
    let snap = runner.update(|g| {
        g.state = final_phase;
        g.ended_at = Some(now_str());
        g.error = error;
        g.current_task_id = None;
        g.note = None;
    });
    if let Err(e) = runner.persist_run(&snap) {
        tracing::error!(%e, "scenario run: persist failed");
    }
    tracing::info!(run = %run_id, phase = final_phase.as_str(), results = snap.results.len(), "scenario run ended");
}

/// 짝의 이송 지시를 연다 — 출발 = PICK 대상, 도착 = 같은 로봇의 다음 스텝(짝 DROP) 대상.
fn open_pair_order(st: &AppState, scenario: &Scenario, plan: &Plan, cur: Cursor, step: &Step, run_id: &str) -> Result<String, ApiError> {
    let r = st.robot(plan.robot_for(step))?;
    let to =
        scenario.steps.iter().skip(cur.step_index as usize + 1).find(|n| plan.robot_for(n) == plan.robot_for(step)).filter(|n| n.task_type == gr_proto::TaskType::Drop).and_then(|n| n.target.clone());
    let n = crate::stock::transfer::NewOrder {
        robot: Some(r.id),
        plc: r.plc.clone(),
        item_code: step.item_code.unwrap_or(0),
        count: step.count,
        from: step.target.clone(),
        to,
        source: format!("scenario:{run_id}"),
        note: format!("{} · 스텝 {}", scenario.name, cur.step_index + 1),
    };
    Ok(st.stock.open_order(n)?.id)
}

/// 아직 `planned` 인(한 번도 보내지 않은) 지시만 `aborted` 로.
fn close_unsent(st: &AppState, id: Option<String>, note: &str) {
    if let Some(id) = id
        && let Err(e) = st.stock.abort_order(&id, note, true)
    {
        tracing::warn!(%id, %e, "transfer order: abort failed");
    }
}

enum Drain {
    Done,
    Stopped,
    Failed(String),
}

/// 모든 스텝을 보낸 뒤 — 이번 실행의 Task 가 전부 완료될 때까지(정지하면 Task 는 PLC 에 남는다).
async fn drain(st: &AppState, runner: &Arc<Runner>, scenario: &Scenario, ctl: &mut watch::Receiver<Ctl>) -> Drain {
    let mut noted = false;
    loop {
        let states = run_states(st, runner, super::MAX_RESULTS);
        if let Some(msg) = earlier_broken(&stops_run(scenario, states.clone())) {
            return Drain::Failed(msg);
        }
        let left = states.iter().filter(|(_, _, _, s)| *s != TaskState::Completed).count();
        if left == 0 {
            return Drain::Done;
        }
        if !noted {
            runner.update(|s| s.note = Some(format!("마지막 Task 완료 대기 ({left} 건)")));
            noted = true;
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(LEDGER_POLL_MS)) => {}
            _ = wait_stop(ctl) => return Drain::Stopped,
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn execute_step(
    st: &AppState,
    runner: &Arc<Runner>,
    scenario: &Scenario,
    run_id: &str,
    step: &Step,
    robot_id: Option<u8>,
    cur: Cursor,
    attempt: u32,
    tail: bool,
    order: Option<String>,
    ctl: &mut watch::Receiver<Ctl>,
) -> (Outcome, StepResult) {
    let mut res = StepResult { iteration: cur.iteration, step_index: cur.step_index, task_id: None, state: TaskState::Failed, ack: None, started_at: now_str(), ended_at: None, error: None, attempt };
    fn fail(mut res: StepResult, msg: String) -> (Outcome, StepResult) {
        res.error = Some(msg.clone());
        res.ended_at = Some(now_str());
        (Outcome::Failed(msg), res)
    }
    fn stopped(mut res: StepResult, msg: &str) -> (Outcome, StepResult) {
        res.error = Some(msg.into());
        res.ended_at = Some(now_str());
        (Outcome::Stopped, res)
    }

    let req = TaskRequest {
        task_type: step.task_type.name().into(),
        target: step.target.clone(),
        item_code: step.item_code,
        count: step.count.clamp(1, 255) as u8,
        params: step.params.clone(),
        position_override: None,
        note: if step.label.trim().is_empty() { step.note.clone() } else { step.label.clone() },
        source: Some(ScenarioSource { scenario_id: scenario.id.clone(), run_id: run_id.to_string(), iteration: cur.iteration, step_index: cur.step_index }),
        robot: robot_id,
        grip_ref: None,
        station_offset: Default::default(),
        ignore_stack_max: false,
        pallet: step.pallet.clone(),
        transfer_order_id: order,
    };
    let robot = match st.robot(robot_id) {
        Ok(r) => r,
        Err(e) => return fail(res, format!("robot: {e}")),
    };
    // 먼저 한 번 작성해 잘못된 스텝은 게이트를 기다리지 않고 바로 실패시킨다(보낼 값은 게이트 뒤에 다시 작성).
    if let Err(e) = crate::issue::compose(st, &req) {
        return fail(res, format!("compose: {e}"));
    }

    // submission gate
    let mut last_note: Option<String> = None;
    loop {
        if let Some(msg) = earlier_broken(&stops_run(scenario, earlier_states(st, runner))) {
            return fail(res, msg);
        }
        let c = *ctl.borrow_and_update();
        // 짝이 걸려 있으면(PICK 이 이미 나갔다) 정지·일시정지를 미룬다.
        if !tail && c.stop {
            return stopped(res, "제출 전 정지됨");
        }
        if !tail && c.paused {
            if wait_resumed(ctl).await == Go::Stopped {
                return stopped(res, "제출 전 정지됨");
            }
            continue;
        }
        let g = crate::ledger::ops::gate(st, robot);
        let depth = queue_depth_reason(robot.ledger.list().iter().map(|e| e.state));
        if g.can_submit && depth.is_none() {
            break;
        }
        let mut reasons = g.reasons;
        reasons.extend(depth);
        let note = format!("제출 대기: {}", reasons.join("; "));
        if last_note.as_deref() != Some(&note) {
            runner.update(|s| s.note = Some(note.clone()));
            last_note = Some(note);
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(GATE_POLL_MS)) => {}
            _ = ctl.changed() => {}
        }
    }
    if last_note.is_some() {
        runner.update(|s| s.note = None);
    }
    // 게이트를 기다리는 동안 앞 Task 가 끝나 재고가 접혔거나 스테이션 트래킹이 바뀌었을 수 있다 — 보내기 직전 값으로.
    let composed = match crate::issue::compose(st, &req) {
        Ok(c) => c,
        Err(e) => return fail(res, format!("compose: {e}")),
    };

    // subscribe before submitting so the first transition cannot be missed
    let mut rx = st.task_events.subscribe();
    let entry = match crate::ledger::ops::create_and_submit(st, robot, Origin::Scenario, Some(req), Some(composed.params), composed.task, composed.pallet, true).await {
        Ok(e) => e,
        Err(e) => return fail(res, format!("submit: {e}")),
    };
    let id = entry.id.clone();
    res.task_id = Some(id.clone());
    runner.update(|s| s.current_task_id = Some(id.clone()));

    let mut state = entry.state;
    let mut ack = entry.ack.clone();
    let mut tick = tokio::time::interval(Duration::from_millis(LEDGER_POLL_MS));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // PICK 을 보낸 뒤에는 정지를 미룬다 — 짝 DROP 까지 보내고 멈춘다.
    // (짝이 걸린 동안 보내는 스텝도 같다 — `tail`.)
    let holds = tail || step.task_type == gr_proto::TaskType::Pick;
    loop {
        if !holds && ctl.borrow().stop {
            res.state = state;
            res.ack = ack;
            return stopped(res, "대기 중 정지됨 (PLC 태스크는 유지)");
        }
        if let Some(ok) = reached(step.wait_for, state) {
            let why = ack.as_ref().filter(|a| !a.accepted && !a.reason.is_empty()).map(|a| format!(" — {}", a.reason)).unwrap_or_default();
            res.state = state;
            res.ack = ack;
            res.ended_at = Some(now_str());
            return if ok {
                (Outcome::Reached, res)
            } else {
                let msg = format!("task {}/{} {}{why}", entry.work_id, entry.task_id, state.as_str());
                res.error = Some(msg.clone());
                (Outcome::Failed(msg), res)
            };
        }
        tokio::select! {
            r = rx.recv() => match r {
                Ok(LedgerEvent::Upsert { task }) if task.id == id => {
                    state = task.state;
                    ack = task.ack;
                }
                Ok(_) => {}
                Err(RecvError::Lagged(_)) | Err(RecvError::Closed) => {
                    if let Some(e) = robot.ledger.get(&id) {
                        state = e.state;
                        ack = e.ack;
                    }
                }
            },
            _ = tick.tick() => {
                if let Some(e) = robot.ledger.get(&id) {
                    state = e.state;
                    ack = e.ack;
                }
            }
            _ = ctl.changed() => {
                if !holds && ctl.borrow().stop {
                    res.state = state;
                    res.ack = ack;
                    return stopped(res, "대기 중 정지됨 (PLC 태스크는 유지)");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scenario(n: usize, repeat: u32) -> Scenario {
        Scenario { steps: (0..n).map(|_| Step::default()).collect(), repeat, ..Default::default() }
    }

    fn walk(plan: Plan, cap: usize) -> Vec<(u32, u32)> {
        let mut out = vec![];
        let mut c = Some(plan.first());
        while let Some(cur) = c {
            out.push((cur.iteration, cur.step_index));
            if out.len() >= cap {
                break;
            }
            c = plan.advance(cur);
        }
        out
    }

    #[test]
    fn queue_depth_is_running_plus_one() {
        use TaskState::*;
        assert_eq!(queue_depth_reason([Running, Completed, Canceled, Lost, Draft]), None, "running + terminal: next may go");
        assert!(queue_depth_reason([Running, Queued]).unwrap().contains("1 건"), "running + next already queued");
        assert!(queue_depth_reason([Queued]).is_some(), "nothing running but one queued");
        assert!(queue_depth_reason([Submitted]).is_some() && queue_depth_reason([Accepted]).is_some());
        assert_eq!(queue_depth_reason([]), None);
    }

    #[test]
    fn external_cancel_stops_pairs_and_honours_skip_for_others() {
        use gr_proto::TaskType::*;
        let st = |t, f| Step { task_type: t, on_failure: f, ..Default::default() };
        assert!(breaks_run(&st(Pick, OnFailure::Skip)) && breaks_run(&st(Drop, OnFailure::Skip)));
        assert!(breaks_run(&st(Move, OnFailure::Stop)));
        assert!(!breaks_run(&st(Move, OnFailure::Skip)) && !breaks_run(&st(Measure, OnFailure::Skip)));
    }

    #[test]
    fn pair_steps_never_skip() {
        use gr_proto::TaskType::*;
        assert_eq!(decide_pair(Pick, Decision::Next), Decision::Abort);
        assert_eq!(decide_pair(Drop, Decision::Next), Decision::Abort);
        assert_eq!(decide_pair(Drop, Decision::Retry), Decision::Retry);
        assert_eq!(decide_pair(Move, Decision::Next), Decision::Next);
    }

    #[test]
    fn earlier_broken_stops_on_bad_end_only() {
        use TaskState::*;
        assert_eq!(earlier_broken(&[]), None);
        assert_eq!(earlier_broken(&[(0, 1, 1, Completed), (1, 1, 2, Running), (2, 1, 3, Queued), (3, 1, 4, Accepted)]), None);
        for bad in [Failed, Canceled, Rejected, Lost] {
            let m = earlier_broken(&[(0, 1, 1, Completed), (1, 260921, 7, bad)]).unwrap();
            assert!(m.contains("앞 스텝 2") && m.contains("260921/7") && m.contains(bad.as_str()), "{m}");
        }
    }

    #[test]
    fn plan_walks_iterations_and_steps() {
        let p = Plan::new(&scenario(3, 2), &RunOptions::default()).unwrap();
        assert_eq!(p.total_iterations, Some(2));
        assert_eq!(walk(p, 100), vec![(1, 0), (1, 1), (1, 2), (2, 0), (2, 1), (2, 2)]);
    }

    #[test]
    fn plan_single_iteration_single_step() {
        let p = Plan::new(&scenario(1, 1), &RunOptions::default()).unwrap();
        assert_eq!(walk(p, 100), vec![(1, 0)]);
    }

    #[test]
    fn plan_infinite_never_ends() {
        let p = Plan::new(&scenario(2, 0), &RunOptions::default()).unwrap();
        assert_eq!(p.total_iterations, None);
        let w = walk(p, 7);
        assert_eq!(w, vec![(1, 0), (1, 1), (2, 0), (2, 1), (3, 0), (3, 1), (4, 0)]);
        assert!(p.advance(Cursor { iteration: 1_000_000, step_index: 1 }).is_some());
    }

    #[test]
    fn plan_start_step_only_affects_first_iteration() {
        let p = Plan::new(&scenario(3, 2), &RunOptions { repeat: None, start_step: Some(2), ..Default::default() }).unwrap();
        assert_eq!(walk(p, 100), vec![(1, 2), (2, 0), (2, 1), (2, 2)]);
    }

    #[test]
    fn plan_repeat_override_and_errors() {
        let p = Plan::new(&scenario(2, 5), &RunOptions { repeat: Some(1), start_step: None, ..Default::default() }).unwrap();
        assert_eq!(walk(p, 100), vec![(1, 0), (1, 1)]);
        let p = Plan::new(&scenario(2, 5), &RunOptions { repeat: Some(0), start_step: None, ..Default::default() }).unwrap();
        assert_eq!(p.total_iterations, None);
        assert!(Plan::new(&scenario(0, 1), &RunOptions::default()).is_err());
        assert!(Plan::new(&scenario(2, 1), &RunOptions { repeat: None, start_step: Some(2), ..Default::default() }).is_err());
    }

    #[test]
    fn step_robot_overrides_run_robot() {
        let mut s = scenario(2, 1);
        s.steps[1].robot = Some(1);
        let p = Plan::new(&s, &RunOptions { robot: Some(2), ..Default::default() }).unwrap();
        assert_eq!(p.robot_for(&s.steps[0]), Some(2));
        assert_eq!(p.robot_for(&s.steps[1]), Some(1));
        let p = Plan::new(&s, &RunOptions::default()).unwrap();
        assert_eq!(p.robot_for(&s.steps[0]), None);
    }

    #[test]
    fn on_failure_policies() {
        assert_eq!(decide(OnFailure::Stop, 1, 3), Decision::Abort);
        assert_eq!(decide(OnFailure::Skip, 1, 3), Decision::Next);
        assert_eq!(decide(OnFailure::Retry, 1, 3), Decision::Retry);
        assert_eq!(decide(OnFailure::Retry, 3, 3), Decision::Retry);
        assert_eq!(decide(OnFailure::Retry, 4, 3), Decision::Abort);
        assert_eq!(decide(OnFailure::Retry, 1, 0), Decision::Abort);
    }

    #[test]
    fn wait_for_semantics() {
        use TaskState::*;
        assert_eq!(reached(WaitFor::Accepted, Submitted), None);
        assert_eq!(reached(WaitFor::Accepted, Accepted), Some(true));
        assert_eq!(reached(WaitFor::Accepted, Queued), Some(true));
        assert_eq!(reached(WaitFor::Accepted, Completed), Some(true));
        assert_eq!(reached(WaitFor::Completed, Accepted), None);
        assert_eq!(reached(WaitFor::Completed, Running), None);
        assert_eq!(reached(WaitFor::Completed, Completed), Some(true));
        for s in [Rejected, Failed, Canceled, Lost] {
            assert_eq!(reached(WaitFor::Accepted, s), Some(false));
            assert_eq!(reached(WaitFor::Completed, s), Some(false));
        }
    }
}
