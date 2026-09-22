//! Scenario manager (M3-C slice): typed scenario documents, sqlite storage, run history and the
//! single-active-run `Runner`. The run loop itself lives in `runner.rs`, JSON/CSV I/O and
//! validation in `io.rs`, HTTP in `routes.rs`.
//!
//! `Runner::new(db)` is created before `AppState` exists (main.rs), so the run loop is spawned
//! lazily from the `POST /api/scenarios/{id}/run` handler with a clone of `AppState`.

pub mod io;
pub mod routes;
pub mod runner;

use std::sync::{Arc, Mutex, PoisonError};

use gr_proto::TaskType;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use tokio::sync::{broadcast, watch};

use crate::db::Db;
use crate::error::ApiError;
use crate::ledger::{Target, TaskAck, TaskState};
use crate::util::now_str;

// ---------------------------------------------------------------------------------------------
// Scenario document
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WaitFor {
    Accepted,
    #[default]
    Completed,
}

impl WaitFor {
    pub fn as_str(self) -> &'static str {
        match self {
            WaitFor::Accepted => "accepted",
            WaitFor::Completed => "completed",
        }
    }
    pub fn parse(s: &str) -> Option<WaitFor> {
        match s.trim().to_ascii_lowercase().as_str() {
            "accepted" => Some(WaitFor::Accepted),
            "completed" | "" => Some(WaitFor::Completed),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnFailure {
    #[default]
    Stop,
    Skip,
    Retry,
}

impl OnFailure {
    pub fn as_str(self) -> &'static str {
        match self {
            OnFailure::Stop => "stop",
            OnFailure::Skip => "skip",
            OnFailure::Retry => "retry",
        }
    }
    pub fn parse(s: &str) -> Option<OnFailure> {
        match s.trim().to_ascii_lowercase().as_str() {
            "stop" | "" => Some(OnFailure::Stop),
            "skip" => Some(OnFailure::Skip),
            "retry" => Some(OnFailure::Retry),
            _ => None,
        }
    }
}

/// One scenario step (matches `ScenarioStep` in types.ts). Unknown JSON fields are tolerated and
/// missing ones take defaults so older documents still load.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Step {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub task_type: TaskType,
    pub target: Option<Target>,
    pub item_code: Option<u32>,
    pub count: u32,
    /// Partial `TaskParams` (snake_case keys) overriding the defaults profile.
    pub params: Json,
    pub wait_for: WaitFor,
    pub wait_after_ms: u64,
    pub on_failure: OnFailure,
    pub note: String,
    /// Robot to send this step to ( = default robot).
    pub robot: Option<u8>,
    /// 팔렛 슬롯(`TaskRequest.pallet` 그대로) — 팔렛 패턴 화면의 "시나리오로 내보내기"가 싣는다. CSV 에는 없다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pallet: Option<crate::pallet::compose::PalletRef>,
}

impl Default for Step {
    fn default() -> Self {
        Self {
            id: String::new(),
            label: String::new(),
            task_type: TaskType::Pick,
            target: None,
            item_code: None,
            count: 1,
            params: Json::Object(Default::default()),
            wait_for: WaitFor::Completed,
            wait_after_ms: 0,
            on_failure: OnFailure::Stop,
            note: String::new(),
            robot: None,
            pallet: None,
        }
    }
}

