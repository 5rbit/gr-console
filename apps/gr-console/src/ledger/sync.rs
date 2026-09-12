//! Ledger ↔ PLC synchronisation (M3-B slice).
//!
//! Every Fast-tier snapshot of the status PLC's `OPCUA.STAT` is folded into the ledger:
//! 1. submission echo (`RES.Header == entry.header`) → `Accepted` / `Rejected` with the decoded ack;
//!    a submitted task that shows up in `Now`/`Queue` without an echo gets a synthesised ack;
//!    no echo within `echo_timeout_ms` → `Failed`.
//! 2. terminal rings (`Completed` / `Canceled` / `Rejected`, depth 10, newest at [0]) are diffed
//!    against a per-ring cursor (`top_key` of the newest entry seen) that is persisted in
//!    `sync_cursor`, so a restart neither re-emits old ring entries nor misses new ones. If the
//!    cursor key is no longer in the ring, more than 10 entries arrived since the last snapshot:
//!    every ring entry is applied and active entries that vanished are marked `Lost` with an
//!    explicit "ring overflow" note.
//! 3. `Now` / `Queue[i]` positions → `Running{step}` / `Queued{slot}`; keys unknown to the ledger
//!    become `Origin::External` entries; `Lost` entries that reappear are re-promoted.
//!    `Task.Status.Complete` / `.Canceled` raised for the `Now` task end it **before** the ring
//!    catches up — the PLC clears `Now` and pushes the ring on a later cycle, and without the bits
//!    the console could only wait (and, past `lost_grace_ms`, wrongly call the task `Lost`).
//! 4. active entries that are on no PLC array for longer than `lost_grace_ms` → `Lost`.
//!
//! A cancel/complete the console asked for (`ops::cancel` / `ops::force_complete`) leaves a
//! `"delete requested"` / `"complete requested"` note in the history; the terminal transition that
//! follows is still recorded by the PLC (it is the one that ended the task) but the note says it was
//! on request, so the timeline distinguishes "operator canceled" from "PLC canceled on its own".

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gr_proto::status::TaskLocation;
use gr_proto::{StatusView, TaskData, TaskKey};
use tokio::sync::broadcast;

use super::{Actor, Ledger, LedgerEntry, PlcSeen, TaskAck, TaskState};
use crate::error::ApiError;
use crate::plc::{PlcHandle, Tier};

pub const RING_DEPTH: usize = 10;
/// How long a queued/running entry may be absent from every PLC array before it is `Lost`
/// (covers the PLC cycle between clearing `Now` and pushing the ring).
pub const LOST_GRACE_MS: u64 = 3000;
pub const OVERFLOW_NOTE: &str = "ring overflow: transitions may be missing";

/// Per-ring cursor: the key at `[0]` when the ring was last seen (`None` = the ring was empty).
/// `primed` distinguishes "never looked at this ring" (pre-existing history is not news) from
/// "looked and it was empty" (the next entry *is* news).
#[derive(Debug)]
struct RingCursor {
    ring: &'static str,
    top: Option<TaskKey>,
    primed: bool,
    dirty: bool,
}

#[derive(Debug, Default, PartialEq)]
struct RingDiff {
    /// New keys, oldest first.
    new: Vec<TaskKey>,
    /// The previous top key fell off the ring: more than `RING_DEPTH` entries since last time.
    overflow: bool,
}

impl RingCursor {
    fn new(ring: &'static str, persisted: Option<Option<TaskKey>>) -> RingCursor {
        RingCursor { ring, top: persisted.flatten(), primed: persisted.is_some(), dirty: false }
    }

    fn diff(&mut self, ring: &[TaskKey]) -> RingDiff {
        let newest = ring.first().copied();
        let d = match (self.primed, self.top) {
            // never seen this ring: prime silently (pre-existing history is not "new")
            (false, _) => RingDiff::default(),
            // ring was empty (or reset) last time: everything in it is new, nothing was missed
            (true, None) => RingDiff { new: ring.iter().rev().copied().collect(), overflow: false },
            // ring emptied (PLC reset): nothing to report, cursor follows
            (true, Some(_)) if ring.is_empty() => RingDiff::default(),
            (true, Some(top)) => match ring.iter().position(|k| *k == top) {
                Some(p) => RingDiff { new: ring[..p].iter().rev().copied().collect(), overflow: false },
                None => RingDiff { new: ring.iter().rev().copied().collect(), overflow: true },
            },
        };
        if !self.primed {
            self.primed = true;
            self.dirty = true;
        }
        if self.top != newest {
            self.top = newest;
            self.dirty = true;
        }
        d
    }
}

/// Mutable state of the sync loop (cursors + in-memory last-seen clock).
pub struct SyncState {
    completed: RingCursor,
    canceled: RingCursor,
    rejected: RingCursor,
    last_seen: HashMap<TaskKey, Instant>,
    pub echo_timeout_ms: u64,
    pub lost_grace_ms: u64,
}

