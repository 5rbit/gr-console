//! End-to-end: hub on port 0 + in-process simulators. Every test is bounded by a timeout.

use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use plc_link::framing::http::write_request;
use plc_link::framing::{ContentLength, Framing};
use plc_link::vectors::golden_bytes;
use plc_link::{Codec, Format, Message, Role, Strictness};
use plc_link_server::contract::{self, Loaded};
use plc_link_server::httpc::{self, HttpConn};
use plc_link_server::hub::Mode;
use plc_link_server::selftest::{Case, command_body, fast_sim, matrix, ndjson_bin_rejection, run_case, start_case_sim, start_server, wait_until};
use plc_link_server::server::Running;
use plc_link_server::sim::{self, SimTarget};
use plc_link_server::wire::{Fault, StreamDecoder};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// GR2_PLC + the plc-link fixture LNK UDTs (fill-in only) + embedded fallbacks: independent of `gr-contract sync`.
fn test_contract() -> Loaded {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/plc-link/tests/fixtures/link");
    contract::load(&contract::contract_dir("GR2_PLC"), &[fixture], &contract::default_messages(), true).expect("test contract")
}

async fn server() -> Running {
    start_server(test_contract()).await.expect("server")
}

async fn within<F: Future>(secs: u64, f: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(secs), f).await.expect("test timed out")
}

