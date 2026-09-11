//! `CmdWriter`: session task with reconnect, node-map lifecycle and batched writes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use opcua::client::Session;
use opcua::types::{
    AttributeId, DataValue, NumericRange, ReadValueId, TimestampsToReturn, WriteValue,
};
use tokio::sync::{Notify, watch};

use crate::browse;
use crate::connect::{self, map_err, with_timeout};
use crate::nodemap::{NodeMap, NodeMapInfo};
use crate::value::{coerce, from_variant};
use crate::{HeaderWire, MemberValue, OpcError, OpcState, OpcUaConfig, PlcValue, TaskOp};

const BACKOFF_MIN_MS: u64 = 1_000;
const BACKOFF_MAX_MS: u64 = 30_000;

/// Header field paths in write order.
pub const HEADER_PATHS: [&str; 6] = [
    "Header.Protocol",
    "Header.CMD_ID",
    "Header.CMD",
    "Header.SRC",
    "Header.DST",
    "Header.SEQ",
];

/// In-memory `cmd_id` / `seq` counters; both skip 0 and wrap.
#[derive(Debug, Clone)]
pub struct Counters {
    cmd_id: u8,
    seq: u16,
}

impl Counters {
    /// Seed from wall-clock time so a restarted process does not repeat recent headers.
    pub fn seeded_now() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let secs = (nanos / 1_000_000_000) as u64;
        let millis = (nanos / 1_000_000) as u64;
        Self::with(((secs % 255) + 1) as u8, ((millis % 65_535) + 1) as u16)
    }

    /// Start so that the *next* values returned are the ones after `cmd_id` / `seq`.
    pub fn with(cmd_id: u8, seq: u16) -> Self {
        Self { cmd_id, seq }
    }

    /// Next cmd_id in 1..=255 (0 is never returned).
    pub fn next_cmd_id(&mut self) -> u8 {
        self.cmd_id = if self.cmd_id == u8::MAX { 1 } else { self.cmd_id + 1 };
        self.cmd_id
    }

    /// Next seq in 1..=65535 (0 is never returned).
    pub fn next_seq(&mut self) -> u16 {
        self.seq = if self.seq == u16::MAX { 1 } else { self.seq + 1 };
        self.seq
    }
}

struct Active {
    session: Arc<Session>,
}

struct Inner {
    cfg: OpcUaConfig,
    state_tx: watch::Sender<OpcState>,
    active: RwLock<Option<Active>>,
    map: RwLock<Option<Arc<NodeMap>>>,
    endpoints: RwLock<Vec<String>>,
    counters: Mutex<Counters>,
    submit: tokio::sync::Mutex<()>,
    shutdown: AtomicBool,
    notify: Notify,
}

impl Inner {
    fn set_state(&self, s: OpcState) {
        tracing::debug!(state = ?s, "opcua state");
        let _ = self.state_tx.send(s);
    }

    fn session(&self) -> Result<Arc<Session>, OpcError> {
        self.active
            .read()
            .ok()
            .and_then(|g| g.as_ref().map(|a| a.session.clone()))
            .ok_or(OpcError::NotReady)
    }

    fn map(&self) -> Result<Arc<NodeMap>, OpcError> {
        self.map
            .read()
            .ok()
            .and_then(|g| g.clone())
            .ok_or(OpcError::NotReady)
    }

    fn set_map(&self, map: Arc<NodeMap>) {
        if let Ok(mut g) = self.map.write() {
            *g = Some(map);
        }
    }

    fn set_active(&self, a: Option<Active>) {
        if let Ok(mut g) = self.active.write() {
            *g = a;
        }
    }

    fn ready_state(&self, map: &NodeMap) -> OpcState {
        OpcState::Ready {
            node_count: map.members.len(),
            ns: map.ns,
        }
    }
}

/// Handle to the OPC UA command writer. Cheap to clone.
#[derive(Clone)]
pub struct CmdWriter {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for CmdWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CmdWriter")
            .field("endpoint", &self.inner.cfg.endpoint)
            .field("state", &self.state())
            .finish()
    }
}

