//! 작업 대기열(2026-10-01, 설계 시안 https://claude.ai/artifact/UqzD1D5mD9gTKZRg2hyB7Q).
//!
//! 사람(작업 명령 화면)과 자동(규칙 · 요청)이 만든 작업이 로봇별 대기열 하나에 모이고, 보내기 루프
//! (`dispatch`) 하나가 PLC 로 보낸다. 작업 = 짝(PICK + DROP) 또는 한 건(MOVE · MEASURE · UP · 복구 DROP).
//!
//! - **WorkId** 는 대기열에 들어갈 때(단계 `wait`) 하나 붙는다. 짝의 PICK · DROP 은 같은 WorkId 의 TaskId 1 · 2.
//!   PLC 의 완료 · 삭제가 WorkId + TaskId 로 맞추고, 취소는 같은 WorkId 의 뒤 Task 까지 데려간다(`ops::cascade_after`).
//! - **이송 지시**는 짝이 대기열에 들어갈 때 "예정"으로 연다(두 Task 에 같은 id).
//! - **단계**: 예정(`pre`, 규칙이 조건을 기다림) → 대기(`wait`) → 할당(`assigned`, PLC 로 나감) → 실행(`running`)
//!   → 완료 · 실패 · 취소. 할당 뒤 단계는 원장 Task 상태에서 유도한다(`derive_stage`).
//! - **보내기 규칙**(`pick_next`): 짝 중간이면 그 DROP 이 다음, 아니면 대기 중 우선이 가장 높은 작업(같으면 먼저 온 것).
//!   PLC 에는 실행 1 + 다음 1 까지(`DEPTH`).

pub mod dispatch;
pub mod routes;
pub mod templates;

use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use tokio::sync::broadcast;

use crate::db::Db;
use crate::error::ApiError;
use crate::ledger::{Origin, TaskRequest, TaskState};
use crate::util::now_str;

/// PLC 에 둘 Task 수 — 실행 중 1 + 다음 1.
pub const DEPTH: usize = 2;
/// 사람이 만든 작업의 기본 우선(자동은 규칙 값).
pub const MANUAL_PRIORITY: i32 = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Pre,
    Wait,
    Assigned,
    Running,
    Done,
    Failed,
    Canceled,
}

