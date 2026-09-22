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
    pub link: LinkCfg,
    /// Robots reachable through GRM (up to 2). Empty = one robot derived from `cmd` + `opcua`.
    pub robots: Vec<RobotCfg>,
    pub conveyor: ConveyorCfg,
    pub demo: bool,
}

/// 컨베이어 화물 코드 트래킹(`stock::conveyor`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ConveyorCfg {
    /// GRM `ConnectionPrev/Next` 밖의 추가 연결 `[from, to]`(스테이션 Id). **품목 코드만** 넘기고 PLC 에는
    /// 쓰지 않는다 — GRM 트래킹(측정 OD)은 그대로. 예: 테스트 베드 `[[2003, 2102]]`(2003 은 GRM 에 연결이
    /// 없지만 화물이 2102 로 흘러간다). GRM 연결과 겹치면 이 값이 이긴다.
    pub extra_links: Vec<[u16; 2]>,
}

/// PLC socket link listener (`docs/link/wire-spec.md`). A PLC slot configured as Active connects here and
/// pushes Status, MeasLog and Trace chunks; the console only ever sends a read-only `TraceCfg` back.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct LinkCfg {
    pub enabled: bool,
    pub bind: String,
}
impl Default for LinkCfg {
    fn default() -> Self {
        Self { enabled: true, bind: "0.0.0.0:2000".into() }
    }
}

/// One gantry robot behind GRM: its OPC UA command root (`GR[n].CMD`), OPC UA destination id and the
/// S7 PLC (name in `plcs`) whose OPCUA DB carries the echo / task arrays.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RobotCfg {
    pub id: u8,
    pub name: String,
    pub plc: String,
    pub opcua_root: String,
    pub dst: u16,
}
impl Default for RobotCfg {
    fn default() -> Self {
        Self { id: 2, name: "GR2".into(), plc: "GR2".into(), opcua_root: "GR[2].CMD".into(), dst: 4002 }
    }
}

