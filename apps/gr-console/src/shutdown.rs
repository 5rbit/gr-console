//! 안전하게 끄기 — Ctrl+C · 창 닫기 · `--stop` 이 모두 **같은 절차 하나**를 탄다.
//!
//! "HTTP 서버를 멈춘다" 로는 부족하다. PLC 에 반쯤 쓴 명령(예: Task Delete 를 올리고 600 ms 뒤 0 으로 되돌리는
//! 사이, CELL 표를 여러 조각으로 쓰고 다시 읽어 확인하는 사이)을 끊으면 PLC 에 이상한 상태가 남는다. 그래서:
//!
//! 1. 새 일을 받지 않는다 — 시작한 뒤의 쓰기 요청(POST/PUT/PATCH/DELETE)은 503 "종료 중", 읽기는 끝까지 된다.
//! 2. 시나리오는 다음 스텝을 내지 않는다(이미 PLC 에 보낸 Task 는 그대로 — 취소하지 않는다).
//! 3. 진행 중인 PLC 쓰기(`InFlight` 로 감싼 구간)가 끝나기를 기다린다 — 기본 15 s, 넘기면 무엇이 남았는지 남긴다.
//! 4. 기록 중이면 정상 정지 경로로 끝 스냅샷과 파일을 마무리한다.
//! 5. OPC UA 세션 · S7 연결을 닫고 SQLite WAL 을 체크포인트한다.
//! 6. 안내 파일을 지우고 잠금을 풀고 끝낸다(호출자).
//!
//! Windows 창 닫기 · 로그오프 · 시스템 종료는 OS 가 몇 초(창 닫기 ≈ 5 s)만 준다 — 같은 절차를 짧은 예산(4 s)으로,
//! 3 → 5 → 6 을 먼저 챙기며 돈다. 그래서 완전히 안전한 방법은 `stop.cmd` · `--stop` · Ctrl+C 다.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;
use tokio::sync::{Notify, watch};

use crate::cmd::CommandPort;
use crate::db::Db;
use crate::error::ApiError;
use crate::instance;
use crate::plc::PlcHandle;
use crate::record::Recorder;
use crate::scenario::Runner;

/// 정상 종료(Ctrl+C · `--stop`)에서 진행 중인 PLC 쓰기를 기다리는 한도.
pub const DRAIN_TIMEOUT: Duration = Duration::from_secs(15);
/// 창 닫기 · 로그오프 · 시스템 종료: OS 가 프로세스를 죽이기 전까지 쓸 전체 예산.
pub const URGENT_BUDGET: Duration = Duration::from_secs(4);
/// 급할 때 연결 닫기 · 체크포인트 몫으로 남겨 두는 시간.
const URGENT_RESERVE: Duration = Duration::from_millis(1500);

#[derive(Default)]
struct State_ {
    draining: bool,
    next: u64,
    inflight: BTreeMap<u64, (String, Instant)>,
    /// 급한 종료의 절대 한도(창 닫기 등) — 없으면 정상 예산
    hard_deadline: Option<Instant>,
}

struct Inner {
    state: Mutex<State_>,
    changed: Notify,
    begun: watch::Sender<Option<String>>,
}

/// 종료 조정자 — 앱 전체에 하나. 복제는 같은 것을 가리킨다.
#[derive(Clone)]
pub struct Shutdown(Arc<Inner>);

impl Default for Shutdown {
    fn default() -> Self {
        Self::new()
    }
}

/// 진행 중인 PLC 쓰기 하나 — 드롭되면 끝난 것으로 센다(패닉·`?` 조기 반환도).
pub struct InFlight {
    inner: Arc<Inner>,
    id: u64,
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.inner.state.lock().unwrap_or_else(PoisonError::into_inner).inflight.remove(&self.id);
        self.inner.changed.notify_waiters();
    }
}

