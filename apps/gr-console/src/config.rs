//! `gr-console.toml` + environment overrides.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub server: ServerCfg,
    pub paths: PathsCfg,
    pub plcs: Vec<PlcCfg>,
    pub opcua: OpcUaCfg,
    pub poll: PollCfg,
    pub cmd: CmdCfg,
    pub demo: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerCfg {
    pub bind: String,
    /// Comma separated extra CORS origins; `*` opens everything.
    pub cors: String,
    pub open_browser: bool,
}
impl Default for ServerCfg {
    fn default() -> Self {
        Self { bind: "127.0.0.1:8090".into(), cors: String::new(), open_browser: false }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PathsCfg {
    pub data_dir: PathBuf,
    pub contract_dir: PathBuf,
    pub web_dir: Option<PathBuf>,
    pub sqlite: PathBuf,
}
impl Default for PathsCfg {
    fn default() -> Self {
        Self { data_dir: "data".into(), contract_dir: "plc/contract".into(), web_dir: None, sqlite: "data/gr-console.db".into() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlcRole {
    Gr,
    Grm,
}

/// One S7 source. `contract` is the folder name under `contract_dir` (e.g. `GR2_PLC`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PlcCfg {
    pub name: String,
    pub role: PlcRole,
    pub contract: String,
    pub host: String,
    pub port: u16,
    pub rack: u8,
    pub slot: u8,
    pub connection_type: u8,
    pub timeout_ms: u64,
    /// DBs read at the fast tier (whole DB).
    pub fast: Vec<String>,
    /// DBs read at the webmon tier.
    pub webmon: Vec<String>,
    /// DBs read at the slow tier.
    pub slow: Vec<String>,
    /// DBs verified but read on demand only (e.g. MEASLOG_HIST).
    pub on_demand: Vec<String>,
    /// Semantic checks: `db.path == value`.
    pub checks: Vec<SemanticCheck>,
}
impl Default for PlcCfg {
    fn default() -> Self {
        Self {
            name: "GR2".into(),
            role: PlcRole::Gr,
            contract: "GR2_PLC".into(),
            host: "192.168.1.102".into(),
            port: 102,
            rack: 0,
            slot: 1,
            connection_type: 1,
            timeout_ms: 3000,
            fast: vec!["OPCUA".into(), "TASK".into()],
            webmon: vec!["WEBMON".into()],
            slow: vec!["PARA".into(), "ALARM".into(), "Interface_GRM".into(), "CELL".into(), "STATION".into(), "MEASLOG".into()],
            on_demand: vec!["MEASLOG_HIST".into()],
            checks: vec![
                SemanticCheck { db: "OPCUA".into(), path: "STAT.ComponentID".into(), equals: 4002 },
                SemanticCheck { db: "PARA".into(), path: "Machine.ID".into(), equals: 2 },
            ],
        }
    }
}

impl PlcCfg {
    pub fn grm_default() -> Self {
        Self {
            name: "GRM".into(),
            role: PlcRole::Grm,
            contract: "GRM_PLC".into(),
            host: "192.168.1.10".into(),
            fast: vec!["OPCUA".into()],
            webmon: vec![],
            slow: vec!["STATION".into(), "CELL".into(), "MACHINE".into()],
            on_demand: vec![],
            checks: vec![SemanticCheck { db: "OPCUA".into(), path: "GR[2].STAT.ComponentID".into(), equals: 4002 }],
            ..Self::default()
        }
    }
    pub fn all_dbs(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for d in self.fast.iter().chain(&self.webmon).chain(&self.slow).chain(&self.on_demand) {
            if !v.contains(d) {
                v.push(d.clone());
            }
        }
        v
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SemanticCheck {
    pub db: String,
    pub path: String,
    pub equals: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct OpcUaCfg {
    pub endpoint: String,
    pub security_policy: String,
    pub security_mode: String,
    pub user: String,
    pub pass: String,
    pub ns_hint: u16,
    pub db_name: String,
    pub root_path: String,
    pub connect_timeout_ms: u64,
    pub write_timeout_ms: u64,
    pub node_cache: Option<PathBuf>,
    pub pki_dir: Option<PathBuf>,
    pub trust_server_cert: bool,
}
impl Default for OpcUaCfg {
    fn default() -> Self {
        Self {
            endpoint: "opc.tcp://192.168.1.10:4840".into(),
            security_policy: "None".into(),
            security_mode: "None".into(),
            user: String::new(),
            pass: String::new(),
            ns_hint: 3,
            db_name: "OPCUA".into(),
            root_path: "GR[2].CMD".into(),
            connect_timeout_ms: 5000,
            write_timeout_ms: 3000,
            node_cache: Some("data/opcua-nodes.json".into()),
            pki_dir: Some("data/pki".into()),
            trust_server_cert: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PollCfg {
    pub fast_ms: u64,
    pub webmon_ms: u64,
    pub slow_ms: u64,
    pub grm_fast_ms: u64,
    pub reconnect_ms: u64,
}
impl Default for PollCfg {
    fn default() -> Self {
        Self { fast_ms: 200, webmon_ms: 500, slow_ms: 5000, grm_fast_ms: 500, reconnect_ms: 3000 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CmdCfg {
    pub protocol: u8,
    pub src: u16,
    pub dst: u16,
    pub echo_timeout_ms: u64,
    /// PLC (name in `plcs`) whose OPCUA DB carries the echo (STAT.RES) and task arrays.
    pub status_plc: String,
    pub grm_plc: String,
}
impl Default for CmdCfg {
    fn default() -> Self {
        Self { protocol: 1, src: 5000, dst: 4002, echo_timeout_ms: 5000, status_plc: "GR2".into(), grm_plc: "GRM".into() }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerCfg::default(),
            paths: PathsCfg::default(),
            plcs: vec![PlcCfg::default(), PlcCfg::grm_default()],
            opcua: OpcUaCfg::default(),
            poll: PollCfg::default(),
            cmd: CmdCfg::default(),
            demo: false,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Config> {
        let mut c: Config = if path.exists() {
            toml::from_str(&std::fs::read_to_string(path)?)?
        } else {
            tracing::warn!(path = %path.display(), "config file not found, using defaults");
            Config::default()
        };
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        if let Some(v) = env("GR_CONSOLE_ADDR") {
            c.server.bind = v;
        }
        if let Some(v) = env("GR_CONSOLE_CORS") {
            c.server.cors = v;
        }
        if let Some(v) = env("GR_CONSOLE_WEB_DIR") {
            c.paths.web_dir = Some(v.into());
        }
        if let Some(v) = env("GR_CONSOLE_DATA_DIR") {
            c.paths.data_dir = PathBuf::from(&v);
            c.paths.sqlite = c.paths.data_dir.join("gr-console.db");
        }
        if let Some(v) = env("GR_CONSOLE_OPCUA_ENDPOINT") {
            c.opcua.endpoint = v;
        }
        if let Some(v) = env("GR_CONSOLE_OPCUA_USER") {
            c.opcua.user = v;
        }
        if let Some(v) = env("GR_CONSOLE_OPCUA_PASS") {
            c.opcua.pass = v;
        }
        if let Some(v) = env("GR_CONSOLE_GR2_HOST")
            && let Some(p) = c.plcs.iter_mut().find(|p| p.name == "GR2")
        {
            p.host = v;
        }
        if let Some(v) = env("GR_CONSOLE_GRM_HOST")
            && let Some(p) = c.plcs.iter_mut().find(|p| p.name == "GRM")
        {
            p.host = v;
        }
        if env("GR_CONSOLE_DEMO").is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true")) {
            c.demo = true;
        }
        Ok(c)
    }

    pub fn plc(&self, name: &str) -> Option<&PlcCfg> {
        self.plcs.iter().find(|p| p.name.eq_ignore_ascii_case(name))
    }

    pub fn example_toml() -> String {
        toml::to_string_pretty(&Config::default()).unwrap_or_default()
    }
}
