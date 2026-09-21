//! 한 번에 하나만 — 기계 전체에서 gr-console 인스턴스 하나만 뜨게 막고, 떠 있는 것을 `--stop` 으로 끈다.
//!
//! 두 콘솔이 같은 PLC 에 명령을 쓰고 같은 원장(SQLite)을 만지면 Task 번호·에코가 엉킨다. 그래서 설정을
//! 읽은 직후, `data/` 에 무엇이든 쓰거나 포트를 잡기 **전에** 잠금을 쥔다.
//!
//! 잠금은 잘 알려진 파일(`%ProgramData%\gr-console\gr-console.lock`, 그 밖의 OS 는
//! `<temp>/gr-console/gr-console.lock`)에 거는 OS 배타 잠금(`File::try_lock` — Windows `LockFileEx`,
//! Unix `flock`)이다. 이름 있는 뮤텍스(`Global\gr-console`)는 FFI(`unsafe`)가 필요한데 워크스페이스가
//! `unsafe_code = "forbid"` 라 쓰지 않는다. 파일 잠금도 프로세스가 죽으면 OS 가 풀어 주므로 뮤텍스와
//! 같은 성질이다 — 비정상 종료 뒤에도 다음 실행이 막히지 않는다.
//!
//! `instance.json` 은 **안내용**이다(PID · 주소 · data 폴더 · 종료 토큰). 사용자별 폴더
//! (`%LOCALAPPDATA%\gr-console\`, 그 밖의 OS 는 `$XDG_RUNTIME_DIR` 또는 temp)에 둔다 — 종료 토큰을 같은
//! 사용자만 읽게. 진짜 가드는 잠금이라, 이 파일이 없거나 낡아도 잠금이 잡혀 있으면 거절한다.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// 이미 다른 인스턴스가 실행 중이라 시작하지 않았다.
pub const EXIT_ALREADY_RUNNING: i32 = 3;
/// HTTP 포트를 잡지 못했다(다른 프로그램이 쓰는 중 · 권한/예약 범위).
pub const EXIT_PORT_IN_USE: i32 = 4;

/// 개발·테스트용 예외 — 켜면 잠금을 아예 쥐지 않는다(데모·스모크를 나란히 돌릴 때).
pub const ALLOW_MULTI_ENV: &str = "GR_CONSOLE_ALLOW_MULTI";

/// `--stop` 이 부르는 루프백 전용 종료 엔드포인트와 토큰 헤더.
pub const SHUTDOWN_PATH: &str = "/api/admin/shutdown";
pub const TOKEN_HEADER: &str = "x-gr-stop-token";

const LOCK_FILE: &str = "gr-console.lock";
const INFO_FILE: &str = "instance.json";

/// 실행 중인 인스턴스가 남기는 안내 — 두 번째 실행과 `--stop` 이 읽는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceInfo {
    pub pid: u32,
    /// 설정의 `[server] bind` (예: `127.0.0.1:8090`)
    pub bind: String,
    /// 브라우저로 열 주소
    pub url: String,
    pub data_dir: String,
    pub started_at: String,
    pub version: String,
    #[serde(default)]
    pub demo: bool,
    /// 실행 중인 쪽 설정의 `open_browser` — 두 번째 실행이 브라우저를 대신 열지 정한다
    #[serde(default)]
    pub open_browser: bool,
    /// `--stop` 이 종료 엔드포인트에 내미는 무작위 토큰(실행마다 새로)
    #[serde(default)]
    pub stop_token: String,
}

/// 잠금을 쥔 상태. 프로세스가 끝날 때까지 들고 있는다(드롭하면 잠금이 풀린다).
#[derive(Debug)]
pub struct Guard {
    _lock: File,
    info: InfoFile,
}

/// 안내 파일의 경로와 주인 PID — 종료 절차가 끝에 지우도록 따로 복제해 넘긴다.
#[derive(Debug, Clone)]
pub struct InfoFile {
    path: PathBuf,
    pid: u32,
}