impl SyncState {
    /// Loads the persisted cursors of `ledger`'s PLC.
    pub fn load(ledger: &Ledger, echo_timeout_ms: u64) -> SyncState {
        SyncState {
            completed: RingCursor::new("completed", ledger.load_cursor("completed")),
            canceled: RingCursor::new("canceled", ledger.load_cursor("canceled")),
            rejected: RingCursor::new("rejected", ledger.load_cursor("rejected")),
            last_seen: HashMap::new(),
            echo_timeout_ms,
            lost_grace_ms: LOST_GRACE_MS,
        }
    }

    fn persist(&mut self, ledger: &Ledger) {
        for c in [&mut self.completed, &mut self.canceled, &mut self.rejected] {
            if c.dirty {
                if let Err(e) = ledger.save_cursor(c.ring, c.top) {
                    tracing::warn!(ring = c.ring, "cursor persist failed: {e}");
                } else {
                    c.dirty = false;
                }
            }
        }
    }
}

fn key_str(k: TaskKey) -> String {
    format!("{}:{}", k.work_id, k.task_id)
}

fn parse_key(s: &str) -> Option<TaskKey> {
    let (w, t) = s.split_once(':')?;
    Some(TaskKey { work_id: w.trim().parse().ok()?, task_id: t.trim().parse().ok()? })
}

impl Ledger {
    /// Cursor of a terminal ring (`completed` | `canceled` | `rejected`) as persisted in `sync_cursor`:
    /// outer `None` = no row yet (ring never observed), inner `None` = the ring was empty.
    pub fn load_cursor(&self, ring: &str) -> Option<Option<TaskKey>> {
        self.db.with(|c| c.query_row("SELECT top_key FROM sync_cursor WHERE plc = ?1 AND ring = ?2", (&self.plc, ring), |r| r.get::<_, Option<String>>(0))).ok().map(|s| s.and_then(|s| parse_key(&s)))
    }

    pub fn save_cursor(&self, ring: &str, top: Option<TaskKey>) -> Result<(), ApiError> {
        self.db.with(|c| {
            c.execute("INSERT INTO sync_cursor (plc, ring, top_key) VALUES (?1, ?2, ?3) ON CONFLICT(plc, ring) DO UPDATE SET top_key = excluded.top_key", (&self.plc, ring, top.map(key_str)))
        })?;
        Ok(())
    }

    /// Upsert without a state transition (position / step updates), throttled to avoid event spam:
    /// only persists when state, step, queue index or ack presence changed.
    pub fn upsert_quiet(&self, entry: LedgerEntry) -> Result<(), ApiError> {
        if let Some(old) = self.get(&entry.id) {
            let pos = |p: &Option<PlcSeen>| p.as_ref().map(|p| (p.step, p.queue_index));
            let same = old.state == entry.state && pos(&old.plc) == pos(&entry.plc) && old.ack.is_some() == entry.ack.is_some();
            if same {
                return Ok(());
            }
        }
        self.upsert(entry).map(|_| ())
    }
}

pub fn spawn(ledger: Arc<Ledger>, plc: PlcHandle, echo_timeout_ms: u64) {
    tokio::spawn(async move {
        let mut rx = plc.events.subscribe();
        let mut st = SyncState::load(&ledger, echo_timeout_ms);
        tracing::info!(plc = plc.name(), completed = ?st.completed.top, canceled = ?st.canceled.top, rejected = ?st.rejected.top, "ledger sync: cursors restored");
        loop {
            match rx.recv().await {
                Ok(ev) if ev.tier == Tier::Fast && ev.dbs.iter().any(|d| d == "OPCUA") => {}
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return,
            }
            let Some(stat) = plc.decode_path("OPCUA", "STAT") else { continue };
            let Ok(view) = StatusView::from_json(&stat) else { continue };
            if let Err(e) = apply(&ledger, &view, &mut st) {
                tracing::warn!("ledger sync: {e}");
            }
        }
    });
}

fn ring_keys(v: &[TaskData]) -> Vec<TaskKey> {
    v.iter().filter(|t| !t.is_zero()).map(|t| t.key()).collect()
}

pub const DELETE_REQUESTED: &str = "delete requested";
pub const COMPLETE_REQUESTED: &str = "complete requested";

/// The last history line is a pending console request (`delete requested` / `complete requested`).
fn last_request(e: &LedgerEntry) -> Option<&str> {
    // a cascade note is `"delete requested (cascade from task N)"` — same request, same attribution
    e.history.iter().rev().find(|t| t.from == Some(t.to)).and_then(|t| t.note.as_deref()).and_then(|n| [DELETE_REQUESTED, COMPLETE_REQUESTED].into_iter().find(|k| n.starts_with(k)))
}

/// Note for a PLC-side terminal transition, attributing it to the console request that asked for it.
fn terminal_note(e: &LedgerEntry, state: TaskState, plc_note: &str) -> String {
    match (state, last_request(e)) {
        (TaskState::Canceled, Some(DELETE_REQUESTED)) => format!("{plc_note} (console request)"),
        (TaskState::Completed, Some(COMPLETE_REQUESTED)) => format!("{plc_note} (console request)"),
        _ => plc_note.to_string(),
    }
}

fn synth_ack(accepted: bool, reason: &str) -> TaskAck {
    TaskAck { accepted, code: if accepted { gr_proto::VALID_TASK_DATA } else { 0 }, reason: reason.into(), reject_bits: 0, at: crate::util::now_str() }
}

