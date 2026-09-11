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
}

impl Runner {
    pub fn new(db: Db) -> Arc<Runner> {
        let (tx, _) = broadcast::channel(64);
        // Runs left open by a previous process can never finish — mark them lost.
        let _ = db.with(|c| c.execute("UPDATE scenario_runs SET status = 'lost', ended_at = COALESCE(ended_at, ?1) WHERE status IN ('running','paused','stopping')", [now_str()]));
        Arc::new(Runner { db, run: Mutex::new(RunState::idle()), events: tx, active: Mutex::new(None) })
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
        let mut active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(a) = active.as_ref() {
            if !a.join.is_finished() && self.current().state.is_active() {
                return Err(ApiError::Conflict(format!("scenario run {} is {}", self.current().run_id, self.current().state.as_str())));
            }
            // finished but not yet reaped
            active.take();
        }
        let plan = runner::Plan::new(&scenario, &opts)?;
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
