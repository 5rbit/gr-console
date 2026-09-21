mod bundle;
mod cmd;
mod config;
mod console_info;
mod db;
mod demo;
mod error;
mod instance;
mod issue;
mod laser;
mod ledger;
mod link;
mod logsink;
mod measure;
mod pallet;
mod para;
mod plc;
mod record;
mod registry;
mod routes;
mod scenario;
mod shutdown;
mod sim_opcua;
mod spa;
mod sse;
mod state;
mod status;
mod stock;
mod trace;
mod util;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::http::{HeaderValue, Method, header};
use clap::Parser;
use plc_layout::Contract;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::cmd::CommandPort;
use crate::config::Config;
use crate::state::AppState;

#[derive(Parser)]
#[command(
    version,
    about = "GR gantry engineering console backend",
    after_help = "한 번에 하나만 실행된다(기계 전체). 끄기: 창에서 Ctrl+C 또는 다른 창에서 `gr-console --stop`.\n종료 코드: 0 정상 · 1 오류 · 3 이미 실행 중 · 4 HTTP 포트를 잡지 못함."
)]
struct Cli {
    /// Config file (TOML)
    #[arg(long, default_value = "gr-console.toml")]
    config: PathBuf,
    /// Run against in-process fake PLCs
    #[arg(long)]
    demo: bool,
    /// Demo with the command path over OPC UA: in-process GRM OPC UA server generated from the contract
    #[arg(long)]
    demo_opcua: bool,
    /// Override bind address, e.g. 0.0.0.0:8090
    #[arg(long)]
    bind: Option<String>,
    /// Print an example config and exit
    #[arg(long)]
    example_config: bool,
    /// Development/tests only: skip the single-instance guard (also env GR_CONSOLE_ALLOW_MULTI=1)
    #[arg(long)]
    allow_multi: bool,
    /// Ask the running console to shut down gracefully and wait for it
    #[arg(long)]
    stop: bool,
    /// With --stop: kill the process if the graceful shutdown does not finish
    #[arg(long)]
    force: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if cli.example_config {
        println!("{}", Config::example_toml());
        return Ok(());
    }
    if cli.stop {
        std::process::exit(stop_running_instance(cli.force));
    }
    // 로그는 전용 스레드가 쓴다 — 콘솔 창이 막혀도(빠른 편집 선택) 런타임이 서지 않게(`logsink`, 2026-09-21 교착).
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,opcua=warn,async_opcua=warn".into()))
        .with_writer(logsink::make_writer())
        .init();
    // 기준 디렉터리 — 설정 파일이 있는 곳. CWD에 없으면 실행 파일 옆을 본다(배포: 더블클릭·바로 가기·
    // 서비스는 CWD가 제멋대로다). 둘 다 없으면 CWD(개발 체크아웃의 기본값들이 거기 기준이다).
    let (config_path, base) = locate_config(&cli.config);
    let mut cfg = Config::load(&config_path)?;
    cfg.anchor(&base);
    logsink::open_file_dir(&cfg.paths.data_dir.join("logs"));
    tracing::info!(base = %base.display(), config = %config_path.display(), bundle = bundle::summary(), "paths");
    if cli.demo || cli.demo_opcua {
        cfg.demo = true;
    }
    if let Some(b) = cli.bind {
        cfg.server.bind = b;
    }
    // 한 번에 하나만 — data/ 에 무엇이든 쓰거나 포트를 잡기 전에. 끝날 때까지 쥐고 있는다.
    // `--stop` 이 쓸 토큰은 실행마다 새로 만들어 사용자별 안내 파일에만 남긴다.
    let sd = shutdown::Shutdown::new();
    let (guard, stop_token) = if instance::allow_multi(cli.allow_multi, std::env::var(instance::ALLOW_MULTI_ENV).ok().as_deref()) {
        tracing::warn!("single-instance guard skipped (--allow-multi / {}) — --stop 으로는 끌 수 없습니다", instance::ALLOW_MULTI_ENV);
        (None, None)
    } else {
        let token = uuid::Uuid::new_v4().to_string();
        (Some(acquire_single_instance(&cfg, &token)), Some(token))
    };
    let info_file = guard.as_ref().map(instance::Guard::info_file);
    std::fs::create_dir_all(&cfg.paths.data_dir)?;

