//! `CmdWriter`: session task with reconnect, node-map lifecycle and batched writes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use opcua::client::Session;
use opcua::types::{AttributeId, DataValue, NodeId, NumericRange, ReadValueId, StatusCode, TimestampsToReturn, VariableId, WriteValue};
use tokio::sync::{Notify, watch};

use crate::browse;
use crate::connect::{self, LoopEnd, is_session_dead, map_err, with_timeout};
use crate::nodemap::{NodeMap, NodeMapInfo};
use crate::value::{coerce, from_variant};
use crate::{HeaderWire, IoStats, MemberValue, OpcError, OpcState, OpcUaConfig, PlcValue, TaskOp};

/// 세션별 튜닝 — 등록 노드 ID 와 서버 한도. 세션이 바뀌면 다시 만든다(등록은 세션에 묶인다).
#[derive(Default)]
struct Tuned {
    registered: std::collections::HashMap<NodeId, NodeId>,
    max_write: usize,
    max_read: usize,
    /// 읽기 검증을 통과한 구조체: 멤버 → (쓸 노드 ID, 인코딩 ID, 배치).
    structs: std::collections::HashMap<String, (NodeId, NodeId, crate::structs::StructSpec)>,
    struct_note: Option<String>,
}

impl Tuned {
    fn id(&self, n: NodeId) -> NodeId {
        self.registered.get(&n).cloned().unwrap_or(n)
    }
}

/// 한 번에 등록하는 노드 수 — 서버 MaxNodesPerRegisterNodes 를 모를 때도 안전한 크기.
const REGISTER_CHUNK: usize = 200;

/// 서버 한도(MaxNodesPerRead/Write)를 읽는다. 없거나 0 이면 0(제한 없음).
async fn read_limits(session: &Session, timeout: Duration) -> (u32, u32) {
    let ids = [
        ReadValueId::new_value(NodeId::from(VariableId::Server_ServerCapabilities_OperationLimits_MaxNodesPerRead)),
        ReadValueId::new_value(NodeId::from(VariableId::Server_ServerCapabilities_OperationLimits_MaxNodesPerWrite)),
    ];
    let Ok(Ok(v)) = with_timeout(timeout, session.read(&ids, TimestampsToReturn::Neither, 0.0)).await else { return (0, 0) };
    let num = |i: usize| {
        v.get(i).and_then(|d| d.value.as_ref()).and_then(|x| match from_variant(x) {
            Some(PlcValue::U32(n)) => Some(n),
            Some(PlcValue::U16(n)) => Some(n as u32),
            Some(PlcValue::I32(n)) if n > 0 => Some(n as u32),
            _ => None,
        })
    };
    (num(0).unwrap_or(0), num(1).unwrap_or(0))
}

/// 한도 읽기 + (설정이면) 명령 노드 등록. 실패는 경고만 — 원래 ID 로 계속 쓴다.
async fn tune(session: &Session, map: &NodeMap, cfg: &OpcUaConfig) -> Tuned {
    let (max_read, max_write) = read_limits(session, cfg.write_timeout()).await;
    let mut t = Tuned { max_write: max_write as usize, max_read: max_read as usize, ..Default::default() };
    if cfg.register_nodes {
        let ids: Vec<NodeId> = map.members.keys().filter_map(|k| map.lookup(k).ok().map(|(id, _)| id)).collect();
        for chunk in ids.chunks(REGISTER_CHUNK) {
            match with_timeout(cfg.write_timeout(), session.register_nodes(chunk)).await {
                Ok(Ok(reg)) if reg.len() == chunk.len() => {
                    t.registered.extend(chunk.iter().cloned().zip(reg));
                }
                Ok(Ok(reg)) => {
                    tracing::warn!(asked = chunk.len(), got = reg.len(), "RegisterNodes returned a different count — using plain node ids");
                    t.registered.clear();
                    break;
                }
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "RegisterNodes failed — using plain node ids");
                    t.registered.clear();
                    break;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "RegisterNodes timed out — using plain node ids");
                    t.registered.clear();
                    break;
                }
            }
        }
    }
    for spec in &cfg.struct_specs {
        match verify_struct(session, map, &t, spec, cfg).await {
            Ok((node, enc)) => {
                // 구조체 노드도 등록해 둔다(쓰기 때 등록 ID 로).
                let write_id = if cfg.register_nodes {
                    match with_timeout(cfg.write_timeout(), session.register_nodes(std::slice::from_ref(&node))).await {
                        Ok(Ok(mut r)) if r.len() == 1 => r.remove(0),
                        _ => node.clone(),
                    }
                } else {
                    node.clone()
                };
                tracing::info!(member = %spec.member, node = %node, encoding = %enc, write = cfg.struct_write, "struct layout verified against server");
                t.structs.insert(spec.member.clone(), (write_id, enc, spec.clone()));
            }
            Err(why) => {
                tracing::warn!(member = %spec.member, reason = %why, "struct layout not verified — leaf writes");
                t.struct_note = Some(why);
            }
        }
    }
    tracing::info!(registered = t.registered.len(), max_nodes_per_read = max_read, max_nodes_per_write = max_write, structs = t.structs.len(), "OPC UA session tuned");
    t
}