impl CmdWriter {
    /// Spawn the session task (reconnect loop with 1 s → 30 s backoff, node map
    /// resolved once per connect, cached to `node_cache`). Must be called inside a
    /// tokio runtime. Returns the writer and a watch receiver for the state.
    pub fn spawn(cfg: OpcUaConfig) -> (CmdWriter, watch::Receiver<OpcState>) {
        let (state_tx, state_rx) = watch::channel(OpcState::Disconnected);
        let inner = Arc::new(Inner {
            cfg,
            state_tx,
            active: RwLock::new(None),
            map: RwLock::new(None),
            endpoints: RwLock::new(Vec::new()),
            counters: Mutex::new(Counters::seeded_now()),
            submit: tokio::sync::Mutex::new(()),
            shutdown: AtomicBool::new(false),
            notify: Notify::new(),
        });
        tokio::spawn(run(inner.clone()));
        (CmdWriter { inner }, state_rx)
    }

    /// Current state.
    pub fn state(&self) -> OpcState {
        self.inner.state_tx.borrow().clone()
    }

    /// Summary of the last resolved node map (kept across reconnects).
    pub fn nodes(&self) -> Option<NodeMapInfo> {
        self.inner.map().ok().map(|m| m.info())
    }

    /// Endpoint descriptions seen at the last GetEndpoints.
    pub fn endpoints(&self) -> Vec<String> {
        self.inner
            .endpoints
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Stop the session task and close the session. (Additive; not part of the
    /// original contract.)
    pub fn shutdown(&self) {
        self.inner.shutdown.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
        self.inner.notify.notify_one();
    }

    /// Re-browse the address space now (ignoring the cache) and replace the node map.
    pub async fn rebrowse(&self) -> Result<NodeMapInfo, OpcError> {
        let session = self.inner.session()?;
        let endpoint = self
            .inner
            .map()
            .map(|m| m.endpoint.clone())
            .unwrap_or_else(|_| self.inner.cfg.endpoint.clone());
        let map = Arc::new(browse::resolve(&session, &self.inner.cfg, &endpoint, false).await?);
        self.inner.set_map(map.clone());
        if self.inner.session().is_ok() {
            self.inner.set_state(self.inner.ready_state(&map));
        }
        Ok(map.info())
    }

    /// Write arbitrary members in one `WriteRequest` (node order preserved).
    pub async fn write_members(&self, members: &[MemberValue]) -> Result<(), OpcError> {
        self.write_batch(members).await
    }

    /// Read members in one `ReadRequest`. Returns `(canonical path, value)` pairs.
    pub async fn read_members(&self, paths: &[String]) -> Result<Vec<(String, PlcValue)>, OpcError> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let session = self.inner.session()?;
        let map = self.inner.map()?;
        let mut ids = Vec::with_capacity(paths.len());
        let mut keys = Vec::with_capacity(paths.len());
        for p in paths {
            let (id, _) = map.lookup(p)?;
            keys.push(crate::path::normalize_path(p)?);
            ids.push(ReadValueId::new_value(id));
        }
        let values = with_timeout(
            self.inner.cfg.write_timeout(),
            session.read(&ids, TimestampsToReturn::Neither, 0.0),
        )
        .await?
        .map_err(map_err)?;
        if values.len() != ids.len() {
            return Err(OpcError::Transport(format!(
                "read returned {} results for {} nodes",
                values.len(),
                ids.len()
            )));
        }
        let mut out = Vec::with_capacity(paths.len());
        for (key, dv) in keys.into_iter().zip(values) {
            if let Some(s) = dv.status {
                if s.is_bad() {
                    return Err(OpcError::Status {
                        path: key,
                        code: s.bits(),
                    });
                }
            }
            let v = dv
                .value
                .as_ref()
                .and_then(from_variant)
                .ok_or_else(|| OpcError::Transport(format!("{key}: empty value")))?;
            out.push((key, v));
        }
        Ok(out)
    }

    /// Task submission: (1) `task_members` + all `Command.*` zeros + `Data[0..15]` zeros
    /// in one request, then (2) the six `Header` fields in a second request.
    /// Serialized by an internal mutex; returns the header written.
    pub async fn write_task(
        &self,
        task_members: &[MemberValue],
        cmd_code: u8,
        src: u16,
        dst: u16,
        protocol: u8,
    ) -> Result<HeaderWire, OpcError> {
        let _guard = self.inner.submit.lock().await;
        let map = self.inner.map()?;

        let mut first: Vec<MemberValue> = Vec::with_capacity(task_members.len() + 40);
        let mut seen: Vec<String> = Vec::with_capacity(first.capacity());
        for m in task_members {
            let key = crate::path::normalize_path(&m.path)?;
            seen.push(key);
            first.push(m.clone());
        }
        for p in map
            .paths_with_prefix("Command.")
            .into_iter()
            .chain(map.array_elements("Data"))
        {
            if !seen.contains(&p) {
                seen.push(p.clone());
                first.push(MemberValue::new(p, PlcValue::U8(0)));
            }
        }
        self.write_batch(&first).await?;

        let header = {
            let mut c = self
                .inner
                .counters
                .lock()
                .map_err(|_| OpcError::Transport("counter lock poisoned".into()))?;
            HeaderWire {
                protocol,
                cmd_id: c.next_cmd_id(),
                cmd: cmd_code,
                src,
                dst,
                seq: c.next_seq(),
            }
        };
        self.write_batch(&header_members(&header)).await?;
        Ok(header)
    }

    /// Write `Command.Task.{Complete|Delete}.{WorkId,TaskId}`; `(0,0)` re-arms.
    pub async fn write_task_op(&self, op: TaskOp, work_id: u32, task_id: u32) -> Result<(), OpcError> {
        let base = match op {
            TaskOp::Complete => "Command.Task.Complete",
            TaskOp::Delete => "Command.Task.Delete",
        };
        self.write_batch(&[
            MemberValue::new(format!("{base}.WorkId"), PlcValue::U32(work_id)),
            MemberValue::new(format!("{base}.TaskId"), PlcValue::U32(task_id)),
        ])
        .await
    }

    /// Write all six `Header` fields to 0 (abort).
    pub async fn clear_header(&self) -> Result<(), OpcError> {
        self.write_batch(&header_members(&HeaderWire::default())).await
    }

    async fn write_batch(&self, members: &[MemberValue]) -> Result<(), OpcError> {
        if members.is_empty() {
            return Ok(());
        }
        let session = self.inner.session()?;
        let map = self.inner.map()?;
        let mut writes = Vec::with_capacity(members.len());
        let mut keys = Vec::with_capacity(members.len());
        for m in members {
            let (id, kind) = map.lookup(&m.path)?;
            let key = crate::path::normalize_path(&m.path)?;
            let variant = coerce(&m.value, kind, &key)?;
            writes.push(WriteValue {
                node_id: id,
                attribute_id: AttributeId::Value as u32,
                index_range: NumericRange::None,
                value: DataValue::value_only(variant),
            });
            keys.push(key);
        }
        let results = with_timeout(self.inner.cfg.write_timeout(), session.write(&writes))
            .await?
            .map_err(map_err)?;
        if results.len() != writes.len() {
            return Err(OpcError::Transport(format!(
                "write returned {} results for {} nodes",
                results.len(),
                writes.len()
            )));
        }
        for (key, code) in keys.into_iter().zip(results) {
            if !code.is_good() {
                tracing::warn!(path = %key, status = %code, "write rejected");
                return Err(OpcError::Status {
                    path: key,
                    code: code.bits(),
                });
            }
        }
        Ok(())
    }
}

/// Header struct → the six member writes (in [`HEADER_PATHS`] order).
pub fn header_members(h: &HeaderWire) -> Vec<MemberValue> {
    vec![
        MemberValue::new(HEADER_PATHS[0], PlcValue::U8(h.protocol)),
        MemberValue::new(HEADER_PATHS[1], PlcValue::U8(h.cmd_id)),
        MemberValue::new(HEADER_PATHS[2], PlcValue::U8(h.cmd)),
        MemberValue::new(HEADER_PATHS[3], PlcValue::U16(h.src)),
        MemberValue::new(HEADER_PATHS[4], PlcValue::U16(h.dst)),
        MemberValue::new(HEADER_PATHS[5], PlcValue::U16(h.seq)),
    ]
}

/// Sleep for the backoff, or return early on shutdown.
async fn backoff_sleep(inner: &Inner, ms: u64) {
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_millis(ms)) => {}
        _ = inner.notify.notified() => {}
    }
}