impl Step {
    /// Ensures a stable id and an object-shaped `params`.
    pub fn normalize(&mut self) {
        if self.id.trim().is_empty() {
            self.id = uuid::Uuid::new_v4().to_string();
        }
        if !self.params.is_object() {
            self.params = Json::Object(Default::default());
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Scenario {
    pub id: String,
    pub name: String,
    pub description: String,
    pub steps: Vec<Step>,
    /// 0 = infinite.
    pub repeat: u32,
    pub created_at: String,
    pub updated_at: String,
}

impl Default for Scenario {
    fn default() -> Self {
        Self { id: String::new(), name: String::new(), description: String::new(), steps: vec![], repeat: 1, created_at: now_str(), updated_at: now_str() }
    }
}

impl Scenario {
    pub fn normalize(&mut self) {
        for s in &mut self.steps {
            s.normalize();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Run state
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    #[default]
    Idle,
    Running,
    Paused,
    Stopping,
    Stopped,
    Done,
    Failed,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Running => "running",
            Phase::Paused => "paused",
            Phase::Stopping => "stopping",
            Phase::Stopped => "stopped",
            Phase::Done => "done",
            Phase::Failed => "failed",
        }
    }
    /// A run in this phase still owns the runner (no second run may start).
    pub fn is_active(self) -> bool {
        matches!(self, Phase::Running | Phase::Paused | Phase::Stopping)
    }
}

/// Outcome of one step attempt (matches `StepResult` in types.ts; `error`/`attempt` are extra).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StepResult {
    pub iteration: u32,
    pub step_index: u32,
    #[serde(default)]
    pub task_id: Option<String>,
    /// Ledger state the step ended in (`failed` when no task could be composed/submitted).
    pub state: TaskState,
    #[serde(default)]
    pub ack: Option<TaskAck>,
    pub started_at: String,
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub attempt: u32,
}

/// Live state of the (single) run, broadcast on every change and persisted at the end.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RunState {
    pub run_id: String,
    pub scenario_id: String,
    pub scenario_name: String,
    pub state: Phase,
    /// 1-based while running (0 before the first step).
    pub iteration: u32,
    /// `None` = infinite.
    pub total_iterations: Option<u32>,
    /// 0-based index of the current step.
    pub step_index: u32,
    pub step_count: u32,
    /// Run robot (`RunOptions.robot`) — steps without their own robot go here; `None` = default robot.
    pub robot: Option<u8>,
    /// Display name of the run robot (resolved at start).
    pub robot_name: Option<String>,
    pub current_task_id: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub error: Option<String>,
    /// Transient status line (e.g. why the submission gate is closed). Extra to types.ts.
    pub note: Option<String>,
    /// Last `MAX_RESULTS` step results (oldest first).
    pub results: Vec<StepResult>,
}

pub const MAX_RESULTS: usize = 200;

impl RunState {
    pub fn idle() -> RunState {
        RunState { state: Phase::Idle, ..Default::default() }
    }
    pub fn push_result(&mut self, r: StepResult) {
        self.results.push(r);
        if self.results.len() > MAX_RESULTS {
            let drop = self.results.len() - MAX_RESULTS;
            self.results.drain(..drop);
        }
    }
}

/// Control word shared between the HTTP handlers and the run loop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ctl {
    pub paused: bool,
    pub stop: bool,
}

struct Active {
    ctl: watch::Sender<Ctl>,
    join: tokio::task::JoinHandle<()>,
}

pub struct Runner {
    db: Db,
    pub run: Mutex<RunState>,
    pub events: broadcast::Sender<RunState>,
    active: Mutex<Option<Active>>,
    /// 콘솔 종료로 멈춘 실행인가 — 원장에 그렇게 남기고, 새 실행은 받지 않는다.
    shutting_down: std::sync::atomic::AtomicBool,
}

/// 종료로 멈춘 실행에 남기는 사유.
pub const SHUTDOWN_NOTE: &str = "콘솔 종료로 정지됨 — PLC 에 이미 보낸 Task 는 그대로 둡니다";

impl Runner {
    pub fn new(db: Db) -> Arc<Runner> {
        let (tx, _) = broadcast::channel(64);
        // Runs left open by a previous process can never finish — mark them lost.
        let _ = db.with(|c| c.execute("UPDATE scenario_runs SET status = 'lost', ended_at = COALESCE(ended_at, ?1) WHERE status IN ('running','paused','stopping')", [now_str()]));
        Arc::new(Runner { db, run: Mutex::new(RunState::idle()), events: tx, active: Mutex::new(None), shutting_down: std::sync::atomic::AtomicBool::new(false) })
    }