/// 구조체 노드와 그 리프들을 **한 Read** 로 읽어, 리프 값을 인코딩한 바이트가 서버 본문과 같은지 본다(읽기만).
async fn verify_struct(session: &Session, map: &NodeMap, t: &Tuned, spec: &crate::structs::StructSpec, cfg: &OpcUaConfig) -> Result<(NodeId, NodeId), String> {
    use crate::structs;
    let node = structs::struct_node(spec, |p| map.lookup(p).ok().map(|(id, _)| id)).ok_or_else(|| format!("{}: struct node id not derivable from the node map", spec.member))?;
    let leaves: Vec<&str> = spec.leaves().map(|(p, _)| p).collect();
    let mut ids = vec![ReadValueId::new_value(node.clone())];
    for p in &leaves {
        let (id, _) = map.lookup(p).map_err(|e| format!("{}: {e}", spec.member))?;
        ids.push(ReadValueId::new_value(t.id(id)));
    }
    if t.max_read > 0 && ids.len() > t.max_read {
        return Err(format!("{}: {} nodes exceed MaxNodesPerRead {}", spec.member, ids.len(), t.max_read));
    }
    let v = with_timeout(cfg.write_timeout(), session.read(&ids, TimestampsToReturn::Neither, 0.0)).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
    if v.len() != ids.len() {
        return Err(format!("{}: read returned {} of {} values", spec.member, v.len(), ids.len()));
    }
    let (enc, raw) = v[0].value.as_ref().and_then(structs::raw_body).ok_or_else(|| format!("{}: struct value is not a raw ExtensionObject (status {:?})", spec.member, v[0].status))?;
    let mut values = std::collections::HashMap::new();
    for (p, dv) in leaves.iter().zip(&v[1..]) {
        let x = dv.value.as_ref().and_then(from_variant).ok_or_else(|| format!("{p}: unreadable leaf"))?;
        values.insert(p.to_string(), x);
    }
    structs::verify(spec, &values, &raw)?;
    Ok((node, enc))
}

const BACKOFF_MIN_MS: u64 = 1_000;
const BACKOFF_MAX_MS: u64 = 30_000;

/// 구조체로 묶은 쓰기: (남은 리프, [(키, 구조체 WriteValue)]).
type Packed = (Vec<MemberValue>, Vec<(String, WriteValue)>);

/// Header field paths in write order.
pub const HEADER_PATHS: [&str; 6] = ["Header.Protocol", "Header.CMD_ID", "Header.CMD", "Header.SRC", "Header.DST", "Header.SEQ"];

/// In-memory `cmd_id` / `seq` counters; both skip 0 and wrap.
#[derive(Debug, Clone)]
pub struct Counters {
    cmd_id: u8,
    seq: u16,
}

impl Counters {
    /// Seed from wall-clock time so a restarted process does not repeat recent headers.
    pub fn seeded_now() -> Self {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
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
    /// A request on the active session came back session-invalid: the run loop drops it and reconnects.
    dead: Notify,
    dead_reason: Mutex<Option<String>>,
    tuned: RwLock<Tuned>,
    stats: Mutex<IoStats>,
}

impl Inner {
    fn set_state(&self, s: OpcState) {
        tracing::debug!(state = ?s, "opcua state");
        // `send` drops the value when every receiver is gone (the console keeps only the writer), which froze
        // `state()` at `Disconnected` and blocked every submission. `send_replace` always stores it.
        self.state_tx.send_replace(s);
    }

