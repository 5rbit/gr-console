//! Scenario run loop. The planning parts (`Plan`, `Cursor`, `decide`, `reached`) are pure and unit
//! tested; `run_loop` glues them to the issue/ledger slices.
//!
//! Per step: compose → wait for the submission gate (250 ms poll, honours pause/stop) →
//! `create_and_submit` → follow the ledger entry (broadcast events, 500 ms poll fallback) until the
//! step's `wait_for` state is reached or the task ends badly → apply `on_failure` → `wait_after_ms`.

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
    tracing::info!(run = %run_id, scenario = %scenario.name, steps = plan.step_count, iterations = ?plan.total_iterations, "scenario run started");
    let mut cur = plan.first();
    let mut error: Option<String> = None;
    let final_phase: Phase = 'outer: loop {
        if wait_resumed(&mut ctl).await == Go::Stopped {
            break Phase::Stopped;
        }
        runner.update(|g| {
            g.iteration = cur.iteration;
            g.step_index = cur.step_index;
            g.current_task_id = None;
            g.note = None;
        });
        let step = &scenario.steps[cur.step_index as usize];
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let (outcome, result) = execute_step(&st, &runner, &scenario, &run_id, step, plan.robot_for(step), cur, attempt, &mut ctl).await;
            runner.update(|g| g.push_result(result));
            match outcome {
                Outcome::Reached => break,
                Outcome::Stopped => break 'outer Phase::Stopped,
                Outcome::Failed(msg) => match decide(step.on_failure, attempt, MAX_RETRIES) {
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
                        error = Some(format!("스텝 {} ({label}) 실패: {msg}", cur.step_index + 1));
                        break 'outer Phase::Failed;
                    }
                },
            }
        }
        if step.wait_after_ms > 0 && sleep_interruptible(step.wait_after_ms, &mut ctl).await == Go::Stopped {
            break Phase::Stopped;
        }
        match plan.advance(cur) {
            Some(n) => cur = n,
            None => break Phase::Done,
        }
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

#[allow(clippy::too_many_arguments)]
/// 다음 스텝(같은 회차)이 같은 로봇·같은 스테이션 그룹의 스테이션 PICK/DROP 이면 Multi-Picking(`Some(true)`).
/// 로봇이 스텝마다 다르면 묶지 않는다 — 다음 작업을 이어받는 쪽이 같은 로봇이어야 부분 리프트가 뜻이 있다.
fn multi_pick_auto(scenario: &Scenario, idx: usize, robot_id: Option<u8>) -> Option<bool> {
    let (a, b) = (scenario.steps.get(idx)?, scenario.steps.get(idx + 1)?);
    if b.robot.is_some() && b.robot != robot_id {
        return None;
    }
    crate::issue::same_station_group(a.target.as_ref(), a.task_type, b.target.as_ref(), b.task_type).then_some(true)
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
        multi_pick: multi_pick_auto(scenario, cur.step_index as usize, robot_id),
    };
    let robot = match st.robot(robot_id) {
        Ok(r) => r,
        Err(e) => return fail(res, format!("robot: {e}")),
    };
    let composed = match crate::issue::compose(st, &req) {
        Ok(c) => c,
        Err(e) => return fail(res, format!("compose: {e}")),
    };

    // submission gate
    let mut last_note: Option<String> = None;
    loop {
        let c = *ctl.borrow_and_update();
        if c.stop {
            return stopped(res, "제출 전 정지됨");
        }
        if c.paused {
            if wait_resumed(ctl).await == Go::Stopped {
                return stopped(res, "제출 전 정지됨");
            }
            continue;
        }
        let g = crate::ledger::ops::gate(st, robot);
        if g.can_submit {
            break;
        }
        let note = format!("제출 대기: {}", g.reasons.join("; "));
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
    loop {
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
                if ctl.borrow().stop {
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