impl Shutdown {
    pub fn new() -> Self {
        let (begun, _) = watch::channel(None);
        Shutdown(Arc::new(Inner { state: Mutex::new(State_::default()), changed: Notify::new(), begun }))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State_> {
        self.0.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// PLC 쓰기 구간에 들어간다. 종료가 시작됐으면 503 으로 거절 — 검사와 등록이 한 잠금 안이라 경합이 없다.
    pub fn enter(&self, what: impl Into<String>) -> Result<InFlight, ApiError> {
        let what = what.into();
        let mut s = self.lock();
        if s.draining {
            return Err(ApiError::ShuttingDown(format!("콘솔 종료 중 — {what} 을(를) 시작하지 않습니다")));
        }
        s.next += 1;
        let id = s.next;
        s.inflight.insert(id, (what, Instant::now()));
        Ok(InFlight { inner: self.0.clone(), id })
    }

    pub fn is_draining(&self) -> bool {
        self.lock().draining
    }

    /// 종료를 시작한다. 처음이면 true. `urgent` 는 이미 시작한 종료의 한도도 줄인다(Ctrl+C 뒤 창 닫기).
    pub fn begin(&self, reason: &str, urgent: bool) -> bool {
        let first = {
            let mut s = self.lock();
            if urgent {
                let d = Instant::now() + URGENT_BUDGET;
                s.hard_deadline = Some(s.hard_deadline.map_or(d, |x| x.min(d)));
            }
            !std::mem::replace(&mut s.draining, true)
        };
        if first {
            self.0.begun.send_replace(Some(reason.to_string()));
        } else if urgent {
            tracing::warn!(reason, "shutdown hurried");
        }
        self.0.changed.notify_waiters();
        first
    }

    /// 종료가 시작될 때까지 기다린다 — 이유를 돌려준다.
    pub async fn wait_begun(&self) -> String {
        let mut rx = self.0.begun.subscribe();
        match rx.wait_for(Option::is_some).await {
            Ok(r) => r.clone().unwrap_or_default(),
            Err(_) => String::new(),
        }
    }

    /// 급한 종료의 절대 한도.
    pub fn hard_deadline(&self) -> Option<Instant> {
        self.lock().hard_deadline
    }

    /// 지금 진행 중인 PLC 쓰기(설명, 경과).
    pub fn pending(&self) -> Vec<(String, Duration)> {
        self.lock().inflight.values().map(|(w, t)| (w.clone(), t.elapsed())).collect()
    }

    /// 진행 중인 쓰기가 0 이 될 때까지. 한도(`timeout`, 급하면 더 짧게)를 넘기면 남은 것을 돌려준다.
    /// `on_change` 는 남은 목록이 바뀔 때마다 불린다(콘솔 진행 줄).
    pub async fn drain(&self, timeout: Duration, mut on_change: impl FnMut(&[String])) -> Result<(), Vec<String>> {
        let start = Instant::now();
        let mut last: Option<Vec<String>> = None;
        loop {
            let notified = self.0.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let now: Vec<String> = self.pending().into_iter().map(|(w, _)| w).collect();
            if now.is_empty() {
                return Ok(());
            }
            if last.as_ref() != Some(&now) {
                on_change(&now);
                last = Some(now.clone());
            }
            let mut deadline = start + timeout;
            if let Some(h) = self.hard_deadline() {
                deadline = deadline.min(h.checked_sub(URGENT_RESERVE).unwrap_or(h));
            }
            if Instant::now() >= deadline {
                return Err(now);
            }
            tokio::select! {
                _ = notified => {}
                _ = tokio::time::sleep_until(deadline.into()) => {}
                // 급한 한도가 나중에 생겨도 알아채게 가끔 깨어난다
                _ = tokio::time::sleep(Duration::from_millis(200)) => {}
            }
        }
    }

    /// 남은 예산 안의 한도 — 급하지 않으면 `normal`, 급하면 절대 한도까지 남은 시간에서 `reserve` 를 뺀 것.
    fn budget(&self, normal: Duration, reserve: Duration) -> Duration {
        match self.hard_deadline() {
            None => normal,
            Some(h) => normal.min(h.saturating_duration_since(Instant::now()).saturating_sub(reserve)),
        }
    }
}

/// 종료가 시작된 뒤의 쓰기 요청을 503 으로 — 읽기(GET/HEAD/OPTIONS)와 종료 엔드포인트는 통과.
pub async fn reject_writes_when_draining(State(sd): State<Shutdown>, req: Request, next: Next) -> Response {
    let read_only = matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if !read_only && sd.is_draining() && req.uri().path() != instance::SHUTDOWN_PATH {
        return ApiError::ShuttingDown("콘솔 종료 중 — 새 쓰기·명령을 받지 않습니다".into()).into_response();
    }
    next.run(req).await
}

/// `POST /api/admin/shutdown` — `--stop` 전용. 루프백에서, 안내 파일의 토큰으로만. 웹 화면에는 버튼이 없다.
pub fn admin_router(sd: Shutdown, token: Option<String>) -> Router {
    Router::new().route(
        instance::SHUTDOWN_PATH,
        post(move |ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: axum::http::HeaderMap| {
            let sd = sd.clone();
            let token = token.clone();
            async move {
                let presented = headers.get(instance::TOKEN_HEADER).and_then(|v| v.to_str().ok());
                match instance::authorize(peer.ip(), presented, token.as_deref()) {
                    Ok(()) => {
                        let first = sd.begin("--stop", false);
                        tracing::info!(%peer, first, "shutdown requested over the admin endpoint");
                        (StatusCode::ACCEPTED, Json(json!({ "ok": true, "pid": std::process::id(), "already": !first }))).into_response()
                    }
                    Err(code) => {
                        tracing::warn!(%peer, code, "shutdown request refused");
                        StatusCode::from_u16(code).unwrap_or(StatusCode::FORBIDDEN).into_response()
                    }
                }
            }
        }),
    )
}

/// 종료 절차가 만지는 것들.
pub struct Parts {
    pub shutdown: Shutdown,
    pub scenario: Arc<Runner>,
    pub recorder: Arc<Recorder>,
    pub plcs: Vec<PlcHandle>,
    pub cmds: Vec<Arc<CommandPort>>,
    pub db: Db,
}

/// 절차가 한 일 — 테스트와 로그용.
#[derive(Debug, Default)]
pub struct Report {
    pub reason: String,
    pub scenario_stopped: bool,
    /// 기다린 PLC 쓰기(처음 본 목록)
    pub waited_for: Vec<String>,
    /// 한도를 넘겨 끝나지 않은 PLC 쓰기
    pub unfinished: Vec<String>,
    pub recording_finalized: Option<String>,
    pub checkpoint: Option<(i64, i64, i64)>,
}

fn say(line: &str) {
    println!("{line}");
    tracing::info!("{line}");
}

/// 종료 절차(1 ~ 5). 6(안내 파일·잠금)은 호출자가 이 뒤에 한다.
pub async fn run(p: &Parts, reason: &str) -> Report {
    let sd = &p.shutdown;
    let mut rep = Report { reason: reason.to_string(), ..Default::default() };
    say(&format!("종료 요청({reason}) — 새 PLC 쓰기·명령을 받지 않습니다."));

    // 2) 시나리오: 다음 스텝을 내지 않는다. 이미 보낸 Task 는 PLC 에 둔다.
    if p.scenario.stop_for_shutdown() {
        rep.scenario_stopped = true;
        say("종료 중: 시나리오 정지 — 다음 스텝을 내지 않습니다(이미 보낸 Task 는 PLC 에 그대로).");
    }

    // 3) 진행 중인 PLC 쓰기 마무리
    let drained = sd
        .drain(DRAIN_TIMEOUT, |now| {
            if rep.waited_for.is_empty() {
                rep.waited_for = now.to_vec();
            }
            say(&format!("종료 중: PLC 쓰기 {}건 마무리 대기… ({})", now.len(), now.join(", ")));
        })
        .await;
    match drained {
        Ok(()) if !rep.waited_for.is_empty() => say("종료 중: PLC 쓰기 마무리됨."),
        Ok(()) => {}
        Err(left) => {
            let pend = sd.pending().into_iter().map(|(w, t)| format!("{w} ({:.1} s)", t.as_secs_f32())).collect::<Vec<_>>();
            tracing::error!(pending = ?pend, "shutdown: PLC write still running at the deadline");
            say(&format!("종료 중: 시간 초과 — 아직 끝나지 않은 PLC 쓰기: {}", pend.join(", ")));
            rep.unfinished = left;
        }
    }
    // 시나리오 루프가 정지 상태를 원장에 남길 때까지(짧게)
    if rep.scenario_stopped && !p.scenario.wait_finished(sd.budget(Duration::from_secs(3), URGENT_RESERVE)).await {
        tracing::warn!("shutdown: scenario loop did not finish in time");
    }

    // 4) 기록 마무리 — 급할 때는 연결·DB 몫을 남기고 남는 시간에만
    let rec_budget = sd.budget(Duration::from_secs(5), URGENT_RESERVE);
    if !rec_budget.is_zero() && p.recorder.has_session().await {
        say("종료 중: 기록 저장…");
        match tokio::time::timeout(rec_budget, p.recorder.stop_if_active()).await {
            Ok(Some(m)) => {
                say(&format!("종료 중: 기록 저장 완료 ({}).", m.id));
                rep.recording_finalized = Some(m.id);
            }
            Ok(None) => {}
            Err(_) => say("종료 중: 기록 마무리 시간 초과 — samples.jsonl 은 남아 있습니다."),
        }
    }

    // 5) 연결 닫기
    say("종료 중: OPC UA · S7 연결 닫는 중…");
    let close_budget = sd.budget(Duration::from_secs(3), Duration::from_millis(500)).max(Duration::from_millis(300));
    let cmds = futures::future::join_all(p.cmds.iter().map(|c| c.close(close_budget)));
    let plcs = futures::future::join_all(p.plcs.iter().map(|h| h.shutdown(close_budget)));
    let _ = tokio::join!(cmds, plcs);
    say("종료 중: 연결 닫음.");
    match p.db.checkpoint() {
        Ok(c) => {
            rep.checkpoint = Some(c);
            say("종료 중: DB 정리(WAL 체크포인트) 완료.");
        }
        Err(e) => {
            tracing::error!("wal checkpoint: {e}");
            say(&format!("종료 중: DB 체크포인트 실패 — {e} (다음 실행에서 자동 복구됩니다)"));
        }
    }
    tracing::info!(reason = %rep.reason, scenario_stopped = rep.scenario_stopped, waited_for = ?rep.waited_for, unfinished = ?rep.unfinished, recording = ?rep.recording_finalized, "shutdown routine done");
    rep
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PlcCfg, PollCfg};

    fn scratch() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("gr-console-shutdown-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[tokio::test]
    async fn drain_waits_for_in_flight_then_proceeds() {
        let sd = Shutdown::new();
        let op = sd.enter("CELL 표 쓰기").unwrap();
        assert!(sd.begin("test", false));
        assert!(!sd.begin("again", false), "second begin is not first");
        let t = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            drop(op);
        });
        let mut seen = Vec::new();
        let t0 = Instant::now();
        sd.drain(Duration::from_secs(5), |now| seen.push(now.to_vec())).await.unwrap();
        assert!(t0.elapsed() >= Duration::from_millis(100), "it waited");
        assert_eq!(seen, vec![vec!["CELL 표 쓰기".to_string()]]);
        t.await.unwrap();
        assert!(sd.pending().is_empty());
    }