fn case(mode: Mode, framing: Framing, format: Format) -> Case {
    Case { mode, framing, format }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn matrix_active_and_passive() {
    within(180, async {
        let mut srv = server().await;
        for c in matrix() {
            let r = run_case(&mut srv, &c).await;
            assert!(r.ok, "{}: {:#?}", r.case, r.checks);
        }
        let r = ndjson_bin_rejection(srv.hub.codec()).await;
        assert!(r.ok, "{:#?}", r.checks);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn split_writes_and_coalesce() {
    within(120, async {
        for fault in [Fault::SplitWrites, Fault::Coalesce] {
            let mut srv = server().await;
            for c in [
                case(Mode::Active, Framing::Frame, Format::Bin),
                case(Mode::Active, Framing::Ndjson, Format::Json),
                case(Mode::Active, Framing::Http, Format::Json),
                case(Mode::Passive, Framing::Frame, Format::Json),
            ] {
                let name = c.plc_name();
                let sim = start_case_sim(&mut srv, &c, |cfg| cfg.fault = Some(fault)).await.unwrap();
                let hub = srv.hub.clone();
                let ok = wait_until(Duration::from_secs(10), || sim.stats().measlog_acked >= 3 && hub.plc(&name).is_some_and(|p| p.registry_ok == Some(true))).await;
                assert!(ok, "{fault:?} {}: {:?} {:?}", c.label(), sim.stats(), hub.plc(&name));
                assert_eq!(sim.stats().ack_errors, 0, "{fault:?} {}", c.label());
                let errors: Vec<_> = hub.log_query(&Default::default()).into_iter().filter(|e| e.plc == name && !e.ok).collect();
                assert!(errors.is_empty(), "{fault:?} {}: {errors:?}", c.label());
            }
        }
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bad_sig_acks_4_and_the_link_stays() {
    within(60, async {
        let mut srv = server().await;
        for c in [case(Mode::Active, Framing::Frame, Format::Bin), case(Mode::Active, Framing::Http, Format::Bin)] {
            let name = c.plc_name();
            let sim = start_case_sim(&mut srv, &c, |cfg| cfg.fault = Some(Fault::BadSig)).await.unwrap();
            let hub = srv.hub.clone();
            let status_id = hub.codec().spec_by_name("Status").unwrap().id;
            let ok = wait_until(Duration::from_secs(10), || sim.stats().ack_errors >= 3 && hub.last(&name, status_id).is_some()).await;
            let st = sim.stats();
            assert!(ok, "{}: {st:?}", c.label());
            assert_eq!(st.measlog_acked, 0, "{}: every MeasLog is rejected: {st:?}", c.label());
            if c.framing == Framing::Frame {
                // over HTTP every POST (Status too) gets an Ack, so only FRAME has the MeasLog Ack last
                assert_eq!(st.last_ack_code, Some(4), "{}: MeasLog Ack code SIG_MISMATCH: {st:?}", c.label());
            }
            let p = hub.plc(&name).unwrap();
            assert!(p.connected, "{}", c.label());
            if c.framing == Framing::Frame {
                assert_eq!((st.connects, p.counters.connects), (1, 1), "session kept");
            }
            let entries = hub.log_query(&Default::default());
            assert!(entries.iter().any(|e| e.plc == name && !e.ok && e.error.as_deref().is_some_and(|x| x.contains("SIG_MISMATCH"))), "{}", c.label());
        }
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bad_magic_closes_then_reconnects() {
    within(60, async {
        let mut srv = server().await;
        for c in [case(Mode::Active, Framing::Frame, Format::Json), case(Mode::Passive, Framing::Frame, Format::Bin), case(Mode::Active, Framing::Http, Format::Json)] {
            let name = c.plc_name();
            let sim = start_case_sim(&mut srv, &c, |cfg| cfg.fault = Some(Fault::BadMagic)).await.unwrap();
            let hub = srv.hub.clone();
            let ok = wait_until(Duration::from_secs(15), || {
                let st = sim.stats();
                // an HTTP-active link is "connected" by its last request, a TCP reconnect does not count
                st.bad_magic_sent == 1 && st.connects >= 2 && hub.plc(&name).is_some_and(|p| p.connected && p.registry_ok == Some(true) && (c.framing == Framing::Http || p.counters.connects >= 2))
            })
            .await;
            assert!(ok, "{}: {:?} {:?}", c.label(), sim.stats(), hub.plc(&name));
            if c.mode == Mode::Active && c.framing == Framing::Frame {
                let entries = hub.log_query(&Default::default());
                assert!(entries.iter().any(|e| e.plc == name && e.error.as_deref().is_some_and(|x| x.contains("BAD_MAGIC"))));
                assert!(sim.stats().ack_errors >= 1, "Ack BAD_MAGIC before close");
            }
            let meas_before = sim.stats().measlog_acked;
            assert!(wait_until(Duration::from_secs(5), || sim.stats().measlog_acked > meas_before || c.mode == Mode::Passive).await);
        }
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn strict_api_errors_and_queue() {
    within(60, async {
        let mut srv = server().await;
        let api = srv.api();
        let c = case(Mode::Active, Framing::Frame, Format::Json);
        let name = c.plc_name();
        let sim = start_case_sim(&mut srv, &c, |_| {}).await.unwrap();
        let hub = srv.hub.clone();
        assert!(wait_until(Duration::from_secs(8), || hub.plc(&name).is_some_and(|p| p.registry_ok == Some(true))).await);
        let path = format!("/api/plcs/{name}/command");
        let t = Duration::from_secs(10);

        let (s, v) = httpc::post_json(&api, &path, &json!({"Header": {"Bogus": 1}}), t).await.unwrap();
        assert_eq!((s, v["code"].as_str()), (400, Some("PARSE")), "{v}");
        assert!(v["path"].as_str().unwrap_or_default().contains("Bogus"), "{v}");
        let (s, v) = httpc::post_json(&api, &path, &json!({"Header": {"SRC": 70000}}), t).await.unwrap();
        assert_eq!(s, 400, "{v}");
        assert!(v["path"].as_str().unwrap_or_default().contains("SRC"), "{v}");
        assert!(v["error"].as_str().is_some());
        let (s, v) = httpc::post_json(&api, &path, &json!({"type": "Status", "seq": 1, "sig": 0, "data": {}}), t).await.unwrap();
        assert_eq!(s, 400, "{v}");
        let r = httpc::call(&api, "POST", &path, Some(("application/json", b"{nope")), t).await.unwrap();
        assert_eq!(r.status, 400);

        // envelope body accepted, 202 without wait
        let body = command_body(hub.codec(), 3).unwrap();
        let (s, v) = httpc::post_json(&api, &path, &json!({"type": "Command", "seq": 0, "sig": 0, "data": body}), t).await.unwrap();
        assert_eq!(s, 202, "{v}");
        assert!(v["id"].as_u64().is_some());

        // unknown PLC: 404, queue=1 → queued, cancel
        let (s, _) = httpc::post_json(&api, "/api/plcs/NOPE/command", &body, t).await.unwrap();
        assert_eq!(s, 404);
        let (s, v) = httpc::post_json(&api, "/api/plcs/NOPE/command?queue=1", &body, t).await.unwrap();
        assert_eq!((s, v["state"].as_str()), (202, Some("queued")), "{v}");
        let id = v["id"].as_u64().unwrap();
        let (s, v) = httpc::get_json(&api, "/api/plcs/NOPE/commands").await.unwrap();
        assert_eq!((s, v["queue"].as_array().map(Vec::len)), (200, Some(1)));
        let r = httpc::call(&api, "DELETE", &format!("/api/plcs/NOPE/commands/{id}"), None, t).await.unwrap();
        assert_eq!((r.status, r.json().unwrap()["state"].as_str().map(str::to_string)), (200, Some("cancelled".to_string())));

        // wait_ms timeout → 504 with a queued command
        let (s, v) = httpc::post_json(&api, "/api/plcs/NOPE/command?queue=1&wait_ms=200", &body, t).await.unwrap();
        assert_eq!((s, v["state"].as_str()), (504, Some("queued")), "{v}");

        // disconnected PLC without queue → 409
        drop(sim);
        assert!(wait_until(Duration::from_secs(5), || hub.plc(&name).is_some_and(|p| !p.connected)).await);
        let (s, v) = httpc::post_json(&api, &path, &body, t).await.unwrap();
        assert_eq!((s, v["code"].as_str()), (409, Some("NOT_CONNECTED")), "{v}");

        // schema, template (declaration order), registry, health, convert
        let (s, v) = httpc::get_json(&api, "/api/schema/Command").await.unwrap();
        assert_eq!(s, 200);
        assert!(v["members"].as_array().is_some_and(|m| m.len() > 10));
        let r = httpc::call(&api, "GET", "/api/schema/command/template", None, t).await.unwrap();
        assert!(String::from_utf8_lossy(&r.body).starts_with("{\"NTP\":\"1970-01-01T00:00:00.000\",\"Header\":"), "{}", String::from_utf8_lossy(&r.body));
        let (_, v) = httpc::get_json(&api, "/api/registry").await.unwrap();
        assert_eq!(v["hash"].as_u64(), Some(hub.codec().registry().hash() as u64));
        let (s, v) = httpc::get_json(&api, "/api/health").await.unwrap();
        assert_eq!((s, v["ok"].as_bool()), (200, Some(true)));
        let (s, v) = httpc::post_json(&api, "/api/convert", &json!({"msg": "Command", "from": "json", "value": body}), t).await.unwrap();
        assert_eq!(s, 200, "{v}");
        let size = hub.codec().spec_by_name("Command").unwrap().size as usize;
        assert_eq!(v["bin_hex"].as_str().unwrap().len(), size * 2);
        assert_eq!(v["frame_bin_hex"].as_str().unwrap().len(), (size + 16) * 2);
        let (s, back) = httpc::post_json(&api, "/api/convert", &json!({"msg": "command", "from": "bin_hex", "value": v["bin_hex"]}), t).await.unwrap();
        assert_eq!((s, &back["json_text"]), (200, &v["json_text"]));
        let (s, _) = httpc::post_json(&api, "/api/convert", &json!({"msg": "Command", "from": "bin_hex", "value": "00"}), t).await.unwrap();
        assert_eq!(s, 400);
        let r = httpc::call(&api, "GET", "/", None, t).await.unwrap();
        assert!(r.status == 200 && String::from_utf8_lossy(&r.body).contains("addEventListener('msg'"));
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sse_delivers_named_msg_events() {
    within(30, async {
        let mut srv = server().await;
        let mut s = TcpStream::connect(srv.api_addr).await.unwrap();
        s.write_all(b"GET /api/log/stream HTTP/1.1\r\nHost: test\r\nAccept: text/event-stream\r\n\r\n").await.unwrap();
        let _sim = start_case_sim(&mut srv, &case(Mode::Active, Framing::Ndjson, Format::Json), |_| {}).await.unwrap();
        let mut got = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = tokio::time::timeout(Duration::from_secs(10), s.read(&mut buf)).await.expect("SSE timeout").unwrap();
            assert!(n > 0, "SSE closed");
            got.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&got);
            if let Some(i) = text.find("event: msg") {
                let rest = &text[i..];
                if let Some(d) = rest.find("data: {")
                    && rest[d..].contains('\n')
                {
                    assert!(text.starts_with("HTTP/1.1 200"));
                    assert!(text.to_ascii_lowercase().contains("content-type: text/event-stream"));
                    assert!(rest[d..].contains("\"dir\":"));
                    break;
                }
            }
        }
    })
    .await;
}

async fn read_msg(s: &mut TcpStream, dec: &mut StreamDecoder, codec: &Codec, want: &str) -> Message {
    let mut buf = [0u8; 4096];
    loop {
        while let Some(raw) = dec.next().unwrap() {
            let m = codec.decode_raw(&raw, Role::Plc, Strictness::Lenient).unwrap();
            if codec.spec(m.id).unwrap().name == want {
                return m;
            }
        }
        let n = tokio::time::timeout(Duration::from_secs(5), s.read(&mut buf)).await.expect("read timeout").unwrap();
        assert!(n > 0, "closed while waiting for {want}");
        dec.feed(&buf[..n]);
    }
}

fn mismatched_hello(codec: &Codec, plc: &str) -> Message {
    let h = codec.hello(plc, 3, 1).unwrap();
    let mut v = codec.data_value(&h).unwrap();
    v["RegistryHash"] = json!(codec.registry().hash() ^ 1);
    let (mut m, _) = codec.from_json_data("Hello", &v, Strictness::Strict).unwrap();
    m.seq = 1;
    m
}

fn ack_code(codec: &Codec, m: &Message) -> (i64, i64) {
    let v: Value = codec.data_value(m).unwrap();
    (v["RefType"].as_i64().unwrap(), v["Code"].as_i64().unwrap())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn registry_hash_mismatch_refuses_bin_accepts_json() {
    within(30, async {
        let srv = server().await;
        let codec = srv.hub.codec().clone();
        let meas = codec.spec_by_name("MeasLog").unwrap().clone();
        let payload = golden_bytes(codec.contract(), meas.udt.as_deref().unwrap()).unwrap();

        // FRAME session
        let mut s = TcpStream::connect(srv.plc_addr).await.unwrap();
        let mut dec = StreamDecoder::pc(Framing::Frame).unwrap();
        s.write_all(&codec.to_frame(&mismatched_hello(&codec, "MISMATCH"), Format::Json).unwrap()).await.unwrap();
        read_msg(&mut s, &mut dec, &codec, "Hello").await;
        let hub = srv.hub.clone();
        assert!(wait_until(Duration::from_secs(5), || hub.plc("MISMATCH").is_some_and(|p| p.registry_ok == Some(false) && p.contract_mismatch)).await);
        let m = Message { id: meas.id, seq: 2, payload: payload.clone() };
        s.write_all(&codec.to_frame(&m, Format::Bin).unwrap()).await.unwrap();
        let ack = read_msg(&mut s, &mut dec, &codec, "Ack").await;
        assert_eq!(ack_code(&codec, &ack), (meas.id as i64, 7), "BIN refused with FORMAT_NOT_ALLOWED");
        let m = Message { seq: 3, ..m };
        s.write_all(&codec.to_frame(&m, Format::Json).unwrap()).await.unwrap();
        let ack = read_msg(&mut s, &mut dec, &codec, "Ack").await;
        assert_eq!(ack_code(&codec, &ack), (meas.id as i64, 0), "JSON accepted");
        assert!(hub.last("MISMATCH", meas.id).is_some_and(|l| l.msg.seq == 3));

        // HTTP-active
        let mut c = HttpConn::connect(&srv.plc_addr.to_string(), 65535, Duration::from_secs(5)).await.unwrap();
        let host = c.host.clone();
        let post = |m: &Message, fmt: Format, route: &str| codec.to_http_request(m, fmt, "POST", &format!("/api/plc/MISM_HTTP/{route}"), &host).unwrap();
        let r = c.roundtrip(&post(&mismatched_hello(&codec, "MISM_HTTP"), Format::Json, "hello"), Duration::from_secs(5)).await.unwrap();
        assert_eq!(ack_code(&codec, &codec.decode_raw(&r, Role::Plc, Strictness::Lenient).unwrap()).1, 0);
        assert_eq!(hub.plc("MISM_HTTP").and_then(|p| p.registry_ok), Some(false));
        let r = c.roundtrip(&post(&m, Format::Bin, "measlog"), Duration::from_secs(5)).await.unwrap();
        assert_eq!(r.http.as_ref().and_then(|h| h.status), Some(200));
        assert_eq!(ack_code(&codec, &codec.decode_raw(&r, Role::Plc, Strictness::Lenient).unwrap()), (meas.id as i64, 7));
        let r = c.roundtrip(&post(&m, Format::Json, "measlog"), Duration::from_secs(5)).await.unwrap();
        assert_eq!(ack_code(&codec, &codec.decode_raw(&r, Role::Plc, Strictness::Lenient).unwrap()), (meas.id as i64, 0));

        // HTTP framing errors: 411 without Content-Length, 404 unknown route
        let req = write_request("POST", "/api/plc/MISM_HTTP/measlog", &host, &[("Content-Type", "application/json")], None, ContentLength::Plain);
        let mut c2 = HttpConn::connect(&srv.plc_addr.to_string(), 65535, Duration::from_secs(5)).await.unwrap();
        let r = c2.roundtrip(&req, Duration::from_secs(5)).await.unwrap();
        assert_eq!(r.http.as_ref().and_then(|h| h.status), Some(411));
        let mut c3 = HttpConn::connect(&srv.plc_addr.to_string(), 65535, Duration::from_secs(5)).await.unwrap();
        let r = c3.roundtrip(&plc_link::framing::http::write_get("/nope", &host, Format::Json), Duration::from_secs(5)).await.unwrap();
        assert_eq!(r.http.as_ref().and_then(|h| h.status), Some(404));
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn command_replies_result_ack_busy() {
    within(60, async {
        let mut srv = server().await;
        let api = srv.api();
        let hub = srv.hub.clone();
        let body = command_body(hub.codec(), 5).unwrap();
        let t = Duration::from_secs(10);
        for c in [
            case(Mode::Passive, Framing::Http, Format::Json),
            case(Mode::Passive, Framing::Http, Format::Bin),
            case(Mode::Active, Framing::Frame, Format::Json),
            case(Mode::Active, Framing::Http, Format::Bin),
        ] {
            let name = c.plc_name();
            let sim = start_case_sim(&mut srv, &c, |cfg| {
                cfg.ack_every = Some(2);
                cfg.ack_code = 104;
                cfg.busy_every = Some(3);
            })
            .await
            .unwrap();
            assert!(wait_until(Duration::from_secs(8), || hub.plc(&name).is_some_and(|p| p.connected && p.registry_ok == Some(true))).await, "{}", c.label());
            let passive_http = c.mode == Mode::Passive && c.framing == Framing::Http;
            for (state, result_type, code, status) in [("done", "CommandResult", None, 200), ("rejected", "Ack", Some(104), 200), ("rejected", "Ack", Some(9), 503)] {
                let (s, v) = httpc::post_json(&api, &format!("/api/plcs/{name}/command?wait_ms=5000"), &body, t).await.unwrap();
                assert_eq!(s, 200, "{}: {v}", c.label());
                assert_eq!((v["state"].as_str(), v["result_type"].as_str(), v["code"].as_i64()), (Some(state), Some(result_type), code), "{}: {v}", c.label());
                assert_eq!(v["http_status"].as_u64(), passive_http.then_some(status), "{}: {v}", c.label());
            }
            assert_eq!(sim.stats().command_acks, 2, "{}", c.label());
        }
        // a passive HTTP PLC answers an unreadable command with 400 Ack(PARSE), and 204 with Content-Length 00000
        let mut cfg = fast_sim(&case(Mode::Passive, Framing::Http, Format::Json), SimTarget::Listen("127.0.0.1:0".into()));
        cfg.measlog_ms = 60_000;
        let mut s = sim::start(hub.codec().clone(), cfg).await.unwrap();
        let addr = s.local_addr.unwrap().to_string();
        let r = httpc::call(&addr, "POST", "/api/command", Some(("application/json", b"{nope")), t).await.unwrap();
        let v = r.json().unwrap();
        assert_eq!((r.status, v["type"].as_str(), v["data"]["Code"].as_i64()), (400, Some("Ack"), Some(6)), "{v}");
        s.stop();
    })
    .await;
}