impl InfoFile {
    /// 우리가 쓴 안내 파일만 지운다(다른 인스턴스가 덮어쓴 것은 그대로 둔다). 여러 번 불러도 된다.
    pub fn remove(&self) {
        if read_info(&self.path).is_some_and(|i| i.pid == self.pid) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

impl Guard {
    pub fn info_file(&self) -> InfoFile {
        self.info.clone()
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.info.remove();
    }
}

/// 이미 다른 프로세스가 잠금을 쥐고 있을 때 알 수 있는 것.
#[derive(Debug)]
pub struct Held {
    pub lock_path: PathBuf,
    /// 안내 파일(없거나 깨졌으면 None — 다른 사용자가 띄웠거나 안내 쓰기가 실패했다)
    pub info: Option<InstanceInfo>,
    /// 안내 파일의 주소로 TCP 접속이 되는가 — 안 되면 아직 시작 중이거나 안내가 낡았다
    pub reachable: bool,
}

#[derive(Debug)]
pub enum Acquire {
    Acquired(Guard),
    Held(Held),
}

/// 잠금·안내 폴더를 옮기는 개발·시험용 환경 변수(현장에서는 쓰지 않는다 — 둘을 다른 폴더로 두면
/// 그 폴더 안에서만 "하나" 가 보장된다).
pub const LOCK_DIR_ENV: &str = "GR_CONSOLE_LOCK_DIR";
pub const INFO_DIR_ENV: &str = "GR_CONSOLE_INFO_DIR";

fn env_dir(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// 잠금 폴더 — 사용자와 상관없이 기계 하나에 하나.
pub fn default_lock_dir() -> PathBuf {
    if let Some(p) = env_dir(LOCK_DIR_ENV) {
        return p;
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if cfg!(windows)
        && let Some(p) = std::env::var_os("ProgramData").filter(|v| !v.is_empty())
    {
        candidates.push(PathBuf::from(p).join("gr-console"));
    }
    candidates.push(std::env::temp_dir().join("gr-console"));
    first_creatable(candidates)
}

/// 안내 파일 폴더 — 사용자별(종료 토큰이 들어 있다).
pub fn default_info_dir() -> PathBuf {
    if let Some(p) = env_dir(INFO_DIR_ENV).or_else(|| env_dir(LOCK_DIR_ENV)) {
        return p;
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    let var = if cfg!(windows) { "LOCALAPPDATA" } else { "XDG_RUNTIME_DIR" };
    if let Some(p) = std::env::var_os(var).filter(|v| !v.is_empty()) {
        candidates.push(PathBuf::from(p).join("gr-console"));
    }
    candidates.push(std::env::temp_dir().join("gr-console"));
    first_creatable(candidates)
}

fn first_creatable(candidates: Vec<PathBuf>) -> PathBuf {
    for c in &candidates {
        if std::fs::create_dir_all(c).is_ok() {
            return c.clone();
        }
    }
    candidates.into_iter().last().unwrap_or_else(|| PathBuf::from("gr-console"))
}

/// `--allow-multi` 이거나 `GR_CONSOLE_ALLOW_MULTI` 가 참 값이면 가드를 건너뛴다.
pub fn allow_multi(flag: bool, env: Option<&str>) -> bool {
    flag || env.is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
}

fn open_lock(lock_dir: &Path) -> io::Result<(File, PathBuf)> {
    std::fs::create_dir_all(lock_dir)?;
    let lock_path = lock_dir.join(LOCK_FILE);
    // 다른 사용자가 만든 잠금 파일은 읽기만 될 수 있다 — 잠금은 읽기 핸들로도 걸린다.
    let file = match OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&lock_path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => File::open(&lock_path)?,
        Err(e) => return Err(e),
    };
    Ok((file, lock_path))
}

/// 잠금을 쥐어 본다. 쥐면 `info_dir` 에 안내 파일을 쓴다 — 쓰기 실패는 경고만(가드는 잠금이다).
pub fn acquire(lock_dir: &Path, info_dir: &Path, info: &InstanceInfo) -> io::Result<Acquire> {
    let (file, lock_path) = open_lock(lock_dir)?;
    let info_path = info_dir.join(INFO_FILE);
    match file.try_lock() {
        Ok(()) => {
            if let Err(e) = std::fs::create_dir_all(info_dir).and_then(|_| write_info(&info_path, info)) {
                tracing::warn!(path = %info_path.display(), "instance info not written: {e}");
            }
            Ok(Acquire::Acquired(Guard { _lock: file, info: InfoFile { path: info_path, pid: info.pid } }))
        }
        Err(TryLockError::WouldBlock) => {
            let info = read_info(&info_path);
            let reachable = info.as_ref().is_some_and(|i| reachable(&i.bind));
            Ok(Acquire::Held(Held { lock_path, info, reachable }))
        }
        Err(TryLockError::Error(e)) => Err(e),
    }
}

/// 지금 실행 중인가(잠금이 잡혀 있나). 잡혀 있지 않은데 안내 파일이 남아 있으면 낡은 것이라 지운다.
#[derive(Debug)]
pub enum Probe {
    NotRunning,
    Running(Option<InstanceInfo>),
}

pub fn probe(lock_dir: &Path, info_dir: &Path) -> io::Result<Probe> {
    let (file, _) = open_lock(lock_dir)?;
    let info_path = info_dir.join(INFO_FILE);
    match file.try_lock() {
        Ok(()) => {
            drop(file);
            if info_path.exists() {
                let _ = std::fs::remove_file(&info_path);
            }
            Ok(Probe::NotRunning)
        }
        Err(TryLockError::WouldBlock) => Ok(Probe::Running(read_info(&info_path))),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

/// 잠금이 풀릴 때까지(= 실행 중인 프로세스가 끝날 때까지) 기다린다. 풀렸으면 true.
pub fn wait_released(lock_dir: &Path, timeout: Duration, mut tick: impl FnMut(Duration)) -> bool {
    let t0 = Instant::now();
    loop {
        if let Ok((f, _)) = open_lock(lock_dir)
            && f.try_lock().is_ok()
        {
            return true;
        }
        if t0.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
        tick(t0.elapsed());
    }
}

impl Held {
    /// 콘솔에 찍을 한국어 안내.
    pub fn message(&self) -> String {
        match &self.info {
            Some(i) if self.reachable => {
                format!("이미 실행 중입니다 — PID {}, {} (data: {}). 한 번에 하나만 실행할 수 있습니다.", i.pid, i.url, i.data_dir)
            }
            Some(i) => format!("이미 실행 중입니다(아직 시작 중이거나 {} 에 응답이 없음) — PID {}, data: {}. 한 번에 하나만 실행할 수 있습니다.", i.url, i.pid, i.data_dir),
            None => format!(
                "다른 gr-console 인스턴스가 실행 중입니다(잠금: {}). 주소 정보는 없습니다 — 열려 있는 gr-console 창을 찾거나 작업 관리자에서 gr-console 을 확인하세요.",
                self.lock_path.display()
            ),
        }
    }

    /// 실행 중인 쪽이 브라우저를 여는 설정이고 응답도 하면 그 주소.
    pub fn browse_url(&self) -> Option<&str> {
        self.info.as_ref().filter(|i| self.reachable && i.open_browser).map(|i| i.url.as_str())
    }
}

fn write_info(path: &Path, info: &InstanceInfo) -> io::Result<()> {
    // 임시 파일에 쓰고 바꿔 끼운다 — 두 번째 실행이 반쯤 쓴 JSON 을 읽지 않게.
    let tmp = path.with_extension(format!("json.{}.tmp", info.pid));
    let mut o = OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600); // 종료 토큰 — 같은 사용자만
    }
    o.open(&tmp)?.write_all(&serde_json::to_vec_pretty(info).map_err(io::Error::other)?)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

fn read_info(path: &Path) -> Option<InstanceInfo> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// bind 주소를 이 PC 에서 접속할 주소로 — `0.0.0.0`/`::` 은 루프백.
pub fn connect_addr(bind: &str) -> Option<SocketAddr> {
    let addr = bind.to_socket_addrs().ok()?.next()?;
    Some(if addr.ip().is_unspecified() {
        match addr {
            SocketAddr::V4(a) => SocketAddr::from(([127, 0, 0, 1], a.port())),
            SocketAddr::V6(a) => SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, a.port())),
        }
    } else {
        addr
    })
}

fn reachable(bind: &str) -> bool {
    connect_addr(bind).is_some_and(|a| TcpStream::connect_timeout(&a, Duration::from_millis(500)).is_ok())
}

/// 아주 작은 HTTP/1.1 요청 — `--stop` 한 번 부르자고 HTTP 클라이언트 크레이트를 들이지 않는다. 상태 코드를 돌려준다.
pub fn http_request(addr: SocketAddr, method: &str, path: &str, headers: &[(&str, &str)]) -> io::Result<u16> {
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    s.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: 0\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes())?;
    let mut buf = Vec::new();
    // 상태 줄만 있으면 된다 — 본문은 끝까지 읽지 않아도 된다
    let mut chunk = [0u8; 512];
    while !buf.windows(2).any(|w| w == b"\r\n") {
        let n = s.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let line = String::from_utf8_lossy(&buf);
    line.split_whitespace().nth(1).and_then(|c| c.parse().ok()).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("bad HTTP response: {:?}", line.lines().next())))
}

/// 종료 엔드포인트 허가 — 루프백에서, 이 실행의 토큰으로만. 토큰이 없는 실행(`--allow-multi`)은 엔드포인트가 없는 셈.
pub fn authorize(peer: std::net::IpAddr, presented: Option<&str>, expected: Option<&str>) -> Result<(), u16> {
    let Some(expected) = expected.filter(|t| !t.is_empty()) else { return Err(404) };
    if !peer.is_loopback() {
        return Err(403);
    }
    match presented {
        Some(p) if constant_eq(p.as_bytes(), expected.as_bytes()) => Ok(()),
        _ => Err(403),
    }
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 진짜 잠금 폴더가 아니라 테스트마다 따로 — 테스트가 실행 중인 콘솔을 막거나 막히지 않게.
    fn scratch() -> PathBuf {
        let d = std::env::temp_dir().join(format!("gr-console-instance-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn info(pid: u32, bind: &str) -> InstanceInfo {
        InstanceInfo {
            pid,
            bind: bind.into(),
            url: format!("http://{bind}/"),
            data_dir: "D:/data".into(),
            started_at: "2026-09-21 10:00:00".into(),
            version: "0.1.0".into(),
            demo: false,
            open_browser: true,
            stop_token: format!("token-{pid}"),
        }
    }

    #[test]
    fn second_acquire_is_refused_until_the_first_drops() {
        let dir = scratch();
        let Acquire::Acquired(g) = acquire(&dir, &dir, &info(100, "127.0.0.1:1")).unwrap() else { panic!("first must acquire") };
        let Acquire::Held(h) = acquire(&dir, &dir, &info(200, "127.0.0.1:2")).unwrap() else { panic!("second must be refused") };
        // 안내는 첫 번째 것이 그대로 — 두 번째 실행이 덮어쓰지 않는다
        assert_eq!(h.info.as_ref().map(|i| i.pid), Some(100));
        assert!(matches!(probe(&dir, &dir).unwrap(), Probe::Running(Some(ref i)) if i.pid == 100));
        drop(g);
        assert!(!dir.join(INFO_FILE).exists(), "info file removed on drop");
        assert!(wait_released(&dir, Duration::from_millis(10), |_| {}));
        assert!(matches!(acquire(&dir, &dir, &info(300, "127.0.0.1:3")).unwrap(), Acquire::Acquired(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn info_file_round_trip_in_its_own_dir() {
        let (lock_dir, info_dir) = (scratch(), scratch());
        let me = info(4242, "127.0.0.1:8111");
        let Acquire::Acquired(g) = acquire(&lock_dir, &info_dir, &me).unwrap() else { panic!() };
        assert_eq!(read_info(&info_dir.join(INFO_FILE)), Some(me));
        assert!(!lock_dir.join(INFO_FILE).exists());
        // 종료 절차가 먼저 지워도(여러 번) 드롭이 문제없다
        g.info_file().remove();
        g.info_file().remove();
        assert!(!info_dir.join(INFO_FILE).exists());
        drop(g);
        let _ = std::fs::remove_dir_all(&lock_dir);
        let _ = std::fs::remove_dir_all(&info_dir);
    }

    #[test]
    fn reachable_holder_is_reported_with_its_url() {
        let dir = scratch();
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let bind = l.local_addr().unwrap().to_string();
        let Acquire::Acquired(_g) = acquire(&dir, &dir, &info(11, &bind)).unwrap() else { panic!() };
        let Acquire::Held(h) = acquire(&dir, &dir, &info(12, "127.0.0.1:9")).unwrap() else { panic!() };
        assert!(h.reachable);
        assert_eq!(h.browse_url(), Some(format!("http://{bind}/").as_str()));
        assert!(h.message().contains("PID 11"), "{}", h.message());
        assert!(h.message().contains(&bind));
    }

    #[test]
    fn stale_or_missing_info_still_refuses() {
        let dir = scratch();
        let Acquire::Acquired(_g) = acquire(&dir, &dir, &info(21, "127.0.0.1:1")).unwrap() else { panic!() };
        // 안내가 있어도 주소가 죽어 있으면 브라우저는 열지 않는다
        let Acquire::Held(h) = acquire(&dir, &dir, &info(22, "127.0.0.1:2")).unwrap() else { panic!() };
        assert!(h.info.is_some() && !h.reachable);
        assert_eq!(h.browse_url(), None);
        assert!(h.message().contains("PID 21"));
        // 안내가 깨졌거나 없으면: 그래도 거절, 잠금 위치를 알려 준다
        std::fs::write(dir.join(INFO_FILE), b"{not json").unwrap();
        let Acquire::Held(h) = acquire(&dir, &dir, &info(23, "127.0.0.1:3")).unwrap() else { panic!() };
        assert!(h.info.is_none() && !h.reachable);
        assert!(h.message().contains(LOCK_FILE), "{}", h.message());
        std::fs::remove_file(dir.join(INFO_FILE)).unwrap();
        assert!(matches!(acquire(&dir, &dir, &info(24, "127.0.0.1:4")).unwrap(), Acquire::Held(Held { info: None, .. })));
        assert!(matches!(probe(&dir, &dir).unwrap(), Probe::Running(None)));
        assert!(!wait_released(&dir, Duration::from_millis(50), |_| {}));
    }

    #[test]
    fn stale_info_from_a_crashed_holder_is_replaced_or_cleaned() {
        let dir = scratch();
        // 잠금 없이 남은 안내 = 앞선 인스턴스가 비정상 종료했다. `--stop` 은 "실행 중 아님" 으로 보고 지운다.
        std::fs::write(dir.join(INFO_FILE), serde_json::to_vec(&info(30, "127.0.0.1:1")).unwrap()).unwrap();
        assert!(matches!(probe(&dir, &dir).unwrap(), Probe::NotRunning));
        assert!(!dir.join(INFO_FILE).exists());
        // 새 인스턴스는 잠금을 쥐고 덮어쓴다
        std::fs::write(dir.join(INFO_FILE), serde_json::to_vec(&info(31, "127.0.0.1:1")).unwrap()).unwrap();
        let Acquire::Acquired(g) = acquire(&dir, &dir, &info(32, "127.0.0.1:2")).unwrap() else { panic!() };
        assert_eq!(read_info(&dir.join(INFO_FILE)).map(|i| i.pid), Some(32));
        drop(g);
        assert!(!dir.join(INFO_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nothing_running_probe() {
        let dir = scratch();
        assert!(matches!(probe(&dir, &dir).unwrap(), Probe::NotRunning));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn allow_multi_override() {
        assert!(allow_multi(true, None));
        assert!(allow_multi(false, Some("1")));
        assert!(allow_multi(false, Some(" TRUE ")));
        assert!(!allow_multi(false, Some("0")));
        assert!(!allow_multi(false, Some("")));
        assert!(!allow_multi(false, None));
    }

    #[test]
    fn shutdown_endpoint_authorization() {
        let lo: std::net::IpAddr = "127.0.0.1".parse().unwrap();
        let lo6: std::net::IpAddr = "::1".parse().unwrap();
        let lan: std::net::IpAddr = "192.168.1.50".parse().unwrap();
        assert_eq!(authorize(lo, Some("abc"), Some("abc")), Ok(()));
        assert_eq!(authorize(lo6, Some("abc"), Some("abc")), Ok(()));
        assert_eq!(authorize(lo, Some("abd"), Some("abc")), Err(403));
        assert_eq!(authorize(lo, Some("ab"), Some("abc")), Err(403));
        assert_eq!(authorize(lo, None, Some("abc")), Err(403));
        // 토큰을 알아도 다른 PC 에서는 안 된다
        assert_eq!(authorize(lan, Some("abc"), Some("abc")), Err(403));
        // 토큰 없는 실행(--allow-multi): 엔드포인트가 없는 것처럼
        assert_eq!(authorize(lo, Some(""), None), Err(404));
        assert_eq!(authorize(lo, Some(""), Some("")), Err(404));
    }

    #[test]
    fn connect_addr_maps_unspecified_to_loopback() {
        assert_eq!(connect_addr("0.0.0.0:8090"), Some("127.0.0.1:8090".parse().unwrap()));
        assert_eq!(connect_addr("[::]:8090"), Some("[::1]:8090".parse().unwrap()));
        assert_eq!(connect_addr("127.0.0.1:8111"), Some("127.0.0.1:8111".parse().unwrap()));
        assert_eq!(connect_addr("nonsense"), None);
    }
}