    fn session(&self) -> Result<Arc<Session>, OpcError> {
        self.active.read().ok().and_then(|g| g.as_ref().map(|a| a.session.clone())).ok_or(OpcError::NotReady)
    }

    fn map(&self) -> Result<Arc<NodeMap>, OpcError> {
        self.map.read().ok().and_then(|g| g.clone()).ok_or(OpcError::NotReady)
    }

    fn set_map(&self, map: Arc<NodeMap>) {
        if let Ok(mut g) = self.map.write() {
            *g = Some(map);
        }
    }

    fn set_tuned(&self, t: Tuned) {
        if let Ok(mut s) = self.stats.lock() {
            s.registered = t.registered.len();
            s.max_nodes_per_write = t.max_write as u32;
            s.max_nodes_per_read = t.max_read as u32;
            s.struct_verified = t.structs.keys().cloned().collect();
            s.struct_active = if self.cfg.struct_write { s.struct_verified.clone() } else { Vec::new() };
            s.struct_note = t.struct_note.clone();
        }
        if let Ok(mut g) = self.tuned.write() {
            *g = t;
        }
    }

    fn set_active(&self, a: Option<Active>) {
        if let Ok(mut g) = self.active.write() {
            *g = a;
        }
    }

    /// Called from the request path when the server says the session is gone: stop offering the session
    /// right away (gate / submit see not-ready) and wake the run loop to reconnect. Ignored when `session`
    /// is no longer the active one (already handled).
    fn mark_dead(&self, session: &Arc<Session>, code: StatusCode) {
        let Ok(mut g) = self.active.write() else { return };
        if !g.as_ref().is_some_and(|a| Arc::ptr_eq(&a.session, session)) {
            return;
        }
        *g = None;
        drop(g);
        let msg = dead_message(code, "");
        if let Ok(mut r) = self.dead_reason.lock() {
            *r = Some(format!("request: {code}"));
        }
        self.set_state(OpcState::Failed { error: msg, retry_in_ms: BACKOFF_MIN_MS });
        self.dead.notify_one();
    }

    fn take_dead_reason(&self) -> Option<String> {
        self.dead_reason.lock().ok().and_then(|mut g| g.take())
    }

    /// Map a service-level error; a session-invalid code also triggers the reconnect.
    fn service_err(&self, session: &Arc<Session>, e: opcua::types::Error) -> OpcError {
        let code = e.status();
        if is_session_dead(code) {
            self.mark_dead(session, code);
            return OpcError::Transport(format!("{} (요청 실패)", dead_message(code, "")));
        }
        map_err(e)
    }

    fn ready_state(&self, map: &NodeMap) -> OpcState {
        OpcState::Ready { node_count: map.members.len(), ns: map.ns }
    }
}

/// Handle to the OPC UA command writer. Cheap to clone.
#[derive(Clone)]
pub struct CmdWriter {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for CmdWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CmdWriter").field("endpoint", &self.inner.cfg.endpoint).field("state", &self.state()).finish()
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
            dead: Notify::new(),
            dead_reason: Mutex::new(None),
            tuned: RwLock::new(Tuned::default()),
            stats: Mutex::new(IoStats::default()),
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
        self.inner.endpoints.read().map(|g| g.clone()).unwrap_or_default()
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
        let endpoint = self.inner.map().map(|m| m.endpoint.clone()).unwrap_or_else(|_| self.inner.cfg.endpoint.clone());
        let map = Arc::new(browse::resolve(&session, &self.inner.cfg, &endpoint, false).await?);
        let t = tune(&session, &map, &self.inner.cfg).await;
        self.inner.set_tuned(t);
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
        let (max_read, tuned_ids): (usize, Vec<NodeId>) = {
            let t = self.inner.tuned.read().map_err(|_| OpcError::Transport("tuned lock poisoned".into()))?;
            let mut v = Vec::with_capacity(paths.len());
            for p in paths {
                let (id, _) = map.lookup(p)?;
                keys.push(crate::path::normalize_path(p)?);
                v.push(t.id(id));
            }
            (t.max_read, v)
        };
        for id in tuned_ids {
            ids.push(ReadValueId::new_value(id));
        }
        // 서버 한도(MaxNodesPerRead)를 넘으면 나눠 읽는다.
        let per = if max_read == 0 { ids.len().max(1) } else { max_read };
        let mut values = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(per) {
            let v = with_timeout(self.inner.cfg.write_timeout(), session.read(chunk, TimestampsToReturn::Neither, 0.0)).await?.map_err(|e| self.inner.service_err(&session, e))?;
            values.extend(v);
        }
        if values.len() != ids.len() {
            return Err(OpcError::Transport(format!("read returned {} results for {} nodes", values.len(), ids.len())));
        }
        let mut out = Vec::with_capacity(paths.len());
        for (key, dv) in keys.into_iter().zip(values) {
            if let Some(s) = dv.status
                && s.is_bad()
            {
                return Err(OpcError::Status { path: key, code: s.bits() });
            }
            let v = dv.value.as_ref().and_then(from_variant).ok_or_else(|| OpcError::Transport(format!("{key}: empty value")))?;
            out.push((key, v));
        }
        Ok(out)
    }

