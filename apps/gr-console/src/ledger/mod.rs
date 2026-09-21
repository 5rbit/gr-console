//! Task ledger: console-side record of every task (ours or external), persisted in sqlite, with a
//! broadcast of changes. M1 provides the types, CRUD, id allocation and `submit()`; the sync engine
//! (`sync.rs`), operations (`ops.rs`) and routes (`routes.rs`) are the M3-B slice.

pub mod ops;
pub mod robot_cmd;
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

impl Origin {
    pub fn parse(s: &str) -> Option<Origin> {
        Some(match s.to_ascii_lowercase().as_str() {
            "console" => Origin::Console,
            "scenario" => Origin::Scenario,
            "external" => Origin::External,
            _ => return None,
        })
    }
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
    /// Robot to send to (`None` = default robot).
    #[serde(default)]
    pub robot: Option<u8>,
    /// Grip reference override: `mid` (tire centre) | `pick_bead` (measured upper bead − PickBeadOffset,
    /// `bead+offset` on screen). `None` = `Defaults.grip_ref`. 옛 값 `bead` 는 `pick_bead` 로 읽힌다.
    #[serde(default)]
    pub grip_ref: Option<String>,
    /// 스테이션 보정(GRM StationCenterAdjust 재현) — 기본 켜짐. `false` / `"off"` 로 끈다.
    #[serde(default, skip_serializing_if = "crate::issue::station_offset::OffsetSwitch::is_default")]
    pub station_offset: crate::issue::station_offset::OffsetSwitch,
    /// 셀 단수 Max(품목 `spec.stack_max`)를 넘는 DROP 도 제출한다 — 기본은 제출 거부(409).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ignore_stack_max: bool,
    /// 팔렛 슬롯(`{seq, level}` 또는 `{auto: true}`) — 켜진 팔렛 프로파일이 있는 스테이션 대상에만.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pallet: Option<crate::pallet::compose::PalletRef>,
    /// Multi-Picking 상황(기본값 `situations.multi_pick`) — 스테이션 PICK/DROP 에만. `None` = 적용 안 함
    /// (시나리오 실행기는 다음 스텝이 같은 스테이션 그룹이면 스스로 `Some(true)` 로 채운다).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multi_pick: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    /// 마지막으로 계산한 스테이션 보정 감사 기록(제출 시점 값). doc_json 에 같이 저장되므로 마이그레이션 없음.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station_offset: Option<crate::issue::station_offset::StationOffsetAudit>,
    /// 팔렛 슬롯 근거(`pallet::compose::PalletAudit`) — auto 로 고른 seq/단도 여기서 고정된다. doc_json 에 같이 저장.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pallet: Option<crate::pallet::compose::PalletAudit>,
}

impl LedgerEntry {
    pub fn key(&self) -> TaskKey {
        TaskKey { work_id: self.work_id, task_id: self.task_id }
    }
}

