//! Clients for passive PLCs (`--connect NAME=ip:port,framing=…,format=…,poll_ms=N`): FRAME / NDJSON sessions
//! where the PC sends Hello first, or HTTP polling of `GET /api/status` / `/api/measlog/last` with commands via
//! `POST /api/command`. Reconnect backoff 1 → 10 s.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use plc_link::framing::PC_MAX_PAYLOAD;
use plc_link::framing::http::write_get;
use plc_link::{Format, Framing, LinkError, Message, RawFrame, Report, Role, Strictness};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::httpc::HttpConn;
use crate::hub::{Hub, Mode, Outbound};
use crate::log::Dir;
use crate::session::{self, SessionParams, error_entry, hello_data, report_warnings};
use crate::util::SeqGen;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectSpec {
    pub name: String,
    pub addr: String,
    pub framing: Framing,
    pub format: Format,
    pub poll_ms: u64,
}

pub fn parse_framing(s: &str) -> Result<Framing, String> {
    match s.to_ascii_lowercase().as_str() {
        "frame" => Ok(Framing::Frame),
        "ndjson" => Ok(Framing::Ndjson),
        "http" => Ok(Framing::Http),
        _ => Err(format!("framing {s:?} (frame|ndjson|http)")),
    }
}

pub fn parse_format(s: &str) -> Result<Format, String> {
    match s.to_ascii_lowercase().as_str() {
        "json" => Ok(Format::Json),
        "bin" => Ok(Format::Bin),
        _ => Err(format!("format {s:?} (json|bin)")),
    }
}

/// NDJSON cannot carry BIN (FORMAT_NOT_ALLOWED).
pub fn check_combo(framing: Framing, format: Format) -> Result<(), String> {
    if framing == Framing::Ndjson && format == Format::Bin { Err("FORMAT_NOT_ALLOWED: NDJSON carries JSON only (use framing=frame or http for BIN)".into()) } else { Ok(()) }
}

impl FromStr for ConnectSpec {
    type Err = String;

    /// `NAME=ip:port[,framing=frame|ndjson|http][,format=json|bin][,poll_ms=N]`
    fn from_str(s: &str) -> Result<Self, String> {
        let (name, rest) = s.split_once('=').ok_or_else(|| format!("--connect {s:?}: expected NAME=ip:port,..."))?;
        let name = name.trim();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err(format!("--connect: bad PLC name {name:?}"));
        }
        let mut parts = rest.split(',');
        let addr = parts.next().unwrap_or("").trim().to_string();
        if !addr.contains(':') {
            return Err(format!("--connect {name}: address {addr:?} must be ip:port"));
        }
        let mut spec = ConnectSpec { name: name.to_string(), addr, framing: Framing::Frame, format: Format::Json, poll_ms: 500 };
        for kv in parts {
            let (k, v) = kv.split_once('=').ok_or_else(|| format!("--connect {name}: expected key=value, got {kv:?}"))?;
            match k.trim() {
                "framing" => spec.framing = parse_framing(v.trim())?,
                "format" => spec.format = parse_format(v.trim())?,
                "poll_ms" => spec.poll_ms = v.trim().parse().map_err(|_| format!("--connect {name}: poll_ms {v:?}"))?,
                other => return Err(format!("--connect {name}: unknown key {other:?} (framing, format, poll_ms)")),
            }
        }
        check_combo(spec.framing, spec.format)?;
        spec.poll_ms = spec.poll_ms.max(50);
        Ok(spec)
    }
}

/// Registers the PLC and starts its reconnecting client task.
pub fn spawn(hub: Arc<Hub>, spec: ConnectSpec) -> tokio::task::JoinHandle<()> {
    hub.ensure_plc(&spec.name, Mode::Passive, Some(spec.framing), Some(spec.format), Some(spec.addr.clone()));
    tokio::spawn(run(hub, spec))
}

async fn run(hub: Arc<Hub>, spec: ConnectSpec) {
    let mut backoff = hub.cfg.reconnect_min;
    loop {
        match tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(&spec.addr)).await {
            Ok(Ok(s)) => {
                let _ = s.set_nodelay(true);
                let ok = match spec.framing {
                    Framing::Http => http_poller(&hub, &spec, s).await,
                    f => {
                        let p = SessionParams { framing: f, mode: Mode::Passive, opener: true, plc: Some(spec.name.clone()), format: spec.format, peer: spec.addr.clone() };
                        let end = session::run(hub.clone(), s, Vec::new(), p).await;
                        tracing::info!(plc = %spec.name, reason = %end.reason, "passive PLC session ended");
                        end.hello_seen
                    }
                };
                if ok {
                    backoff = hub.cfg.reconnect_min;
                }
            }
            Ok(Err(e)) => hub.connect_failed(&spec.name, format!("connect {}: {e}", spec.addr)),
            Err(_) => hub.connect_failed(&spec.name, format!("connect {}: timeout", spec.addr)),
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(hub.cfg.reconnect_max);
    }
}

