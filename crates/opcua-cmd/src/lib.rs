//! OPC UA command writer for the Siemens S7-1500 `"OPCUA"."GR"[n]."CMD"` structure.
//!
//! The crate owns a single OPC UA session (with reconnect), discovers the node ids of
//! every leaf member under the configured root by browsing (with a JSON cache and a
//! string-node-id fallback), and offers type-correct batched writes for the command
//! protocol used by the GR console.
//!
//! See `README.md` for configuration and diagnostics.

mod browse;
mod connect;
mod nodemap;
mod path;
pub mod structs;
mod value;
mod writer;

use std::path::PathBuf;

/// 탐색 도구·시험용 — 콘솔과 같은 연결 절차(보안·인증·타임아웃)로 세션을 연다.
pub use connect::{Connection, connect};
pub use nodemap::{NodeMapInfo, array_bases_from_paths, rebase_array_keys, server_root_path};
pub use path::normalize_path;
pub use value::PlcKind;
pub use writer::CmdWriter;

/// Authentication used when activating the session.
#[derive(Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Auth {
    /// Anonymous user token.
    #[default]
    Anonymous,
    /// User name / password token.
    UserPass { user: String, pass: String },
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Auth::Anonymous => write!(f, "Anonymous"),
            Auth::UserPass { user, .. } => write!(f, "UserPass {{ user: {user:?}, pass: \"***\" }}"),
        }
    }
}

/// Connection / discovery configuration.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct OpcUaConfig {
    /// e.g. `opc.tcp://192.168.1.10:4840`
    pub endpoint: String,
    /// `None` | `Basic256Sha256` | `Aes128_Sha256_RsaOaep` | `Aes256_Sha256_RsaPss`
    /// (case-insensitive, `-`/`_` ignored; `Basic256` and `Basic128Rsa15` also accepted).
    pub security_policy: String,
    /// `None` | `Sign` | `SignAndEncrypt`
    pub security_mode: String,
    /// Session authentication.
    pub auth: Auth,
    /// Namespace index used for the string-node-id fallback (S7-1500: 3).
    pub ns_hint: u16,
    /// Global DB name, e.g. `OPCUA`.
    pub db_name: String,
    /// Path under the DB to the command struct, array indices in brackets: `GR[2].CMD`.
    pub root_path: String,
    /// Time allowed for GetEndpoints + CreateSession/ActivateSession (+ browse calls).
    pub connect_timeout_ms: u64,
    /// Per-request timeout for Read/Write.
    pub write_timeout_ms: u64,
    /// Requested session timeout (raised to at least 3x the keep-alive interval).
    pub session_timeout_ms: u64,
    /// Requested secure channel token lifetime (the library renews at 75 %).
    pub channel_lifetime_ms: u64,
    /// Session health probe: the library keep-alive reads `Server_ServerStatus_State` (i=2259) at this
    /// interval. A session-invalid answer (BadSessionIdInvalid, ...) drops the session and reconnects.
    pub keepalive_interval_ms: u64,
    /// Consecutive keep-alive timeouts that count as a dead session.
    pub keepalive_fail_limit: u32,
    /// JSON cache of the resolved node map (loaded on connect, verified by a Read).
    pub node_cache: Option<PathBuf>,
    /// Client PKI directory. A self-signed application certificate is created here when
    /// the security policy is not `None`. Defaults to `<temp>/gr-console-opcua-pki`.
    pub pki_dir: Option<PathBuf>,
    /// Trust the server certificate without it being in the PKI trusted folder.
    pub trust_server_cert: bool,
    /// Declared lower bound of each array under the root, keyed by member path without indices
    /// (`TaskData.Position` → 1), usually built from the PLC contract with [`array_bases_from_paths`].
    /// The S7-1500 server exposes arrays of elementary types 0-based; such browsed/cached keys are
    /// rebased to PLC indices so writes by PLC index resolve. Empty → no rebasing.
    pub array_bases: std::collections::BTreeMap<String, i64>,
    /// 세션마다 명령 노드를 `RegisterNodes` 로 등록하고 등록 ID 로 읽고 쓴다 — Siemens S7-1500 서버는 등록 노드의
    /// 반복 접근을 최적화한다(Siemens 109737901 "optimized access"). 등록이 실패하면 원래 ID 로 계속 쓴다.
    pub register_nodes: bool,
    /// 통째로 쓸 수 있는 구조체 멤버의 바이너리 배치(보통 계약에서 `TaskData`). 세션마다 읽기로 검증만 한다.
    pub struct_specs: Vec<structs::StructSpec>,
    /// 검증된 구조체를 실제로 노드 하나로 쓴다(끄면 검증 결과만 보고하고 리프 그룹 쓰기).
    pub struct_write: bool,
}

/// 쓰기·읽기 통계(세션을 넘어 누적, 등록·한도는 지금 세션).
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct IoStats {
    /// Write 서비스 호출 수(한도로 나눈 조각 포함).
    pub write_calls: u64,
    /// 쓴 노드 수 합.
    pub write_nodes: u64,
    /// 실패한 쓰기(서비스 오류·거부 상태·응답 없음).
    pub write_failures: u64,
    /// 마지막 / 최대 쓰기 한 번(요청 하나 = 조각 전체) 시간(ms).
    pub write_last_ms: u64,
    pub write_max_ms: u64,
    /// 세션을 다시 연 횟수(첫 연결 제외).
    pub reconnects: u64,
    /// 지금 세션에서 등록된 노드 수(0 = 등록 안 씀·실패).
    pub registered: usize,
    /// 서버 한도(0 = 제한 없음 · 모름).
    pub max_nodes_per_write: u32,
    pub max_nodes_per_read: u32,
    /// 이 세션에서 읽기 검증을 통과한 구조체 멤버(`TaskData`).
    pub struct_verified: Vec<String>,
    /// 구조체 쓰기가 켜져 실제로 쓰는 멤버.
    pub struct_active: Vec<String>,
    /// 검증 실패·쓰기 거부 사유(마지막 것).
    pub struct_note: Option<String>,
    /// 구조체 노드 하나로 보낸 쓰기 수.
    pub struct_writes: u64,
}

