//! `plc-link serve | simulate | selftest`.

use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use plc_link::{Format, Framing};

use crate::client::{ConnectSpec, parse_format, parse_framing};
use crate::contract::{self, Loaded};
use crate::hub::HubConfig;
use crate::server::{self, ServeConfig};
use crate::sim::{self, SimConfig, SimTarget};
use crate::wire::Fault;

#[derive(Parser)]
#[command(name = "plc-link", version, about = "PLC socket link test server, PLC simulator and self test (docs/link/README.md)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// PLC port + control API (+ clients for passive PLCs)
    Serve(ServeArgs),
    /// Play a PLC: --server ADDR (active) or --listen ADDR (passive)
    Simulate(SimArgs),
    /// In-process server × simulator matrix; exit 1 on failure
    Selftest(SelftestArgs),
}

#[derive(Args)]
struct ContractArgs {
    /// Contract name below plc/contract, or a contract directory
    #[arg(long, default_value = "GR2_PLC")]
    contract: String,
    /// Message registry (default plc/link/messages.toml)
    #[arg(long)]
    messages: Option<PathBuf>,
    /// Do not add the embedded LNK_* UDTs when the contract lacks them
    #[arg(long)]
    no_fallback: bool,
}

impl ContractArgs {
    fn load(&self) -> anyhow::Result<Loaded> {
        let messages = self.messages.clone().unwrap_or_else(contract::default_messages);
        let l = contract::load(&contract::contract_dir(&self.contract), &[], &messages, !self.no_fallback)?;
        let reg = l.codec.registry();
        let unavailable: Vec<&str> = reg.messages.iter().filter(|m| !m.available).map(|m| m.name.as_str()).collect();
        tracing::info!(contract = %l.name, registry_hash = %format!("0x{:08X}", reg.hash()), ?unavailable, fallback_udts = ?l.fallback_udts, "contract loaded");
        Ok(l)
    }
}

fn parse_connect(s: &str) -> Result<ConnectSpec, String> {
    s.parse()
}

fn parse_fault(s: &str) -> Result<Fault, String> {
    s.parse()
}

#[derive(Args)]
struct ServeArgs {
    /// PLC port (use 0.0.0.0:2000 for a real PLC and open the Windows firewall)
    #[arg(long, default_value = "127.0.0.1:2000")]
    plc_bind: String,
    /// Control API / HTML (loopback, no authentication)
    #[arg(long, default_value = "127.0.0.1:8091")]
    api_bind: String,
    #[command(flatten)]
    contract: ContractArgs,
    /// Passive PLC: NAME=ip:port[,framing=frame|ndjson|http][,format=json|bin][,poll_ms=N] (repeatable)
    #[arg(long, value_parser = parse_connect)]
    connect: Vec<ConnectSpec>,
    /// Log ring size
    #[arg(long, default_value_t = 5000)]
    log_cap: usize,
    /// JSONL log directory (log-YYYYMMDD.jsonl)
    #[arg(long, default_value = "data/plc-link")]
    log_dir: PathBuf,
    #[arg(long)]
    no_log_file: bool,
    /// Queued / sent command time to live (s)
    #[arg(long, default_value_t = 60)]
    cmd_ttl_s: u64,
}

#[derive(Args)]
struct SimArgs {
    /// Active PLC: server PLC port to connect to
    #[arg(long, conflicts_with = "listen", required_unless_present = "listen")]
    server: Option<String>,
    /// Passive PLC: address to listen on
    #[arg(long)]
    listen: Option<String>,
    /// Hello.Plc / HTTP route name
    #[arg(long, default_value = "GR2")]
    plc: String,
    #[arg(long, default_value = "frame", value_parser = parse_framing)]
    framing: Framing,
    #[arg(long, default_value = "json", value_parser = parse_format)]
    format: Format,
    #[command(flatten)]
    contract: ContractArgs,
    #[arg(long, default_value_t = 2000)]
    measlog_ms: u64,
    #[arg(long, default_value_t = 1000)]
    status_ms: u64,
    /// HTTP-active command poll interval
    #[arg(long, default_value_t = 500)]
    poll_ms: u64,
    /// Exit after N MeasLogs were acknowledged
    #[arg(long)]
    count: Option<u64>,
    #[arg(long)]
    exit_after_s: Option<u64>,
    /// Every N-th CommandResult rejects (Data[0] = 16#80)
    #[arg(long)]
    reject_every: Option<u64>,
    /// Every N-th command is answered with Ack(--ack-code) (HTTP passive: 200 Ack)
    #[arg(long)]
    ack_every: Option<u64>,
    /// Ack code for --ack-every (runtime codes 100..105, e.g. 104 UNSUPPORTED)
    #[arg(long, default_value_t = 104)]
    ack_code: i16,
    /// Every N-th command is answered with Ack(BUSY=9) (HTTP passive: 503 Ack)
    #[arg(long)]
    busy_every: Option<u64>,
    /// split-writes | coalesce | bad-sig | bad-magic
    #[arg(long, value_parser = parse_fault)]
    fault: Option<Fault>,
    #[arg(long, default_value_t = 1000)]
    reconnect_ms: u64,
}

