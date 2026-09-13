//! In-process matrix: hub + simulator for PLC-active and PLC-passive FRAME×JSON, FRAME×BIN, NDJSON×JSON,
//! HTTP×JSON, HTTP×BIN, plus the NDJSON×BIN rejection. Also used by `tests/e2e_matrix.rs`.

use std::time::{Duration, Instant};

use plc_layout::ast::TypeRef;
use plc_link::framing::Framing;
use plc_link::vectors::{golden_value, zero_value};
use plc_link::{Codec, ErrCode, Format, RawFrame, Role, Strictness};
use serde::Serialize;
use serde_json::{Value, json};

use crate::client::ConnectSpec;
use crate::contract::Loaded;
use crate::httpc;
use crate::hub::{HubConfig, Mode};
use crate::server::{self, Running, ServeConfig};
use crate::sim::{self, Sim, SimConfig, SimTarget, region};
use crate::util::parse_hex;

#[derive(Clone, Debug, Serialize)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CaseResult {
    pub case: String,
    pub ok: bool,
    pub elapsed_ms: u64,
    pub checks: Vec<Check>,
}

impl CaseResult {
    fn new(case: String, t0: Instant, checks: Vec<Check>) -> Self {
        CaseResult { case, ok: !checks.is_empty() && checks.iter().all(|c| c.ok), elapsed_ms: t0.elapsed().as_millis() as u64, checks }
    }
}

fn check(checks: &mut Vec<Check>, name: &str, ok: bool, detail: impl Into<String>) -> bool {
    checks.push(Check { name: name.to_string(), ok, detail: detail.into() });
    ok
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    pub mode: Mode,
    pub framing: Framing,
    pub format: Format,
}

fn framing_name(f: Framing) -> &'static str {
    match f {
        Framing::Frame => "frame",
        Framing::Ndjson => "ndjson",
        Framing::Http => "http",
    }
}

impl Case {
    pub fn label(&self) -> String {
        format!("{} {}×{}", if self.mode == Mode::Active { "active" } else { "passive" }, framing_name(self.framing), self.format.name())
    }

    /// PLC name (fits Hello.Plc String[16]).
    pub fn plc_name(&self) -> String {
        format!("{}_{}_{}", if self.mode == Mode::Active { "A" } else { "P" }, framing_name(self.framing), self.format.name()).to_ascii_uppercase()
    }
}

pub const COMBOS: [(Framing, Format); 5] =
    [(Framing::Frame, Format::Json), (Framing::Frame, Format::Bin), (Framing::Ndjson, Format::Json), (Framing::Http, Format::Json), (Framing::Http, Format::Bin)];

pub fn matrix() -> Vec<Case> {
    [Mode::Active, Mode::Passive].into_iter().flat_map(|mode| COMBOS.into_iter().map(move |(framing, format)| Case { mode, framing, format })).collect()
}

/// Hub settings for in-process runs: no log file, fast reconnect.
pub fn test_hub_config() -> HubConfig {
    HubConfig { reconnect_min: Duration::from_millis(200), reconnect_max: Duration::from_secs(1), log_dir: None, ..HubConfig::default() }
}

pub async fn start_server(contract: Loaded) -> anyhow::Result<Running> {
    server::start(ServeConfig { plc_bind: "127.0.0.1:0".into(), api_bind: "127.0.0.1:0".into(), contract, hub: test_hub_config(), connects: Vec::new() }).await
}

pub fn fast_sim(case: &Case, target: SimTarget) -> SimConfig {
    let mut c = SimConfig::new(&case.plc_name(), case.framing, case.format, target);
    c.measlog_ms = 150;
    c.status_ms = 150;
    c.poll_ms = 50;
    c.reconnect = Duration::from_millis(200);
    c
}

