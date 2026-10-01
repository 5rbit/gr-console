//! 보내기 루프 — 로봇마다 "지금 한 건 보낼 수 있나"를 0.5 s 마다(그리고 원장 이벤트마다) 판단한다.
//!
//! 순서: 끝난 · 진행 중 작업의 단계를 원장에서 다시 읽는다 → 로봇마다 [켜짐 · 멈춤 아님] 이면
//! `pick_next` → 에코 대기 · 게이트 · 깊이(실행 1 + 다음 1) · 두 로봇 영역 → 보낸다.
//! 보내기가 거부되면(품목 모름 · StackMax · 짝 검사 · PLC 거부) **그 로봇을 멈추고** 알린다 — 재고가 어긋난 채
//! 다음 작업을 보내지 않는다(결정 2026-10-01). 영역 대기 · 게이트는 실패가 아니라 기다림이다.
//! OPC UA · PLC 응답 없음 같은 일시 오류는 아무것도 나가지 않은 것이므로 3 s 뒤 다시 보내고, 5 번 연달아 실패할 때만 멈춘다.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use tokio::sync::broadcast::error::RecvError;

use super::{DEPTH, Job, JobEvent, Stage, derive_stage, pick_next, store};
use crate::error::ApiError;
use crate::ledger::{LedgerEntry, TaskState};
use crate::state::{AppState, RobotCtx};
use crate::util::now_str;

const TICK: Duration = Duration::from_millis(500);
/// 일시 오류 다시 보내기 — 간격과 멈추기 전 최대 횟수.
const RETRY_GAP: Duration = Duration::from_secs(3);
const RETRY_MAX: u32 = 5;

/// 작업별 연속 일시 실패 수와 다음 시도 시각(메모리만 — 콘솔을 다시 켜면 처음부터 센다).
#[derive(Default)]
struct Retries(HashMap<String, (u32, Instant)>);

impl Retries {
    fn waiting(&self, id: &str, now: Instant) -> Option<u32> {
        self.0.get(id).filter(|(_, at)| now < *at).map(|(n, _)| *n)
    }
    fn bump(&mut self, id: &str, now: Instant) -> u32 {
        let e = self.0.entry(id.to_string()).or_insert((0, now));
        e.0 += 1;
        e.1 = now + RETRY_GAP;
        e.0
    }
    fn clear(&mut self, id: &str) {
        self.0.remove(id);
    }
}

fn retries() -> std::sync::MutexGuard<'static, Retries> {
    static R: OnceLock<Mutex<Retries>> = OnceLock::new();
    R.get_or_init(Default::default).lock().unwrap_or_else(PoisonError::into_inner)
}

/// 아무것도 나가지 않은 일시 오류 — OPC UA 쓰기 무응답 · PLC 연결 끊김 · 종료 중.
fn transient(e: &ApiError) -> bool {
    matches!(e, ApiError::OpcNotReady(_) | ApiError::PlcUnavailable(_) | ApiError::ShuttingDown(_))
}

pub fn spawn(st: AppState) {
    tokio::spawn(async move {
        let mut rx = st.task_events.subscribe();
        loop {
            tokio::select! {
                r = rx.recv() => {
                    if matches!(r, Err(RecvError::Closed)) {
                        tokio::time::sleep(TICK).await;
                    }
                }
                () = tokio::time::sleep(TICK) => {}
            }
            refresh(&st);
            for r in st.robots.iter() {
                let Some(s) = store() else { continue };
                let d = s.dispatch_state(r.id);
                if !d.enabled || d.paused.is_some() {
                    continue;
                }
                if let Err(e) = step(&st, r).await {
                    tracing::warn!(robot = %r.name, %e, "jobs: 보내기 루프 오류");
                }
            }
        }
    });
}

