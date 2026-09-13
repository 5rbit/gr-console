//! Control API (axum): health, registry, schema, PLC state, commands, log, SSE, conversion, HTML page.

// handlers return ready-made error responses
#![allow(clippy::result_large_err)]

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, post};
use futures::StreamExt;
use futures::stream::Stream;
use plc_link::wire_json::{udt_bytes_to_json_text, zero_udt_bytes};
use plc_link::{Format, LinkError, Message, MessageSpec, Role, Strictness};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use serde_json::{Value, json};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

use crate::hub::{CmdView, Hub, HubError, HubEvent, LogQuery};
use crate::log::Dir;
use crate::util::{hex, parse_hex};

pub const INDEX_HTML: &str = include_str!("../static/index.html");

#[derive(Clone, Debug, Serialize)]
pub struct ServerInfo {
    pub plc_bind: String,
    pub api_bind: String,
    pub contract_dir: String,
    pub messages: String,
    pub fallback_udts: Vec<String>,
    pub log_dir: Option<String>,
}

#[derive(Clone)]
pub struct ApiState {
    pub hub: Arc<Hub>,
    pub info: Arc<ServerInfo>,
}

pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/", get(|| async { Html(INDEX_HTML) }))
        .route("/api/health", get(health))
        .route("/api/registry", get(registry))
        .route("/api/schema/{msg}", get(schema))
        .route("/api/schema/{msg}/template", get(template))
        .route("/api/plcs", get(plcs))
        .route("/api/plcs/{plc}", get(plc))
        .route("/api/plcs/{plc}/last/{msg}", get(last))
        .route("/api/plcs/{plc}/command", post(command))
        .route("/api/plcs/{plc}/commands", get(commands))
        .route("/api/plcs/{plc}/commands/{id}", delete(cancel))
        .route("/api/plcs/{plc}/send/{msg}", post(send))
        .route("/api/plcs/{plc}/disconnect", post(disconnect))
        .route("/api/log", get(log))
        .route("/api/log/stream", get(stream))
        .route("/api/convert", post(convert))
        .with_state(state)
}

fn err(status: StatusCode, error: impl Into<String>, code: &str, path: Option<String>) -> Response {
    (status, axum::Json(json!({"error": error.into(), "code": code, "path": path}))).into_response()
}

fn link_err(status: StatusCode, e: &LinkError) -> Response {
    err(status, e.msg.clone(), e.code.name(), e.path.clone())
}

fn hub_err(e: HubError) -> Response {
    match e {
        HubError::NotFound => err(StatusCode::NOT_FOUND, "unknown PLC", "NOT_FOUND", None),
        HubError::NotConnected => err(StatusCode::CONFLICT, "PLC is not connected (use ?queue=1 to queue)", "NOT_CONNECTED", None),
        HubError::Unsupported(s) => err(StatusCode::CONFLICT, s, "UNSUPPORTED", None),
    }
}

fn spec_or_404<'a>(hub: &'a Hub, msg: &str) -> Result<&'a MessageSpec, Response> {
    hub.codec().registry().lookup(msg).ok_or_else(|| err(StatusCode::NOT_FOUND, format!("unknown message {msg:?}"), "UNKNOWN_TYPE", None))
}

fn raw(text: String) -> Option<Box<RawValue>> {
    RawValue::from_string(text).ok()
}

async fn health(State(s): State<ApiState>) -> Response {
    let hub = &s.hub;
    let reg = hub.codec().registry();
    let plcs = hub.plcs();
    axum::Json(json!({
        "ok": true,
        "name": "plc-link",
        "version": env!("CARGO_PKG_VERSION"),
        "contract": hub.contract_name,
        "registry_hash": format!("0x{:08X}", reg.hash()),
        "unavailable": reg.messages.iter().filter(|m| !m.available).map(|m| &m.name).collect::<Vec<_>>(),
        "started": hub.started,
        "uptime_s": hub.uptime().as_secs(),
        "log_entries": hub.log_len(),
        "plcs": plcs.len(),
        "connected": plcs.iter().filter(|p| p.connected).count(),
        "server": &*s.info,
    }))
    .into_response()
}

async fn registry(State(s): State<ApiState>) -> Response {
    let reg = s.hub.codec().registry();
    axum::Json(json!({
        "hash": reg.hash(),
        "hash_hex": format!("0x{:08X}", reg.hash()),
        "hash_text": reg.hash_text(),
        "proto_version": reg.doc.proto_version,
        "heartbeat_ms": reg.doc.heartbeat_ms,
        "max_payload": reg.doc.max_payload,
        "plc_max_payload": reg.doc.plc_max_payload,
        "messages": reg.messages,
        "errors": reg.doc.errors,
        "routes": reg.doc.routes,
    }))
    .into_response()
}