/// Folds one status snapshot into the ledger.
pub fn apply(ledger: &Ledger, view: &StatusView, st: &mut SyncState) -> Result<(), ApiError> {
    let now = crate::util::now_str();
    let dc = st.completed.diff(&ring_keys(&view.task.completed));
    let dn = st.canceled.diff(&ring_keys(&view.task.canceled));
    let dr = st.rejected.diff(&ring_keys(&view.task.rejected));
    let overflow = dc.overflow || dn.overflow || dr.overflow;
    if !(dc.new.is_empty() && dn.new.is_empty() && dr.new.is_empty()) {
        tracing::debug!(completed = ?dc.new, canceled = ?dn.new, rejected = ?dr.new, "ledger sync: new ring entries");
    }
    if overflow {
        tracing::warn!(completed = dc.overflow, canceled = dn.overflow, rejected = dr.overflow, "ledger sync: ring overflow — more than {RING_DEPTH} entries since the last snapshot");
    }

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
            continue;
        }
        // echo missed (or already overwritten) but the task is visible on the PLC
        let (to, ack, note) = match view.locate(e.key()) {
            TaskLocation::Now | TaskLocation::Queue(_) => (TaskState::Accepted, synth_ack(true, "accepted (seen on PLC)"), "seen on PLC without echo"),
            TaskLocation::Completed(_) => (TaskState::Completed, synth_ack(true, "accepted (seen on PLC)"), "completed on PLC without echo"),
            TaskLocation::Canceled(_) => (TaskState::Canceled, synth_ack(true, "accepted (seen on PLC)"), "canceled on PLC without echo"),
            TaskLocation::Rejected(_) => (TaskState::Rejected, synth_ack(false, "rejected (seen in Rejected ring)"), "rejected on PLC without echo"),
            TaskLocation::Absent => {
                if let Some(sub) = e.submitted_at.as_deref()
                    && age_ms(sub) > st.echo_timeout_ms
                {
                    let mut e2 = e.clone();
                    e2.error = Some(format!("no echo from PLC within {} ms", st.echo_timeout_ms));
                    ledger.transition(e2, TaskState::Failed, Actor::System, Some("echo timeout".into()))?;
                }
                continue;
            }
        };
        let mut e2 = e.clone();
        if e2.ack.is_none() {
            e2.ack = Some(ack);
        }
        ledger.transition(e2, to, Actor::Plc, Some(note.into()))?;
    }

    // 2. terminal rings (new entries since the cursor)
    for (keys, state, note) in [(&dc.new, TaskState::Completed, "completed on PLC"), (&dn.new, TaskState::Canceled, "canceled on PLC"), (&dr.new, TaskState::Rejected, "rejected on PLC")] {
        for k in keys {
            let found = ledger.find_by_key(*k);
            tracing::debug!(key = ?k, to = state.as_str(), entry = ?found.as_ref().map(|e| (e.seq, e.state)), "ledger sync: ring entry");
            match found {
                Some(e) if !e.state.is_terminal() => {
                    let note = terminal_note(&e, state, note);
                    let mut e2 = e;
                    if state == TaskState::Rejected && e2.ack.is_none() {
                        // the echo was missed but the PLC did answer: keep the (synthesised) verdict
                        e2.ack = Some(synth_ack(false, "rejected (seen in Rejected ring)"));
                    }
                    ledger.transition(e2, state, Actor::Plc, Some(note))?;
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
    let tick = Instant::now();
    if !view.task.now.is_zero() {
        let k = view.task.now.key();
        seen.push(k);
        st.last_seen.insert(k, tick);
        let e = match ledger.find_by_key(k) {
            Some(e) => e,
            None => ledger.create_external(&view.task.now, TaskState::Running)?,
        };
        // `Task.Status.Canceled` / `.Complete` describe the `Now` task: the PLC raises the bit first and
        // moves the task to the ring on a later cycle. Ending the entry here (not after the ring) is
        // what makes a console-requested Delete/Complete visibly *answered* within one snapshot.
        let bits = &view.task.status;
        let ended = if bits.canceled {
            Some((TaskState::Canceled, "Task.Status.Canceled"))
        } else if bits.complete && !bits.inprogress {
            Some((TaskState::Completed, "Task.Status.Complete"))
        } else {
            None
        };
        if let Some((state, plc_note)) = ended
            && !e.state.is_terminal()
        {
            let note = terminal_note(&e, state, plc_note);
            let mut e2 = e.clone();
            e2.plc = Some(PlcSeen { step: view.task.status.step, queue_index: Some(0), last_seen_at: now.clone() });
            ledger.transition(e2, state, Actor::Plc, Some(note))?;
        } else if !e.state.is_terminal() {
            let mut e2 = e.clone();
            e2.plc = Some(PlcSeen { step: view.task.status.step, queue_index: Some(0), last_seen_at: now.clone() });
            if e2.state != TaskState::Running {
                let note = (e.state == TaskState::Lost).then(|| "re-seen on PLC".to_string());
                ledger.transition(e2, TaskState::Running, Actor::Plc, note)?;
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
        st.last_seen.insert(k, tick);
        let e = match ledger.find_by_key(k) {
            Some(e) => e,
            None => ledger.create_external(t, TaskState::Queued)?,
        };
        if !e.state.is_terminal() && e.state != TaskState::Running {
            let mut e2 = e.clone();
            e2.plc = Some(PlcSeen { step: 0, queue_index: Some(i), last_seen_at: now.clone() });
            if e2.state != TaskState::Queued {
                let note = (e.state == TaskState::Lost).then(|| "re-seen on PLC".to_string());
                ledger.transition(e2, TaskState::Queued, Actor::Plc, note)?;
            } else {
                ledger.upsert_quiet(e2)?;
            }
        }
    }

    // 4. active entries that are on no array: missed ring transition → terminal, else Lost after grace
    let grace = Duration::from_millis(st.lost_grace_ms);
    for e in ledger.list().into_iter().filter(|e| matches!(e.state, TaskState::Accepted | TaskState::Queued | TaskState::Running)) {
        let k = e.key();
        if seen.contains(&k) {
            continue;
        }
        let loc = view.locate(k);
        tracing::debug!(seq = e.seq, state = e.state.as_str(), ?loc, "ledger sync: active entry not in Now/Queue");
        match loc {
            TaskLocation::Now | TaskLocation::Queue(_) => {}
            TaskLocation::Completed(_) => {
                let note = terminal_note(&e, TaskState::Completed, "found in Completed ring");
                ledger.transition(e, TaskState::Completed, Actor::Plc, Some(note))?;
            }
            TaskLocation::Canceled(_) => {
                let note = terminal_note(&e, TaskState::Canceled, "found in Canceled ring");
                ledger.transition(e, TaskState::Canceled, Actor::Plc, Some(note))?;
            }
            TaskLocation::Rejected(_) => {
                let mut e2 = e;
                if e2.ack.is_none() {
                    e2.ack = Some(synth_ack(false, "rejected (seen in Rejected ring)"));
                }
                ledger.transition(e2, TaskState::Rejected, Actor::Plc, Some("found in Rejected ring".into()))?;
            }
            TaskLocation::Absent => {
                if e.state == TaskState::Accepted {
                    // accepted by echo but never materialised in the queue
                    if !overflow && age_ms(&e.state_at) <= st.echo_timeout_ms.saturating_mul(2).max(st.lost_grace_ms) {
                        continue;
                    }
                    let note = if overflow { OVERFLOW_NOTE } else { "accepted but never appeared on PLC" };
                    ledger.transition(e, TaskState::Lost, Actor::System, Some(note.into()))?;
                    continue;
                }
                let stale = overflow || st.last_seen.get(&k).map(|t| t.elapsed() >= grace).unwrap_or(true);
                if stale {
                    st.last_seen.remove(&k);
                    let note = if overflow { OVERFLOW_NOTE } else { "disappeared from PLC arrays" };
                    ledger.transition(e, TaskState::Lost, Actor::System, Some(note.into()))?;
                }
            }
        }
    }

    st.persist(ledger);
    Ok(())
}

fn age_ms(rfc3339: &str) -> u64 {
    super::parse_rfc3339(rfc3339).map(|t| (time::OffsetDateTime::now_utc() - t).whole_milliseconds().max(0) as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::ledger::Origin;
    use gr_proto::{Header, StatusView, TaskData, TaskKey};

    fn key(w: u32, t: u32) -> TaskKey {
        TaskKey { work_id: w, task_id: t }
    }

    fn task(k: TaskKey) -> TaskData {
        TaskData { work_id: k.work_id, task_id: k.task_id, task_type: 0x41, ..Default::default() }
    }

    fn header(seq: u16) -> Header {
        Header { protocol: 1, cmd_id: 1, cmd: 0x41, src: 5000, dst: 4002, seq }
    }

    /// Empty PLC status with the real array shapes (Queue 4, rings 10).
    fn view() -> StatusView {
        let mut v = StatusView::default();
        v.task.queue = vec![TaskData::default(); 4];
        v.task.completed = vec![TaskData::default(); RING_DEPTH];
        v.task.canceled = vec![TaskData::default(); RING_DEPTH];
        v.task.rejected = vec![TaskData::default(); RING_DEPTH];
        v.res.data = vec![0; 16];
        v
    }

    fn push_ring(ring: &mut [TaskData], t: TaskData) {
        ring.rotate_right(1);
        ring[0] = t;
    }

    fn echo(v: &mut StatusView, h: Header, data: &[u8]) {
        v.res.header = h;
        v.res.data = data.to_vec();
        v.res.data.resize(16, 0);
    }

    const ACCEPT: [u8; 7] = [0, 0, 1, 0, 0, 1, 1];
    const REJECT_429: [u8; 7] = [0x04, 0x01, 0xAD, 0, 0, 1, 1];

    fn fresh() -> (Db, Arc<Ledger>, SyncState) {
        let db = Db::open_memory().unwrap();
        let ledger = Ledger::new(db.clone(), "GR2", None).unwrap();
        let mut st = SyncState::load(&ledger, 5000);
        st.lost_grace_ms = 0;
        (db, ledger, st)
    }

    /// Creates + "submits" an entry without a command port.
    fn submitted(ledger: &Ledger, h: Header) -> LedgerEntry {
        let e = ledger.create(Origin::Console, None, None, task(key(0, 0))).unwrap();
        let mut e = e;
        e.header = Some(h);
        e.submitted_at = Some(crate::util::now_str());
        ledger.transition(e, TaskState::Submitted, Actor::Ui, None).unwrap()
    }

    #[test]
    fn ring_cursor_diff_prime_new_and_overflow() {
        let mut c = RingCursor::new("completed", None);
        // first sight primes silently
        assert_eq!(c.diff(&[key(1, 3), key(1, 2), key(1, 1)]), RingDiff::default());
        assert_eq!(c.top, Some(key(1, 3)));
        assert!(c.primed && c.dirty);
        // two new entries, oldest first
        let d = c.diff(&[key(1, 5), key(1, 4), key(1, 3), key(1, 2)]);
        assert_eq!(d, RingDiff { new: vec![key(1, 4), key(1, 5)], overflow: false });
        // nothing new
        assert_eq!(c.diff(&[key(1, 5), key(1, 4)]), RingDiff::default());
        // top fell off → overflow, all applied
        let d = c.diff(&[key(2, 2), key(2, 1)]);
        assert!(d.overflow);
        assert_eq!(d.new, vec![key(2, 1), key(2, 2)]);
        // ring emptied (PLC reset): no news, cursor follows; the next entry is news without overflow
        assert_eq!(c.diff(&[]), RingDiff::default());
        assert_eq!(c.top, None);
        assert_eq!(c.diff(&[key(3, 1)]), RingDiff { new: vec![key(3, 1)], overflow: false });

        // primed on an empty ring (persisted NULL): the first entry is news
        let mut e = RingCursor::new("canceled", Some(None));
        assert_eq!(e.diff(&[]), RingDiff::default());
        assert_eq!(e.diff(&[key(4, 2), key(4, 1)]), RingDiff { new: vec![key(4, 1), key(4, 2)], overflow: false });
    }

    #[test]
    fn accept_echo_then_queue_now_completed() {
        let (_db, ledger, mut st) = fresh();
        let e = submitted(&ledger, header(7));
        let k = e.key();
        let mut v = view();
        apply(&ledger, &v, &mut st).unwrap(); // prime rings, nothing else
        assert_eq!(ledger.get(&e.id).unwrap().state, TaskState::Submitted);

        echo(&mut v, header(7), &ACCEPT);
        v.task.queue[0] = task(k);
        apply(&ledger, &v, &mut st).unwrap();
        let e1 = ledger.get(&e.id).unwrap();
        assert_eq!(e1.state, TaskState::Queued);
        let ack = e1.ack.as_ref().unwrap();
        assert!(ack.accepted);
        assert_eq!(ack.code, 1);
        assert_eq!(e1.plc.as_ref().unwrap().queue_index, Some(0));
        assert!(e1.history.iter().any(|t| t.to == TaskState::Accepted && t.by == Actor::Plc));

        v.task.queue[0] = TaskData::default();
        v.task.now = task(k);
        v.task.status.step = 300;
        apply(&ledger, &v, &mut st).unwrap();
        let e2 = ledger.get(&e.id).unwrap();
        assert_eq!(e2.state, TaskState::Running);
        assert_eq!(e2.plc.as_ref().unwrap().step, 300);

        v.task.now = TaskData::default();
        v.task.status.step = 0;
        push_ring(&mut v.task.completed, task(k));
        apply(&ledger, &v, &mut st).unwrap();
        let e3 = ledger.get(&e.id).unwrap();
        assert_eq!(e3.state, TaskState::Completed);
        assert!(e3.ended_at.is_some());
        // the ring diff (not the step-4 fallback) must be what completes it
        assert_eq!(e3.history.last().unwrap().note.as_deref(), Some("completed on PLC"));
        assert_eq!(ledger.load_cursor("completed"), Some(Some(k)));
        // rings that were empty when first seen are persisted as "primed, empty"
        assert_eq!(ledger.load_cursor("canceled"), Some(None));
    }

    #[test]
    fn reject_echo() {
        let (_db, ledger, mut st) = fresh();
        let e = submitted(&ledger, header(9));
        let mut v = view();
        echo(&mut v, header(9), &REJECT_429);
        push_ring(&mut v.task.rejected, task(e.key()));
        apply(&ledger, &v, &mut st).unwrap();
        let e1 = ledger.get(&e.id).unwrap();
        assert_eq!(e1.state, TaskState::Rejected);
        let ack = e1.ack.as_ref().unwrap();
        assert!(!ack.accepted);
        assert_eq!(ack.code, 429);
        assert_eq!(ack.reject_bits, 0x04);
        // a second snapshot with the same ring does not re-transition
        let n = e1.history.len();
        apply(&ledger, &v, &mut st).unwrap();
        assert_eq!(ledger.get(&e.id).unwrap().history.len(), n);
    }

    #[test]
    fn seen_without_echo_synthesises_ack_and_rejected_ring_keeps_verdict() {
        let (_db, ledger, mut st) = fresh();
        let a = submitted(&ledger, header(11));
        let mut v = view();
        v.task.queue[1] = task(a.key());
        apply(&ledger, &v, &mut st).unwrap();
        let a1 = ledger.get(&a.id).unwrap();
        assert_eq!(a1.state, TaskState::Queued);
        assert_eq!(a1.ack.as_ref().unwrap().reason, "accepted (seen on PLC)");
        assert_eq!(a1.plc.as_ref().unwrap().queue_index, Some(1));

        // a different submission rejected without echo (only visible in the ring)
        let b = submitted(&ledger, header(12));
        push_ring(&mut v.task.rejected, task(b.key()));
        apply(&ledger, &v, &mut st).unwrap();
        let b1 = ledger.get(&b.id).unwrap();
        assert_eq!(b1.state, TaskState::Rejected);
        assert!(!b1.ack.as_ref().unwrap().accepted);
    }

    #[test]
    fn echo_timeout_fails_the_submission() {
        let (_db, ledger, mut st) = fresh();
        let e = submitted(&ledger, header(13));
        let mut e2 = ledger.get(&e.id).unwrap();
        e2.submitted_at = Some("2020-01-01T00:00:00Z".into());
        ledger.upsert(e2).unwrap();
        apply(&ledger, &view(), &mut st).unwrap();
        let e3 = ledger.get(&e.id).unwrap();
        assert_eq!(e3.state, TaskState::Failed);
        assert!(e3.error.as_deref().unwrap().contains("no echo"));
    }

    #[test]
    fn external_tasks_are_created_from_arrays_and_rings() {
        let (_db, ledger, mut st) = fresh();
        let mut v = view();
        apply(&ledger, &v, &mut st).unwrap();
        let mut ext = task(key(77, 1));
        ext.cell.id = 105;
        ext.position = [1.0, 2.0, 3.0, 4.0];
        v.task.now = ext.clone();
        v.task.queue[0] = task(key(77, 2));
        push_ring(&mut v.task.completed, task(key(77, 0)));
        apply(&ledger, &v, &mut st).unwrap();
        let running = ledger.find_by_key(key(77, 1)).unwrap();
        assert_eq!(running.state, TaskState::Running);
        assert_eq!(running.origin, Origin::External);
        assert!(running.request.is_none());
        assert_eq!(running.position, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(running.plc_task.cell.id, 105);
        assert_eq!(ledger.find_by_key(key(77, 2)).unwrap().state, TaskState::Queued);
        let done = ledger.find_by_key(key(77, 0)).unwrap();
        assert_eq!(done.state, TaskState::Completed);
        assert_eq!(done.origin, Origin::External);
    }

    #[test]
    fn lost_and_repromote() {
        let (_db, ledger, mut st) = fresh();
        let mut v = view();
        apply(&ledger, &v, &mut st).unwrap();
        v.task.queue[0] = task(key(5, 1));
        apply(&ledger, &v, &mut st).unwrap();
        let e = ledger.find_by_key(key(5, 1)).unwrap();
        assert_eq!(e.state, TaskState::Queued);
        // vanishes from every array (grace 0) → Lost
        v.task.queue[0] = TaskData::default();
        apply(&ledger, &v, &mut st).unwrap();
        let e1 = ledger.get(&e.id).unwrap();
        assert_eq!(e1.state, TaskState::Lost);
        assert_eq!(e1.history.last().unwrap().by, Actor::System);
        // shows up again in Now → Running with a note
        v.task.now = task(key(5, 1));
        apply(&ledger, &v, &mut st).unwrap();
        let e2 = ledger.get(&e.id).unwrap();
        assert_eq!(e2.state, TaskState::Running);
        assert_eq!(e2.history.last().unwrap().note.as_deref(), Some("re-seen on PLC"));
        // and finally lands in the ring even after being Lost in between
        v.task.now = TaskData::default();
        push_ring(&mut v.task.canceled, task(key(5, 1)));
        apply(&ledger, &v, &mut st).unwrap();
        assert_eq!(ledger.get(&e.id).unwrap().state, TaskState::Canceled);
    }

    /// Pushes a console request note the way `ops::cancel` / `ops::force_complete` do.
    fn request(ledger: &Ledger, id: &str, note: &str) {
        let mut e = ledger.get(id).unwrap();
        e.history.push(crate::ledger::Transition { from: Some(e.state), to: e.state, at: crate::util::now_str(), by: Actor::Ui, note: Some(note.into()) });
        ledger.upsert(e).unwrap();
    }

    #[test]
    fn status_canceled_bit_ends_the_now_task_before_the_ring() {
        let (_db, ledger, mut st) = fresh();
        st.lost_grace_ms = 60_000;
        let mut v = view();
        apply(&ledger, &v, &mut st).unwrap();
        v.task.now = task(key(8, 1));
        v.task.status.inprogress = true;
        apply(&ledger, &v, &mut st).unwrap();
        let e = ledger.find_by_key(key(8, 1)).unwrap();
        assert_eq!(e.state, TaskState::Running);
        // the console asked for a Delete; the PLC answers with the bit while Now still holds the task
        request(&ledger, &e.id, DELETE_REQUESTED);
        v.task.status.inprogress = false;
        v.task.status.canceled = true;
        apply(&ledger, &v, &mut st).unwrap();
        let e1 = ledger.get(&e.id).unwrap();
        assert_eq!(e1.state, TaskState::Canceled);
        assert_eq!(e1.history.last().unwrap().by, Actor::Plc);
        assert_eq!(e1.history.last().unwrap().note.as_deref(), Some("Task.Status.Canceled (console request)"));
        // the ring catching up later neither re-transitions nor duplicates history
        let n = e1.history.len();
        v.task.now = TaskData::default();
        v.task.status.canceled = false;
        push_ring(&mut v.task.canceled, task(key(8, 1)));
        apply(&ledger, &v, &mut st).unwrap();
        let e2 = ledger.get(&e.id).unwrap();
        assert_eq!(e2.state, TaskState::Canceled);
        assert_eq!(e2.history.len(), n);
    }

    #[test]
    fn status_complete_bit_ends_the_now_task_and_plc_cancel_is_not_attributed() {
        let (_db, ledger, mut st) = fresh();
        let mut v = view();
        apply(&ledger, &v, &mut st).unwrap();
        v.task.now = task(key(8, 2));
        v.task.status.inprogress = true;
        apply(&ledger, &v, &mut st).unwrap();
        let e = ledger.find_by_key(key(8, 2)).unwrap();
        // Complete while Inprogress is still up is not an end (bits mid-update): stays Running
        v.task.status.complete = true;
        apply(&ledger, &v, &mut st).unwrap();
        assert_eq!(ledger.get(&e.id).unwrap().state, TaskState::Running);
        v.task.status.inprogress = false;
        apply(&ledger, &v, &mut st).unwrap();
        let e1 = ledger.get(&e.id).unwrap();
        assert_eq!(e1.state, TaskState::Completed);
        assert_eq!(e1.history.last().unwrap().note.as_deref(), Some("Task.Status.Complete"));

        // a PLC-side cancel without a console request keeps the plain note
        v.task.now = task(key(8, 3));
        v.task.status.complete = false;
        v.task.status.inprogress = true;
        apply(&ledger, &v, &mut st).unwrap();
        v.task.now = TaskData::default();
        v.task.status.inprogress = false;
        push_ring(&mut v.task.canceled, task(key(8, 3)));
        apply(&ledger, &v, &mut st).unwrap();
        let c = ledger.find_by_key(key(8, 3)).unwrap();
        assert_eq!(c.state, TaskState::Canceled);
        assert_eq!(c.history.last().unwrap().note.as_deref(), Some("canceled on PLC"));
    }

    #[test]
    fn ring_cancel_after_console_request_is_attributed() {
        let (_db, ledger, mut st) = fresh();
        let mut v = view();
        apply(&ledger, &v, &mut st).unwrap();
        v.task.queue[0] = task(key(8, 4));
        apply(&ledger, &v, &mut st).unwrap();
        let e = ledger.find_by_key(key(8, 4)).unwrap();
        request(&ledger, &e.id, DELETE_REQUESTED);
        v.task.queue[0] = TaskData::default();
        push_ring(&mut v.task.canceled, task(key(8, 4)));
        apply(&ledger, &v, &mut st).unwrap();
        let e1 = ledger.get(&e.id).unwrap();
        assert_eq!(e1.state, TaskState::Canceled);
        assert_eq!(e1.history.last().unwrap().note.as_deref(), Some("canceled on PLC (console request)"));
    }

    #[test]
    fn lost_respects_grace() {
        let (_db, ledger, mut st) = fresh();
        st.lost_grace_ms = 60_000;
        let mut v = view();
        apply(&ledger, &v, &mut st).unwrap();
        v.task.now = task(key(6, 1));
        apply(&ledger, &v, &mut st).unwrap();
        v.task.now = TaskData::default();
        apply(&ledger, &v, &mut st).unwrap();
        // still Running: the PLC gets a grace period to push the ring
        assert_eq!(ledger.find_by_key(key(6, 1)).unwrap().state, TaskState::Running);
        push_ring(&mut v.task.completed, task(key(6, 1)));
        apply(&ledger, &v, &mut st).unwrap();
        assert_eq!(ledger.find_by_key(key(6, 1)).unwrap().state, TaskState::Completed);
    }

    #[test]
    fn ring_overflow_marks_vanished_entries() {
        let (_db, ledger, mut st) = fresh();
        let mut v = view();
        push_ring(&mut v.task.completed, task(key(9, 0)));
        apply(&ledger, &v, &mut st).unwrap(); // cursor = 9:0
        v.task.queue[0] = task(key(9, 1));
        apply(&ledger, &v, &mut st).unwrap();
        let queued = ledger.find_by_key(key(9, 1)).unwrap();
        // 12 completions happen between two snapshots: 9:1 and the cursor both fall off the ring
        v.task.queue[0] = TaskData::default();
        for i in 20..32 {
            push_ring(&mut v.task.completed, task(key(9, i)));
        }
        apply(&ledger, &v, &mut st).unwrap();
        let q1 = ledger.get(&queued.id).unwrap();
        assert_eq!(q1.state, TaskState::Lost);
        assert_eq!(q1.history.last().unwrap().note.as_deref(), Some(OVERFLOW_NOTE));
        // all 10 visible ring entries were registered as external completions
        for i in 22..32 {
            assert_eq!(ledger.find_by_key(key(9, i)).unwrap().state, TaskState::Completed);
        }
        assert!(ledger.find_by_key(key(9, 21)).is_none());
        assert_eq!(ledger.load_cursor("completed"), Some(Some(key(9, 31))));
    }

    #[test]
    fn cursor_survives_restart() {
        let db = Db::open_memory().unwrap();
        let ledger = Ledger::new(db.clone(), "GR2", None).unwrap();
        let mut st = SyncState::load(&ledger, 5000);
        let mut v = view();
        push_ring(&mut v.task.completed, task(key(3, 1)));
        apply(&ledger, &v, &mut st).unwrap();
        push_ring(&mut v.task.completed, task(key(3, 2)));
        push_ring(&mut v.task.rejected, task(key(3, 3)));
        apply(&ledger, &v, &mut st).unwrap();
        assert_eq!(ledger.find_by_key(key(3, 2)).unwrap().state, TaskState::Completed);
        let before = ledger.query(None, None, None, 100, 0).unwrap().1;

        // "restart": a second Ledger on the same sqlite, fresh SyncState
        let ledger2 = Ledger::new(db.clone(), "GR2", None).unwrap();
        let mut st2 = SyncState::load(&ledger2, 5000);
        assert_eq!(st2.completed.top, Some(key(3, 2)));
        assert_eq!(st2.rejected.top, Some(key(3, 3)));
        // same ring → nothing re-emitted
        apply(&ledger2, &v, &mut st2).unwrap();
        assert_eq!(ledger2.query(None, None, None, 100, 0).unwrap().1, before);
        // one more completion → exactly one new entry
        push_ring(&mut v.task.completed, task(key(3, 4)));
        apply(&ledger2, &v, &mut st2).unwrap();
        assert_eq!(ledger2.query(None, None, None, 100, 0).unwrap().1, before + 1);
        assert_eq!(ledger2.find_by_key(key(3, 4)).unwrap().state, TaskState::Completed);
        assert_eq!(ledger2.load_cursor("completed"), Some(Some(key(3, 4))));

        // a third start on a PLC whose rings were wiped: nothing re-emitted, no overflow noise
        let ledger3 = Ledger::new(db.clone(), "GR2", None).unwrap();
        let mut st3 = SyncState::load(&ledger3, 5000);
        let mut wiped = view();
        apply(&ledger3, &wiped, &mut st3).unwrap();
        assert_eq!(ledger3.load_cursor("completed"), Some(None));
        push_ring(&mut wiped.task.completed, task(key(3, 9)));
        apply(&ledger3, &wiped, &mut st3).unwrap();
        let e = ledger3.find_by_key(key(3, 9)).unwrap();
        assert_eq!(e.state, TaskState::Completed);
        assert_ne!(e.history.last().unwrap().note.as_deref(), Some(OVERFLOW_NOTE));
    }

    #[test]
    fn quiet_upsert_skips_unchanged_positions() {
        let (_db, ledger, mut st) = fresh();
        let mut rx = ledger.events.subscribe();
        let mut v = view();
        v.task.now = task(key(8, 1));
        v.task.status.step = 100;
        apply(&ledger, &v, &mut st).unwrap();
        let mut n = 0;
        while rx.try_recv().is_ok() {
            n += 1;
        }
        assert!(n >= 1);
        apply(&ledger, &v, &mut st).unwrap();
        assert!(rx.try_recv().is_err(), "no event when nothing changed");
        v.task.status.step = 200;
        apply(&ledger, &v, &mut st).unwrap();
        assert!(rx.try_recv().is_ok(), "step change is broadcast");
    }

    #[test]
    fn remove_and_stats_and_filters() {
        let (_db, ledger, mut st) = fresh();
        let mut v = view();
        apply(&ledger, &v, &mut st).unwrap();
        push_ring(&mut v.task.completed, task(key(4, 1)));
        push_ring(&mut v.task.rejected, task(key(4, 2)));
        v.task.queue[0] = task(key(4, 3));
        apply(&ledger, &v, &mut st).unwrap();
        let s = ledger.stats().unwrap();
        assert_eq!(s.total, 3);
        assert_eq!(s.active, 1);
        assert_eq!(s.completed_today, 1);
        assert_eq!(s.rejected_today, 1);
        assert_eq!(s.by_state.get("queued"), Some(&1));

        let (items, total) = ledger.query_filtered(super::super::QueryFilter { origin: Some(Origin::External), states: Some(vec![TaskState::Completed]), limit: 10, ..Default::default() }).unwrap();
        assert_eq!(total, 1);
        assert_eq!(items[0].task_id, 1);
        let (_, total) = ledger.query_filtered(super::super::QueryFilter { since: Some("2999-01-01T00:00:00Z".into()), limit: 10, ..Default::default() }).unwrap();
        assert_eq!(total, 0);

        let done = ledger.find_by_key(key(4, 1)).unwrap();
        ledger.remove(&done.id).unwrap();
        assert!(ledger.get(&done.id).is_none());
        let queued = ledger.find_by_key(key(4, 3)).unwrap();
        assert!(matches!(ledger.remove(&queued.id), Err(ApiError::Conflict(_))));
    }
}