impl Default for OpcUaConfig {
    fn default() -> Self {
        Self {
            endpoint: "opc.tcp://192.168.1.10:4840".to_string(),
            security_policy: "None".to_string(),
            security_mode: "None".to_string(),
            auth: Auth::Anonymous,
            ns_hint: 3,
            db_name: "OPCUA".to_string(),
            root_path: "GR[2].CMD".to_string(),
            connect_timeout_ms: 5_000,
            write_timeout_ms: 3_000,
            session_timeout_ms: 30_000,
            channel_lifetime_ms: 60_000,
            keepalive_interval_ms: 5_000,
            keepalive_fail_limit: 2,
            node_cache: None,
            pki_dir: None,
            trust_server_cert: true,
            array_bases: std::collections::BTreeMap::new(),
            register_nodes: true,
            struct_specs: Vec::new(),
            struct_write: false,
        }
    }
}

impl OpcUaConfig {
    pub(crate) fn connect_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(if self.connect_timeout_ms == 0 { 5_000 } else { self.connect_timeout_ms })
    }
    pub(crate) fn write_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(if self.write_timeout_ms == 0 { 3_000 } else { self.write_timeout_ms })
    }
    pub(crate) fn keepalive_interval(&self) -> std::time::Duration {
        std::time::Duration::from_millis(if self.keepalive_interval_ms == 0 { 5_000 } else { self.keepalive_interval_ms })
    }
    pub(crate) fn keepalive_fail_limit(&self) -> u32 {
        if self.keepalive_fail_limit == 0 { 2 } else { self.keepalive_fail_limit }
    }
    pub(crate) fn channel_lifetime(&self) -> u32 {
        let v = if self.channel_lifetime_ms == 0 { 60_000 } else { self.channel_lifetime_ms };
        u32::try_from(v).unwrap_or(u32::MAX)
    }
    /// Requested session timeout: configured value (0 → 30 s), but never below 3x the keep-alive interval,
    /// so a server that honours the request cannot expire an idle-but-probed session between two probes.
    pub(crate) fn session_timeout(&self) -> u32 {
        let v = if self.session_timeout_ms == 0 { 30_000 } else { self.session_timeout_ms };
        let min = u64::try_from(self.keepalive_interval().as_millis()).unwrap_or(u64::MAX).saturating_mul(3);
        let v = v.max(min);
        u32::try_from(v).unwrap_or(u32::MAX)
    }
}

/// Session state published through the watch channel returned by [`CmdWriter::spawn`].
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum OpcState {
    Disconnected,
    Connecting,
    Browsing,
    Ready { node_count: usize, ns: u16 },
    Failed { error: String, retry_in_ms: u64 },
}

/// A scalar value for one PLC leaf member.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum PlcValue {
    Bool(bool),
    U8(u8),
    I8(i8),
    U16(u16),
    I16(i16),
    U32(u32),
    I32(i32),
    F32(f32),
    F64(f64),
    Str(String),
}

/// One member write: path relative to `root_path` (e.g. `TaskData.Position[3]`) and value.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MemberValue {
    pub path: String,
    pub value: PlcValue,
}

impl MemberValue {
    pub fn new(path: impl Into<String>, value: PlcValue) -> Self {
        Self { path: path.into(), value }
    }
}

/// The six `Header` fields as written to the PLC.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HeaderWire {
    pub protocol: u8,
    pub cmd_id: u8,
    pub cmd: u8,
    pub src: u16,
    pub dst: u16,
    pub seq: u16,
}

/// Which `Command.Task.*` pair to write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TaskOp {
    Complete,
    Delete,
}

/// Errors returned by [`CmdWriter`].
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum OpcError {
    /// No session, or the node map has not been resolved yet.
    #[error("OPC UA session not ready")]
    NotReady,
    /// The path is not in the resolved node map.
    #[error("node missing for member {0}")]
    NodeMissing(String),
    /// The server answered with a bad status code for this member.
    #[error("bad status 0x{code:08X} for {path}")]
    Status { path: String, code: u32 },
    /// The request did not complete within the configured timeout.
    #[error("OPC UA request timed out")]
    Timeout,
    /// Transport / service-level failure.
    #[error("OPC UA transport error: {0}")]
    Transport(String),
    /// Configuration problem (policy not offered, auth rejected, bad path, bad value type).
    #[error("OPC UA configuration error: {0}")]
    Config(String),
    /// Task header write got no answer (timeout / transport) after the task data went through — the server may or
    /// may not have applied it. The caller records the task as submitted and lets the PLC echo decide, instead of
    /// reporting a failure that invites a second (duplicate) submission.
    #[error("header write unanswered ({detail}) — the command may have been applied")]
    HeaderUncertain { header: HeaderWire, detail: String },
}

impl OpcError {
    /// Human-readable name of a bad status code (best effort; falls back to hex).
    pub fn status_name(code: u32) -> String {
        let sc = opcua::types::StatusCode::from(code);
        format!("{sc}")
    }
}