    // contracts — 디스크에 없으면(배포 실행 파일) 내장 번들을 `data/contract/`에 풀어 쓴다
    let mut contract_root = cfg.paths.contract_dir.clone();
    if !contract_root.is_dir() {
        let extracted = cfg.paths.data_dir.join("contract");
        if bundle::extract_contract(&extracted)? {
            tracing::info!(to = %extracted.display(), "contracts extracted from the bundle");
            contract_root = extracted;
        }
    }
    let mut contracts: HashMap<String, Arc<Contract>> = HashMap::new();
    for p in &cfg.plcs {
        if !contracts.contains_key(&p.contract) {
            let dir = contract_root.join(&p.contract);
            let c = Contract::load_dir(&dir)
                .map_err(|e| anyhow::anyhow!("contract {}: {e} — plc/contract가 실행 파일 옆에 없고 내장 번들도 없습니다(패키지는 --features embed로 빌드합니다)", dir.display()))?;
            tracing::info!(contract = %p.contract, udts = c.udts.len(), dbs = c.dbs.len(), consts = c.consts.len(), skipped = c.skipped.len(), "contract loaded");
            contracts.insert(p.contract.clone(), Arc::new(c));
        }
    }

    // demo world (fake PLCs) — one fake PLC per robot so switching robots shows different data; rewrites hosts
    let demo_world = if cfg.demo {
        let grm_contract = cfg.plcs.iter().find(|p| p.role == config::PlcRole::Grm).map(|p| p.contract.clone()).unwrap_or_else(|| "GRM_PLC".into());
        let grm = contracts.get(&grm_contract).cloned().ok_or_else(|| anyhow::anyhow!("demo needs a GRM contract ({grm_contract})"))?;
        let mut demo_robots: Vec<demo::DemoRobot> = Vec::new();
        for r in cfg.robots_effective() {
            let Some(p) = cfg.plcs.iter().find(|p| p.name == r.plc && p.role == config::PlcRole::Gr) else {
                tracing::warn!(robot = r.id, plc = %r.plc, "demo: robot PLC is not a gr [[plcs]] entry — no fake PLC");
                continue;
            };
            if demo_robots.iter().any(|d| d.plc_name == p.name) {
                continue;
            }
            let Some(contract) = contracts.get(&p.contract).cloned() else { continue };
            let gr_index = demo::DemoRobot::gr_index_of(&r.opcua_root).unwrap_or_else(|| usize::from(r.id.max(1)) - 1);
            // Machine.ID = robot id: what the `PARA.Machine.ID` semantic check of each robot's [[plcs]] expects
            demo_robots.push(demo::DemoRobot { plc_name: p.name.clone(), contract, dst: r.dst, gr_index, machine_id: r.id });
        }
        anyhow::ensure!(!demo_robots.is_empty(), "demo needs at least one robot whose plc is a [[plcs]] entry with role = \"gr\"");
        let w = demo::DemoWorld::start(demo_robots, grm, 200).await?;
        // a gr PLC no robot owns shares the first robot's fake PLC
        let first = w.robot_addrs()[0].1;
        for p in cfg.plcs.iter_mut() {
            let addr = if p.role == config::PlcRole::Grm { w.grm_addr } else { w.addr_of(&p.name).unwrap_or(first) };
            p.host = addr.ip().to_string();
            p.port = addr.port();
        }
        tracing::info!(robots = ?w.robot_addrs(), grm = %w.grm_addr, "demo PLCs started");
        Some(w)
    } else {
        None
    };
    let cfg = Arc::new(cfg);

    // --demo-opcua: GRM OPC UA server generated from the contract, fed into the demo world
    let sim_opcua = match (&demo_world, cli.demo_opcua) {
        (Some(w), true) => {
            let grm = contracts.get("GRM_PLC").cloned().ok_or_else(|| anyhow::anyhow!("--demo-opcua needs GRM_PLC contract"))?;
            let roots: Vec<String> = cfg.robots_effective().into_iter().map(|r| r.opcua_root.clone()).collect();
            let s = sim_opcua::start(w.clone(), &grm, &roots, cfg.paths.data_dir.join("sim-opcua-pki")).await?;
            tracing::info!(endpoint = %s.endpoint, leaves = s.leaves, ns = s.ns, "sim GRM OPC UA server started");
            Some(s)
        }
        _ => None,
    };