    #[tokio::test]
    async fn new_work_is_refused_once_draining() {
        let sd = Shutdown::new();
        let _a = sd.enter("before").unwrap();
        sd.begin("test", false);
        let e = sd.enter("after").err().expect("refused");
        assert_eq!(e.code(), "shutting_down");
        assert_eq!(sd.pending().len(), 1);
    }

    #[tokio::test]
    async fn drain_timeout_reports_the_pending_op() {
        let sd = Shutdown::new();
        let _stuck = sd.enter("Task 취소 T-1").unwrap();
        sd.begin("test", false);
        let left = sd.drain(Duration::from_millis(100), |_| {}).await.unwrap_err();
        assert_eq!(left, vec!["Task 취소 T-1".to_string()]);
    }

    #[tokio::test]
    async fn urgent_begin_shortens_the_drain() {
        let sd = Shutdown::new();
        let _stuck = sd.enter("slow").unwrap();
        sd.begin("Ctrl+C", false);
        sd.begin("창 닫기", true);
        let t0 = Instant::now();
        assert!(sd.drain(DRAIN_TIMEOUT, |_| {}).await.is_err());
        // 4 s 예산 - 1.5 s 여유 ≈ 2.5 s, 15 s 가 아니라
        assert!(t0.elapsed() < Duration::from_secs(4), "{:?}", t0.elapsed());
    }

