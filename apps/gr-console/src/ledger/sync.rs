//! Ledger ↔ PLC synchronisation (M3-B slice). M1 ships the skeleton: submission echo, position
//! tracking and ring diff are implemented here so the demo shows the full lifecycle; the M3-B agent
//! refines cursors persistence, `Lost` handling and edge cases.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use gr_proto::{StatusView, TaskKey};
use tokio::sync::broadcast;

use super::{Actor, Ledger, LedgerEntry, TaskAck, TaskState};
use crate::plc::{PlcHandle, Tier};

#[derive(Default)]
struct RingCursor {
    seen: VecDeque<TaskKey>,
}

impl RingCursor {
    /// Returns keys that are new since the last call (oldest first).
    fn diff(&mut self, ring: &[TaskKey]) -> Vec<TaskKey> {
        let new: Vec<TaskKey> = ring.iter().copied().take_while(|k| !self.seen.contains(k)).collect();
        for k in ring.iter().rev() {
            if !self.seen.contains(k) {
                self.seen.push_front(*k);
            }
        }
        while self.seen.len() > 40 {
            self.seen.pop_back();
        }
        new.into_iter().rev().collect()
    }
    fn prime(&mut self, ring: &[TaskKey]) {
        self.seen = ring.iter().copied().collect();
    }
}

pub fn spawn(ledger: Arc<Ledger>, plc: PlcHandle, echo_timeout_ms: u64) {
    tokio::spawn(async move {
        let mut rx = plc.events.subscribe();
        let mut completed = RingCursor::default();
        let mut canceled = RingCursor::default();
        let mut rejected = RingCursor::default();
        let mut primed = false;
        loop {
            match rx.recv().await {
                Ok(ev) if ev.tier == Tier::Fast && ev.dbs.iter().any(|d| d == "OPCUA") => {}
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return,
            }
            let Some(stat) = plc.decode_path("OPCUA", "STAT") else { continue };
            let Ok(view) = StatusView::from_json(&stat) else { continue };
            let keys = |v: &[gr_proto::TaskData]| -> Vec<TaskKey> { v.iter().filter(|t| !t.is_zero()).map(|t| t.key()).collect() };
            let (rc, rn, rr) = (keys(&view.task.completed), keys(&view.task.canceled), keys(&view.task.rejected));
            if !primed {
                completed.prime(&rc);
                canceled.prime(&rn);
                rejected.prime(&rr);
                primed = true;
            }
            let new_completed = completed.diff(&rc);
            let new_canceled = canceled.diff(&rn);
            let new_rejected = rejected.diff(&rr);
            if let Err(e) = apply(&ledger, &view, &new_completed, &new_canceled, &new_rejected, echo_timeout_ms) {
                tracing::warn!("ledger sync: {e}");
            }
        }
    });
}