    // S7 sources
    let mut plcs = HashMap::new();
    for p in &cfg.plcs {
        let c = contracts[&p.contract].clone();
        let h = plc::spawn(p.clone(), c, cfg.poll.clone())?;
        plcs.insert(p.name.clone(), h);
    }
    let plcs = Arc::new(plcs);

    // storage
    let db = db::Db::open(&cfg.paths.sqlite)?;
    // 팔렛 패턴 저장소 — 처음 한 번만 내장 사양서 R4 로 채운다(그 뒤로는 운전자 편집이 기준).
    if pallet::store::PalletStore::new(db.clone()).ensure_seeded().map_err(|e| anyhow::anyhow!("pallet pattern seed: {e}"))? {
        tracing::info!("pallet patterns seeded from spec_r4");
    }
    let registry = registry::Registry::new(db.clone());
    let scenario = scenario::Runner::new(db.clone());
    let stock = stock::Stock::new(db.clone());
    let (events, _) = tokio::sync::broadcast::channel(256);
    let (task_events, _) = tokio::sync::broadcast::channel::<ledger::LedgerEvent>(512);
    stock::spawn(stock.clone(), task_events.clone(), registry.clone());

    // robots: one command port (OPC UA GR[n].CMD via GRM) + one ledger + one sync loop each
    // GRM contract: array lower bounds for the OPC UA node map (elementary arrays are exposed 0-based).
    let grm_for_opc: Option<Arc<Contract>> = {
        let name = cfg.plcs.iter().find(|p| p.role == config::PlcRole::Grm).map(|p| p.contract.clone()).unwrap_or_else(|| "GRM_PLC".into());
        let c = contracts.get(&name).cloned();
        if c.is_none() {
            tracing::warn!(contract = %name, "no GRM contract — OPC UA array index rebasing disabled");
        }
        c
    };
    let mut robots = Vec::new();
    for r in cfg.robots_effective() {
        let rcfg = config::CmdCfg { dst: r.dst, status_plc: r.plc.clone(), ..cfg.cmd.clone() };
        let cmd = match &demo_world {
            Some(w) if sim_opcua.is_none() => CommandPort::Demo { world: w.clone(), cfg: rcfg, last: Mutex::new(None) },
            _ => {
                let mut o = cfg.opcua.clone();
                if let Some(s) = &sim_opcua {
                    o.endpoint = s.endpoint.clone();
                    o.security_policy = "None".into();
                    o.security_mode = "None".into();
                    o.user = String::new();
                    o.ns_hint = s.ns;
                    o.node_cache = None;
                    o.pki_dir = Some(cfg.paths.data_dir.join("sim-opcua-client-pki"));
                    o.trust_server_cert = true;
                }
                let o = &o;
                let ocfg = opcua_cmd::OpcUaConfig {
                    endpoint: o.endpoint.clone(),
                    security_policy: o.security_policy.clone(),
                    security_mode: o.security_mode.clone(),
                    auth: if o.user.is_empty() { opcua_cmd::Auth::Anonymous } else { opcua_cmd::Auth::UserPass { user: o.user.clone(), pass: o.pass.clone() } },
                    ns_hint: o.ns_hint,
                    db_name: o.db_name.clone(),
                    // 설정·S7 해석은 PLC 첨자(`GR[2]`), OPC UA 서버는 구조체 배열도 0 기준(`"GR"[1]`) — 2026-09-21
                    // 실기에서 `"GR"[2]` 에 쓴 명령이 PLC GR[3] 에 들어가 GR2 에 가지 않았다.
                    root_path: grm_for_opc.as_deref().map(|g| opc_server_root(g, &o.db_name, &r.opcua_root)).unwrap_or_else(|| r.opcua_root.clone()),
                    connect_timeout_ms: o.connect_timeout_ms,
                    write_timeout_ms: o.write_timeout_ms,
                    session_timeout_ms: o.session_timeout_ms,
                    channel_lifetime_ms: o.channel_lifetime_ms,
                    keepalive_interval_ms: o.keepalive_interval_ms,
                    keepalive_fail_limit: o.keepalive_fail_limit,
                    node_cache: o.node_cache.clone().map(|p| if robots_is_multi(&cfg) { p.with_extension(format!("gr{}.json", r.id)) } else { p }),
                    pki_dir: o.pki_dir.clone(),
                    trust_server_cert: o.trust_server_cert,
                    array_bases: grm_for_opc.as_deref().map(|g| cmd::opcua_array_bases(g, &o.db_name, &r.opcua_root)).unwrap_or_default(),
                };
                let (writer, _state_rx) = opcua_cmd::CmdWriter::spawn(ocfg);
                CommandPort::Opc { writer, cfg: rcfg, last: Mutex::new(None), endpoint: o.endpoint.clone() }
            }
        };
        let ledger = ledger::Ledger::new(db.clone(), &r.plc, Some(task_events.clone()))?;
        // 로봇마다 자기 상태 PLC 의 WEBMON 스트림과 MEASLOG 미러를 든다 — 사이드바에서 고른 호기의 데이터만 보이게.
        let status = status::StatusBus::new();
        let measure = measure::MeasureStore::new(db.clone(), &r.plc, registry.clone());
        match plcs.get(&r.plc) {
            Some(h) => {
                ledger::sync::spawn(ledger.clone(), h.clone(), cfg.cmd.echo_timeout_ms);
                if h.has_db("WEBMON") {
                    status.follow(h.clone(), "WEBMON".into(), if cfg.demo { "demo" } else { "plc" }, r.id);
                }
                measure.attach(h.clone());
            }
            None => tracing::error!(robot = r.id, plc = %r.plc, "robot status PLC not configured"),
        }
        tracing::info!(robot = r.id, name = %r.name, plc = %r.plc, root = %r.opcua_root, dst = r.dst, "robot");
        robots.push(state::RobotCtx { id: r.id, name: r.name.clone(), plc: r.plc.clone(), opcua_root: r.opcua_root.clone(), dst: r.dst, cmd: Arc::new(cmd), ledger, status, measure });
    }
    let robots = Arc::new(robots);
    let first = robots.first().expect("at least one robot");
    let cmd = first.cmd.clone();
    let ledger = first.ledger.clone();

