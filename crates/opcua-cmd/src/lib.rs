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
mod value;
mod writer;

use std::path::PathBuf;

pub use nodemap::NodeMapInfo;
pub use path::normalize_path;
pub use value::PlcKind;
pub use writer::CmdWriter;

/// Authentication used when activating the session.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Auth {
    /// Anonymous user token.
    Anonymous,
    /// User name / password token.
    UserPass { user: String, pass: String },
}

impl Default for Auth {
    fn default() -> Self {
        Auth::Anonymous
    }
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
    /// Requested session timeout.
    pub session_timeout_ms: u64,
    /// JSON cache of the resolved node map (loaded on connect, verified by a Read).
    pub node_cache: Option<PathBuf>,
    /// Client PKI directory. A self-signed application certificate is created here when
    /// the security policy is not `None`. Defaults to `<temp>/gr-console-opcua-pki`.
    pub pki_dir: Option<PathBuf>,
    /// Trust the server certificate without it being in the PKI trusted folder.
    pub trust_server_cert: bool,
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
            node_cache: None,
            pki_dir: None,
            trust_server_cert: true,
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
    pub(crate) fn session_timeout(&self) -> u32 {
        let v = if self.session_timeout_ms == 0 { 30_000 } else { self.session_timeout_ms };
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
}

impl OpcError {
    /// Human-readable name of a bad status code (best effort; falls back to hex).
    pub fn status_name(code: u32) -> String {
        let sc = opcua::types::StatusCode::from(code);
        format!("{sc}")
    }
}