struct Poller<'a> {
    hub: &'a Hub,
    spec: &'a ConnectSpec,
    conn: HttpConn,
    seq: SeqGen,
}

const TIMEOUT: Duration = Duration::from_secs(5);

impl Poller<'_> {
    fn format(&self) -> Format {
        if self.hub.registry_ok(&self.spec.name) == Some(false) { Format::Json } else { self.spec.format }
    }

    fn log_error(&self, raw: &RawFrame, e: &LinkError) {
        self.hub.log(error_entry(self.hub, &self.spec.name, raw, e, raw.payload.len(), &self.spec.addr), None);
    }

    fn log_rx(&self, m: &Message, raw: &RawFrame, report: &Report) {
        let mut e = self.hub.msg_entry(&self.spec.name, Dir::Rx, Framing::Http, raw.format, m, raw.payload.len(), Some(&self.spec.addr));
        e.warnings = report_warnings(report);
        self.hub.log(e, Some(m));
    }

    /// Decodes a 200 response; 204 → None; other statuses and decode errors are logged → None.
    fn decode(&self, what: &str, raw: RawFrame) -> Option<(Message, RawFrame, Report)> {
        let status = raw.http.as_ref().and_then(|h| h.status).unwrap_or(0);
        if status == 204 {
            return None;
        }
        if status != 200 {
            self.log_error(&raw, &LinkError::parse(format!("{what}: HTTP {status}")));
            return None;
        }
        if raw.format == Format::Bin && self.hub.registry_ok(&self.spec.name) == Some(false) && what != "GET /api/hello" {
            self.log_error(&raw, &LinkError::new(plc_link::ErrCode::FormatNotAllowed, "registry hash mismatch (contract_mismatch): BIN refused"));
            return None;
        }
        match self.hub.codec().decode_raw_report(&raw, Role::Pc, Strictness::Lenient) {
            Ok((m, r)) => Some((m, raw, r)),
            Err(e) => {
                self.log_error(&raw, &e);
                None
            }
        }
    }

    async fn get(&mut self, path: &str) -> anyhow::Result<Option<(Message, RawFrame, Report)>> {
        let req = write_get(path, &self.conn.host, self.format());
        let raw = self.conn.roundtrip(&req, TIMEOUT).await?;
        Ok(self.decode(&format!("GET {path}"), raw))
    }

    async fn hello(&mut self) -> anyhow::Result<bool> {
        let Some((m, raw, report)) = self.get("/api/hello").await? else { return Ok(false) };
        let h = self.hub.codec().hello_info(&m);
        let mut e = self.hub.msg_entry(&self.spec.name, Dir::Rx, Framing::Http, raw.format, &m, raw.payload.len(), Some(&self.spec.addr));
        e.warnings = report_warnings(&report);
        if !h.registry_ok {
            e.warnings.push("registry hash mismatch (contract_mismatch, BIN refused)".into());
        }
        self.hub.log(e, Some(&m));
        self.hub.set_hello(&self.spec.name, hello_data(&h), h.registry_ok);
        Ok(true)
    }

    /// POST /api/command → CommandResult (or Ack). Outer error = connection problem.
    async fn command(&mut self, id: Option<u64>, mut m: Message) -> anyhow::Result<()> {
        let name = self.spec.name.clone();
        m.seq = self.seq.next();
        let fmt = self.format();
        let codec = self.hub.codec().clone();
        let req = match codec.to_http_request(&m, fmt, "POST", "/api/command", &self.conn.host) {
            Ok(r) => r,
            Err(e) => {
                if let Some(id) = id {
                    self.hub.command_failed(&name, id, e.to_string());
                }
                return Ok(());
            }
        };
        let body_len = codec.http_parts(&m, fmt).map(|p| p.1.len()).unwrap_or(0);
        self.hub.log(self.hub.msg_entry(&name, Dir::Tx, Framing::Http, fmt, &m, body_len, Some(&self.spec.addr)), Some(&m));
        if let Some(id) = id {
            self.hub.command_sent(&name, id, m.seq);
        }
        let raw = match self.conn.roundtrip(&req, TIMEOUT).await {
            Ok(r) => r,
            Err(e) => {
                if let Some(id) = id {
                    self.hub.command_failed(&name, id, e.to_string());
                }
                return Err(e);
            }
        };
        // 200 CommandResult | 200 Ack(rc) | 400 Ack(read error) | 503 Ack(BUSY)
        let status = raw.http.as_ref().and_then(|h| h.status).unwrap_or(0);
        if raw.payload.is_empty() {
            self.log_error(&raw, &LinkError::parse(format!("POST /api/command: HTTP {status} without body")));
            if let Some(id) = id {
                self.hub.command_failed_http(&name, id, format!("HTTP {status} without body"), Some(status));
            }
            return Ok(());
        }
        match codec.decode_raw_report(&raw, Role::Pc, Strictness::Lenient) {
            Ok((r, report)) => {
                let mut e = self.hub.msg_entry(&name, Dir::Rx, Framing::Http, raw.format, &r, raw.payload.len(), Some(&self.spec.addr));
                e.warnings = report_warnings(&report);
                if status != 200 {
                    e.warnings.push(format!("HTTP {status}"));
                }
                self.hub.log(e, Some(&r));
                let matched = match id {
                    Some(id) => self.hub.command_response(&name, id, &r, status),
                    None => self.hub.command_reply(&name, &r),
                };
                if matched.is_none()
                    && let Some(id) = id
                {
                    let what = codec.spec(r.id).map(|s| s.name.clone()).unwrap_or_default();
                    self.hub.command_failed_http(&name, id, format!("HTTP {status}: unexpected {what} in the response"), Some(status));
                }
            }
            Err(e) => {
                self.log_error(&raw, &e);
                if let Some(id) = id {
                    self.hub.command_failed_http(&name, id, format!("HTTP {status}: {e}"), Some(status));
                }
            }
        }
        Ok(())
    }
}