    let recorder = record::Recorder::new(cfg.paths.data_dir.join("records"));
    // Trace needs LNK_Trace in the contract; without it the endpoints answer "trace is not configured" instead of
    // failing the whole start-up.
    // 트레이스 계약 = LNK_Trace 가 있는 **첫 로봇 PLC**(설정 순서). 예전에는 무조건 첫 로봇이라, GR1 이 앞이면
    // (GR1 에는 TRACE_LNK 가 없다) GR2 트레이스까지 통째로 꺼졌고, 켜져도 다른 로봇 요청을 첫 로봇 레이아웃으로 풀었다.
    let mut trace = None;
    let mut trace_why = Vec::new();
    for r in robots.iter() {
        let Some(c) = cfg.plcs.iter().find(|p| p.name == r.plc).and_then(|p| contracts.get(&p.contract).cloned()) else {
            trace_why.push(format!("{}: no contract", r.plc));
            continue;
        };
        match trace::TraceStore::new(&r.plc, c, cfg.paths.data_dir.join("traces")) {
            Ok(t) => {
                tracing::info!(plc = %r.plc, "trace enabled");
                trace = Some(t);
                break;
            }
            Err(e) => trace_why.push(format!("{}: {e}", r.plc)),
        }
    }
    if trace.is_none() {
        tracing::warn!("trace disabled: {}", trace_why.join("; "));
    }
    let st = AppState { cfg: cfg.clone(), plcs, cmd, robots, task_events, db, ledger, registry, scenario, stock, recorder, trace, events, shutdown: sd.clone() };

