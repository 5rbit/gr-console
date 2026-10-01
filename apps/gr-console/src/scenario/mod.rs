//! 저장된 시나리오 문서와 지난 실행 기록(읽기 전용). **실행기는 은퇴했다**(2026-10-01) — PLC 로 보내는 곳은 작업
//! 대기열(`jobs`) 하나다. 시나리오는 수동작업 목록으로 가져와(`jobs::templates`) 불러온다. 순서 있는 작업은 수동작업,
//! 조건으로 도는 작업은 규칙 · 규칙 세트(`taskgen`)가 맡는다. JSON/CSV 입출력 · 검사는 `io.rs`, HTTP 는 `routes.rs`.

pub mod io;
pub mod routes;

use std::sync::{Arc, Mutex, PoisonError};

use gr_proto::TaskType;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use tokio::sync::broadcast;

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
    /// 사람이 지운 예정 스텝 `(회차, 스텝)` — 실행기는 차례가 와도 보내지 않는다.
    pub skipped: Vec<(u32, u32)>,
}

impl RunState {
    pub fn idle() -> RunState {
        RunState { state: Phase::Idle, ..Default::default() }
    }
}

/// 시나리오 저장소(이름은 옛 그대로). 실행 상태는 늘 쉼 — 실행기는 은퇴했다.
pub struct Runner {
    db: Db,
    pub run: Mutex<RunState>,
    pub events: broadcast::Sender<RunState>,
}

impl Runner {
    pub fn new(db: Db) -> Arc<Runner> {
        let (tx, _) = broadcast::channel(64);
        // Runs left open by a previous process can never finish — mark them lost.
        let _ = db.with(|c| c.execute("UPDATE scenario_runs SET status = 'lost', ended_at = COALESCE(ended_at, ?1) WHERE status IN ('running','paused','stopping')", [now_str()]));
        Arc::new(Runner { db, run: Mutex::new(RunState::idle()), events: tx })
    }

    pub fn current(&self) -> RunState {
        self.run.lock().unwrap_or_else(PoisonError::into_inner).clone()
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

    pub fn runs(&self, limit: usize) -> Result<Vec<RunState>, ApiError> {
        let rows: Vec<String> = self.db.with(|c| {
            let mut st = c.prepare("SELECT doc_json FROM scenario_runs ORDER BY started_at DESC LIMIT ?1")?;
            let it = st.query_map([limit as i64], |r| r.get::<_, String>(0))?;
            it.collect()
        })?;
        Ok(rows.into_iter().filter_map(|s| serde_json::from_str(&s).ok()).collect())
    }

    /// 콘솔 종료 — 멈출 실행이 없다(실행기 은퇴). 종료 절차의 자리를 지킨다.
    pub fn stop_for_shutdown(&self) -> bool {
        false
    }

    pub async fn wait_finished(&self, _timeout: std::time::Duration) -> bool {
        true
    }
}