impl Config {
    /// Robots to run: the configured list, or the single legacy robot from `cmd` + `opcua`.
    pub fn robots_effective(&self) -> Vec<RobotCfg> {
        if !self.robots.is_empty() {
            return self.robots.clone();
        }
        vec![RobotCfg { id: (self.cmd.dst % 100) as u8, name: self.cmd.status_plc.clone(), plc: self.cmd.status_plc.clone(), opcua_root: self.opcua.root_path.clone(), dst: self.cmd.dst }]
    }
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
    /// 하트비트 비트(`DB.경로`) — 값이 일정 시간 안 바뀌면 PLC 프로그램이 멈춘 것(CPU STOP 에도 S7 읽기는 성공한다).
    /// GR 은 `OPCUA.STAT.Status.HeartBeat`(= MACHINE.Status.HeartBeat = Clock_2Hz). 경로가 없으면 감시하지 않는다.
    pub heartbeat: Option<String>,
    /// 빠른 주기에서 **이 범위만** 읽는 DB(`DB 이름 → 멤버 경로들`) — 나머지는 느린 주기에 전체를 새로 읽어 채운다.
    /// 큰 DB 에서 쓰는 곳이 일부뿐일 때(GRM OPCUA 18.5 KB 중 CMD 헤더 · STATION). GRM 은 비어 있으면 로봇 설정에서 채운다.
    pub fast_ranges: std::collections::BTreeMap<String, Vec<String>>,
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
            // 주기 폴링은 쓰는 것만(2026-09-21 통신 점검): TASK(빠른 주기 트래픽의 63 %)·ALARM·Interface_GRM 은 백엔드도
            // 화면도 읽지 않아 요청 시 읽기로 옮겼다 — 레이아웃 검사는 계속 받는다(`all_dbs`).
            fast: vec!["OPCUA".into()],
            webmon: vec!["WEBMON".into()],
            slow: vec!["PARA".into(), "CELL".into(), "STATION".into(), "MEASLOG".into(), "LASERDIAG".into()],
            on_demand: vec!["MEASLOG_HIST".into(), "TASK".into(), "ALARM".into(), "Interface_GRM".into()],
            // OPCUA.STAT.ComponentID 는 PLC 프로그램이 쓰지 않아 실기에서 항상 0 이다(2026-09-14) — 검사하면 layout_ok=false 로 제출이 막힌다.
            checks: vec![SemanticCheck { db: "PARA".into(), path: "Machine.ID".into(), equals: 2 }],
            heartbeat: Some("OPCUA.STAT.Status.HeartBeat".into()),
            fast_ranges: Default::default(),
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
            // MACHINE(16 KB) 은 쓰는 곳이 없어 요청 시 읽기로.
            slow: vec!["STATION".into(), "CELL".into()],
            on_demand: vec!["MACHINE".into()],
            // GR[n].STAT.ComponentID 도 실기에서 0 — GRM 은 DB 크기 검사만 한다.
            checks: vec![],
            // GRM 의 하트비트는 MACHINE(요청 시 읽기)에만 있다 — 감시하지 않는다.
            heartbeat: None,
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
    /// 요청 세션 타임아웃 (keep-alive 의 3 배보다 작으면 3 배로 올려 요청).
    pub session_timeout_ms: u64,
    /// 요청 보안 채널 토큰 수명 (라이브러리가 75 % 에서 갱신).
    pub channel_lifetime_ms: u64,
    /// 세션 상태 점검(keep-alive: Server_ServerStatus_State 읽기) 주기. 세션 무효 응답이면 바로 재연결.
    pub keepalive_interval_ms: u64,
    /// keep-alive 연속 시간 초과 몇 번이면 세션이 죽은 것으로 보나.
    pub keepalive_fail_limit: u32,
    pub node_cache: Option<PathBuf>,
    pub pki_dir: Option<PathBuf>,
    pub trust_server_cert: bool,
    /// 명령 노드를 세션마다 RegisterNodes 로 등록해 등록 ID 로 쓴다(S7-1500 권장, 기본 켬).
    pub register_nodes: bool,
    /// `TaskData` 를 리프 51 개 대신 구조체 노드 하나로 쓴다. 세션마다 읽기로 배치를 검증한 뒤에만 쓰고, 서버가 거절하면
    /// 그 세션은 리프 쓰기로 돌아간다. 실기 쓰기 확인 전이라 기본 끔(검증 결과는 끈 채로도 통신 툴팁에 나온다).
    pub struct_write: bool,
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
            session_timeout_ms: 60_000,
            channel_lifetime_ms: 60_000,
            // 2 s × 실패 2 회 ≈ 5 s 안에 죽은 세션을 안다(예전 5 s → 10~15 s).
            keepalive_interval_ms: 2_000,
            keepalive_fail_limit: 2,
            node_cache: Some("data/opcua-nodes.json".into()),
            pki_dir: Some("data/pki".into()),
            trust_server_cert: true,
            register_nodes: true,
            struct_write: false,
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
        // GR 빠른 주기 100 ms — 안 쓰는 DB 를 뺀 뒤라 요청 수는 예전 200 ms 때보다 적고, 에코·상태 반영은 두 배 빠르다.
        Self { fast_ms: 100, webmon_ms: 500, slow_ms: 5000, grm_fast_ms: 500, reconnect_ms: 3000 }
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
            link: LinkCfg::default(),
            robots: vec![],
            conveyor: ConveyorCfg::default(),
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
            // DB·노드 캐시·인증서는 `anchor` 가 data_dir 을 따라 옮긴다(기본값일 때).
            c.paths.data_dir = PathBuf::from(&v);
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

    /// 상대 경로를 `base`에 붙인다. 개발 체크아웃은 CWD가 기준이었지만, 배포된 실행 파일은 어디서
    /// 실행되든(더블클릭 · 바로 가기 · 서비스) **실행 파일 옆**을 기준으로 잡아야 `data/`가 한 곳에 쌓인다.
    pub fn anchor(&mut self, base: &Path) {
        // data 아래 기본값(`data/gr-console.db` · `data/opcua-nodes.json` · `data/pki`)은 data_dir 을 따른다 — 설정에서
        // data_dir 만 바꿔도 DB·노드 캐시·인증서가 함께 옮겨 간다(포터블). 명시한 값은 그대로 둔다.
        let default_data = PathBuf::from("data");
        if self.paths.data_dir != default_data {
            let rebase = |p: &mut PathBuf| {
                if let Ok(rest) = p.strip_prefix(&default_data) {
                    *p = self.paths.data_dir.join(rest);
                }
            };
            rebase(&mut self.paths.sqlite);
            if let Some(n) = self.opcua.node_cache.as_mut() {
                rebase(n);
            }
            if let Some(k) = self.opcua.pki_dir.as_mut() {
                rebase(k);
            }
        }
        let fix = |p: &mut PathBuf| {
            if p.is_relative() {
                *p = base.join(&*p);
            }
        };
        fix(&mut self.paths.data_dir);
        fix(&mut self.paths.contract_dir);
        fix(&mut self.paths.sqlite);
        if let Some(w) = self.paths.web_dir.as_mut() {
            fix(w);
        }
        if let Some(n) = self.opcua.node_cache.as_mut() {
            fix(n);
        }
        if let Some(k) = self.opcua.pki_dir.as_mut() {
            fix(k);
        }
    }