/// 보낸 단계의 Task 상태를 원장에서 읽어 작업 단계를 맞춘다. PLC 가 거부 · 실패시키면 그 로봇을 멈춘다.
pub fn refresh(st: &AppState) {
    let Some(s) = store() else { return };
    for j in s.list().into_iter().filter(|j| !j.stage.is_end() && j.sent_any()) {
        let mut changed = false;
        let mut job = j.clone();
        for step in job.steps.iter_mut() {
            if let Some(id) = &step.task {
                let now = st.find_task(id).map(|(_, e)| e.state);
                if now != step.state {
                    step.state = now;
                    changed = true;
                }
            }
        }
        // 유실(PLC 에서 사라짐) — 단계는 그대로 두고 이유를 말한다. 사람이 PLC 를 확인한 뒤 [다시 보내기] 또는 취소.
        if let Some(i) = job.steps.iter().position(|s| s.state == Some(TaskState::Lost)) {
            let why = format!("-{} {} 유실 — PLC 에 없음: 확인 뒤 다시 보내기 또는 취소", i + 1, job.steps[i].request.task_type.to_ascii_uppercase());
            if job.wait_reason.as_deref() != Some(why.as_str()) {
                job.wait_reason = Some(why);
                changed = true;
            }
        }
        let states: Vec<Option<TaskState>> = job.steps.iter().map(|s| s.state).collect();
        let to = derive_stage(job.stage, &states);
        if to != job.stage {
            let note = match to {
                Stage::Failed => "PLC 거부 · 실패".to_string(),
                Stage::Canceled => "취소됨".to_string(),
                other => other.label().to_string(),
            };
            job.set_stage(to, note);
            if to != Stage::Wait {
                job.wait_reason = None;
            }
            changed = true;
            if to == Stage::Failed
                && let Some(robot) = job.robot
            {
                pause(st, robot, &format!("WorkId {} 실패 — 재고 · 그리퍼를 확인한 뒤 다시 켜세요", job.work_id.unwrap_or(0)));
            }
        }
        if changed && let Err(e) = s.save(&job) {
            tracing::warn!(job = %job.id, %e, "jobs: 저장 실패");
        }
    }
}

fn pause(st: &AppState, robot: u8, why: &str) {
    let Some(s) = store() else { return };
    let mut d = s.dispatch_state(robot);
    d.paused = Some(why.to_string());
    if let Err(e) = s.set_dispatch(&d) {
        tracing::warn!(%e, "jobs: 멈춤 저장 실패");
    }
    let name = st.robot(Some(robot)).map(|r| r.name.clone()).unwrap_or_default();
    let plc = st.robot(Some(robot)).map(|r| r.plc.clone()).unwrap_or_default();
    tracing::warn!(robot = %name, %why, "jobs: 보내기 멈춤");
    crate::evtlog::console("CON_SCHED", &plc, 0, 0, 0, format!("{name} 작업 보내기 멈춤: {why}"));
}

/// 한 번 판단해서 보낼 수 있으면 한 건 보낸다. 반환 = 보낸 Task 또는 기다리는 이유.
pub async fn step(st: &AppState, r: &RobotCtx) -> Result<Result<LedgerEntry, String>, ApiError> {
    let s = store().ok_or_else(|| ApiError::Internal("job store not ready".into()))?;
    let jobs = s.list();
    let Some((id, idx, mid)) = pick_next(&jobs, r.id) else { return Ok(Err("보낼 작업 없음".into())) };
    let job = jobs.into_iter().find(|j| j.id == id).ok_or_else(|| ApiError::NotFound(id.clone()))?;
    if retries().waiting(&job.id, Instant::now()).is_some() {
        return Ok(Err(job.wait_reason.clone().unwrap_or_else(|| "다시 보내기 기다림".into())));
    }
    if let Some(w) = wait_reason(st, r, &job, idx, mid) {
        note_wait(&job, &w);
        return Ok(Err(w));
    }
    match send(st, r, &job, idx).await {
        Ok(e) => {
            retries().clear(&job.id);
            Ok(Ok(e))
        }
        Err(e) => {
            let msg = e.to_string();
            // 판단과 쓰기 사이에 게이트가 닫혔으면(예: GRM 이 앞 명령을 아직 안 가져감) 실패가 아니라 기다림.
            if msg.contains("영역 대기") || !crate::ledger::ops::gate(st, r).can_submit {
                note_wait(&job, &msg);
                return Ok(Err(msg));
            }
            if transient(&e) {
                let n = retries().bump(&job.id, Instant::now());
                if n < RETRY_MAX {
                    let w = format!("PLC 쓰기 응답 없음 {n}/{RETRY_MAX} — {} s 뒤 다시: {msg}", RETRY_GAP.as_secs());
                    tracing::warn!(robot = %r.name, job = %job.id, attempt = n, %msg, "jobs: 일시 오류, 다시 보냄");
                    note_wait(&job, &w);
                    return Ok(Err(w));
                }
            }
            retries().clear(&job.id);
            let _ = s.update(&job.id, |j| {
                j.error = Some(msg.clone());
                j.wait_reason = Some(format!("거부: {msg}"));
                Ok(())
            });
            let what = if transient(&e) { format!("{RETRY_MAX} 번 연달아 쓰기 실패") } else { "보내기 거부".into() };
            pause(st, r.id, &format!("WorkId {} {what} — {msg}", job.work_id.unwrap_or(0)));
            Ok(Err(msg))
        }
    }
}