    /// Task submission: (1) `task_members` + all `Command.*` zeros + `Data[0..15]` zeros
    /// in one request, then (2) the six `Header` fields in a second request.
    /// Serialized by an internal mutex; returns the header written.
    pub async fn write_task(&self, task_members: &[MemberValue], cmd_code: u8, src: u16, dst: u16, protocol: u8) -> Result<HeaderWire, OpcError> {
        let _guard = self.inner.submit.lock().await;
        let map = self.inner.map()?;

        let mut first: Vec<MemberValue> = Vec::with_capacity(task_members.len() + 40);
        let mut seen: Vec<String> = Vec::with_capacity(first.capacity());
        for m in task_members {
            let key = crate::path::normalize_path(&m.path)?;
            seen.push(key);
            first.push(m.clone());
        }
        let mut zeros = Vec::new();
        for p in map.paths_with_prefix("Command.").into_iter().chain(map.array_elements("Data")) {
            if !seen.contains(&p) {
                seen.push(p.clone());
                zeros.push(MemberValue::new(p, PlcValue::U8(0)));
            }
        }
        // 검증된 구조체면 그 멤버의 리프들을 노드 하나로(값이 안 맞거나 서버가 거절하면 이번 세션은 리프로 되돌린다).
        // 헤더를 쓰기 전이라 되풀이해도 안전하다 — GRM 은 헤더가 채워져야 중계한다.
        let packed = if self.inner.cfg.struct_write { self.pack_structs(&first)? } else { None };
        match packed {
            Some((rest, extra)) => {
                let all: Vec<MemberValue> = rest.into_iter().chain(zeros.iter().cloned()).collect();
                match self.write_batch_with(&all, &extra).await {
                    Ok(()) => {
                        if let Ok(mut st) = self.inner.stats.lock() {
                            st.struct_writes += extra.len() as u64;
                        }
                    }
                    Err(OpcError::Status { path, code }) if extra.iter().any(|(k, _)| *k == path) => {
                        let why = format!("{path}: server refused struct write ({})", StatusCode::from(code));
                        tracing::warn!(reason = %why, "struct write refused — falling back to leaf writes for this session");
                        self.disable_structs(why);
                        first.extend(zeros);
                        self.write_batch(&first).await?;
                    }
                    Err(e) => return Err(e),
                }
            }
            None => {
                first.extend(zeros);
                self.write_batch(&first).await?;
            }
        }

        let header = {
            let mut c = self.inner.counters.lock().map_err(|_| OpcError::Transport("counter lock poisoned".into()))?;
            HeaderWire { protocol, cmd_id: c.next_cmd_id(), cmd: cmd_code, src, dst, seq: c.next_seq() }
        };
        match self.write_batch(&header_members(&header)).await {
            Ok(()) => Ok(header),
            // 응답만 못 받은 것 — 적용됐을 수 있다(중복 제출 방지를 위해 호출자가 "제출됨·에코로 판정"으로 적는다).
            Err(e @ (OpcError::Timeout | OpcError::Transport(_))) => Err(OpcError::HeaderUncertain { header, detail: e.to_string() }),
            Err(e) => Err(e),
        }
    }