    pub fn current(&self) -> RunState {
        self.run.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Mutates the run state under the lock, then broadcasts the new snapshot.
    pub fn update(&self, f: impl FnOnce(&mut RunState)) -> RunState {
        let snap = {
            let mut g = self.run.lock().unwrap_or_else(PoisonError::into_inner);
            f(&mut g);
            g.clone()
        };
        let _ = self.events.send(snap.clone());
        snap
    }

    // ---- scenarios

    pub fn list(&self) -> Result<Vec<Scenario>, ApiError> {
        let rows: Vec<String> = self.db.with(|c| {
            let mut st = c.prepare("SELECT doc_json FROM scenarios ORDER BY updated_at DESC")?;
            let it = st.query_map([], |r| r.get::<_, String>(0))?;
            it.collect()
        })?;
        Ok(rows.into_iter().filter_map(|s| serde_json::from_str(&s).ok()).collect())
    }

    pub fn get(&self, id: &str) -> Result<Option<Scenario>, ApiError> {
        let row = self.db.with(|c| {
            c.query_row("SELECT doc_json FROM scenarios WHERE id = ?1", [id], |r| r.get::<_, String>(0))
                .map(Some)
                .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })
        })?;
        Ok(row.and_then(|s| serde_json::from_str(&s).ok()))
    }

    pub fn save(&self, mut s: Scenario) -> Result<Scenario, ApiError> {
        if s.name.trim().is_empty() {
            return Err(ApiError::BadRequest("scenario name is required".into()));
        }
        if s.id.is_empty() {
            s.id = uuid::Uuid::new_v4().to_string();
            s.created_at = now_str();
        }
        s.normalize();
        // 파라미터 검사(오타·종류·범위)를 저장 때 — 예전에는 실행해야 드러났다. 등록 여부(셀·품목)는
        // 레지스트리가 바뀔 수 있어 저장을 막지 않는다(검증 버튼·실행 전 검사가 본다).
        let shape = io::validate_shape(&s);
        // 막는 것은 파라미터 문제뿐 — 대상·품목 미입력 같은 것은 편집 중 저장을 막지 않는다.
        let errors: Vec<String> = shape.iter().filter(|x| x.field == "params").map(|x| format!("스텝 {} {}: {}", x.step_index.map(|i| i + 1).unwrap_or(0), x.field, x.message)).collect();
        if !errors.is_empty() {
            return Err(ApiError::BadRequest(format!("시나리오 검사 실패 — {}", errors.join(" · "))));
        }
        s.updated_at = now_str();
        let doc = serde_json::to_string(&s)?;
        self.db.with(|c| {
            c.execute(
                "INSERT INTO scenarios (id, name, doc_json, created_at, updated_at) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET name=excluded.name, doc_json=excluded.doc_json, updated_at=excluded.updated_at",
                (&s.id, &s.name, &doc, &s.created_at, &s.updated_at),
            )
        })?;
        Ok(s)
    }

    pub fn delete(&self, id: &str) -> Result<bool, ApiError> {
        Ok(self.db.with(|c| c.execute("DELETE FROM scenarios WHERE id = ?1", [id]))? > 0)
    }

    // ---- run history

    pub(crate) fn persist_run(&self, rs: &RunState) -> Result<(), ApiError> {
        let doc = serde_json::to_string(rs)?;
        self.db.with(|c| {
            c.execute(
                "INSERT INTO scenario_runs (id, scenario_id, started_at, ended_at, status, doc_json) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET ended_at=excluded.ended_at, status=excluded.status, doc_json=excluded.doc_json",
                (&rs.run_id, &rs.scenario_id, &rs.started_at, &rs.ended_at, rs.state.as_str(), &doc),
            )
        })?;
        Ok(())
    }

    pub fn runs(&self, limit: usize) -> Result<Vec<RunState>, ApiError> {
        let rows: Vec<String> = self.db.with(|c| {
            let mut st = c.prepare("SELECT doc_json FROM scenario_runs ORDER BY started_at DESC LIMIT ?1")?;
            let it = st.query_map([limit as i64], |r| r.get::<_, String>(0))?;
            it.collect()
        })?;
        Ok(rows.into_iter().filter_map(|s| serde_json::from_str(&s).ok()).collect())
    }

    // ---- run control (the loop is in runner.rs)

    /// Starts a run. Fails with `Conflict` while another run is active.
    pub fn start(self: &Arc<Self>, st: crate::state::AppState, scenario: Scenario, opts: runner::RunOptions) -> Result<RunState, ApiError> {
        if scenario.steps.is_empty() {
            return Err(ApiError::BadRequest("scenario has no steps".into()));
        }
        if self.is_shutting_down() {
            return Err(ApiError::ShuttingDown("콘솔 종료 중 — 새 시나리오 실행을 시작하지 않습니다".into()));
        }
        let mut active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(a) = active.as_ref() {
            if !a.join.is_finished() && self.current().state.is_active() {
                return Err(ApiError::Conflict(format!("scenario run {} is {}", self.current().run_id, self.current().state.as_str())));
            }
            // finished but not yet reaped
            active.take();
        }
        let plan = runner::Plan::new(&scenario, &opts)?;
        // 없는 로봇으로 달리면 첫 스텝에서야 실패한다 — 시작 전에 막는다.
        // 로봇을 안 든 스텝이 가는 곳 — 둘 이상이면 반드시 받는다(빠지면 첫 로봇으로 가던 계획 카드 "저장 후 실행").
        let run_robot = st.robot_required(plan.robot, "시나리오 실행")?;
        for (i, s) in scenario.steps.iter().enumerate() {
            if let Some(id) = s.robot
                && st.robot(Some(id)).is_err()
            {
                return Err(ApiError::BadRequest(format!("스텝 {}: 로봇 {id} 이(가) 설정에 없음", i + 1)));
            }
        }
        let started = now_str();
        let rs = RunState {
            run_id: uuid::Uuid::new_v4().to_string(),
            scenario_id: scenario.id.clone(),
            scenario_name: scenario.name.clone(),
            state: Phase::Running,
            iteration: 0,
            total_iterations: plan.total_iterations,
            step_index: plan.start_step,
            step_count: plan.step_count,
            robot: Some(run_robot.id),
            robot_name: Some(run_robot.name.clone()),
            current_task_id: None,
            started_at: started,
            ended_at: None,
            error: None,
            note: None,
            results: vec![],
        };
        let snap = self.update(|g| *g = rs);
        let _ = self.persist_run(&snap);
        let (ctl_tx, ctl_rx) = watch::channel(Ctl::default());
        let me = self.clone();
        let join = tokio::spawn(async move { runner::run_loop(st, me, scenario, plan, ctl_rx).await });
        *active = Some(Active { ctl: ctl_tx, join });
        Ok(snap)
    }

    fn with_ctl(&self, f: impl FnOnce(&mut Ctl)) -> bool {
        let active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        match active.as_ref() {
            Some(a) => {
                a.ctl.send_modify(f);
                true
            }
            None => false,
        }
    }

    pub fn pause(&self) -> Result<RunState, ApiError> {
        let cur = self.current();
        if cur.state != Phase::Running {
            return Err(ApiError::Conflict(format!("run is {}", cur.state.as_str())));
        }
        self.with_ctl(|c| c.paused = true);
        Ok(self.update(|g| g.state = Phase::Paused))
    }

    pub fn resume(&self) -> Result<RunState, ApiError> {
        let cur = self.current();
        if cur.state != Phase::Paused {
            return Err(ApiError::Conflict(format!("run is {}", cur.state.as_str())));
        }
        self.with_ctl(|c| c.paused = false);
        Ok(self.update(|g| g.state = Phase::Running))
    }

    /// 콘솔 종료 — 다음 스텝을 내지 않게 한다. 이미 PLC 에 보낸 Task 는 취소하지 않는다.
    /// 돌고 있던 실행을 멈춘 경우에만 true.
    pub fn stop_for_shutdown(&self) -> bool {
        self.shutting_down.store(true, std::sync::atomic::Ordering::SeqCst);
        if !matches!(self.current().state, Phase::Running | Phase::Paused) {
            return false;
        }
        self.with_ctl(|c| {
            c.stop = true;
            c.paused = false;
        });
        self.update(|g| {
            g.state = Phase::Stopping;
            g.note = Some(SHUTDOWN_NOTE.into());
        });
        true
    }

    pub(crate) fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// 실행 루프가 끝날 때까지(원장 기록까지) 기다린다. 한도 안에 끝났으면 true.
    pub async fn wait_finished(&self, timeout: std::time::Duration) -> bool {
        let t0 = std::time::Instant::now();
        loop {
            let done = {
                let g = self.active.lock().unwrap_or_else(PoisonError::into_inner);
                g.as_ref().is_none_or(|a| a.join.is_finished())
            };
            if done {
                return true;
            }
            if t0.elapsed() >= timeout {
                return false;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    pub fn stop(&self) -> Result<RunState, ApiError> {
        let cur = self.current();
        if !matches!(cur.state, Phase::Running | Phase::Paused) {
            return Err(ApiError::Conflict(format!("run is {}", cur.state.as_str())));
        }
        self.with_ctl(|c| {
            c.stop = true;
            c.paused = false;
        });
        Ok(self.update(|g| g.state = Phase::Stopping))
    }
}