    // demo: seed registries from the fake PLC tables once they are readable
    if cfg.demo {
        let st2 = st.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            let plc = st2.default_plc_name().to_string();
            for what in ["cells", "stations"] {
                let Ok(h) = st2.plc(&plc) else { continue };
                let snap = h.snap();
                let db = if what == "cells" { "CELL" } else { "STATION" };
                let Some(d) = snap.db(db) else { continue };
                let count = d.json["Count"].as_u64().unwrap_or(0) as usize;
                if what == "cells" {
                    for c in d.json["Cell"].as_array().into_iter().flatten().take(count) {
                        if let Ok(cell) = serde_json::from_value::<gr_proto::CellInfo>(c.clone()) {
                            let _ = st2.registry.upsert_cell(&cell, "plc", false, Some(util::now_str()));
                        }
                    }
                } else {
                    for s in d.json["Station"].as_array().into_iter().flatten().take(count) {
                        if let Ok(p) = serde_json::from_value::<gr_proto::StationPara>(s.clone()) {
                            let _ = st2.registry.upsert_station(&p, "plc", false, Some(util::now_str()));
                        }
                    }
                }
            }
            if st2.registry.items().map(|v| v.is_empty()).unwrap_or(true) {
                for (code, name, id, od, h, cnt) in
                    [(1001u32, "225/45R17", 381.0f32, 780.0f32, 240.0f32, 4u8), (1002, "245/40R19", 431.8, 860.0, 260.0, 3), (1003, "275/35R20", 508.0, 1020.0, 300.0, 3)]
                {
                    let item = gr_proto::StockItem { code, count: cnt, inner_diameter: id, outer_diameter: od, lower_bid_height: 20.0, upper_bid_height: h - 20.0, height: h, deflection_factor: 0.0 };
                    let _ = st2.registry.upsert_item(code, name, &item, "demo");
                }
            }
            // 재고: 셀 101..106 에 데모 품목을 쌓아 두면 레이아웃·계획 화면에 바로 보인다
            if st2.stock.list().map(|v| v.is_empty()).unwrap_or(true) {
                for (cell, code, n) in [(101u16, 1001u32, 3u32), (102, 1002, 2), (103, 1003, 4), (104, 1001, 1), (105, 1002, 3), (106, 1003, 2)] {
                    let _ = st2.stock.set(cell, code, n, "", "demo seed");
                }
            }
            for r in st2.robots.iter() {
                if let Ok(h) = st2.robot_plc(r) {
                    let _ = r.measure.sync(h).await;
                }
            }
        });
    }

    // PLC socket link: Trace chunks and the TraceCfg acknowledgement reach the trace store through it.
    if let (true, Some(tr)) = (cfg.link.enabled, st.trace.clone()) {
        link::spawn(&cfg.link.bind, tr.contract_arc(), tr).await;
    }

    // web — 종료 미들웨어가 가장 바깥이라, 종료가 시작되면 새 쓰기 요청이 슬라이스까지 오지 않는다.
    let parts = shutdown::Parts {
        shutdown: sd.clone(),
        scenario: st.scenario.clone(),
        recorder: st.recorder.clone(),
        plcs: st.plcs.values().cloned().collect(),
        cmds: st.robots.iter().map(|r| r.cmd.clone()).collect(),
        db: st.db.clone(),
    };
    let base = console_info::router(cfg.demo).merge(shutdown::admin_router(sd.clone(), stop_token.clone())).merge(routes::router(st));
    let (app, web_source) = spa::attach(base, cfg.paths.web_dir.as_deref());
    let app = app.layer(cors_layer(&cfg.server.cors)).layer(axum::middleware::from_fn_with_state(sd.clone(), shutdown::reject_writes_when_draining));
    let addr: std::net::SocketAddr = cfg.server.bind.parse().map_err(|e| anyhow::anyhow!("bind {}: {e}", cfg.server.bind))?;
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            let Some(msg) = bind_failure_message(addr, &e) else { return Err(e.into()) };
            eprintln!("{msg}");
            drop(guard);
            std::process::exit(instance::EXIT_PORT_IN_USE);
        }
    };
    tracing::info!(%addr, web = %web_source, demo = cfg.demo, "gr-console listening");
    let url = browse_url(addr);
    logsink::print(&format!("gr-console  {url}  (web: {web_source}, demo: {})", cfg.demo));
    logsink::print("끄기: 이 창에서 Ctrl+C, 또는 다른 창에서 gr-console --stop (창을 그냥 닫는 것은 최후 수단)");
    if cfg.server.open_browser {
        open_browser(&url);
    }
    spawn_signal_watchers(sd.clone());
    // 서버는 제 태스크에서 계속 돈다 — 종료 절차 동안에도 읽기 요청과 `--stop` 응답이 끝까지 나간다
    // (쓰기는 미들웨어가 503 으로 막는다). 절차가 끝나면 프로세스를 끝낸다.
    let mut server = tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await });
    tokio::select! {
        r = &mut server => {
            match r {
                Ok(Ok(())) => tracing::warn!("http server ended on its own"),
                Ok(Err(e)) => return Err(e.into()),
                Err(e) => return Err(anyhow::anyhow!("http server task: {e}")),
            }
        }
        reason = sd.wait_begun() => {
            shutdown::run(&parts, &reason).await;
        }
    }
    // 6) 안내 파일과 잠금
    if let Some(f) = &info_file {
        f.remove();
    }
    drop(guard);
    say_done();
    // 남은 백그라운드 태스크(폴링·동기화)를 기다리지 않고 지금 끝낸다 — 중요한 것은 위에서 다 마무리했다.
    std::process::exit(0);
}