    /// Write `Command.Task.{Complete|Delete}.{WorkId,TaskId}`; `(0,0)` re-arms.
    pub async fn write_task_op(&self, op: TaskOp, work_id: u32, task_id: u32) -> Result<(), OpcError> {
        let base = match op {
            TaskOp::Complete => "Command.Task.Complete",
            TaskOp::Delete => "Command.Task.Delete",
        };
        self.write_batch(&[MemberValue::new(format!("{base}.WorkId"), PlcValue::U32(work_id)), MemberValue::new(format!("{base}.TaskId"), PlcValue::U32(task_id))]).await
    }

    /// Write all six `Header` fields to 0 (abort).
    pub async fn clear_header(&self) -> Result<(), OpcError> {
        self.write_batch(&header_members(&HeaderWire::default())).await
    }

    /// 검증된 구조체 멤버의 리프를 떼어 구조체 쓰기 하나로. 해당 없으면 None.
    /// 반환: (남은 리프, [(키, 구조체 WriteValue)]).
    fn pack_structs(&self, members: &[MemberValue]) -> Result<Option<Packed>, OpcError> {
        let t = self.inner.tuned.read().map_err(|_| OpcError::Transport("tuned lock poisoned".into()))?;
        if t.structs.is_empty() {
            return Ok(None);
        }
        let mut rest = Vec::with_capacity(members.len());
        let mut groups: std::collections::HashMap<&str, std::collections::HashMap<String, PlcValue>> = Default::default();
        for m in members {
            let key = crate::path::normalize_path(&m.path)?;
            match t.structs.keys().find(|s| key.starts_with(s.as_str()) && key[s.len()..].starts_with('.')) {
                Some(s) => {
                    groups.entry(s.as_str()).or_default().insert(key, m.value.clone());
                }
                None => rest.push(m.clone()),
            }
        }
        if groups.is_empty() {
            return Ok(None);
        }
        let mut extra = Vec::new();
        for (member, values) in groups {
            let (id, enc, spec) = &t.structs[member];
            // 배치에 없는 리프는 0 으로 쓰인다 — 리프 쓰기와 뜻이 달라지므로 전부 있을 때만.
            if let Some((p, _)) = spec.leaves().find(|(p, _)| !values.contains_key(*p)) {
                tracing::debug!(member, missing = p, "struct write skipped: not every leaf given");
                return Ok(None);
            }
            let body = crate::structs::encode(spec, &values)?;
            let wv = WriteValue { node_id: id.clone(), attribute_id: AttributeId::Value as u32, index_range: NumericRange::None, value: DataValue::value_only(crate::structs::to_variant(enc, body)) };
            extra.push((member.to_string(), wv));
        }
        Ok(Some((rest, extra)))
    }

    fn disable_structs(&self, why: String) {
        if let Ok(mut t) = self.inner.tuned.write() {
            t.structs.clear();
            t.struct_note = Some(why.clone());
        }
        if let Ok(mut s) = self.inner.stats.lock() {
            s.struct_active.clear();
            s.struct_verified.clear();
            s.struct_note = Some(why);
        }
    }

    async fn write_batch(&self, members: &[MemberValue]) -> Result<(), OpcError> {
        self.write_batch_with(members, &[]).await
    }