fn apply(ledger: &Ledger, view: &StatusView, new_completed: &[TaskKey], new_canceled: &[TaskKey], new_rejected: &[TaskKey], echo_timeout_ms: u64) -> Result<(), crate::error::ApiError> {
    let now = crate::util::now_str();
    // 1. submission echo
    for e in ledger.list().into_iter().filter(|e| e.state == TaskState::Submitted) {
        let Some(h) = e.header.clone() else { continue };
        if view.res.header == h {
            let info = view.reject_info();
            let ack = TaskAck::from_reject(&info, view.res.data.first().copied().unwrap_or(0));
            let mut e2 = e.clone();
            e2.ack = Some(ack.clone());
            let to = if ack.accepted { TaskState::Accepted } else { TaskState::Rejected };
            ledger.transition(e2, to, Actor::Plc, Some(ack.reason.clone()))?;
        } else if view.locate(e.key()) != gr_proto::status::TaskLocation::Absent {
            // echo missed but the task is visible on the PLC
            ledger.transition(e, TaskState::Accepted, Actor::Plc, Some("seen on PLC without echo".into()))?;
        } else if let Some(sub) = e.submitted_at.as_deref()
            && age_ms(sub) > echo_timeout_ms
        {
            let mut e2 = e.clone();
            e2.error = Some("no echo from PLC".into());
            ledger.transition(e2, TaskState::Failed, Actor::System, Some("echo timeout".into()))?;
        }
    }
    // 2. terminal rings
    for (keys, state, note) in [(new_completed, TaskState::Completed, "completed on PLC"), (new_canceled, TaskState::Canceled, "canceled on PLC"), (new_rejected, TaskState::Rejected, "rejected on PLC")] {
        for k in keys {
            match ledger.find_by_key(*k) {
                Some(e) if !e.state.is_terminal() => {
                    ledger.transition(e, state, Actor::Plc, Some(note.into()))?;
                }
                Some(_) => {}
                None => {
                    if let Some(t) = view.task.completed.iter().chain(&view.task.canceled).chain(&view.task.rejected).find(|t| t.key() == *k) {
                        ledger.create_external(t, state)?;
                    }
                }
            }
        }
    }
    // 3. positions (now / queue) incl. external tasks
    let mut seen: Vec<TaskKey> = Vec::new();
    if !view.task.now.is_zero() {
        let k = view.task.now.key();
        seen.push(k);
        let e = match ledger.find_by_key(k) {
            Some(e) => e,
            None => ledger.create_external(&view.task.now, TaskState::Running)?,
        };
        if !e.state.is_terminal() {
            let mut e2 = e.clone();
            e2.plc = Some(super::PlcSeen { step: view.task.status.step, queue_index: Some(0), last_seen_at: now.clone() });
            if e2.state != TaskState::Running {
                ledger.transition(e2, TaskState::Running, Actor::Plc, None)?;
            } else {
                ledger.upsert_quiet(e2)?;
            }
        }
    }
    for (i, t) in view.task.queue.iter().enumerate().filter(|(_, t)| !t.is_zero()) {
        let k = t.key();
        if seen.contains(&k) {
            continue;
        }
        seen.push(k);
        let e = match ledger.find_by_key(k) {
            Some(e) => e,
            None => ledger.create_external(t, TaskState::Queued)?,
        };
        if !e.state.is_terminal() && e.state != TaskState::Running {
            let mut e2 = e.clone();
            e2.plc = Some(super::PlcSeen { step: 0, queue_index: Some(i), last_seen_at: now.clone() });
            if e2.state != TaskState::Queued {
                ledger.transition(e2, TaskState::Queued, Actor::Plc, None)?;
            } else {
                ledger.upsert_quiet(e2)?;
            }
        }
    }
    // 4. lost: queued/running entries that vanished from every array for a while
    for e in ledger.list().into_iter().filter(|e| matches!(e.state, TaskState::Queued | TaskState::Running)) {
        if seen.contains(&e.key()) {
            continue;
        }
        let stale = e.plc.as_ref().map(|p| age_ms(&p.last_seen_at) > 3000).unwrap_or(true);
        if stale && view.locate(e.key()) == gr_proto::status::TaskLocation::Absent {
            ledger.transition(e, TaskState::Lost, Actor::System, Some("disappeared from PLC arrays".into()))?;
        }
    }
    Ok(())
}

fn age_ms(rfc3339: &str) -> u64 {
    time::OffsetDateTime::parse(rfc3339, &time::format_description::well_known::Rfc3339)
        .map(|t| (time::OffsetDateTime::now_utc() - t).whole_milliseconds().max(0) as u64)
        .unwrap_or(0)
}

impl Ledger {
    /// Upsert without a state transition (position / step updates), throttled to avoid event spam:
    /// only persists when step or queue index changed.
    pub fn upsert_quiet(&self, entry: LedgerEntry) -> Result<(), crate::error::ApiError> {
        if let Some(old) = self.get(&entry.id) {
            let same = match (&old.plc, &entry.plc) {
                (Some(a), Some(b)) => a.step == b.step && a.queue_index == b.queue_index,
                _ => false,
            };
            if same {
                return Ok(());
            }
        }
        self.upsert(entry).map(|_| ())
    }
}

#[allow(dead_code)]
pub const RING_DEPTH: usize = 10;
#[allow(dead_code)]
pub const POLL_HINT: Duration = Duration::from_millis(200);