#[derive(Args)]
struct SelftestArgs {
    /// Write the results as JSON
    #[arg(long)]
    out: Option<PathBuf>,
    #[command(flatten)]
    contract: ContractArgs,
}

pub fn main() -> i32 {
    crate::util::init_local_offset();
    let cli = Cli::parse();
    let level = if matches!(cli.cmd, Cmd::Selftest(_)) { "warn" } else { "info" };
    let _ = tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| level.into())).try_init();
    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: tokio runtime: {e}");
            return 1;
        }
    };
    let r = rt.block_on(async move {
        match cli.cmd {
            Cmd::Serve(a) => serve(a).await,
            Cmd::Simulate(a) => simulate(a).await,
            Cmd::Selftest(a) => selftest(a).await,
        }
    });
    match r {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    }
}

async fn serve(a: ServeArgs) -> anyhow::Result<i32> {
    let loaded = a.contract.load()?;
    let hash = loaded.codec.registry().hash();
    let hub = HubConfig { log_cap: a.log_cap.max(1), cmd_ttl: Duration::from_secs(a.cmd_ttl_s), log_dir: (!a.no_log_file).then(|| a.log_dir.clone()), ..HubConfig::default() };
    let running = server::start(ServeConfig { plc_bind: a.plc_bind, api_bind: a.api_bind, contract: loaded, hub, connects: a.connect }).await?;
    println!("plc-link serve  contract {}  registry hash 0x{hash:08X}", running.hub.contract_name);
    println!("  PLC port     {}  (FRAME / NDJSON / HTTP sniffed)", running.plc_addr);
    println!("  control API  http://{}/", running.api_addr);
    tokio::signal::ctrl_c().await?;
    Ok(0)
}

async fn simulate(a: SimArgs) -> anyhow::Result<i32> {
    let loaded = a.contract.load()?;
    let target = match (&a.server, &a.listen) {
        (Some(s), _) => SimTarget::Server(s.clone()),
        (None, Some(l)) => SimTarget::Listen(l.clone()),
        (None, None) => anyhow::bail!("--server or --listen required"),
    };
    let mut cfg = SimConfig::new(&a.plc, a.framing, a.format, target);
    cfg.measlog_ms = a.measlog_ms;
    cfg.status_ms = a.status_ms;
    cfg.poll_ms = a.poll_ms;
    cfg.count = a.count;
    cfg.exit_after = a.exit_after_s.map(Duration::from_secs);
    cfg.reject_every = a.reject_every;
    cfg.ack_every = a.ack_every;
    cfg.ack_code = a.ack_code;
    cfg.busy_every = a.busy_every;
    cfg.fault = a.fault;
    cfg.reconnect = Duration::from_millis(a.reconnect_ms.max(50));
    let sim = sim::start(loaded.codec, cfg).await?;
    if let Some(addr) = sim.local_addr {
        println!("simulator listening on {addr} (passive PLC {})", a.plc);
    }
    let stats = sim.stats_handle();
    let print = || serde_json::to_string(&*stats.lock().unwrap_or_else(|p| p.into_inner())).unwrap_or_default();
    let wait = sim.wait();
    tokio::pin!(wait);
    let mut every = tokio::time::interval(Duration::from_secs(5));
    every.tick().await;
    let result = loop {
        tokio::select! {
            r = &mut wait => break r,
            _ = tokio::signal::ctrl_c() => break Ok(()),
            _ = every.tick() => println!("{}", print()),
        }
    };
    println!("{}", print());
    result.map(|_| 0)
}

async fn selftest(a: SelftestArgs) -> anyhow::Result<i32> {
    let loaded = a.contract.load()?;
    let results = crate::selftest::run_all(loaded).await?;
    println!("{}", crate::selftest::format_table(&results));
    if let Some(out) = &a.out {
        std::fs::write(out, serde_json::to_vec_pretty(&results)?)?;
        println!("results written to {}", out.display());
    }
    Ok(if results.iter().all(|r| r.ok) { 0 } else { 1 })
}