    /// 리프 멤버 + 미리 만든 쓰기(구조체)를 한 그룹 쓰기로. 구조체가 앞에 간다(선언 순서와 무관 — 서로 겹치지 않는다).
    async fn write_batch_with(&self, members: &[MemberValue], extra: &[(String, WriteValue)]) -> Result<(), OpcError> {
        if members.is_empty() && extra.is_empty() {
            return Ok(());
        }
        let session = self.inner.session()?;
        let map = self.inner.map()?;
        let mut writes: Vec<WriteValue> = extra.iter().map(|(_, w)| w.clone()).collect();
        let mut keys: Vec<String> = extra.iter().map(|(k, _)| k.clone()).collect();
        let max_write = {
            let t = self.inner.tuned.read().map_err(|_| OpcError::Transport("tuned lock poisoned".into()))?;
            for m in members {
                let (id, kind) = map.lookup(&m.path)?;
                let key = crate::path::normalize_path(&m.path)?;
                let variant = coerce(&m.value, kind, &key)?;
                // 등록 ID(있으면)로 — S7-1500 은 등록 노드 접근을 최적화한다.
                writes.push(WriteValue { node_id: t.id(id), attribute_id: AttributeId::Value as u32, index_range: NumericRange::None, value: DataValue::value_only(variant) });
                keys.push(key);
            }
            t.max_write
        };
        let t0 = std::time::Instant::now();
        let r = self.write_chunks(&session, &writes, &keys, max_write).await;
        let ms = t0.elapsed().as_millis() as u64;
        if let Ok(mut st) = self.inner.stats.lock() {
            st.write_nodes += writes.len() as u64;
            st.write_last_ms = ms;
            st.write_max_ms = st.write_max_ms.max(ms);
            if r.is_err() {
                st.write_failures += 1;
            }
        }
        r
    }

    /// 한 그룹 쓰기(Write 서비스 한 번에 여러 노드). 서버 한도(MaxNodesPerWrite)를 넘으면 순서대로 나눠 보낸다.
    async fn write_chunks(&self, session: &Arc<Session>, writes: &[WriteValue], keys: &[String], max_write: usize) -> Result<(), OpcError> {
        let per = if max_write == 0 { writes.len().max(1) } else { max_write };
        for (wchunk, kchunk) in writes.chunks(per).zip(keys.chunks(per)) {
            if let Ok(mut st) = self.inner.stats.lock() {
                st.write_calls += 1;
            }
            let results = with_timeout(self.inner.cfg.write_timeout(), session.write(wchunk)).await?.map_err(|e| self.inner.service_err(session, e))?;
            if results.len() != wchunk.len() {
                return Err(OpcError::Transport(format!("write returned {} results for {} nodes", results.len(), wchunk.len())));
            }
            for (key, code) in kchunk.iter().zip(results) {
                if !code.is_good() {
                    tracing::warn!(path = %key, status = %code, "write rejected");
                    return Err(OpcError::Status { path: key.clone(), code: code.bits() });
                }
            }
        }
        Ok(())
    }

    /// 쓰기·읽기 통계와 지금 세션의 튜닝(등록 수·서버 한도).
    pub fn stats(&self) -> IoStats {
        self.inner.stats.lock().map(|s| s.clone()).unwrap_or_default()
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
    let mut sessions_opened: u64 = 0;
    while !inner.shutdown.load(Ordering::SeqCst) {
        inner.set_state(OpcState::Connecting);
        let conn = match connect::connect(&inner.cfg).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, retry_in_ms = backoff, "connect failed");
                inner.set_state(OpcState::Failed { error: e.to_string(), retry_in_ms: backoff });
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
                inner.set_state(OpcState::Failed { error: e.to_string(), retry_in_ms: backoff });
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
        let t = tune(&conn.session, &map, &inner.cfg).await;
        inner.set_tuned(t);
        inner.set_map(map.clone());
        if sessions_opened > 0
            && let Ok(mut st) = inner.stats.lock()
        {
            st.reconnects += 1;
        }
        sessions_opened += 1;
        // A leftover reason belongs to an earlier session.
        inner.take_dead_reason();
        inner.set_active(Some(Active { session: conn.session.clone() }));
        inner.set_state(inner.ready_state(&map));
        backoff = BACKOFF_MIN_MS;

        let mut event_loop = conn.event_loop;
        let ended = loop {
            tokio::select! {
                r = &mut event_loop => break SessionEnd::Loop(r),
                _ = inner.notify.notified() => break SessionEnd::Shutdown,
                _ = inner.dead.notified() => {
                    // A stale permit (no reason stored) is ignored.
                    if let Some(detail) = inner.take_dead_reason() {
                        break SessionEnd::Dead { detail };
                    }
                }
            }
        };
        inner.set_active(None);
        match ended {
            SessionEnd::Loop(Ok(LoopEnd::SessionDead { code, detail })) => {
                // The channel may still be up but the session is gone: reconnect right away (the
                // server is reachable), one warn line instead of the library's per-keep-alive errors.
                let error = dead_message(code, &detail);
                tracing::warn!(status = %code, %detail, retry_in_ms = BACKOFF_MIN_MS, "session invalid, reconnecting");
                inner.set_state(OpcState::Failed { error, retry_in_ms: BACKOFF_MIN_MS });
                backoff_sleep(&inner, BACKOFF_MIN_MS).await;
                backoff = BACKOFF_MIN_MS;
            }
            SessionEnd::Dead { detail } => {
                // Found by a request (state already Failed, session already withdrawn). Dropping the
                // driver task drops the transport, so the old session stops producing log noise. Try a short close first so a
                // session the server still holds does not linger until its timeout (GRM has a session limit).
                let _ = with_timeout(Duration::from_secs(1), conn.session.disconnect()).await;
                event_loop.abort();
                tracing::warn!(%detail, retry_in_ms = BACKOFF_MIN_MS, "session invalid, reconnecting");
                backoff_sleep(&inner, BACKOFF_MIN_MS).await;
                backoff = BACKOFF_MIN_MS;
            }
            SessionEnd::Loop(r) => {
                let why = match r {
                    Ok(LoopEnd::Closed(code)) => format!("connection lost: {code}"),
                    Ok(LoopEnd::SessionDead { code, .. }) => format!("session invalid: {code}"),
                    Err(e) => format!("event loop panicked: {e}"),
                };
                tracing::warn!(error = %why, retry_in_ms = backoff, "session ended");
                inner.set_state(OpcState::Failed { error: why, retry_in_ms: backoff });
                backoff_sleep(&inner, backoff).await;
                backoff = (backoff * 2).min(BACKOFF_MAX_MS);
            }
            SessionEnd::Shutdown => {
                let _ = with_timeout(Duration::from_secs(2), conn.session.disconnect()).await;
                event_loop.abort();
                break;
            }
        }
    }
    inner.set_state(OpcState::Disconnected);
}