/// 기다릴 이유(없으면 보낸다). 화면 "이유" 칸의 말.
fn wait_reason(st: &AppState, r: &RobotCtx, job: &Job, idx: usize, mid: bool) -> Option<String> {
    let list = r.ledger.list();
    if list.iter().any(|e| e.state == TaskState::Submitted) {
        return Some("직전 제출 응답 기다림".into());
    }
    let g = crate::ledger::ops::gate(st, r);
    if !g.can_submit {
        return Some(format!("게이트 — {}", g.reasons.join(" · ")));
    }
    if let Some(w) = plc_hold_wait(st, r, job, idx, &list) {
        return Some(w);
    }
    let queued = list.iter().filter(|e| matches!(e.state, TaskState::Accepted | TaskState::Queued | TaskState::Running)).count();
    if queued >= DEPTH {
        return Some(if mid { "짝 DROP — PLC 실행 1 + 다음 1 참".into() } else { "PLC 실행 1 + 다음 1 참".into() });
    }
    if st.robots.len() >= 2
        && let Some(x) = job.steps[idx].request.target.as_ref().and_then(|t| crate::area::target_x(st, t))
        && let Ok(v) = crate::area::view(st, Some(r.id), Some(x))
        && let Some(c) = v.check
        && c.blocked
    {
        return Some(c.reason.unwrap_or_else(|| "영역 대기".into()));
    }
    None
}

/// 단독 DROP 인데 PLC 그리퍼 화물 데이터가 비어 있으면(HoldItem 0) 보내지 않는다 — PLC 가 DROP 030 에서 감지 ≠ 보유로
/// 알람 6017 을 낸다. 같은 로봇에 PICK 이 돌고 있으면 그 PICK 이 채울 것이라 기다리지 않는다.
fn plc_hold_wait(st: &AppState, r: &RobotCtx, job: &Job, idx: usize, list: &[LedgerEntry]) -> Option<String> {
    if st.cfg.demo || job.steps.len() != 1 || !job.steps[idx].request.task_type.eq_ignore_ascii_case("DROP") {
        return None;
    }
    let pick_live = list.iter().any(|e| {
        gr_proto::TaskType::from_code(e.plc_task.task_type) == Some(gr_proto::TaskType::Pick)
            && matches!(e.state, TaskState::Submitted | TaskState::Accepted | TaskState::Queued | TaskState::Running)
    });
    let v = crate::ledger::ops::status_view(st, r)?;
    (!pick_live && !v.task.status.hold_item).then(|| "PLC 그리퍼 화물 데이터 없음(HoldItem 0) — PLC HMI Item 화면에서 화물을 지정하면 보냄 (그대로 보내면 알람 6017)".into())
}

fn note_wait(job: &Job, why: &str) {
    if job.wait_reason.as_deref() == Some(why) {
        return;
    }
    if let Some(s) = store() {
        let _ = s.update(&job.id, |j| {
            j.wait_reason = Some(why.to_string());
            Ok(())
        });
    }
}

/// 단계 `idx` 를 지금 값으로 다시 작성해 보낸다(WorkId 는 작업 것, TaskId = 단계 번호 + 1).
async fn send(st: &AppState, r: &RobotCtx, job: &Job, idx: usize) -> Result<LedgerEntry, ApiError> {
    let s = store().ok_or_else(|| ApiError::Internal("job store not ready".into()))?;
    let work_id = job.work_id.ok_or_else(|| ApiError::Internal(format!("job {} has no WorkId", job.id)))?;
    let mut req = job.steps[idx].request.clone();
    req.robot = Some(r.id);
    let c = crate::issue::compose(st, &req)?;
    let key = gr_proto::TaskKey { work_id, task_id: idx as u32 + 1 };
    let e = crate::ledger::ops::create_and_submit_keyed(st, r, job.origin, Some(req), Some(c.params), c.task, c.pallet, true, Some(key)).await?;
    let label = format!("{work_id}-{} {}", idx + 1, job.steps[idx].request.task_type.to_ascii_uppercase());
    s.update(&job.id, |j| {
        j.steps[idx].task = Some(e.id.clone());
        j.steps[idx].state = Some(e.state);
        j.wait_reason = None;
        j.error = None;
        if j.stage == Stage::Wait {
            j.stage = Stage::Assigned;
            j.history.push(JobEvent { at: now_str(), stage: Stage::Assigned, note: format!("{label} 보냄") });
        } else {
            j.history.push(JobEvent { at: now_str(), stage: j.stage, note: format!("{label} 보냄") });
        }
        Ok(())
    })?;
    tracing::info!(robot = %r.name, task = %label, "jobs: 보냄");
    Ok(e)
}