fn say_done() {
    logsink::print("종료 완료.");
    logsink::flush(std::time::Duration::from_secs(2));
    tracing::info!("shutdown complete");
    use std::io::Write;
    let _ = std::io::stdout().flush();
}

/// Ctrl+C · 창 닫기 · 로그오프 · 시스템 종료 · SIGTERM → 같은 종료 절차. 창 닫기 계열은 OS 가 몇 초만 주므로 급한 예산.
fn spawn_signal_watchers(sd: shutdown::Shutdown) {
    let s = sd.clone();
    tokio::spawn(async move {
        // 첫 Ctrl+C 는 정상 종료, 두 번째는 "급하게" — 기다리는 PLC 쓰기가 있을 때 운전자가 재촉할 수 있게.
        let mut urgent = false;
        while tokio::signal::ctrl_c().await.is_ok() {
            s.begin(if urgent { "Ctrl+C 두 번" } else { "Ctrl+C" }, urgent);
            urgent = true;
        }
    });
    #[cfg(windows)]
    {
        use tokio::signal::windows;
        macro_rules! watch {
            ($f:path, $why:expr, $urgent:expr) => {
                if let Ok(mut s) = $f() {
                    let sd = sd.clone();
                    tokio::spawn(async move {
                        s.recv().await;
                        sd.begin($why, $urgent);
                    });
                }
            };
        }
        // 창 닫기 · 로그오프 · 시스템 종료: 핸들러가 돌아오면 OS 가 바로 죽인다(tokio 가 붙잡아 준다) — 예산 4 초.
        watch!(windows::ctrl_close, "콘솔 창 닫기", true);
        watch!(windows::ctrl_logoff, "로그오프", true);
        watch!(windows::ctrl_shutdown, "Windows 종료", true);
        watch!(windows::ctrl_break, "Ctrl+Break", false);
    }
    #[cfg(unix)]
    {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            tokio::spawn(async move {
                s.recv().await;
                sd.begin("SIGTERM", false);
            });
        }
    }
}

/// `--stop` — 실행 중인 콘솔에 정상 종료를 요청하고 끝날 때까지 기다린다. 프로세스 종료 코드를 돌려준다.
fn stop_running_instance(force: bool) -> i32 {
    let (lock_dir, info_dir) = (instance::default_lock_dir(), instance::default_info_dir());
    let info = match instance::probe(&lock_dir, &info_dir) {
        Ok(instance::Probe::NotRunning) => {
            println!("실행 중인 콘솔이 없습니다.");
            return 0;
        }
        Ok(instance::Probe::Running(info)) => info,
        Err(e) => {
            eprintln!("잠금을 확인할 수 없습니다({}): {e}", lock_dir.display());
            return 1;
        }
    };
    let Some(info) = info else {
        eprintln!(
            "콘솔이 실행 중이지만(잠금: {}) 이 사용자 계정에는 정보 파일이 없습니다 — 다른 사용자가 띄웠을 수 있습니다.\n그 계정에서 gr-console --stop 을 실행하거나, 작업 관리자에서 gr-console 을 끝내세요.",
            lock_dir.join("gr-console.lock").display()
        );
        return 1;
    };
    println!("종료 요청: PID {} ({})", info.pid, info.url);
    let asked = match instance::connect_addr(&info.bind) {
        Some(addr) => match instance::http_request(addr, "POST", instance::SHUTDOWN_PATH, &[(instance::TOKEN_HEADER, info.stop_token.as_str())]) {
            Ok(202) => true,
            Ok(code) => {
                eprintln!("종료 엔드포인트가 거절했습니다(HTTP {code}) — 토큰이 맞지 않거나 다른 프로그램이 그 포트를 쓰고 있습니다.");
                false
            }
            Err(e) => {
                eprintln!("종료 엔드포인트에 닿지 못했습니다({addr}): {e}");
                false
            }
        },
        None => {
            eprintln!("주소를 해석할 수 없습니다: {}", info.bind);
            false
        }
    };
    if asked {
        println!("정상 종료 절차를 기다립니다(진행 중인 PLC 쓰기를 마무리합니다)…");
        if instance::wait_released(&lock_dir, std::time::Duration::from_secs(30), |_| {}) {
            println!("종료했습니다 (PID {}).", info.pid);
            return 0;
        }
        eprintln!("30초 안에 끝나지 않았습니다 (PID {}).", info.pid);
    }
    if !force {
        eprintln!("강제로 끝내려면: gr-console --stop --force  (또는 {})", kill_hint(info.pid));
        eprintln!("강제 종료는 진행 중이던 PLC 쓰기를 끊을 수 있습니다.");
        return 1;
    }
    eprintln!("강제 종료합니다 (PID {}) — 진행 중이던 PLC 쓰기가 끊길 수 있습니다.", info.pid);
    let killed = if cfg!(windows) {
        std::process::Command::new("taskkill").args(["/F", "/PID", &info.pid.to_string()]).status()
    } else {
        std::process::Command::new("kill").args(["-9", &info.pid.to_string()]).status()
    };
    match killed {
        Ok(s) if s.success() => {
            instance::wait_released(&lock_dir, std::time::Duration::from_secs(5), |_| {});
            println!("강제 종료했습니다 (PID {}).", info.pid);
            0
        }
        Ok(s) => {
            eprintln!("강제 종료 실패(exit {:?}) — {}", s.code(), kill_hint(info.pid));
            1
        }
        Err(e) => {
            eprintln!("강제 종료 실패: {e} — {}", kill_hint(info.pid));
            1
        }
    }
}

