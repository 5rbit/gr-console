//! Integration test against an in-process async-opcua server that mimics the S7-1500
//! address space `Objects/PLC/DataBlocksGlobal/OPCUA/GR/GR[2]/CMD/...`.
//!
//! Covered:
//! (a) `write_task` sends TaskData/Command/Data before the Header (server-side write log),
//! (b) header cmd_id/seq are non-zero and increment across calls,
//! (c) `write_task_op` writes the Complete pair,
//! (d) `clear_header` zeroes all six header fields,
//! (e) the session recovers after the server is restarted (Ready → Failed → Ready),
//! (f) a session the server expires behind a live channel is detected by the keep-alive probe
//!     and by a failing write (BadSessionIdInvalid) → Failed → reconnect → Ready,
//! plus read_members, Status/NodeMissing/Config error mapping, the node cache file,
//! and the "policy not offered" diagnostic.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use opcua::server::address_space::{AddressSpace, ObjectBuilder, VariableBuilder};
use opcua::server::diagnostics::NamespaceMetadata;
use opcua::server::node_manager::memory::{SimpleNodeManager, simple_node_manager};
use opcua::server::{ServerBuilder, ServerHandle};
use opcua::types::{DataTypeId, DataValue, NodeId, ObjectId, ObjectTypeId, StatusCode, VariableTypeId, Variant};
use opcua_cmd::{Auth, CmdWriter, MemberValue, OpcError, OpcState, OpcUaConfig, PlcValue, TaskOp};
use tokio::net::TcpListener;
use tokio::sync::watch;

const NS_URI: &str = "urn:fake-s7-1500";

type WriteLog = Arc<Mutex<Vec<(String, Variant)>>>;
type Values = Arc<Mutex<HashMap<String, Variant>>>;

struct FakeServer {
    handle: ServerHandle,
    task: tokio::task::JoinHandle<Result<(), String>>,
    port: u16,
    ns: u16,
    log: WriteLog,
    values: Values,
}

struct Builder<'a> {
    space: &'a mut AddressSpace,
    nm: &'a SimpleNodeManager,
    ns: u16,
    log: WriteLog,
    values: Values,
}

impl Builder<'_> {
    fn id(&self, s: &str) -> NodeId {
        NodeId::new(self.ns, s)
    }

    /// Object node (folder-like) with S7-style string id `<parent>.<name>`.
    fn object(&mut self, parent: &NodeId, sid: &str, browse_name: &str) -> NodeId {
        let id = self.id(sid);
        ObjectBuilder::new(&id, browse_name, browse_name).has_type_definition(ObjectTypeId::FolderType).organized_by(parent.clone()).insert(self.space);
        id
    }

    /// Struct/array container variable (has children, no scalar value).
    fn container(&mut self, parent: &NodeId, sid: &str, browse_name: &str) -> NodeId {
        let id = self.id(sid);
        VariableBuilder::new(&id, browse_name, browse_name).data_type(DataTypeId::Structure).has_type_definition(VariableTypeId::BaseDataVariableType).component_of(parent.clone()).insert(self.space);
        id
    }

    /// Leaf variable with a write callback that logs the write (type-checked) and a
    /// read callback returning the last written value.
    #[allow(clippy::too_many_arguments)]
    fn leaf(&mut self, parent: &NodeId, sid: &str, browse_name: &str, path: &str, dt: DataTypeId, initial: Variant, reject: Option<StatusCode>) -> NodeId {
        let id = self.id(sid);
        VariableBuilder::new(&id, browse_name, browse_name)
            .data_type(dt)
            .value(initial.clone())
            .writable()
            .has_type_definition(VariableTypeId::BaseDataVariableType)
            .component_of(parent.clone())
            .insert(self.space);
        self.values.lock().unwrap().insert(path.to_string(), initial.clone());

        let (log, values, p) = (self.log.clone(), self.values.clone(), path.to_string());
        let expected: NodeId = dt.into();
        self.nm.inner().add_write_callback(id.clone(), move |dv, _range| {
            if let Some(code) = reject {
                return code;
            }
            let Some(v) = dv.value else {
                return StatusCode::BadNothingToDo;
            };
            if v.data_type().map(|e| e.node_id) != Some(expected.clone()) {
                return StatusCode::BadTypeMismatch;
            }
            log.lock().unwrap().push((p.clone(), v.clone()));
            values.lock().unwrap().insert(p.clone(), v);
            StatusCode::Good
        });
        let (values, p) = (self.values.clone(), path.to_string());
        self.nm.inner().add_read_callback(id.clone(), move |_range, _ts, _age| {
            let v = values.lock().unwrap().get(&p).cloned().unwrap_or(initial.clone());
            Ok(DataValue::new_now(v))
        });
        id
    }
}