impl Stage {
    pub fn is_end(self) -> bool {
        matches!(self, Stage::Done | Stage::Failed | Stage::Canceled)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Pre => "pre",
            Stage::Wait => "wait",
            Stage::Assigned => "assigned",
            Stage::Running => "running",
            Stage::Done => "done",
            Stage::Failed => "failed",
            Stage::Canceled => "canceled",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Stage::Pre => "예정",
            Stage::Wait => "대기",
            Stage::Assigned => "할당",
            Stage::Running => "실행",
            Stage::Done => "완료",
            Stage::Failed => "실패",
            Stage::Canceled => "취소",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobStep {
    pub request: TaskRequest,
    /// 원장 Task id(보낸 뒤).
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub state: Option<TaskState>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobEvent {
    pub at: String,
    pub stage: Stage,
    pub note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub seq: i64,
    pub robot: Option<u8>,
    pub stage: Stage,
    pub origin: Origin,
    /// 자동 할당의 주체(`rule:<id>` · `req:<id>` …). 사람이 만든 작업은 없음.
    #[serde(default)]
    pub via: Option<String>,
    pub priority: i32,
    #[serde(default)]
    pub work_id: Option<u32>,
    #[serde(default)]
    pub transfer_order_id: Option<String>,
    pub steps: Vec<JobStep>,
    /// 지금 왜 안 나가나(대기 · 예정).
    #[serde(default)]
    pub wait_reason: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub note: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(default)]
    pub history: Vec<JobEvent>,
}

fn op(r: &TaskRequest) -> String {
    r.task_type.to_ascii_uppercase()
}

impl Job {
    /// 아직 안 보낸 첫 단계.
    pub fn next_step(&self) -> Option<usize> {
        self.steps.iter().position(|s| s.task.is_none())
    }
    pub fn sent_any(&self) -> bool {
        self.steps.iter().any(|s| s.task.is_some())
    }
    fn set_stage(&mut self, to: Stage, note: impl Into<String>) {
        let note = note.into();
        if self.stage == to && note.is_empty() {
            return;
        }
        self.stage = to;
        let now = now_str();
        if to.is_end() && self.ended_at.is_none() {
            self.ended_at = Some(now.clone());
        }
        self.history.push(JobEvent { at: now.clone(), stage: to, note });
        self.updated_at = now;
    }
}

/// 보낸 Task 들의 상태로 단계를 유도한다. 끝난 단계는 그대로, 아무것도 안 보냈으면 지금 단계(예정 · 대기).
pub fn derive_stage(cur: Stage, states: &[Option<TaskState>]) -> Stage {
    use TaskState as T;
    if cur.is_end() {
        return cur;
    }
    let sent: Vec<T> = states.iter().flatten().copied().filter(|s| *s != T::Draft).collect();
    if sent.is_empty() {
        return cur;
    }
    if sent.contains(&T::Canceled) {
        return Stage::Canceled;
    }
    if sent.iter().any(|s| matches!(s, T::Rejected | T::Failed)) {
        return Stage::Failed;
    }
    if states.len() == sent.len() && sent.iter().all(|s| *s == T::Completed) {
        return Stage::Done;
    }
    if sent.iter().any(|s| matches!(s, T::Running | T::Completed)) {
        return Stage::Running;
    }
    Stage::Assigned
}

/// 다음에 보낼 것: (작업 id, 단계 번호, 짝 중간인가). 로봇 `robot` 의 작업만.
pub fn pick_next(jobs: &[Job], robot: u8) -> Option<(String, usize, bool)> {
    let mine = jobs.iter().filter(|j| j.robot == Some(robot));
    // 짝 중간 — 보낸 작업에 아직 안 보낸 단계가 있으면 그것이 먼저(다른 작업은 끼어들지 못한다).
    if let Some(j) = mine.clone().filter(|j| matches!(j.stage, Stage::Assigned | Stage::Running) && j.sent_any()).filter_map(|j| j.next_step().map(|i| (j, i))).min_by_key(|(j, _)| j.seq) {
        return Some((j.0.id.clone(), j.1, true));
    }
    mine.filter(|j| j.stage == Stage::Wait).min_by(|a, b| b.priority.cmp(&a.priority).then(a.seq.cmp(&b.seq))).map(|j| (j.id.clone(), 0, false))
}

/// 대기열 순서(우선 높은 것 → 먼저 온 것). 화면의 "순번".
pub fn queue_order(jobs: &[Job], robot: u8) -> Vec<String> {
    let mut w: Vec<&Job> = jobs.iter().filter(|j| j.robot == Some(robot) && j.stage == Stage::Wait).collect();
    w.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.seq.cmp(&b.seq)));
    w.into_iter().map(|j| j.id.clone()).collect()
}

/// 로봇별 보내기 스위치.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DispatchState {
    pub robot: u8,
    pub enabled: bool,
    /// 거부 · 실패로 멈춘 사유(풀기 전까지 보내지 않는다).
    pub paused: Option<String>,
}

pub struct JobStore {
    db: Db,
    /// 끝나지 않은 작업 + 이번 실행에서 끝난 작업(최근). 정본은 DB.
    cache: Mutex<Vec<Job>>,
    pub events: broadcast::Sender<Json>,
}

static STORE: OnceLock<Arc<JobStore>> = OnceLock::new();

pub fn store() -> Option<&'static Arc<JobStore>> {
    STORE.get()
}

impl JobStore {
    pub fn open(db: Db) -> Result<Arc<JobStore>, ApiError> {
        let rows: Vec<String> = db.with(|c| {
            let mut st = c.prepare("SELECT doc_json FROM jobs WHERE ended_at IS NULL OR ended_at >= datetime('now', '-1 day') ORDER BY seq")?;
            let it = st.query_map([], |r| r.get::<_, String>(0))?;
            it.collect()
        })?;
        let jobs = rows.into_iter().filter_map(|s| serde_json::from_str(&s).ok()).collect();
        let (tx, _) = broadcast::channel(256);
        Ok(Arc::new(JobStore { db, cache: Mutex::new(jobs), events: tx }))
    }