pub async fn wait_until(timeout: Duration, mut f: impl FnMut() -> bool) -> bool {
    let t0 = Instant::now();
    loop {
        if f() {
            return true;
        }
        if t0.elapsed() >= timeout {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Starts the case's simulator (passive: also the server's client for it). `tweak` adjusts the config.
pub async fn start_case_sim(srv: &mut Running, case: &Case, tweak: impl FnOnce(&mut SimConfig)) -> anyhow::Result<Sim> {
    let codec = srv.hub.codec().clone();
    match case.mode {
        Mode::Active => {
            let mut cfg = fast_sim(case, SimTarget::Server(srv.plc_addr.to_string()));
            tweak(&mut cfg);
            sim::start(codec, cfg).await
        }
        Mode::Passive => {
            let mut cfg = fast_sim(case, SimTarget::Listen("127.0.0.1:0".into()));
            tweak(&mut cfg);
            let s = sim::start(codec, cfg).await?;
            let addr = s.local_addr.ok_or_else(|| anyhow::anyhow!("simulator did not report its address"))?;
            srv.add_connect(ConnectSpec { name: case.plc_name(), addr: addr.to_string(), framing: case.framing, format: case.format, poll_ms: 100 });
            Ok(s)
        }
    }
}

/// A Command body: Header set, TaskData = golden value of its UDT, everything else zero.
pub fn command_body(codec: &Codec, seq: u16) -> anyhow::Result<Value> {
    let spec = codec.spec_by_name("Command")?;
    let c = codec.contract();
    let udt = c.udt(spec.udt.as_deref().unwrap_or_default())?;
    let mut body = serde_json::Map::new();
    for f in &udt.fields {
        match (&f.ty, f.name.as_str()) {
            (TypeRef::Udt(n), "TaskData") => {
                body.insert(f.name.clone(), golden_value(c, n)?);
            }
            (TypeRef::Udt(n), "Header") => {
                let mut h = zero_value(c, n)?;
                for (k, v) in [("Protocol", json!(1)), ("CMD_ID", json!(1)), ("CMD", json!(16)), ("SRC", json!(5000)), ("DST", json!(2)), ("SEQ", json!(seq))] {
                    if h.get(k).is_some() {
                        h[k] = v;
                    }
                }
                body.insert(f.name.clone(), h);
            }
            _ => {}
        }
    }
    Ok(Value::Object(body))
}

/// CommandResult.Task bytes == Command.TaskData bytes.
pub fn task_bytes_equal(codec: &Codec, command: &[u8], result: &[u8]) -> anyhow::Result<bool> {
    let c = codec.contract();
    let cmd_udt = codec.spec_by_name("Command")?.udt.clone().unwrap_or_default();
    let res_udt = codec.spec_by_name("CommandResult")?.udt.clone().unwrap_or_default();
    let (so, ss) = region(c, &cmd_udt, "TaskData")?;
    let (d_o, ds) = region(c, &res_udt, "Task")?;
    Ok(ss == ds && command.get(so..so + ss).is_some() && command.get(so..so + ss) == result.get(d_o..d_o + ds))
}

/// Runs one case against a running server.
pub async fn run_case(srv: &mut Running, case: &Case) -> CaseResult {
    let t0 = Instant::now();
    let mut checks = Vec::new();
    let name = case.plc_name();
    let hub = srv.hub.clone();
    let codec = hub.codec().clone();
    let api = srv.api();
    let sim = match start_case_sim(srv, case, |_| {}).await {
        Ok(s) => s,
        Err(e) => {
            check(&mut checks, "simulator", false, format!("{e:#}"));
            return CaseResult::new(case.label(), t0, checks);
        }
    };
    let ok = wait_until(Duration::from_secs(8), || hub.plc(&name).is_some_and(|p| p.connected && p.registry_ok == Some(true))).await;
    let detail = hub.plc(&name).map(|p| format!("connected {} registry_ok {:?} {}", p.connected, p.registry_ok, p.last_error.unwrap_or_default())).unwrap_or_else(|| "no PLC entry".into());
    if !check(&mut checks, "hello", ok, detail) {
        return CaseResult::new(case.label(), t0, checks);
    }

    let meas_id = codec.spec_by_name("MeasLog").map(|s| s.id).unwrap_or(0);
    let status_id = codec.spec_by_name("Status").map(|s| s.id).unwrap_or(0);
    let need_ack = !(case.mode == Mode::Passive && case.framing == Framing::Http);
    let ok = wait_until(Duration::from_secs(8), || hub.last(&name, meas_id).is_some() && (!need_ack || sim.stats().measlog_acked >= 1)).await;
    let st = sim.stats();
    check(
        &mut checks,
        if need_ack { "measlog+ack" } else { "measlog" },
        ok,
        format!("sent {} acked {} ack_errors {} {}", st.measlog_sent, st.measlog_acked, st.ack_errors, st.last_error.unwrap_or_default()),
    );

    let ok = wait_until(Duration::from_secs(5), || hub.last(&name, status_id).is_some()).await;
    let (json_ok, bin_ok) = if ok {
        let j = httpc::get_json(&api, &format!("/api/plcs/{name}/last/Status")).await.map(|(s, v)| s == 200 && v["data"].is_object()).unwrap_or(false);
        let size = codec.spec(status_id).map(|s| s.size as usize).unwrap_or(0);
        let b = httpc::call(&api, "GET", &format!("/api/plcs/{name}/last/Status?format=bin"), None, Duration::from_secs(5))
            .await
            .map(|r| r.status == 200 && r.body.len() == size && r.header("x-gr-type") == Some("Status"))
            .unwrap_or(false);
        (j, b)
    } else {
        (false, false)
    };
    check(&mut checks, "status", ok && json_ok && bin_ok, format!("received {ok} json {json_ok} bin {bin_ok}"));

    let command = async {
        let body = command_body(&codec, 7)?;
        let (sent, _) = codec.from_json_data("Command", &body, Strictness::Strict)?;
        let (status, v) = httpc::post_json(&api, &format!("/api/plcs/{name}/command?wait_ms=5000"), &body, Duration::from_secs(10)).await?;
        let result = parse_hex(v["result_hex"].as_str().unwrap_or_default()).map_err(anyhow::Error::msg)?;
        let same = task_bytes_equal(&codec, &sent.payload, &result)?;
        let task_json = v["result"]["Task"] == body["TaskData"];
        anyhow::Ok((
            status == 200 && v["state"] == "done" && v["result_type"] == "CommandResult" && same && task_json,
            format!("HTTP {status} state {} seq {} task bytes equal {same} json equal {task_json} elapsed {} ms", v["state"], v["seq"], v["elapsed_ms"]),
        ))
    };
    match command.await {
        Ok((ok, d)) => check(&mut checks, "command", ok, d),
        Err(e) => check(&mut checks, "command", false, format!("{e:#}")),
    };

    match httpc::get_json(&api, &format!("/api/log?plc={name}&limit=5000")).await {
        Ok((200, Value::Array(entries))) => {
            let bad: Vec<String> = entries.iter().filter(|e| e["ok"] != true).map(|e| format!("#{} {} {}", e["id"], e["name"], e["error"])).collect();
            let rx_meas = entries.iter().any(|e| e["dir"] == "rx" && e["name"] == "MeasLog");
            let tx_cmd = entries.iter().any(|e| e["dir"] == "tx" && e["name"] == "Command");
            check(&mut checks, "log", bad.is_empty() && rx_meas && tx_cmd, format!("{} entries, rx MeasLog {rx_meas}, tx Command {tx_cmd}, errors {bad:?}", entries.len()));
        }
        other => {
            check(&mut checks, "log", false, format!("{other:?}"));
        }
    }
    drop(sim);
    CaseResult::new(case.label(), t0, checks)
}

/// NDJSON cannot carry BIN: `--connect`, the simulator, the encoder and the codec all refuse it.
pub async fn ndjson_bin_rejection(codec: &Codec) -> CaseResult {
    let t0 = Instant::now();
    let mut checks = Vec::new();
    let e = "X=127.0.0.1:1,framing=ndjson,format=bin".parse::<ConnectSpec>().unwrap_err();
    check(&mut checks, "--connect", e.contains("FORMAT_NOT_ALLOWED"), e);
    let cfg = SimConfig::new("X", Framing::Ndjson, Format::Bin, SimTarget::Server("127.0.0.1:1".into()));
    let e = sim::start(codec.clone(), cfg).await.err().map(|e| e.to_string()).unwrap_or_default();
    check(&mut checks, "simulate", e.contains("FORMAT_NOT_ALLOWED"), e);
    let hb = codec.heartbeat(1);
    let enc = hb.as_ref().map_err(|e| e.clone()).and_then(|m| crate::wire::encode(codec, Framing::Ndjson, Format::Bin, m));
    check(&mut checks, "encode", enc.as_ref().is_err_and(|e| e.code == ErrCode::FormatNotAllowed), format!("{:?}", enc.err()));
    let raw = RawFrame { framing: Framing::Ndjson, format: Format::Bin, msg_type: None, seq: None, sig: None, payload: Vec::new(), http: None };
    let dec = codec.decode_raw(&raw, Role::Pc, Strictness::Lenient);
    check(&mut checks, "decode", dec.as_ref().is_err_and(|e| e.code == ErrCode::FormatNotAllowed), format!("{:?}", dec.err()));
    CaseResult::new("ndjson×bin rejected".into(), t0, checks)
}

pub async fn run_all(contract: Loaded) -> anyhow::Result<Vec<CaseResult>> {
    let mut srv = start_server(contract).await?;
    let mut out = Vec::new();
    for case in matrix() {
        out.push(run_case(&mut srv, &case).await);
    }
    out.push(ndjson_bin_rejection(srv.hub.codec()).await);
    Ok(out)
}

pub fn format_table(results: &[CaseResult]) -> String {
    let mut s = format!("{:<24} {:<6} {:>8}  {}\n", "case", "result", "time", "checks");
    s.push_str(&format!("{}\n", "-".repeat(90)));
    for r in results {
        let names: Vec<String> = r.checks.iter().map(|c| format!("{}{}", c.name, if c.ok { "" } else { "✗" })).collect();
        s.push_str(&format!("{:<24} {:<6} {:>5} ms  {}\n", r.case, if r.ok { "PASS" } else { "FAIL" }, r.elapsed_ms, names.join(", ")));
        for c in r.checks.iter().filter(|c| !c.ok) {
            s.push_str(&format!("{:<24}   ✗ {}: {}\n", "", c.name, c.detail));
        }
    }
    let passed = results.iter().filter(|r| r.ok).count();
    s.push_str(&format!("{}\n{passed}/{} passed\n", "-".repeat(90), results.len()));
    s
}