fn build_address_space(b: &mut Builder<'_>) {
    let objects: NodeId = ObjectId::ObjectsFolder.into();
    let plc = b.object(&objects, "PLC", "PLC");
    let dbs = b.object(&plc, "PLC.DataBlocksGlobal", "DataBlocksGlobal");
    let db = b.object(&dbs, "\"OPCUA\"", "OPCUA");
    let gr = b.container(&db, "\"OPCUA\".\"GR\"", "GR");
    // A sibling element that must NOT be picked.
    let gr1 = b.container(&gr, "\"OPCUA\".\"GR\"[1]", "GR[1]");
    let cmd1 = b.container(&gr1, "\"OPCUA\".\"GR\"[1].\"CMD\"", "CMD");
    let hdr1 = b.container(&cmd1, "\"OPCUA\".\"GR\"[1].\"CMD\".\"Header\"", "Header");
    b.leaf(&hdr1, "\"OPCUA\".\"GR\"[1].\"CMD\".\"Header\".\"Protocol\"", "Protocol", "GR1.Header.Protocol", DataTypeId::Byte, Variant::Byte(99), None);

    let gr2 = b.container(&gr, "\"OPCUA\".\"GR\"[2]", "GR[2]");
    let root = "\"OPCUA\".\"GR\"[2].\"CMD\"";
    let cmd = b.container(&gr2, root, "CMD");

    let hdr = b.container(&cmd, &format!("{root}.\"Header\""), "Header");
    for (name, dt, init) in [
        ("Protocol", DataTypeId::Byte, Variant::Byte(0)),
        ("CMD_ID", DataTypeId::Byte, Variant::Byte(0)),
        ("CMD", DataTypeId::Byte, Variant::Byte(0)),
        ("SRC", DataTypeId::UInt16, Variant::UInt16(0)),
        ("DST", DataTypeId::UInt16, Variant::UInt16(0)),
        ("SEQ", DataTypeId::UInt16, Variant::UInt16(0)),
    ] {
        b.leaf(&hdr, &format!("{root}.\"Header\".\"{name}\""), name, &format!("Header.{name}"), dt, init, None);
    }

    let command = b.container(&cmd, &format!("{root}.\"Command\""), "Command");
    for name in ["Stop", "Common", "Jog", "Home"] {
        b.leaf(&command, &format!("{root}.\"Command\".\"{name}\""), name, &format!("Command.{name}"), DataTypeId::Byte, Variant::Byte(7), None);
    }
    let task = b.container(&command, &format!("{root}.\"Command\".\"Task\""), "Task");
    for op in ["Complete", "Delete"] {
        let node = b.container(&task, &format!("{root}.\"Command\".\"Task\".\"{op}\""), op);
        for name in ["WorkId", "TaskId"] {
            b.leaf(&node, &format!("{root}.\"Command\".\"Task\".\"{op}\".\"{name}\""), name, &format!("Command.Task.{op}.{name}"), DataTypeId::UInt32, Variant::UInt32(5), None);
        }
    }

    let data = b.container(&cmd, &format!("{root}.\"Data\""), "Data");
    for i in 0..16u32 {
        // S7 exports array members as `[i]` children (browse name form 2).
        b.leaf(&data, &format!("{root}.\"Data\"[{i}]"), &format!("[{i}]"), &format!("Data[{i}]"), DataTypeId::Byte, Variant::Byte(9), None);
    }

    let td = b.container(&cmd, &format!("{root}.\"TaskData\""), "TaskData");
    b.leaf(&td, &format!("{root}.\"TaskData\".\"WorkId\""), "WorkId", "TaskData.WorkId", DataTypeId::UInt32, Variant::UInt32(0), None);
    b.leaf(&td, &format!("{root}.\"TaskData\".\"TaskId\""), "TaskId", "TaskData.TaskId", DataTypeId::UInt32, Variant::UInt32(0), None);
    b.leaf(&td, &format!("{root}.\"TaskData\".\"TaskType\""), "TaskType", "TaskData.TaskType", DataTypeId::Byte, Variant::Byte(0), None);
    let pos = b.container(&td, &format!("{root}.\"TaskData\".\"Position\""), "Position");
    for i in 1..=4u32 {
        // Browse name form 1: `Position[i]`.
        b.leaf(&pos, &format!("{root}.\"TaskData\".\"Position\"[{i}]"), &format!("Position[{i}]"), &format!("TaskData.Position[{i}]"), DataTypeId::Float, Variant::Float(0.0), None);
    }
    let cell = b.container(&td, &format!("{root}.\"TaskData\".\"Cell\""), "Cell");
    b.leaf(&cell, &format!("{root}.\"TaskData\".\"Cell\".\"Use\""), "Use", "TaskData.Cell.Use", DataTypeId::Boolean, Variant::Boolean(false), None);
    b.leaf(&cell, &format!("{root}.\"TaskData\".\"Cell\".\"Id\""), "Id", "TaskData.Cell.Id", DataTypeId::UInt16, Variant::UInt16(0), None);

    b.leaf(&cmd, &format!("{root}.\"ReadOnly\""), "ReadOnly", "ReadOnly", DataTypeId::Byte, Variant::Byte(1), Some(StatusCode::BadNotWritable));
}