    pub fn list(&self) -> Vec<Job> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn get(&self, id: &str) -> Option<Job> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner).iter().find(|j| j.id == id).cloned()
    }

    /// 끝난 작업까지 DB 에서(화면의 완료 · 실패 · 취소 묶음).
    pub fn history(&self, robot: Option<u8>, limit: u32) -> Result<Vec<Job>, ApiError> {
        let rows: Vec<String> = self.db.with(|c| {
            let mut st = c.prepare("SELECT doc_json FROM jobs WHERE ended_at IS NOT NULL AND (?1 IS NULL OR robot = ?1) ORDER BY ended_at DESC LIMIT ?2")?;
            let it = st.query_map((robot.map(i64::from), i64::from(limit)), |r| r.get::<_, String>(0))?;
            it.collect()
        })?;
        Ok(rows.into_iter().filter_map(|s| serde_json::from_str(&s).ok()).collect())
    }

    pub fn next_seq(&self) -> Result<i64, ApiError> {
        Ok(self.db.next_counter("job_seq", 1)?)
    }

    pub fn save(&self, job: &Job) -> Result<(), ApiError> {
        let doc = serde_json::to_string(job)?;
        self.db.with(|c| {
            c.execute(
                "INSERT INTO jobs (id, seq, robot, stage, priority, work_id, doc_json, created_at, updated_at, ended_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
                 ON CONFLICT(id) DO UPDATE SET robot=excluded.robot, stage=excluded.stage, priority=excluded.priority, work_id=excluded.work_id, doc_json=excluded.doc_json, updated_at=excluded.updated_at, ended_at=excluded.ended_at",
                (&job.id, job.seq, job.robot.map(i64::from), job.stage.as_str(), job.priority, job.work_id.map(i64::from), &doc, &job.created_at, &job.updated_at, &job.ended_at),
            )
        })?;
        {
            let mut g = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
            match g.iter_mut().find(|j| j.id == job.id) {
                Some(slot) => *slot = job.clone(),
                None => g.push(job.clone()),
            }
            // 오래 끝난 것은 캐시에서 덜어 낸다(정본은 DB).
            if g.len() > 400 {
                let cut = g.len() - 400;
                let mut n = 0;
                g.retain(|j| {
                    if n < cut && j.stage.is_end() {
                        n += 1;
                        false
                    } else {
                        true
                    }
                });
            }
        }
        let _ = self.events.send(json!({ "kind": "job", "job": job }));
        Ok(())
    }

    /// 한 작업을 고친다(잠금 없이 읽고-고치고-쓴다 — 보내기 루프는 하나, 화면 조작은 드물다).
    pub fn update(&self, id: &str, f: impl FnOnce(&mut Job) -> Result<(), ApiError>) -> Result<Job, ApiError> {
        let mut j = self.get(id).ok_or_else(|| ApiError::NotFound(format!("job {id}")))?;
        f(&mut j)?;
        j.updated_at = now_str();
        self.save(&j)?;
        Ok(j)
    }

    pub fn dispatch_state(&self, robot: u8) -> DispatchState {
        self.db
            .with(|c| {
                c.query_row("SELECT enabled, paused FROM job_dispatch WHERE robot = ?1", [i64::from(robot)], |r| Ok(DispatchState { robot, enabled: r.get::<_, i64>(0)? != 0, paused: r.get(1)? }))
            })
            .unwrap_or(DispatchState { robot, enabled: false, paused: None })
    }

    pub fn set_dispatch(&self, s: &DispatchState) -> Result<(), ApiError> {
        self.db.with(|c| {
            c.execute(
                "INSERT INTO job_dispatch (robot, enabled, paused, updated_at) VALUES (?1,?2,?3,?4) ON CONFLICT(robot) DO UPDATE SET enabled=excluded.enabled, paused=excluded.paused, updated_at=excluded.updated_at",
                (i64::from(s.robot), i64::from(s.enabled), &s.paused, now_str()),
            )
        })?;
        let _ = self.events.send(json!({ "kind": "dispatch", "dispatch": s }));
        Ok(())
    }
}

/// 새 작업.
#[derive(Clone, Debug, Deserialize)]
pub struct NewJob {
    pub robot: Option<u8>,
    /// 1 건 또는 PICK + DROP 두 건.
    pub steps: Vec<TaskRequest>,
    #[serde(default)]
    pub priority: Option<i32>,
    #[serde(default)]
    pub note: String,
}

