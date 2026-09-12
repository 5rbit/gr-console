mod cmd;
mod config;
mod console_info;
mod db;
mod demo;
mod error;
mod issue;
mod ledger;
mod measure;
mod plc;
mod registry;
mod routes;
mod scenario;
mod spa;
mod sse;
mod state;
mod status;
mod stock;
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
#[command(version, about = "GR gantry engineering console backend")]
struct Cli {
    /// Config file (TOML)
    #[arg(long, default_value = "gr-console.toml")]
    config: PathBuf,
    /// Run against in-process fake PLCs
    #[arg(long)]
    demo: bool,
    /// Override bind address, e.g. 0.0.0.0:8090
    #[arg(long)]
    bind: Option<String>,
    /// Print an example config and exit
    #[arg(long)]
    example_config: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if cli.example_config {
        println!("{}", Config::example_toml());
        return Ok(());
    }
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,opcua=warn,async_opcua=warn".into())).init();
    let mut cfg = Config::load(&cli.config)?;
    if cli.demo {
        cfg.demo = true;
    }
    if let Some(b) = cli.bind {
        cfg.server.bind = b;
    }
    std::fs::create_dir_all(&cfg.paths.data_dir)?;

    // contracts
    let mut contracts: HashMap<String, Arc<Contract>> = HashMap::new();
    for p in &cfg.plcs {
        if !contracts.contains_key(&p.contract) {
            let dir = cfg.paths.contract_dir.join(&p.contract);
            let c = Contract::load_dir(&dir).map_err(|e| anyhow::anyhow!("contract {}: {e}", dir.display()))?;
            tracing::info!(contract = %p.contract, udts = c.udts.len(), dbs = c.dbs.len(), consts = c.consts.len(), skipped = c.skipped.len(), "contract loaded");
            contracts.insert(p.contract.clone(), Arc::new(c));
        }
    }

    // demo world (fake PLCs) — rewrites hosts to the local fake servers
    let demo_world = if cfg.demo {
        let gr2 = contracts.get("GR2_PLC").cloned().ok_or_else(|| anyhow::anyhow!("demo needs GR2_PLC contract"))?;
        let grm = contracts.get("GRM_PLC").cloned().ok_or_else(|| anyhow::anyhow!("demo needs GRM_PLC contract"))?;
        let w = demo::DemoWorld::start(gr2, grm, 200).await?;
        for p in cfg.plcs.iter_mut() {
            let addr = if p.contract == "GRM_PLC" { w.grm_addr } else { w.gr2_addr };
            p.host = addr.ip().to_string();
            p.port = addr.port();
        }
        tracing::info!(gr2 = %w.gr2_addr, grm = %w.grm_addr, "demo PLCs started");
        Some(w)
    } else {
        None
    };
    let cfg = Arc::new(cfg);

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
    let registry = registry::Registry::new(db.clone());
    let measure = measure::MeasureStore::new(db.clone());
    let scenario = scenario::Runner::new(db.clone());
    let stock = stock::Stock::new(db.clone());
    let status = status::StatusBus::new();
    let (events, _) = tokio::sync::broadcast::channel(256);
    let (task_events, _) = tokio::sync::broadcast::channel::<ledger::LedgerEvent>(512);
    stock::spawn(stock.clone(), task_events.clone());

    // robots: one command port (OPC UA GR[n].CMD via GRM) + one ledger + one sync loop each
    let mut robots = Vec::new();
    for r in cfg.robots_effective() {
        let rcfg = config::CmdCfg { dst: r.dst, status_plc: r.plc.clone(), ..cfg.cmd.clone() };
        let cmd = match &demo_world {
            Some(w) => CommandPort::Demo { world: w.clone(), cfg: rcfg, last: Mutex::new(None) },
            None => {
                let o = &cfg.opcua;
                let ocfg = opcua_cmd::OpcUaConfig {
                    endpoint: o.endpoint.clone(),
                    security_policy: o.security_policy.clone(),
                    security_mode: o.security_mode.clone(),
                    auth: if o.user.is_empty() { opcua_cmd::Auth::Anonymous } else { opcua_cmd::Auth::UserPass { user: o.user.clone(), pass: o.pass.clone() } },
                    ns_hint: o.ns_hint,
                    db_name: o.db_name.clone(),
                    root_path: r.opcua_root.clone(),
                    connect_timeout_ms: o.connect_timeout_ms,
                    write_timeout_ms: o.write_timeout_ms,
                    session_timeout_ms: 60_000,
                    node_cache: o.node_cache.clone().map(|p| if robots_is_multi(&cfg) { p.with_extension(format!("gr{}.json", r.id)) } else { p }),
                    pki_dir: o.pki_dir.clone(),
                    trust_server_cert: o.trust_server_cert,
                };
                let (writer, _state_rx) = opcua_cmd::CmdWriter::spawn(ocfg);
                CommandPort::Opc { writer, cfg: rcfg, last: Mutex::new(None), endpoint: o.endpoint.clone() }
            }
        };
        let ledger = ledger::Ledger::new(db.clone(), &r.plc, Some(task_events.clone()))?;
        match plcs.get(&r.plc) {
            Some(h) => ledger::sync::spawn(ledger.clone(), h.clone(), cfg.cmd.echo_timeout_ms),
            None => tracing::error!(robot = r.id, plc = %r.plc, "robot status PLC not configured"),
        }
        tracing::info!(robot = r.id, name = %r.name, plc = %r.plc, root = %r.opcua_root, dst = r.dst, "robot");
        robots.push(state::RobotCtx { id: r.id, name: r.name.clone(), plc: r.plc.clone(), opcua_root: r.opcua_root.clone(), dst: r.dst, cmd: Arc::new(cmd), ledger });
    }
    let robots = Arc::new(robots);
    let first = robots.first().expect("at least one robot");
    let cmd = first.cmd.clone();
    let ledger = first.ledger.clone();

    if let Some(h) = plcs.get(&cfg.cmd.status_plc) {
        status.follow(h.clone(), "WEBMON".into(), if cfg.demo { "demo" } else { "plc" });
        measure.attach(h.clone());
    } else {
        tracing::error!(plc = %cfg.cmd.status_plc, "status PLC not configured");
    }

    let st = AppState { cfg: cfg.clone(), plcs, cmd, robots, task_events, db, ledger, registry, measure, scenario, stock, status, events };

    // demo: seed registries from the fake PLC tables once they are readable
    if cfg.demo {
        let st2 = st.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            for (plc, what) in [("GR2", "cells"), ("GR2", "stations")] {
                let Ok(h) = st2.plc(plc) else { continue };
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
            let _ = st2.measure.sync(st2.plc("GR2").unwrap()).await;
        });
    }

    // web
    let base = console_info::router(cfg.demo).merge(routes::router(st));
    let (app, web_source) = spa::attach(base, cfg.paths.web_dir.as_deref());
    let app = app.layer(cors_layer(&cfg.server.cors));
    let addr: std::net::SocketAddr = cfg.server.bind.parse().map_err(|e| anyhow::anyhow!("bind {}: {e}", cfg.server.bind))?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, web = %web_source, demo = cfg.demo, "gr-console listening");
    println!("gr-console  http://{addr}/  (web: {web_source}, demo: {})", cfg.demo);
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
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