/// 유실된 단계를 다시 보낸다 — 옛 원장 Task 는 실패로 정리하고, 그 단계를 "안 보냄" 으로 되돌린다(같은 WorkId · TaskId 로
/// 다시 나간다 — 원장은 같은 키의 **가장 최근** 것을 본다). 짝 중간이면 그 단계가 다음이다.
pub fn resend(st: &AppState, id: &str) -> Result<Job, ApiError> {
    let s = store().ok_or_else(|| ApiError::Internal("job store not ready".into()))?;
    let job = s.get(id).ok_or_else(|| ApiError::NotFound(format!("job {id}")))?;
    let lost: Vec<usize> = job.steps.iter().enumerate().filter(|(_, x)| x.state == Some(TaskState::Lost)).map(|(i, _)| i).collect();
    if lost.is_empty() {
        return Err(ApiError::Conflict("유실된 단계가 없습니다".into()));
    }
    for &i in &lost {
        if let Some(t) = &job.steps[i].task {
            crate::ledger::ops::mark_failed(st, t, Some("작업 다시 보내기 — 유실 정리".into()))?;
        }
    }
    s.update(id, |j| {
        for &i in &lost {
            j.steps[i].task = None;
            j.steps[i].state = None;
        }
        j.wait_reason = None;
        if !j.sent_any() {
            j.stage = Stage::Wait;
        }
        j.history.push(JobEvent { at: now_str(), stage: j.stage, note: format!("유실 단계 다시 보내기 ({})", lost.iter().map(|i| format!("-{}", i + 1)).collect::<Vec<_>>().join(" ")) });
        Ok(())
    })
}

/// 작업 취소. 안 보낸 작업은 바로 취소(이송 지시 중단), 보낸 작업은 PLC 삭제(같은 WorkId 뒤 Task 도 함께).
/// PICK 이 끝나 그리퍼에 화물이 있는데 DROP 을 안 보냈으면 거부 — 화물이 손에 남는다(맵의 화물 제거로).
pub async fn cancel(st: &AppState, id: &str) -> Result<Job, ApiError> {
    let s = store().ok_or_else(|| ApiError::Internal("job store not ready".into()))?;
    let job = s.get(id).ok_or_else(|| ApiError::NotFound(format!("job {id}")))?;
    if job.stage.is_end() {
        return Err(ApiError::Conflict(format!("이미 {} 된 작업입니다", job.stage.label())));
    }
    if !job.sent_any() {
        if let Some(o) = &job.transfer_order_id {
            st.stock.abort_order(o, "작업 취소(보내기 전)", true)?;
        }
        return s.update(id, |j| {
            j.set_stage(Stage::Canceled, "사람이 취소(보내기 전)");
            j.wait_reason = None;
            Ok(())
        });
    }
    let live = job.steps.iter().filter_map(|st| st.task.clone().map(|t| (t, st.state))).find(|(_, state)| !state.is_some_and(TaskState::is_terminal));
    match live {
        Some((task, _)) => {
            crate::ledger::ops::cancel(st, &task).await?;
            // 단계는 원장 이벤트가 옮긴다(Canceled → 취소). 아직 안 보낸 뒤 단계는 보내지 않게 표시만.
            s.update(id, |j| {
                j.history.push(JobEvent { at: now_str(), stage: j.stage, note: "사람이 취소 요청(PLC 삭제)".into() });
                Ok(())
            })
        }
        None => Err(ApiError::Conflict("PICK 이 끝나 그리퍼에 화물이 있습니다 — DROP 을 보내거나 맵의 로봇 메뉴에서 화물 제거".into())),
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;

    #[test]
    fn retries_count_wait_and_clear() {
        let mut r = Retries::default();
        let t0 = Instant::now();
        assert_eq!(r.waiting("a", t0), None);
        assert_eq!(r.bump("a", t0), 1);
        assert_eq!(r.waiting("a", t0 + Duration::from_secs(1)), Some(1), "간격 안에서는 기다림");
        assert_eq!(r.waiting("a", t0 + RETRY_GAP), None, "간격이 지나면 다시 보낸다");
        for n in 2..=RETRY_MAX {
            assert_eq!(r.bump("a", t0), n);
        }
        assert_eq!(r.waiting("b", t0), None, "작업마다 따로");
        r.clear("a");
        assert_eq!(r.waiting("a", t0), None);
        assert_eq!(r.bump("a", t0), 1, "성공하면 처음부터");
    }

    #[test]
    fn only_nothing_sent_errors_are_transient() {
        assert!(transient(&ApiError::OpcNotReady("timeout".into())));
        assert!(transient(&ApiError::PlcUnavailable("x".into())));
        assert!(!transient(&ApiError::BadRequest("품목 모름".into())));
        assert!(!transient(&ApiError::Conflict("StackMax".into())));
    }
}