/// 모양 검사(순수): 짝은 PICK → DROP 같은 개수 · 같은 품목, 한 건은 PICK 이 아닐 것.
pub fn check_shape(steps: &[TaskRequest]) -> Result<(), String> {
    match steps {
        [one] => {
            if op(one) == "PICK" {
                return Err("PICK 은 짝 DROP 과 함께 넣습니다".into());
            }
            Ok(())
        }
        [p, d] => {
            if op(p) != "PICK" || op(d) != "DROP" {
                return Err("두 건이면 PICK → DROP 짝이어야 합니다".into());
            }
            if p.count != d.count {
                return Err(format!("짝 수량 PICK {} ≠ DROP {}", p.count, d.count));
            }
            if let (Some(a), Some(b)) = (p.item_code, d.item_code)
                && a != b
            {
                return Err(format!("짝 품목 PICK {a} ≠ DROP {b}"));
            }
            Ok(())
        }
        _ => Err("작업은 한 건 또는 PICK + DROP 두 건입니다".into()),
    }
}

/// 대기열에 넣는다: 모양 · 작성 검사 → WorkId → (짝이면) 이송 지시 "예정" → 단계 대기.
pub fn enqueue(st: &crate::state::AppState, n: NewJob, origin: Origin, via: Option<String>) -> Result<Job, ApiError> {
    let store = store().ok_or_else(|| ApiError::Internal("job store not ready".into()))?;
    check_shape(&n.steps).map_err(ApiError::BadRequest)?;
    let r = st.robot_required(n.robot, "작업 추가")?;
    let mut steps = n.steps;
    for s in steps.iter_mut() {
        s.robot = Some(r.id);
        // 사람 작업은 출처 태그가 없다. 자동(생성 규칙)은 `gen:<규칙>` 태그를 그대로 둔다 — 규칙이 진행 중인지 그것으로 본다.
        if origin == Origin::Manual {
            s.source = None;
        }
        s.via = via.clone();
    }
    // 같은 스테이션 그룹의 스테이션 → 스테이션 짝은 Multi-Picking(두 스텝 LiftUpPartial) — 옛 계획 · 실행기와 같은 자동 판정.
    if steps.len() == 2 {
        let tt = |r: &TaskRequest| crate::issue::parse_task_type(&r.task_type).ok();
        if let (Some(a), Some(b)) = (tt(&steps[0]), tt(&steps[1]))
            && crate::issue::same_station_group(steps[0].target.as_ref(), a, steps[1].target.as_ref(), b)
        {
            for s in steps.iter_mut() {
                s.multi_pick.get_or_insert(true);
            }
        }
    }
    // 잘못된 대상 · 품목은 지금 거른다(Z 는 보낼 때 다시 작성).
    let first = crate::issue::compose(st, &steps[0])?;
    if steps.len() == 2 {
        crate::issue::compose(st, &steps[1])?;
    }
    let work_id = r.ledger.allocate_work_id()?;
    let mut order = None;
    if steps.len() == 2 {
        let o = st.stock.open_order(crate::stock::transfer::NewOrder {
            robot: Some(r.id),
            plc: r.plc.clone(),
            item_code: first.task.item.code,
            count: u32::from(steps[0].count),
            from: steps[0].target.clone(),
            to: steps[1].target.clone(),
            source: via.clone().unwrap_or_else(|| "manual".into()),
            note: format!("WorkId {work_id}{}", if n.note.trim().is_empty() { String::new() } else { format!(" · {}", n.note.trim()) }),
        })?;
        for s in steps.iter_mut() {
            s.transfer_order_id = Some(o.id.clone());
        }
        order = Some(o.id);
    }
    let now = now_str();
    let mut job = Job {
        id: uuid::Uuid::new_v4().to_string(),
        seq: store.next_seq()?,
        robot: Some(r.id),
        stage: Stage::Wait,
        origin,
        via,
        priority: n.priority.unwrap_or(MANUAL_PRIORITY),
        work_id: Some(work_id),
        transfer_order_id: order,
        steps: steps.into_iter().map(|request| JobStep { request, task: None, state: None }).collect(),
        wait_reason: None,
        error: None,
        note: n.note,
        created_at: now.clone(),
        updated_at: now,
        ended_at: None,
        history: vec![],
    };
    job.history.push(JobEvent { at: job.created_at.clone(), stage: Stage::Wait, note: format!("대기열에 들어감 · WorkId {work_id}") });
    store.save(&job)?;
    tracing::info!(robot = %r.name, work_id, job = %job.id, "jobs: 대기열에 넣음");
    Ok(job)
}