enum SessionEnd {
    Loop(Result<LoopEnd, tokio::task::JoinError>),
    Dead { detail: String },
    Shutdown,
}

/// Operator-facing state text for a dead session.
pub(crate) fn dead_message(code: StatusCode, detail: &str) -> String {
    if matches!(code, StatusCode::BadTimeout | StatusCode::BadRequestTimeout) {
        let why = if detail.is_empty() { code.to_string() } else { detail.to_string() };
        format!("세션 응답 없음 ({why}) — 다시 연결")
    } else {
        format!("세션 무효 ({code}) — 다시 연결")
    }
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
        let h = HeaderWire { protocol: 1, cmd_id: 2, cmd: 3, src: 4, dst: 5, seq: 6 };
        let m = header_members(&h);
        let paths: Vec<&str> = m.iter().map(|x| x.path.as_str()).collect();
        assert_eq!(paths, HEADER_PATHS);
        assert_eq!(m[5].value, PlcValue::U16(6));
    }

    #[test]
    fn dead_session_messages() {
        assert_eq!(dead_message(StatusCode::BadSessionIdInvalid, "keep-alive: BadSessionIdInvalid"), "세션 무효 (BadSessionIdInvalid) — 다시 연결");
        assert_eq!(dead_message(StatusCode::BadTimeout, "keep-alive timed out 2 times in a row"), "세션 응답 없음 (keep-alive timed out 2 times in a row) — 다시 연결");
    }

    #[test]
    fn session_timeout_covers_three_keepalives() {
        let cfg = OpcUaConfig { session_timeout_ms: 10_000, keepalive_interval_ms: 5_000, ..OpcUaConfig::default() };
        assert_eq!(cfg.session_timeout(), 15_000);
        let cfg = OpcUaConfig::default();
        assert_eq!(cfg.keepalive_interval(), Duration::from_millis(5_000));
        assert_eq!(cfg.session_timeout(), 30_000);
        assert_eq!(cfg.channel_lifetime(), 60_000);
        assert_eq!(cfg.keepalive_fail_limit(), 2);
        let cfg = OpcUaConfig { keepalive_interval_ms: 0, keepalive_fail_limit: 0, session_timeout_ms: 0, channel_lifetime_ms: 0, ..OpcUaConfig::default() };
        assert_eq!((cfg.session_timeout(), cfg.keepalive_fail_limit(), cfg.channel_lifetime()), (30_000, 2, 60_000));
    }
}