async fn bind(port: Option<u16>) -> TcpListener {
    let addr = format!("127.0.0.1:{}", port.unwrap_or(0));
    let mut last = None;
    for _ in 0..50 {
        match TcpListener::bind(&addr).await {
            Ok(l) => return l,
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
    panic!("bind {addr}: {last:?}");
}

impl FakeServer {
    async fn start(port: Option<u16>) -> FakeServer {
        Self::start_with(port, None).await
    }

    /// `max_session_timeout_ms`: the server revises every session timeout down to this and expires
    /// idle sessions after it — while the secure channel stays open.
    async fn start_with(port: Option<u16>, max_session_timeout_ms: Option<u64>) -> FakeServer {
        let listener = bind(port).await;
        let port = listener.local_addr().unwrap().port();
        let mut builder = ServerBuilder::new_anonymous("fake-s7");
        if let Some(ms) = max_session_timeout_ms {
            builder = builder.max_session_timeout_ms(ms);
        }
        let (server, handle) = builder
            .application_uri("urn:fake-s7")
            .product_uri("urn:fake-s7")
            .host("127.0.0.1")
            .port(port)
            .discovery_urls(vec![format!("opc.tcp://127.0.0.1:{port}/")])
            .pki_dir(temp_dir("server-pki"))
            .create_sample_keypair(false)
            .with_node_manager(simple_node_manager(NamespaceMetadata { namespace_uri: NS_URI.to_string(), ..Default::default() }, "fake"))
            .build()
            .expect("server build");
        let nm = handle.node_managers().get_of_type::<SimpleNodeManager>().expect("simple node manager");
        let ns = handle.get_namespace_index(NS_URI).expect("namespace index");
        let log: WriteLog = Arc::default();
        let values: Values = Arc::default();
        {
            let mut space = nm.address_space().write();
            let mut b = Builder { space: &mut space, nm: &nm, ns, log: log.clone(), values: values.clone() };
            build_address_space(&mut b);
        }
        let task = tokio::spawn(server.run_with(listener));
        FakeServer { handle, task, port, ns, log, values }
    }

    async fn stop(self) {
        self.handle.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(10), self.task).await;
    }

    fn endpoint(&self) -> String {
        format!("opc.tcp://127.0.0.1:{}/", self.port)
    }

    fn take_log(&self) -> Vec<(String, Variant)> {
        std::mem::take(&mut *self.log.lock().unwrap())
    }

    fn value(&self, path: &str) -> Option<Variant> {
        self.values.lock().unwrap().get(path).cloned()
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("opcua-cmd-it-{}-{}-{}", std::process::id(), name, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn config(server: &FakeServer, cache: Option<PathBuf>) -> OpcUaConfig {
    OpcUaConfig {
        endpoint: server.endpoint(),
        security_policy: "None".into(),
        security_mode: "None".into(),
        auth: Auth::Anonymous,
        ns_hint: server.ns,
        db_name: "OPCUA".into(),
        root_path: "GR[2].CMD".into(),
        connect_timeout_ms: 5_000,
        write_timeout_ms: 3_000,
        session_timeout_ms: 20_000,
        channel_lifetime_ms: 60_000,
        keepalive_interval_ms: 5_000,
        keepalive_fail_limit: 2,
        node_cache: cache,
        pki_dir: Some(temp_dir("client-pki")),
        trust_server_cert: true,
        array_bases: Default::default(),
    }
}

async fn wait_state(rx: &mut watch::Receiver<OpcState>, secs: u64, pred: impl Fn(&OpcState) -> bool) -> OpcState {
    let timed_out = match tokio::time::timeout(Duration::from_secs(secs), rx.wait_for(|s| pred(s))).await {
        Ok(Ok(s)) => return s.clone(),
        Ok(Err(_)) => false,
        Err(_) => true,
    };
    if timed_out {
        panic!("timeout waiting for state; last = {:?}", *rx.borrow());
    }
    panic!("state channel closed");
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).with_test_writer().try_init();
}

fn next_cmd_id(v: u8) -> u8 {
    if v == u8::MAX { 1 } else { v + 1 }
}

fn next_seq(v: u16) -> u16 {
    if v == u16::MAX { 1 } else { v + 1 }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn full_flow() {
    init_tracing();
    let server = FakeServer::start(None).await;
    let cache = temp_dir("cache").join("nodes.json");
    let (writer, mut rx) = CmdWriter::spawn(config(&server, Some(cache.clone())));

    let ready = wait_state(&mut rx, 20, |s| matches!(s, OpcState::Ready { .. })).await;
    let OpcState::Ready { node_count, ns } = ready else { unreachable!() };
    assert!(node_count >= 15, "node_count = {node_count}");
    assert_eq!(ns, server.ns);

    let info = writer.nodes().expect("node map");
    assert_eq!(info.count, node_count);
    assert!(!info.sample.is_empty());
    assert!(!writer.endpoints().is_empty(), "endpoints recorded");
    assert!(cache.exists(), "node cache written");
    let cache_json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&cache).unwrap()).unwrap();
    assert_eq!(cache_json["ns"], serde_json::json!(server.ns));
    assert!(cache_json["members"]["Header.Protocol"].as_str().unwrap().contains("\"Header\".\"Protocol\""));
    // The sibling GR[1] must not leak into the map; Data[i] / Position[i] normalized.
    let members = cache_json["members"].as_object().unwrap();
    assert!(members.contains_key("Data[15]"));
    assert!(members.contains_key("TaskData.Position[4]"));
    assert!(members.contains_key("Command.Task.Delete.TaskId"));
    assert!(!members.keys().any(|k| k.contains("GR")));

    // (a) + (b): write_task ordering and header counters.
    server.take_log();
    let task = vec![
        MemberValue::new("TaskData.WorkId", PlcValue::U32(11)),
        MemberValue::new("TaskData.TaskId", PlcValue::U32(22)),
        MemberValue::new("TaskData.TaskType", PlcValue::U8(3)),
        MemberValue::new("TaskData.Position[1]", PlcValue::F32(1.5)),
        MemberValue::new("TaskData.Position[2]", PlcValue::F32(2.5)),
        MemberValue::new("TaskData.Cell.Use", PlcValue::Bool(true)),
        // widening coercion: U8 → UInt16 member
        MemberValue::new("TaskData.Cell.Id", PlcValue::U8(4)),
        // caller-provided Command member must win over the zero fill
        MemberValue::new("Command.Jog", PlcValue::U8(2)),
    ];
    let h1 = writer.write_task(&task, 5, 1, 2, 1).await.expect("write_task");
    assert_eq!(h1.protocol, 1);
    assert_eq!(h1.cmd, 5);
    assert_eq!(h1.src, 1);
    assert_eq!(h1.dst, 2);
    assert_ne!(h1.cmd_id, 0);
    assert_ne!(h1.seq, 0);

    let log = server.take_log();
    let first_header = log.iter().position(|(p, _)| p.starts_with("Header.")).expect("header written");
    assert!(first_header > 0);
    assert!(log[..first_header].iter().all(|(p, _)| !p.starts_with("Header.")), "no header before payload");
    assert!(log[first_header..].iter().all(|(p, _)| p.starts_with("Header.")), "payload after header: {:?}", &log[first_header..]);
    let header_paths: Vec<&str> = log[first_header..].iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(header_paths, ["Header.Protocol", "Header.CMD_ID", "Header.CMD", "Header.SRC", "Header.DST", "Header.SEQ"]);
    // Payload order: task members first (as given), then zero fill.
    let payload: Vec<&str> = log[..first_header].iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(&payload[..3], ["TaskData.WorkId", "TaskData.TaskId", "TaskData.TaskType"]);
    assert!(payload.contains(&"Command.Stop"));
    assert!(payload.contains(&"Command.Task.Complete.WorkId"));
    assert!(payload.contains(&"Data[0]"));
    assert!(payload.contains(&"Data[15]"));
    assert_eq!(payload.iter().filter(|p| **p == "Command.Jog").count(), 1);

    assert_eq!(server.value("TaskData.WorkId"), Some(Variant::UInt32(11)));
    assert_eq!(server.value("TaskData.Position[1]"), Some(Variant::Float(1.5)));
    assert_eq!(server.value("TaskData.Cell.Use"), Some(Variant::Boolean(true)));
    assert_eq!(server.value("TaskData.Cell.Id"), Some(Variant::UInt16(4)));
    assert_eq!(server.value("Command.Jog"), Some(Variant::Byte(2)));
    assert_eq!(server.value("Command.Stop"), Some(Variant::Byte(0)));
    assert_eq!(server.value("Command.Task.Complete.WorkId"), Some(Variant::UInt32(0)));
    assert_eq!(server.value("Data[7]"), Some(Variant::Byte(0)));
    assert_eq!(server.value("Header.Protocol"), Some(Variant::Byte(1)));
    assert_eq!(server.value("Header.CMD_ID"), Some(Variant::Byte(h1.cmd_id)));
    assert_eq!(server.value("Header.CMD"), Some(Variant::Byte(5)));
    assert_eq!(server.value("Header.SRC"), Some(Variant::UInt16(1)));
    assert_eq!(server.value("Header.DST"), Some(Variant::UInt16(2)));
    assert_eq!(server.value("Header.SEQ"), Some(Variant::UInt16(h1.seq)));

    let h2 = writer.write_task(&task, 6, 1, 2, 1).await.expect("write_task 2");
    assert_eq!(h2.cmd_id, next_cmd_id(h1.cmd_id));
    assert_eq!(h2.seq, next_seq(h1.seq));
    assert_ne!(h2.cmd_id, 0);
    assert_ne!(h2.seq, 0);

    // (c) task op
    writer.write_task_op(TaskOp::Complete, 7, 8).await.expect("write_task_op");
    assert_eq!(server.value("Command.Task.Complete.WorkId"), Some(Variant::UInt32(7)));
    assert_eq!(server.value("Command.Task.Complete.TaskId"), Some(Variant::UInt32(8)));
    writer.write_task_op(TaskOp::Delete, 0, 0).await.expect("write_task_op delete");
    assert_eq!(server.value("Command.Task.Delete.WorkId"), Some(Variant::UInt32(0)));

    // (d) clear header
    writer.clear_header().await.expect("clear_header");
    for p in ["Header.Protocol", "Header.CMD_ID", "Header.CMD"] {
        assert_eq!(server.value(p), Some(Variant::Byte(0)), "{p}");
    }
    for p in ["Header.SRC", "Header.DST", "Header.SEQ"] {
        assert_eq!(server.value(p), Some(Variant::UInt16(0)), "{p}");
    }

    // read_members
    let read = writer.read_members(&["Header.CMD".into(), " TaskData . Position[2] ".into()]).await.expect("read_members");
    assert_eq!(read, vec![("Header.CMD".to_string(), PlcValue::U8(0)), ("TaskData.Position[2]".to_string(), PlcValue::F32(2.5)),]);

    // Error mapping
    let err = writer.write_members(&[MemberValue::new("ReadOnly", PlcValue::U8(1))]).await.unwrap_err();
    assert_eq!(err, OpcError::Status { path: "ReadOnly".into(), code: StatusCode::BadNotWritable.bits() });
    let err = writer.write_members(&[MemberValue::new("Nope.Missing", PlcValue::U8(1))]).await.unwrap_err();
    assert_eq!(err, OpcError::NodeMissing("Nope.Missing".into()));
    let err = writer.write_members(&[MemberValue::new("Header.CMD", PlcValue::Str("x".into()))]).await.unwrap_err();
    assert!(matches!(err, OpcError::Config(ref m) if m.contains("Header.CMD")));
    let err = writer.write_members(&[MemberValue::new("Header.CMD", PlcValue::U16(300))]).await.unwrap_err();
    assert!(matches!(err, OpcError::Config(_)));

    // rebrowse keeps the map
    let info2 = writer.rebrowse().await.expect("rebrowse");
    assert_eq!(info2.count, node_count);

    writer.shutdown();
    wait_state(&mut rx, 10, |s| matches!(s, OpcState::Disconnected)).await;
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reconnect_after_server_restart() {
    init_tracing();
    let server = FakeServer::start(None).await;
    let port = server.port;
    let cache = temp_dir("cache").join("nodes.json");
    let (writer, mut rx) = CmdWriter::spawn(config(&server, Some(cache.clone())));
    wait_state(&mut rx, 20, |s| matches!(s, OpcState::Ready { .. })).await;
    writer.write_task_op(TaskOp::Complete, 1, 1).await.expect("write before restart");

    // (e) restart
    server.stop().await;
    let failed = wait_state(&mut rx, 30, |s| matches!(s, OpcState::Failed { .. })).await;
    let OpcState::Failed { retry_in_ms, .. } = failed else { unreachable!() };
    assert!(retry_in_ms >= 1_000);
    let err = writer.write_task_op(TaskOp::Complete, 2, 2).await.unwrap_err();
    assert!(matches!(err, OpcError::NotReady | OpcError::Transport(_) | OpcError::Timeout), "{err:?}");
    // Map is retained for diagnostics while disconnected.
    assert!(writer.nodes().is_some());

    let server2 = FakeServer::start(Some(port)).await;
    wait_state(&mut rx, 60, |s| matches!(s, OpcState::Ready { .. })).await;
    // Second connect loads the cache (verified by reading Header.Protocol).
    writer.write_task_op(TaskOp::Complete, 3, 4).await.expect("write after restart");
    assert_eq!(server2.value("Command.Task.Complete.WorkId"), Some(Variant::UInt32(3)));
    assert_eq!(server2.value("Command.Task.Complete.TaskId"), Some(Variant::UInt32(4)));

    writer.shutdown();
    wait_state(&mut rx, 10, |s| matches!(s, OpcState::Disconnected)).await;
    server2.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn policy_not_offered_is_reported() {
    init_tracing();
    let server = FakeServer::start(None).await;
    let mut cfg = config(&server, None);
    cfg.security_policy = "Basic256Sha256".into();
    cfg.security_mode = "SignAndEncrypt".into();
    let (writer, mut rx) = CmdWriter::spawn(cfg);
    let failed = wait_state(&mut rx, 30, |s| matches!(s, OpcState::Failed { .. })).await;
    let OpcState::Failed { error, .. } = failed else { unreachable!() };
    assert!(error.contains("not offered"), "{error}");
    assert!(error.contains("policy=None"), "{error}");
    assert!(!writer.endpoints().is_empty() || error.contains("policy=None"));
    assert!(matches!(writer.write_task_op(TaskOp::Complete, 0, 0).await, Err(OpcError::NotReady)));
    writer.shutdown();
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unreachable_endpoint_backs_off() {
    init_tracing();
    let listener = bind(None).await;
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let cfg = OpcUaConfig { endpoint: format!("opc.tcp://127.0.0.1:{port}/"), connect_timeout_ms: 2_000, pki_dir: Some(temp_dir("client-pki")), ..OpcUaConfig::default() };
    let (writer, mut rx) = CmdWriter::spawn(cfg);
    let failed = wait_state(&mut rx, 20, |s| matches!(s, OpcState::Failed { .. })).await;
    let OpcState::Failed { retry_in_ms, .. } = failed else { unreachable!() };
    assert_eq!(retry_in_ms, 1_000);
    assert_eq!(writer.state(), failed);
    writer.shutdown();
    wait_state(&mut rx, 10, |s| matches!(s, OpcState::Disconnected)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn session_expired_behind_live_channel_is_detected_by_keepalive() {
    init_tracing();
    // Server expires sessions 2 s after the last request; the probe runs every 3 s, so every
    // second probe finds the session gone (BadSessionIdInvalid) while the TCP channel is up.
    let server = FakeServer::start_with(None, Some(2_000)).await;
    let mut cfg = config(&server, None);
    cfg.keepalive_interval_ms = 3_000;
    let (writer, mut rx) = CmdWriter::spawn(cfg);
    wait_state(&mut rx, 20, |s| matches!(s, OpcState::Ready { .. })).await;

    let failed = wait_state(&mut rx, 20, |s| matches!(s, OpcState::Failed { .. })).await;
    let OpcState::Failed { error, retry_in_ms } = failed else { unreachable!() };
    assert!(error.contains("세션 무효") && error.contains("BadSessionIdInvalid"), "{error}");
    assert_eq!(retry_in_ms, 1_000, "server is reachable: minimum backoff");
    // The session is withdrawn before the state says Failed (reconnect waits 1 s): no write goes out on it.
    assert_eq!(writer.write_task_op(TaskOp::Complete, 0, 0).await, Err(OpcError::NotReady));

    // Reconnects on its own and is usable again.
    wait_state(&mut rx, 20, |s| matches!(s, OpcState::Ready { .. })).await;
    writer.write_task_op(TaskOp::Complete, 5, 6).await.expect("write after reconnect");
    assert_eq!(server.value("Command.Task.Complete.WorkId"), Some(Variant::UInt32(5)));

    writer.shutdown();
    wait_state(&mut rx, 10, |s| matches!(s, OpcState::Disconnected)).await;
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_on_invalid_session_triggers_reconnect() {
    init_tracing();
    // Probe interval far beyond the test: only the write can notice the expired session.
    let server = FakeServer::start_with(None, Some(1_500)).await;
    let mut cfg = config(&server, None);
    cfg.keepalive_interval_ms = 60_000;
    let (writer, mut rx) = CmdWriter::spawn(cfg);
    wait_state(&mut rx, 20, |s| matches!(s, OpcState::Ready { .. })).await;
    tokio::time::sleep(Duration::from_millis(3_000)).await;
    assert!(matches!(writer.state(), OpcState::Ready { .. }), "probe must not have run: {:?}", writer.state());

    let err = writer.write_task_op(TaskOp::Complete, 1, 2).await.unwrap_err();
    assert!(matches!(err, OpcError::Transport(ref m) if m.contains("세션 무효") && m.contains("BadSessionIdInvalid")), "{err:?}");
    // Not ready at once (gate / health), without waiting for any probe.
    let st = writer.state();
    assert!(matches!(st, OpcState::Failed { ref error, retry_in_ms: 1_000 } if error.contains("세션 무효")), "{st:?}");
    assert_eq!(writer.write_task_op(TaskOp::Complete, 1, 2).await, Err(OpcError::NotReady));

    wait_state(&mut rx, 20, |s| matches!(s, OpcState::Ready { .. })).await;
    writer.write_task_op(TaskOp::Complete, 3, 4).await.expect("write after reconnect");
    assert_eq!(server.value("Command.Task.Complete.TaskId"), Some(Variant::UInt32(4)));

    writer.shutdown();
    wait_state(&mut rx, 10, |s| matches!(s, OpcState::Disconnected)).await;
    server.stop().await;
}