fn kill_hint(pid: u32) -> String {
    if cfg!(windows) { format!("작업 관리자에서 gr-console.exe 끝내기 또는 taskkill /PID {pid} /F") } else { format!("kill -9 {pid}") }
}

/// 잠금을 쥐거나, 이미 다른 인스턴스가 있으면 안내하고 종료 코드 3 으로 끝낸다.
fn acquire_single_instance(cfg: &Config, stop_token: &str) -> instance::Guard {
    let bind = cfg.server.bind.clone();
    let url = bind.parse::<std::net::SocketAddr>().map(browse_url).unwrap_or_else(|_| format!("http://{bind}/"));
    let me = instance::InstanceInfo {
        pid: std::process::id(),
        bind,
        url,
        data_dir: cfg.paths.data_dir.display().to_string(),
        started_at: util::now_str(),
        version: env!("CARGO_PKG_VERSION").into(),
        demo: cfg.demo,
        open_browser: cfg.server.open_browser,
        stop_token: stop_token.to_string(),
    };
    let dir = instance::default_lock_dir();
    let info_dir = instance::default_info_dir();
    match instance::acquire(&dir, &info_dir, &me) {
        Ok(instance::Acquire::Acquired(g)) => {
            tracing::debug!(lock = %dir.display(), info = %info_dir.display(), "single-instance lock held");
            g
        }
        Ok(instance::Acquire::Held(h)) => {
            eprintln!("{}", h.message());
            if let Some(u) = h.browse_url() {
                eprintln!("브라우저에서 실행 중인 콘솔을 엽니다: {u}");
                open_browser(u);
            }
            eprintln!("(개발용으로 여러 개를 띄우려면 --allow-multi 또는 {}=1)", instance::ALLOW_MULTI_ENV);
            // 더블클릭으로 켠 창은 바로 닫혀 안내를 못 읽는다 — 잠깐 보여 준다.
            if cfg!(windows) {
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
            std::process::exit(instance::EXIT_ALREADY_RUNNING);
        }
        Err(e) => {
            // 잠금 파일을 만들 수조차 없으면 가드 없이 뜨는 것보다 멈추는 편이 안전하다(두 콘솔이 같은 PLC 에 쓴다).
            eprintln!("단일 실행 잠금을 만들 수 없습니다({}): {e}", dir.display());
            std::process::exit(1);
        }
    }
}

/// 포트를 잡지 못한 흔한 두 경우를 사람 말로 — 나머지는 원래 오류 그대로.
fn bind_failure_message(addr: std::net::SocketAddr, e: &std::io::Error) -> Option<String> {
    let port = addr.port();
    match e.kind() {
        std::io::ErrorKind::AddrInUse => {
            Some(format!("포트 {port} 을(를) 이미 다른 프로그램이 쓰고 있습니다({addr}). gr-console.toml 의 [server] bind 를 다른 포트로 바꾸거나(예: \"127.0.0.1:8091\") --bind 로 덮어쓰세요."))
        }
        // Windows 10013: 방화벽·Hyper-V 예약 포트 범위 등
        std::io::ErrorKind::PermissionDenied => Some(format!("포트 {port} 에 묶을 권한이 없습니다({addr}, 예약된 포트 범위일 수 있음). gr-console.toml 의 [server] bind 를 다른 포트로 바꾸세요.")),
        std::io::ErrorKind::AddrNotAvailable => Some(format!("주소 {addr} 은(는) 이 PC 에 없습니다. gr-console.toml 의 [server] bind 를 \"127.0.0.1:{port}\" 또는 \"0.0.0.0:{port}\" 로 바꾸세요.")),
        _ => None,
    }
}

/// 설정 파일 위치와 기준 디렉터리. `--config`가 절대 경로거나 CWD에 있으면 그것, 아니면 실행 파일 옆,
/// 그것도 없으면 CWD(기본값으로 뜬다 — `Config::load`가 경고를 남긴다).
fn locate_config(given: &std::path::Path) -> (PathBuf, PathBuf) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if given.is_absolute() {
        return (given.to_path_buf(), given.parent().map(PathBuf::from).unwrap_or(cwd));
    }
    if cwd.join(given).is_file() {
        return (cwd.join(given), cwd);
    }
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from))
        && exe_dir.join(given).is_file()
    {
        return (exe_dir.join(given), exe_dir);
    }
    // 설정 파일이 어디에도 없다: 개발 체크아웃(CWD에 plc/contract)이면 CWD, 아니면 실행 파일 옆
    if cwd.join("plc/contract").is_dir() {
        return (cwd.join(given), cwd);
    }
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from)).unwrap_or(cwd);
    (exe_dir.join(given), exe_dir)
}