    /// 데모는 운영 데이터와 섞이지 않게 `<data_dir>-demo` 를 쓴다(`anchor` 뒤에 부른다). 데모 시드가 가짜 PLC 의
    /// CELL/STATION 을 레지스트리에 덮어쓰고, 데모 Task 가 원장·발번을 소모하던 것을 막는다. data_dir 아래에 있던
    /// DB·노드 캐시·인증서 경로도 같이 옮긴다. 이름이 이미 `-demo` 로 끝나면 그대로.
    pub fn isolate_demo_data(&mut self) {
        let old = self.paths.data_dir.clone();
        let Some(name) = old.file_name().and_then(|n| n.to_str()).map(str::to_string) else { return };
        if name.ends_with("-demo") {
            return;
        }
        let new = old.with_file_name(format!("{name}-demo"));
        let move_under = |p: &mut PathBuf| {
            if let Ok(rest) = p.strip_prefix(&old) {
                *p = new.join(rest);
            }
        };
        move_under(&mut self.paths.sqlite);
        if let Some(n) = self.opcua.node_cache.as_mut() {
            move_under(n);
        }
        if let Some(k) = self.opcua.pki_dir.as_mut() {
            move_under(k);
        }
        self.paths.data_dir = new;
    }

    #[allow(dead_code)]
    pub fn plc(&self, name: &str) -> Option<&PlcCfg> {
        self.plcs.iter().find(|p| p.name.eq_ignore_ascii_case(name))
    }

    pub fn example_toml() -> String {
        toml::to_string_pretty(&Config::default()).unwrap_or_default()
    }
}

#[cfg(test)]
mod portable_tests {
    use super::*;

    /// data_dir 만 바꾸면 기본 DB·노드 캐시·인증서가 따라간다. 명시한 값은 그대로.
    #[test]
    fn data_dir_carries_default_paths() {
        let mut c = Config::default();
        c.paths.data_dir = "store".into();
        c.anchor(Path::new("/app"));
        assert_eq!(c.paths.sqlite, Path::new("/app/store/gr-console.db"));
        assert_eq!(c.opcua.node_cache.as_deref(), Some(Path::new("/app/store/opcua-nodes.json")));
        assert_eq!(c.opcua.pki_dir.as_deref(), Some(Path::new("/app/store/pki")));
        let mut c = Config::default();
        c.paths.data_dir = "store".into();
        c.paths.sqlite = "elsewhere/x.db".into();
        c.anchor(Path::new("/app"));
        assert_eq!(c.paths.sqlite, Path::new("/app/elsewhere/x.db"));
        // 기본 data 는 그대로
        let mut c = Config::default();
        c.anchor(Path::new("/app"));
        assert_eq!(c.paths.sqlite, Path::new("/app/data/gr-console.db"));
    }

    #[test]
    fn demo_uses_its_own_data_folder() {
        let mut c = Config::default();
        c.anchor(Path::new("/app"));
        c.isolate_demo_data();
        assert_eq!(c.paths.data_dir, Path::new("/app/data-demo"));
        assert_eq!(c.paths.sqlite, Path::new("/app/data-demo/gr-console.db"));
        assert_eq!(c.opcua.pki_dir.as_deref(), Some(Path::new("/app/data-demo/pki")));
        c.isolate_demo_data();
        assert_eq!(c.paths.data_dir, Path::new("/app/data-demo"), "두 번 불러도 그대로");
    }
}