async fn run(inner: Arc<Inner>) {
    let mut backoff = BACKOFF_MIN_MS;
    while !inner.shutdown.load(Ordering::SeqCst) {
        inner.set_state(OpcState::Connecting);
        let conn = match connect::connect(&inner.cfg).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, retry_in_ms = backoff, "connect failed");
                inner.set_state(OpcState::Failed {
                    error: e.to_string(),
                    retry_in_ms: backoff,
                });
                backoff_sleep(&inner, backoff).await;
                backoff = (backoff * 2).min(BACKOFF_MAX_MS);
                continue;
            }
        };
        if let Ok(mut g) = inner.endpoints.write() {
            *g = conn.endpoints.clone();
        }

        inner.set_state(OpcState::Browsing);
        let map = match browse::resolve(&conn.session, &inner.cfg, &conn.endpoint_url, true).await {
            Ok(m) => Arc::new(m),
            Err(e) => {
                tracing::warn!(error = %e, retry_in_ms = backoff, "node map resolution failed");
                let _ = conn.session.disconnect().await;
                conn.event_loop.abort();
                inner.set_state(OpcState::Failed {
                    error: e.to_string(),
                    retry_in_ms: backoff,
                });
                backoff_sleep(&inner, backoff).await;
                backoff = (backoff * 2).min(BACKOFF_MAX_MS);
                continue;
            }
        };
        tracing::info!(
            ns = map.ns,
            count = map.members.len(),
            source = %map.source,
            root = %map.root_nodeid,
            "node map ready"
        );
        inner.set_map(map.clone());
        inner.set_active(Some(Active {
            session: conn.session.clone(),
        }));
        inner.set_state(inner.ready_state(&map));
        backoff = BACKOFF_MIN_MS;

        let mut event_loop = conn.event_loop;
        let ended = tokio::select! {
            r = &mut event_loop => Some(r),
            _ = inner.notify.notified() => None,
        };
        inner.set_active(None);
        match ended {
            Some(r) => {
                let why = match r {
                    Ok(code) => format!("connection lost: {code}"),
                    Err(e) => format!("event loop panicked: {e}"),
                };
                tracing::warn!(error = %why, retry_in_ms = backoff, "session ended");
                inner.set_state(OpcState::Failed {
                    error: why,
                    retry_in_ms: backoff,
                });
                backoff_sleep(&inner, backoff).await;
                backoff = (backoff * 2).min(BACKOFF_MAX_MS);
            }
            None => {
                // Shutdown requested.
                let _ = with_timeout(Duration::from_secs(2), conn.session.disconnect()).await;
                event_loop.abort();
                break;
            }
        }
    }
    inner.set_state(OpcState::Disconnected);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_skip_zero_and_wrap() {
        let mut c = Counters::with(254, 65_534);
        assert_eq!(c.next_cmd_id(), 255);
        assert_eq!(c.next_cmd_id(), 1);
        assert_eq!(c.next_cmd_id(), 2);
        assert_eq!(c.next_seq(), 65_535);
        assert_eq!(c.next_seq(), 1);
        assert_eq!(c.next_seq(), 2);

        let mut c = Counters::with(0, 0);
        assert_eq!(c.next_cmd_id(), 1);
        assert_eq!(c.next_seq(), 1);

        // Full cycle never yields 0.
        let mut c = Counters::with(7, 9);
        for _ in 0..600 {
            assert_ne!(c.next_cmd_id(), 0);
        }
        let mut c = Counters::with(7, 9);
        for _ in 0..70_000 {
            assert_ne!(c.next_seq(), 0);
        }
    }

    #[test]
    fn seeded_counters_are_nonzero() {
        let mut c = Counters::seeded_now();
        assert_ne!(c.next_cmd_id(), 0);
        assert_ne!(c.next_seq(), 0);
    }

    #[test]
    fn header_member_order() {
        let h = HeaderWire {
            protocol: 1,
            cmd_id: 2,
            cmd: 3,
            src: 4,
            dst: 5,
            seq: 6,
        };
        let m = header_members(&h);
        let paths: Vec<&str> = m.iter().map(|x| x.path.as_str()).collect();
        assert_eq!(paths, HEADER_PATHS);
        assert_eq!(m[5].value, PlcValue::U16(6));
    }
}