/// 사람이 열 주소 — `0.0.0.0`/`::`에 묶었으면 브라우저는 루프백으로 연다.
fn browse_url(addr: std::net::SocketAddr) -> String {
    let host = if addr.ip().is_unspecified() { "127.0.0.1".to_string() } else { addr.ip().to_string() };
    format!("http://{host}:{}/", addr.port())
}

/// 기본 브라우저로 연다 — 실패는 조용히 넘긴다(콘솔에 주소가 이미 찍혀 있다). 서버가 뜬 **뒤**에 부른다.
fn open_browser(url: &str) {
    let result = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if let Err(e) = result {
        tracing::warn!(url, "open_browser: {e}");
    }
}

fn cors_layer(extra: &str) -> CorsLayer {
    let mut origins: Vec<HeaderValue> = vec![HeaderValue::from_static("http://localhost:5173"), HeaderValue::from_static("http://127.0.0.1:5173")];
    let mut any = false;
    for o in extra.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if o == "*" {
            any = true;
        } else if let Ok(v) = HeaderValue::from_str(o) {
            origins.push(v);
        }
    }
    let layer = CorsLayer::new().allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE, Method::OPTIONS]).allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]);
    if any {
        tracing::warn!("CORS: any origin allowed");
        layer.allow_origin(AllowOrigin::any())
    } else {
        layer.allow_origin(AllowOrigin::list(origins))
    }
}

fn robots_is_multi(cfg: &Config) -> bool {
    cfg.robots.len() > 1
}

/// `opcua_root`(PLC 첨자, `GR[2].CMD`) → OPC UA 서버의 요소 이름(`GR[1].CMD`). 하한은 GRM 계약에서 읽는다.
fn opc_server_root(grm: &Contract, db: &str, root: &str) -> String {
    match grm.layout_db(db) {
        Ok(l) => {
            let bases = opcua_cmd::array_bases_from_paths(l.members.iter().map(|m| m.path.as_str()));
            let s = opcua_cmd::server_root_path(root, &bases);
            if s != root {
                tracing::info!(plc_root = root, server_root = %s, "OPC UA root: PLC index → server 0-based element");
            }
            s
        }
        Err(e) => {
            tracing::warn!(db, error = %e, "OPC UA root: GRM layout unavailable — using the PLC index as is");
            root.to_string()
        }
    }
}