    /// 쓰기 요청은 503, 읽기는 통과 — 실제 소켓으로.
    #[tokio::test]
    async fn write_requests_get_503_while_draining() {
        let sd = Shutdown::new();
        let app = Router::new()
            .route("/api/x", axum::routing::get(|| async { "r" }).post(|| async { "w" }))
            .merge(admin_router(sd.clone(), Some("tok".into())))
            .layer(axum::middleware::from_fn_with_state(sd.clone(), reject_writes_when_draining));
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app.into_make_service_with_connect_info::<SocketAddr>()).await });
        let req = move |m: &'static str, p: &'static str, h: Vec<(&'static str, &'static str)>| tokio::task::spawn_blocking(move || instance::http_request(addr, m, p, &h).unwrap());
        assert_eq!(req("POST", "/api/x", vec![]).await.unwrap(), 200);
        // 틀린 토큰 · 토큰 없음은 거절, 종료도 시작되지 않는다
        assert_eq!(req("POST", instance::SHUTDOWN_PATH, vec![(instance::TOKEN_HEADER, "nope")]).await.unwrap(), 403);
        assert_eq!(req("POST", instance::SHUTDOWN_PATH, vec![]).await.unwrap(), 403);
        assert!(!sd.is_draining());
        assert_eq!(req("POST", instance::SHUTDOWN_PATH, vec![(instance::TOKEN_HEADER, "tok")]).await.unwrap(), 202);
        assert!(sd.is_draining());
        assert_eq!(sd.wait_begun().await, "--stop");
        assert_eq!(req("POST", "/api/x", vec![]).await.unwrap(), 503);
        assert_eq!(req("GET", "/api/x", vec![]).await.unwrap(), 200);
        // 두 번째 --stop 도 503 이 아니라 받아 준다
        assert_eq!(req("POST", instance::SHUTDOWN_PATH, vec![(instance::TOKEN_HEADER, "tok")]).await.unwrap(), 202);
    }

    #[tokio::test]
    async fn admin_endpoint_absent_without_token() {
        let sd = Shutdown::new();
        let app = admin_router(sd.clone(), None);
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app.into_make_service_with_connect_info::<SocketAddr>()).await });
        let code = tokio::task::spawn_blocking(move || instance::http_request(addr, "POST", instance::SHUTDOWN_PATH, &[(instance::TOKEN_HEADER, "")]).unwrap()).await.unwrap();
        assert_eq!(code, 404);
        assert!(!sd.is_draining());
    }

    /// 전체 절차: 진행 중 쓰기를 기다리고, 기록을 마무리하고, WAL 을 체크포인트한다.
    #[tokio::test]
    async fn routine_drains_finalizes_recording_and_checkpoints() {
        let dir = scratch();
        let db = Db::open(&dir.join("gr-console.db")).unwrap();
        db.set_setting("k", "v").unwrap();
        let recorder = Recorder::new(dir.join("records"));
        // 닿지 않는 PLC — 기록은 읽기 오류를 세며 돈다. 종료 절차가 정상 정지 경로로 meta.json 을 마무리해야 한다.
        let cfg = PlcCfg { name: "T".into(), host: "127.0.0.1".into(), port: 1, timeout_ms: 200, fast: vec![], webmon: vec![], slow: vec![], on_demand: vec![], ..PlcCfg::default() };
        let h = crate::plc::spawn(cfg, Arc::new(plc_layout::Contract::default()), PollCfg::default()).unwrap();
        let meta = recorder.start(h.clone(), 1, crate::record::StartReq { label: "t".into(), kind: String::new(), note: String::new(), rate_ms: None, robot: None }).await.unwrap();
        let sd = Shutdown::new();
        let op = sd.enter("STATION 표 쓰기").unwrap();
        let parts = Parts { shutdown: sd.clone(), scenario: Runner::new(db.clone()), recorder: recorder.clone(), plcs: vec![h], cmds: vec![], db: db.clone() };
        sd.begin("test", false);
        let release = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            drop(op);
        });
        let rep = run(&parts, "test").await;
        release.await.unwrap();
        assert_eq!(rep.waited_for, vec!["STATION 표 쓰기".to_string()]);
        assert!(rep.unfinished.is_empty());
        assert_eq!(rep.recording_finalized.as_deref(), Some(meta.id.as_str()));
        let done: crate::record::SessionMeta = serde_json::from_str(&std::fs::read_to_string(dir.join("records").join(&meta.id).join("meta.json")).unwrap()).unwrap();
        assert!(done.stopped_at.is_some(), "end snapshot written");
        assert!(recorder.status().await.is_none());
        let (busy, _, _) = rep.checkpoint.expect("checkpoint ran");
        assert_eq!(busy, 0);
        assert_eq!(std::fs::metadata(dir.join("gr-console.db-wal")).map(|m| m.len()).unwrap_or(0), 0, "WAL truncated");
    }
}