/// Returns true when the Hello succeeded (backoff reset).
async fn http_poller(hub: &Arc<Hub>, spec: &ConnectSpec, stream: TcpStream) -> bool {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let conn_id = hub.attach(&spec.name, Mode::Passive, Framing::Http, Some(spec.format), &spec.addr, Some(tx));
    let mut p = Poller { hub, spec, conn: HttpConn::new(stream, &spec.addr, PC_MAX_PAYLOAD), seq: SeqGen::default() };
    let mut hello_ok = false;
    let result: anyhow::Result<()> = async {
        hello_ok = p.hello().await?;
        for (id, m) in hub.take_queued(&spec.name) {
            p.command(Some(id), m).await?;
        }
        let mut poll = tokio::time::interval(Duration::from_millis(spec.poll_ms));
        let mut last_meas: Option<(u16, Vec<u8>)> = None;
        let mut last_status: Option<Vec<u8>> = None;
        let command_id = hub.codec().registry().by_name("Command").map(|m| m.id);
        loop {
            tokio::select! {
                o = rx.recv() => match o {
                    Some(Outbound::Command { id, msg }) => p.command(Some(id), msg).await?,
                    Some(Outbound::Send(m)) if Some(m.id) == command_id => p.command(None, m).await?,
                    Some(Outbound::Send(m)) => {
                        let mut e = hub.msg_entry(&spec.name, Dir::Tx, Framing::Http, spec.format, &m, 0, Some(&spec.addr));
                        e.ok = false;
                        e.error = Some("a passive HTTP PLC only accepts Command (POST /api/command)".into());
                        hub.log(e, None);
                    }
                    Some(Outbound::Disconnect) | None => anyhow::bail!("disconnect requested"),
                },
                _ = poll.tick() => {
                    if let Some((m, raw, r)) = p.get("/api/status").await?
                        && last_status.as_ref() != Some(&m.payload) {
                            p.log_rx(&m, &raw, &r);
                            last_status = Some(m.payload.clone());
                        }
                    if let Some((m, raw, r)) = p.get("/api/measlog/last").await? {
                        let key = (m.seq, m.payload.clone());
                        if last_meas.as_ref() != Some(&key) {
                            p.log_rx(&m, &raw, &r);
                            last_meas = Some(key);
                        }
                    }
                    if !hello_ok {
                        hello_ok = p.hello().await?;
                    }
                }
            }
        }
    }
    .await;
    let reason = result.err().map(|e| e.to_string());
    hub.detach(&spec.name, conn_id, reason);
    hello_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_spec_parses() {
        let s: ConnectSpec = "GR2=192.168.0.10:2001,framing=http,format=bin,poll_ms=200".parse().unwrap();
        assert_eq!(s, ConnectSpec { name: "GR2".into(), addr: "192.168.0.10:2001".into(), framing: Framing::Http, format: Format::Bin, poll_ms: 200 });
        let d: ConnectSpec = "X=127.0.0.1:1".parse().unwrap();
        assert_eq!((d.framing, d.format, d.poll_ms), (Framing::Frame, Format::Json, 500));
        assert!("GR2=1.2.3.4:5,framing=ndjson,format=bin".parse::<ConnectSpec>().unwrap_err().contains("FORMAT_NOT_ALLOWED"));
        assert!("GR2".parse::<ConnectSpec>().is_err());
        assert!("GR2=host".parse::<ConnectSpec>().is_err());
        assert!("GR2=h:1,foo=1".parse::<ConnectSpec>().is_err());
    }
}