async fn schema(State(s): State<ApiState>, Path(msg): Path<String>) -> Response {
    let spec = match spec_or_404(&s.hub, &msg) {
        Ok(x) => x,
        Err(r) => return r,
    };
    let c = s.hub.codec().contract();
    let members = match (&spec.udt, spec.available) {
        (Some(u), true) => match c.layout_udt(u) {
            Ok(l) => l.members.iter().map(|m| json!({"path": m.path, "offset": m.offset, "bit": m.bit, "type": m.prim.name(), "size": m.size})).collect(),
            Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), "PARSE", None),
        },
        _ => Vec::new(),
    };
    axum::Json(json!({
        "id": spec.id, "name": spec.name, "udt": spec.udt, "dir": spec.dir, "formats": spec.formats, "ack": spec.ack, "reply": spec.reply,
        "sig": spec.sig, "sig_hex": format!("0x{:08X}", spec.sig), "size": spec.size, "json_max": spec.json_max,
        "available": spec.available, "unavailable_reason": spec.unavailable_reason, "members": members,
    }))
    .into_response()
}

fn json_text_response(text: String) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], text).into_response()
}

async fn template(State(s): State<ApiState>, Path(msg): Path<String>) -> Response {
    let spec = match spec_or_404(&s.hub, &msg) {
        Ok(x) => x,
        Err(r) => return r,
    };
    if !spec.available {
        return err(StatusCode::SERVICE_UNAVAILABLE, format!("{} is not available: {}", spec.name, spec.unavailable_reason.clone().unwrap_or_default()), "UNKNOWN_TYPE", None);
    }
    let Some(udt) = &spec.udt else { return json_text_response("null".into()) };
    let c = s.hub.codec().contract();
    match zero_udt_bytes(c, udt).and_then(|b| udt_bytes_to_json_text(c, udt, &b)) {
        Ok(t) => json_text_response(t),
        Err(e) => link_err(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

async fn plcs(State(s): State<ApiState>) -> Response {
    axum::Json(s.hub.plcs()).into_response()
}

async fn plc(State(s): State<ApiState>, Path(plc): Path<String>) -> Response {
    match s.hub.plc(&plc) {
        Some(v) => axum::Json(json!({"plc": v, "commands": s.hub.commands(&plc)})).into_response(),
        None => hub_err(HubError::NotFound),
    }
}

#[derive(Deserialize)]
struct LastQuery {
    format: Option<String>,
}

async fn last(State(s): State<ApiState>, Path((plc, msg)): Path<(String, String)>, Query(q): Query<LastQuery>) -> Response {
    let spec = match spec_or_404(&s.hub, &msg) {
        Ok(x) => x,
        Err(r) => return r,
    };
    if s.hub.plc(&plc).is_none() {
        return hub_err(HubError::NotFound);
    }
    let Some(l) = s.hub.last(&plc, spec.id) else { return err(StatusCode::NOT_FOUND, format!("no {} received from {plc} yet", spec.name), "NO_PENDING", None) };
    if q.format.as_deref() == Some("bin") {
        let mut r = l.msg.payload.clone().into_response();
        let h = r.headers_mut();
        h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
        for (k, v) in [("x-gr-type", spec.name.clone()), ("x-gr-seq", l.msg.seq.to_string()), ("x-gr-sig", format!("0x{:08X}", spec.sig)), ("x-gr-ts", l.ts.clone())] {
            if let Ok(v) = HeaderValue::from_str(&v) {
                h.insert(k, v);
            }
        }
        return r;
    }
    let data = s.hub.codec().data_json_text(&l.msg).ok().and_then(raw);
    axum::Json(json!({"plc": plc, "type": spec.name, "id": spec.id, "seq": l.msg.seq, "sig": spec.sig, "ts": l.ts, "log_id": l.log_id, "data": data, "hex": hex(&l.msg.payload)})).into_response()
}

/// Body = data JSON, or a full envelope `{"type", "seq", "sig", "data"}` (type must name `expect`).
fn body_data(hub: &Hub, body: &[u8], expect: &MessageSpec) -> Result<Value, Response> {
    if body.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(if expect.has_payload() { json!({}) } else { Value::Null });
    }
    let v: Value = serde_json::from_slice(body).map_err(|e| err(StatusCode::BAD_REQUEST, format!("body is not valid JSON: {e}"), "PARSE", None))?;
    if let Value::Object(m) = &v
        && m.contains_key("data")
        && m.contains_key("type")
    {
        let named = match &m["type"] {
            Value::String(t) => hub.codec().registry().by_name(t),
            Value::Number(n) => n.as_u64().and_then(|x| u16::try_from(x).ok()).and_then(|x| hub.codec().registry().by_id(x)),
            _ => None,
        };
        if named.map(|x| x.id) != Some(expect.id) {
            return Err(err(StatusCode::BAD_REQUEST, format!("envelope type {} is not {}", m["type"], expect.name), "UNKNOWN_TYPE", Some("type".into())));
        }
        return Ok(m["data"].clone());
    }
    Ok(v)
}

fn strict_message(hub: &Hub, spec: &MessageSpec, body: &[u8]) -> Result<Message, Response> {
    let data = body_data(hub, body, spec)?;
    hub.codec().from_json_data(&spec.name, &data, Strictness::Strict).map(|(m, _)| m).map_err(|e| link_err(StatusCode::BAD_REQUEST, &e))
}

#[derive(Deserialize)]
struct CommandQuery {
    wait_ms: Option<u64>,
    queue: Option<String>,
}

fn cmd_json(v: &CmdView) -> Value {
    json!({"id": v.id, "seq": v.seq, "state": v.state, "elapsed_ms": v.elapsed_ms, "result_type": v.result_type, "result": v.result, "result_hex": v.result_hex, "code": v.code, "http_status": v.http_status, "error": v.error})
}

async fn command(State(s): State<ApiState>, Path(plc): Path<String>, Query(q): Query<CommandQuery>, body: Bytes) -> Response {
    let hub = &s.hub;
    let spec = match hub.codec().spec_by_name("Command") {
        Ok(x) => x.clone(),
        Err(e) => return link_err(StatusCode::SERVICE_UNAVAILABLE, &e),
    };
    let msg = match strict_message(hub, &spec, &body) {
        Ok(m) => m,
        Err(r) => return r,
    };
    let queue = q.queue.as_deref().is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
    let wait = q.wait_ms.filter(|w| *w > 0).map(|w| Duration::from_millis(w.min(600_000)));
    let (view, rx) = match hub.submit(&plc, msg, queue, wait.is_some()) {
        Ok(x) => x,
        Err(e) => return hub_err(e),
    };
    let (Some(wait), Some(rx)) = (wait, rx) else {
        return (StatusCode::ACCEPTED, axum::Json(json!({"id": view.id, "seq": view.seq, "state": view.state}))).into_response();
    };
    match tokio::time::timeout(wait, rx).await {
        Ok(Ok(done)) => axum::Json(cmd_json(&done)).into_response(),
        _ => {
            let cur = hub.commands(&plc).and_then(|c| c.queue.into_iter().chain(c.inflight).chain(c.history).find(|x| x.id == view.id)).unwrap_or(view);
            let mut v = cmd_json(&cur);
            v["error"] = json!(format!("no result within {} ms", wait.as_millis()));
            (StatusCode::GATEWAY_TIMEOUT, axum::Json(v)).into_response()
        }
    }
}

async fn commands(State(s): State<ApiState>, Path(plc): Path<String>) -> Response {
    match s.hub.commands(&plc) {
        Some(c) => axum::Json(c).into_response(),
        None => hub_err(HubError::NotFound),
    }
}

async fn cancel(State(s): State<ApiState>, Path((plc, id)): Path<(String, u64)>) -> Response {
    match s.hub.cancel(&plc, id) {
        Some(v) => axum::Json(v).into_response(),
        None => err(StatusCode::NOT_FOUND, format!("no queued or in-flight command {id} for {plc}"), "NOT_FOUND", None),
    }
}

async fn send(State(s): State<ApiState>, Path((plc, msg)): Path<(String, String)>, body: Bytes) -> Response {
    let hub = &s.hub;
    let spec = match spec_or_404(hub, &msg) {
        Ok(x) => x.clone(),
        Err(r) => return r,
    };
    if !spec.receivable_by(Role::Plc) {
        return err(StatusCode::BAD_REQUEST, format!("{} is not sent to a PLC", spec.name), "DIR_NOT_ALLOWED", None);
    }
    let m = match strict_message(hub, &spec, &body) {
        Ok(m) => m,
        Err(r) => return r,
    };
    if spec.name == "Command" {
        return match hub.submit(&plc, m, false, false) {
            Ok((v, _)) => (StatusCode::ACCEPTED, axum::Json(json!({"id": v.id, "seq": v.seq, "state": v.state}))).into_response(),
            Err(e) => hub_err(e),
        };
    }
    match hub.send(&plc, m) {
        Ok(()) => (StatusCode::ACCEPTED, axum::Json(json!({"sent": spec.name}))).into_response(),
        Err(e) => hub_err(e),
    }
}

async fn disconnect(State(s): State<ApiState>, Path(plc): Path<String>) -> Response {
    match s.hub.disconnect(&plc) {
        Ok(()) => axum::Json(json!({"ok": true})).into_response(),
        Err(e) => hub_err(e),
    }
}

#[derive(Deserialize)]
struct LogParams {
    plc: Option<String>,
    msg: Option<String>,
    dir: Option<String>,
    since_id: Option<u64>,
    limit: Option<usize>,
}

async fn log(State(s): State<ApiState>, Query(p): Query<LogParams>) -> Response {
    let dir = match p.dir.as_deref().filter(|d| !d.is_empty()) {
        None => None,
        Some(d) => match Dir::parse(d) {
            Some(x) => Some(x),
            None => return err(StatusCode::BAD_REQUEST, format!("dir {d:?} (rx|tx)"), "PARSE", Some("dir".into())),
        },
    };
    let q = LogQuery { plc: p.plc.filter(|x| !x.is_empty()), msg: p.msg.filter(|x| !x.is_empty()), dir, since_id: p.since_id, limit: p.limit.unwrap_or(200).clamp(1, s.hub.cfg.log_cap.max(1)) };
    axum::Json(s.hub.log_query(&q)).into_response()
}

fn event(name: &'static str, v: impl Serialize) -> Result<Event, Infallible> {
    Ok(Event::default().event(name).json_data(v).unwrap_or_default())
}

async fn stream(State(s): State<ApiState>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = s.hub.subscribe();
    let head = futures::stream::iter(s.hub.plcs().into_iter().map(|p| event("plc", p)));
    let tail = BroadcastStream::new(rx).map(|r| match r {
        Ok(HubEvent::Msg(e)) => event("msg", &*e),
        Ok(HubEvent::Plc(p)) => event("plc", &*p),
        Ok(HubEvent::Cmd(c)) => event("cmd", &*c),
        Err(BroadcastStreamRecvError::Lagged(n)) => Ok(Event::default().event("lag").data(n.to_string())),
    });
    Sse::new(head.chain(tail)).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("ping"))
}

#[derive(Deserialize)]
struct ConvertReq {
    msg: String,
    from: String,
    value: Value,
    #[serde(default)]
    strict: bool,
}

async fn convert(State(s): State<ApiState>, body: Bytes) -> Response {
    let req: ConvertReq = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_REQUEST, format!("expected {{msg, from: \"json\"|\"bin_hex\", value}}: {e}"), "PARSE", None),
    };
    let codec = s.hub.codec();
    let spec = match codec.spec_by_name(&req.msg).or_else(|e| codec.registry().lookup(&req.msg).map(|x| codec.spec(x.id)).unwrap_or(Err(e))) {
        Ok(x) => x.clone(),
        Err(e) => return link_err(StatusCode::NOT_FOUND, &e),
    };
    let (m, report) = match req.from.as_str() {
        "json" => {
            let data = if let Value::Object(o) = &req.value
                && o.contains_key("type")
                && o.contains_key("data")
            {
                o["data"].clone()
            } else {
                req.value.clone()
            };
            match codec.from_json_data(&spec.name, &data, if req.strict { Strictness::Strict } else { Strictness::Lenient }) {
                Ok(x) => x,
                Err(e) => return link_err(StatusCode::BAD_REQUEST, &e),
            }
        }
        "bin_hex" => {
            let Some(text) = req.value.as_str() else { return err(StatusCode::BAD_REQUEST, "value must be a hex string", "PARSE", Some("value".into())) };
            let bytes = match parse_hex(text) {
                Ok(b) => b,
                Err(e) => return err(StatusCode::BAD_REQUEST, e, "PARSE", Some("value".into())),
            };
            if bytes.len() != spec.size as usize {
                return err(StatusCode::BAD_REQUEST, format!("{}: {} bytes, expected {}", spec.name, bytes.len(), spec.size), "BAD_LENGTH", Some("value".into()));
            }
            (Message { id: spec.id, seq: 0, payload: bytes }, Default::default())
        }
        other => return err(StatusCode::BAD_REQUEST, format!("from {other:?} (json|bin_hex)"), "PARSE", Some("from".into())),
    };
    let json_text = match codec.data_json_text(&m) {
        Ok(t) => t,
        Err(e) => return link_err(StatusCode::BAD_REQUEST, &e),
    };
    axum::Json(json!({
        "msg": spec.name, "id": spec.id, "sig": spec.sig, "size": spec.size,
        "json_text": json_text,
        "envelope": codec.json_text(&m).ok(),
        "bin_hex": hex(&m.payload),
        "frame_bin_hex": codec.to_frame(&m, Format::Bin).ok().map(|b| hex(&b)),
        "report": report,
    }))
    .into_response()
}