pub fn spawn(st: crate::state::AppState) -> Result<(), ApiError> {
    let s = JobStore::open(st.db.clone())?;
    let _ = STORE.set(s);
    dispatch::spawn(st);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use TaskState as T;

    fn req(op: &str, count: u8, item: Option<u32>) -> TaskRequest {
        TaskRequest { task_type: op.into(), count, item_code: item, ..Default::default() }
    }

    fn job(id: &str, seq: i64, robot: u8, stage: Stage, prio: i32, steps: &[(&str, Option<&str>)]) -> Job {
        Job {
            id: id.into(),
            seq,
            robot: Some(robot),
            stage,
            origin: Origin::Manual,
            via: None,
            priority: prio,
            work_id: Some(1),
            transfer_order_id: None,
            steps: steps.iter().map(|(o, t)| JobStep { request: req(o, 1, Some(1001)), task: t.map(String::from), state: None }).collect(),
            wait_reason: None,
            error: None,
            note: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            ended_at: None,
            history: vec![],
        }
    }

    #[test]
    fn stage_follows_the_tasks() {
        assert_eq!(derive_stage(Stage::Wait, &[None, None]), Stage::Wait);
        assert_eq!(derive_stage(Stage::Wait, &[Some(T::Submitted), None]), Stage::Assigned);
        assert_eq!(derive_stage(Stage::Assigned, &[Some(T::Running), None]), Stage::Running);
        assert_eq!(derive_stage(Stage::Running, &[Some(T::Completed), None]), Stage::Running, "PICK 끝, DROP 아직 = 실행");
        assert_eq!(derive_stage(Stage::Running, &[Some(T::Completed), Some(T::Queued)]), Stage::Running);
        assert_eq!(derive_stage(Stage::Running, &[Some(T::Completed), Some(T::Completed)]), Stage::Done);
        assert_eq!(derive_stage(Stage::Assigned, &[Some(T::Rejected), None]), Stage::Failed);
        assert_eq!(derive_stage(Stage::Running, &[Some(T::Completed), Some(T::Canceled)]), Stage::Canceled);
        assert_eq!(derive_stage(Stage::Done, &[Some(T::Canceled), None]), Stage::Done, "끝난 단계는 그대로");
        assert_eq!(derive_stage(Stage::Wait, &[Some(T::Draft), None]), Stage::Wait);
    }

    #[test]
    fn mid_pair_drop_goes_first_then_priority() {
        let jobs = vec![
            job("a", 1, 2, Stage::Wait, 50, &[("PICK", None), ("DROP", None)]),
            job("b", 2, 2, Stage::Running, 30, &[("PICK", Some("t1")), ("DROP", None)]),
            job("c", 3, 2, Stage::Wait, 80, &[("MOVE", None)]),
            job("d", 4, 1, Stage::Wait, 99, &[("MOVE", None)]),
        ];
        assert_eq!(pick_next(&jobs, 2), Some(("b".into(), 1, true)));
        let jobs2: Vec<Job> = jobs.into_iter().filter(|j| j.id != "b").collect();
        assert_eq!(pick_next(&jobs2, 2), Some(("c".into(), 0, false)), "우선 80 > 50");
        assert_eq!(pick_next(&jobs2, 1), Some(("d".into(), 0, false)), "로봇별");
        assert_eq!(queue_order(&jobs2, 2), vec!["c".to_string(), "a".to_string()]);
    }

    #[test]
    fn same_priority_keeps_arrival_order() {
        let jobs = vec![job("x", 5, 2, Stage::Wait, 50, &[("MOVE", None)]), job("y", 3, 2, Stage::Wait, 50, &[("MOVE", None)])];
        assert_eq!(pick_next(&jobs, 2).unwrap().0, "y");
    }

    #[test]
    fn shape_rules() {
        assert!(check_shape(&[req("PICK", 1, None)]).is_err());
        assert!(check_shape(&[req("DROP", 1, None)]).is_ok());
        assert!(check_shape(&[req("PICK", 2, Some(1)), req("DROP", 2, Some(1))]).is_ok());
        assert!(check_shape(&[req("PICK", 2, Some(1)), req("DROP", 1, Some(1))]).is_err());
        assert!(check_shape(&[req("PICK", 1, Some(1)), req("DROP", 1, Some(2))]).is_err());
        assert!(check_shape(&[req("DROP", 1, None), req("PICK", 1, None)]).is_err());
        assert!(check_shape(&[]).is_err());
    }
}
