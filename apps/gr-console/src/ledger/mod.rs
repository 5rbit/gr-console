//! Task ledger: console-side record of every task (ours or external), persisted in sqlite, with a
//! broadcast of changes. M1 provides the types, CRUD, id allocation and `submit()`; the sync engine
//! (`sync.rs`), operations (`ops.rs`) and routes (`routes.rs`) are the M3-B slice.

pub mod ops;
pub mod routes;
pub mod sync;

use std::sync::{Arc, Mutex, PoisonError};

use gr_proto::{Header, RejectInfo, TaskData, TaskKey, TaskParams};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use tokio::sync::broadcast;

use crate::db::Db;
use crate::error::ApiError;
use crate::util::now_str;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    Draft,
    Submitted,
    Accepted,
    Rejected,
    Queued,
    Running,
    Completed,
    Canceled,
    Failed,
    Lost,
}

impl TaskState {
    pub fn is_terminal(self) -> bool {
        matches!(self, TaskState::Rejected | TaskState::Completed | TaskState::Canceled | TaskState::Failed)
    }
    pub fn is_active(self) -> bool {
        !self.is_terminal() && self != TaskState::Draft
    }
    pub fn as_str(self) -> &'static str {
        match self {
            TaskState::Draft => "draft",
            TaskState::Submitted => "submitted",
            TaskState::Accepted => "accepted",
            TaskState::Rejected => "rejected",
            TaskState::Queued => "queued",
            TaskState::Running => "running",
            TaskState::Completed => "completed",
            TaskState::Canceled => "canceled",
            TaskState::Failed => "failed",
            TaskState::Lost => "lost",
        }
    }
    pub fn parse(s: &str) -> Option<TaskState> {
        Some(match s {
            "draft" => TaskState::Draft,
            "submitted" => TaskState::Submitted,
            "accepted" => TaskState::Accepted,
            "rejected" => TaskState::Rejected,
            "queued" => TaskState::Queued,
            "running" => TaskState::Running,
            "completed" => TaskState::Completed,
            "canceled" => TaskState::Canceled,
            "failed" => TaskState::Failed,
            "lost" => TaskState::Lost,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Console,
    Scenario,
    External,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Actor {
    Ui,
    Plc,
    Scenario,
    System,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transition {
    pub from: Option<TaskState>,
    pub to: TaskState,
    pub at: String,
    pub by: Actor,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TaskAck {
    pub accepted: bool,
    pub code: u16,
    pub reason: String,
    pub reject_bits: u8,
    pub at: String,
}

impl TaskAck {
    pub fn from_reject(info: &RejectInfo, data0: u8) -> TaskAck {
        TaskAck { accepted: info.accepted(), code: info.validation, reason: info.reason(), reject_bits: data0, at: now_str() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScenarioSource {
    pub scenario_id: String,
    pub run_id: String,
    pub iteration: u32,
    pub step_index: u32,
}

/// What the UI asked for (console-owned, snake_case).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskRequest {
    #[serde(rename = "type")]
    pub task_type: String,
    pub target: Option<Target>,
    pub item_code: Option<u32>,
    pub count: u8,
    pub params: Json,
    pub position_override: Option<[f32; 4]>,
    pub note: String,
    pub source: Option<ScenarioSource>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Target {
    pub kind: String,
    pub id: u16,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PlcSeen {
    pub step: u16,
    pub queue_index: Option<usize>,
    pub last_seen_at: String,
}

/// Ledger entry as served to the frontend (`Task` in types.ts).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub id: String,
    pub seq: i64,
    pub work_id: u32,
    pub task_id: u32,
    pub origin: Origin,
    pub plc_name: String,
    pub request: Option<TaskRequest>,
    pub resolved: Option<TaskParams>,
    pub position: [f32; 4],
    pub plc_task: TaskData,
    pub state: TaskState,
    pub state_at: String,
    pub created_at: String,
    pub submitted_at: Option<String>,
    pub ended_at: Option<String>,
    pub ack: Option<TaskAck>,
    pub header: Option<Header>,
    pub plc: Option<PlcSeen>,
    pub error: Option<String>,
    pub history: Vec<Transition>,
}

impl LedgerEntry {
    pub fn key(&self) -> TaskKey {
        TaskKey { work_id: self.work_id, task_id: self.task_id }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LedgerEvent {
    Snapshot { tasks: Vec<LedgerEntry> },
    Upsert { task: LedgerEntry },
    Remove { id: String },
}

pub struct Ledger {
    db: Db,
    plc: String,
    cache: Mutex<Vec<LedgerEntry>>,
    pub events: broadcast::Sender<LedgerEvent>,
}

impl Ledger {
    pub fn new(db: Db, plc: &str) -> anyhow::Result<Arc<Ledger>> {
        let (tx, _) = broadcast::channel(512);
        let l = Ledger { db, plc: plc.to_string(), cache: Mutex::new(Vec::new()), events: tx };
        l.reload()?;
        Ok(Arc::new(l))
    }

    fn reload(&self) -> anyhow::Result<()> {
        let rows: Vec<String> = self.db.with(|c| {
            let mut st = c.prepare("SELECT doc_json FROM tasks WHERE state NOT IN ('completed','canceled','rejected','failed') OR updated_at > datetime('now','-1 day') ORDER BY seq")?;
            let rows = st.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()
        })?;
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        cache.clear();
        for r in rows {
            if let Ok(e) = serde_json::from_str::<LedgerEntry>(&r) {
                cache.push(e);
            }
        }
        Ok(())
    }

    pub fn plc_name(&self) -> &str {
        &self.plc
    }

    /// Live entries (active + recent terminal), oldest first.
    pub fn list(&self) -> Vec<LedgerEntry> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn get(&self, id: &str) -> Option<LedgerEntry> {
        if let Some(e) = self.cache.lock().unwrap_or_else(PoisonError::into_inner).iter().find(|e| e.id == id) {
            return Some(e.clone());
        }
        self.db
            .with(|c| c.query_row("SELECT doc_json FROM tasks WHERE id = ?1", [id], |r| r.get::<_, String>(0)))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
    }

    pub fn find_by_key(&self, key: TaskKey) -> Option<LedgerEntry> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner).iter().rev().find(|e| e.key() == key).cloned()
    }

    /// Persists + caches + broadcasts an entry.
    pub fn upsert(&self, entry: LedgerEntry) -> Result<LedgerEntry, ApiError> {
        let doc = serde_json::to_string(&entry)?;
        self.db.with(|c| {
            c.execute(
                "INSERT INTO tasks (id, seq, work_id, task_id, origin, plc, state, doc_json, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
                 ON CONFLICT(id) DO UPDATE SET state = excluded.state, doc_json = excluded.doc_json, updated_at = excluded.updated_at, work_id = excluded.work_id, task_id = excluded.task_id",
                (
                    &entry.id,
                    entry.seq,
                    entry.work_id,
                    entry.task_id,
                    serde_json::to_string(&entry.origin).unwrap_or_default().trim_matches('"'),
                    &entry.plc_name,
                    entry.state.as_str(),
                    &doc,
                    &entry.created_at,
                    now_str(),
                ),
            )
        })?;
        {
            let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
            match cache.iter_mut().find(|e| e.id == entry.id) {
                Some(slot) => *slot = entry.clone(),
                None => cache.push(entry.clone()),
            }
        }
        let _ = self.events.send(LedgerEvent::Upsert { task: entry.clone() });
        Ok(entry)
    }

    /// Records a transition (history + task_events table) and upserts.
    pub fn transition(&self, mut entry: LedgerEntry, to: TaskState, by: Actor, note: Option<String>) -> Result<LedgerEntry, ApiError> {
        let from = Some(entry.state);
        let at = now_str();
        entry.history.push(Transition { from, to, at: at.clone(), by, note: note.clone() });
        entry.state = to;
        entry.state_at = at.clone();
        if to.is_terminal() {
            entry.ended_at = Some(at.clone());
        }
        let _ = self.db.with(|c| {
            c.execute(
                "INSERT INTO task_events (task_id, ts, from_state, to_state, by, note) VALUES (?1,?2,?3,?4,?5,?6)",
                (&entry.id, &at, from.map(|s| s.as_str()), to.as_str(), serde_json::to_string(&by).unwrap_or_default().trim_matches('"'), note),
            )
        });
        self.upsert(entry)
    }

    pub fn next_seq(&self) -> Result<i64, ApiError> {
        Ok(self.db.next_counter("ledger_seq", 1)?)
    }

    /// Allocates the next (work_id, task_id) pair. work_id is derived from the day (YYMMDD) + counter
    /// so keys never collide across sessions; task_id is a per-work monotonic counter.
    pub fn allocate_key(&self) -> Result<TaskKey, ApiError> {
        let day = time::OffsetDateTime::now_utc();
        let base = ((day.year() % 100) as u32) * 10000 + (day.month() as u32) * 100 + day.day() as u32; // e.g. 260912
        let n = self.db.next_counter("work_id", 1)? as u32;
        let work_id = base * 1000 + (n % 1000);
        let task_id = self.db.next_counter("task_id", 1)? as u32;
        Ok(TaskKey { work_id, task_id: (task_id % 60000).max(1) })
    }

    /// Creates a new entry for a composed task (Draft or Submitted is decided by the caller).
    pub fn create(&self, origin: Origin, request: Option<TaskRequest>, resolved: Option<TaskParams>, mut task: TaskData) -> Result<LedgerEntry, ApiError> {
        let key = self.allocate_key()?;
        task.work_id = key.work_id;
        task.task_id = key.task_id;
        let now = now_str();
        let e = LedgerEntry {
            id: uuid::Uuid::new_v4().to_string(),
            seq: self.next_seq()?,
            work_id: key.work_id,
            task_id: key.task_id,
            origin,
            plc_name: self.plc.clone(),
            request,
            resolved,
            position: task.position,
            plc_task: task,
            state: TaskState::Draft,
            state_at: now.clone(),
            created_at: now.clone(),
            submitted_at: None,
            ended_at: None,
            ack: None,
            header: None,
            plc: None,
            error: None,
            history: vec![Transition { from: None, to: TaskState::Draft, at: now, by: Actor::Ui, note: None }],
        };
        self.upsert(e)
    }

    /// Registers a task first seen on the PLC.
    pub fn create_external(&self, task: &TaskData, state: TaskState) -> Result<LedgerEntry, ApiError> {
        let now = now_str();
        let e = LedgerEntry {
            id: uuid::Uuid::new_v4().to_string(),
            seq: self.next_seq()?,
            work_id: task.work_id,
            task_id: task.task_id,
            origin: Origin::External,
            plc_name: self.plc.clone(),
            request: None,
            resolved: Some(TaskParams::from_task(task)),
            position: task.position,
            plc_task: task.clone(),
            state,
            state_at: now.clone(),
            created_at: now.clone(),
            submitted_at: None,
            ended_at: if state.is_terminal() { Some(now.clone()) } else { None },
            ack: None,
            header: None,
            plc: None,
            error: None,
            history: vec![Transition { from: None, to: state, at: now, by: Actor::Plc, note: Some("seen on PLC".into()) }],
        };
        self.upsert(e)
    }

    /// Paged query over the persistent table (newest first).
    pub fn query(&self, states: Option<Vec<TaskState>>, task_type: Option<u8>, q: Option<String>, limit: usize, offset: usize) -> Result<(Vec<LedgerEntry>, usize), ApiError> {
        let rows: Vec<String> = self.db.with(|c| {
            let mut st = c.prepare("SELECT doc_json FROM tasks ORDER BY seq DESC")?;
            let rows = st.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()
        })?;
        let mut all: Vec<LedgerEntry> = rows.into_iter().filter_map(|s| serde_json::from_str(&s).ok()).collect();
        if let Some(states) = states {
            all.retain(|e| states.contains(&e.state));
        }
        if let Some(t) = task_type {
            all.retain(|e| e.plc_task.task_type == t);
        }
        if let Some(q) = q.filter(|q| !q.is_empty()) {
            let q = q.to_ascii_lowercase();
            all.retain(|e| {
                e.work_id.to_string().contains(&q)
                    || e.task_id.to_string().contains(&q)
                    || e.plc_task.cell.id.to_string().contains(&q)
                    || e.plc_task.item.code.to_string().contains(&q)
                    || e.request.as_ref().map(|r| r.note.to_ascii_lowercase().contains(&q)).unwrap_or(false)
            });
        }
        let total = all.len();
        Ok((all.into_iter().skip(offset).take(limit).collect(), total))
    }

    pub fn snapshot_event(&self) -> LedgerEvent {
        LedgerEvent::Snapshot { tasks: self.list() }
    }
}