#[derive(Clone, Debug, Serialize)]
#[allow(clippy::large_enum_variant)]
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
    /// One ledger per robot (`plc` = its status PLC). `events` may be shared between robots so one SSE
    /// stream carries every robot's tasks.
    pub fn new(db: Db, plc: &str, events: Option<broadcast::Sender<LedgerEvent>>) -> anyhow::Result<Arc<Ledger>> {
        let tx = events.unwrap_or_else(|| broadcast::channel(512).0);
        let l = Ledger { db, plc: plc.to_string(), cache: Mutex::new(Vec::new()), events: tx };
        l.reload()?;
        Ok(Arc::new(l))
    }

    fn reload(&self) -> anyhow::Result<()> {
        let rows: Vec<String> = self.db.with(|c| {
            let mut st =
                c.prepare("SELECT doc_json FROM tasks WHERE plc = ?1 AND (state NOT IN ('completed','canceled','rejected','failed') OR updated_at > datetime('now','-1 day')) ORDER BY seq")?;
            let rows = st.query_map([&self.plc], |r| r.get::<_, String>(0))?;
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

    /// Kept for the other slices (scenario / issue); the ledger itself does not need it.
    #[allow(dead_code)]
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
        // 테이블은 로봇들이 같이 쓴다 — `plc` 조건이 없으면 첫 로봇(GR1) 원장이 GR2 Task 를 제 것으로 돌려주고,
        // `find_task` 가 그 로봇으로 취소·완료를 보냈다(2026-09-21, GR2 취소가 GR1 RES.Data[6] 로 거부).
        self.db.with(|c| c.query_row("SELECT doc_json FROM tasks WHERE id = ?1 AND plc = ?2", (id, &self.plc), |r| r.get::<_, String>(0))).ok().and_then(|s| serde_json::from_str(&s).ok())
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
            station_offset: None,
            pallet: None,
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
            station_offset: None,
            pallet: None,
        };
        self.upsert(e)
    }

    /// Paged query over the persistent table (newest first). Thin wrapper kept for the other slices;
    /// the routes use [`Ledger::query_filtered`].
    #[allow(dead_code)]
    pub fn query(&self, states: Option<Vec<TaskState>>, task_type: Option<u8>, q: Option<String>, limit: usize, offset: usize) -> Result<(Vec<LedgerEntry>, usize), ApiError> {
        self.query_filtered(QueryFilter { states, task_type, q, limit, offset, ..Default::default() })
    }

    /// All persisted rows, newest first.
    fn all_rows(&self) -> Result<Vec<LedgerEntry>, ApiError> {
        let rows: Vec<String> = self.db.with(|c| {
            let mut st = c.prepare("SELECT doc_json FROM tasks WHERE plc = ?1 ORDER BY seq DESC")?;
            let rows = st.query_map([&self.plc], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()
        })?;
        Ok(rows.into_iter().filter_map(|s| serde_json::from_str(&s).ok()).collect())
    }

    /// Paged query with every filter the `/api/tasks` list endpoint understands.
    pub fn query_filtered(&self, f: QueryFilter) -> Result<(Vec<LedgerEntry>, usize), ApiError> {
        let mut all = self.all_rows()?;
        if let Some(states) = f.states {
            all.retain(|e| states.contains(&e.state));
        }
        if let Some(t) = f.task_type {
            all.retain(|e| e.plc_task.task_type == t);
        }
        if let Some(o) = f.origin {
            all.retain(|e| e.origin == o);
        }
        if let Some(since) = f.since.as_deref().filter(|s| !s.is_empty()) {
            let since_t = parse_rfc3339(since);
            all.retain(|e| match (since_t, parse_rfc3339(&e.created_at)) {
                (Some(a), Some(b)) => b >= a,
                _ => e.created_at.as_str() >= since,
            });
        }
        if let Some(q) = f.q.filter(|q| !q.is_empty()) {
            let q = q.to_ascii_lowercase();
            all.retain(|e| {
                e.seq.to_string() == q
                    || e.work_id.to_string().contains(&q)
                    || e.task_id.to_string().contains(&q)
                    || e.plc_task.cell.id.to_string().contains(&q)
                    || e.plc_task.item.code.to_string().contains(&q)
                    || e.request.as_ref().map(|r| r.note.to_ascii_lowercase().contains(&q)).unwrap_or(false)
            });
        }
        let total = all.len();
        Ok((all.into_iter().skip(f.offset).take(f.limit).collect(), total))
    }

    /// Removes a terminal (or draft) entry from sqlite + cache and broadcasts `Remove`.
    pub fn remove(&self, id: &str) -> Result<(), ApiError> {
        let e = self.get(id).ok_or_else(|| ApiError::NotFound(format!("task {id}")))?;
        if !(e.state.is_terminal() || e.state == TaskState::Draft) {
            return Err(ApiError::Conflict(format!("{} 상태의 태스크는 삭제할 수 없습니다 (종결된 태스크만)", e.state.as_str())));
        }
        self.db.with(|c| {
            c.execute("DELETE FROM task_events WHERE task_id = ?1", [id])?;
            c.execute("DELETE FROM tasks WHERE id = ?1", [id])
        })?;
        self.cache.lock().unwrap_or_else(PoisonError::into_inner).retain(|x| x.id != id);
        let _ = self.events.send(LedgerEvent::Remove { id: id.to_string() });
        Ok(())
    }

    /// Counts per state, today's completed / rejected, and the mean submit→completed time.
    pub fn stats(&self) -> Result<LedgerStats, ApiError> {
        let all = self.all_rows()?;
        let today = now_str().chars().take(10).collect::<String>();
        let mut s = LedgerStats { total: all.len(), ..Default::default() };
        let mut sum = 0.0_f64;
        let mut n = 0_u32;
        for e in &all {
            *s.by_state.entry(e.state.as_str().to_string()).or_insert(0) += 1;
            if e.state.is_active() {
                s.active += 1;
            }
            let ended_today = e.ended_at.as_deref().map(|t| t.starts_with(&today)).unwrap_or(false);
            match e.state {
                TaskState::Completed => {
                    if ended_today {
                        s.completed_today += 1;
                    }
                    if let (Some(a), Some(b)) = (e.submitted_at.as_deref().and_then(parse_rfc3339), e.ended_at.as_deref().and_then(parse_rfc3339)) {
                        let d = (b - a).as_seconds_f64();
                        if d >= 0.0 {
                            sum += d;
                            n += 1;
                        }
                    }
                }
                TaskState::Rejected if ended_today => s.rejected_today += 1,
                _ => {}
            }
        }
        s.avg_complete_secs = if n > 0 { Some(sum / n as f64) } else { None };
        s.completed_samples = n;
        Ok(s)
    }
}

/// Filters for [`Ledger::query_filtered`].
#[derive(Clone, Debug, Default)]
pub struct QueryFilter {
    pub states: Option<Vec<TaskState>>,
    pub task_type: Option<u8>,
    pub origin: Option<Origin>,
    /// RFC 3339 lower bound on `created_at`.
    pub since: Option<String>,
    pub q: Option<String>,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct LedgerStats {
    pub total: usize,
    pub active: usize,
    pub by_state: std::collections::BTreeMap<String, usize>,
    pub completed_today: usize,
    pub rejected_today: usize,
    /// Mean submit→completed duration in seconds over every completed entry (None when there is none).
    pub avg_complete_secs: Option<f64>,
    pub completed_samples: u32,
}

pub(crate) fn parse_rfc3339(s: &str) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
}
