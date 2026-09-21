//! In-process OPC UA server that mimics the GRM S7-1500 address space for `"OPCUA".GR[n].CMD`.
//!
//! The node tree is generated from the GRM contract layout, so bit structs (`Command.Stop.Normal`),
//! arrays and data types match the real UDT instead of a hand-written list. Like the S7-1500 server, arrays of
//! structs keep the PLC index (`GR[2]`) while arrays of elementary types are named 0-based from the declared
//! lower bound (`Array[1..4] of Real` → `Position[0]..[3]`), so the console must rebase them (`array_bases`).
//! Writes land in the demo world's GRM `OPCUA` model — the very bytes the console reads back over
//! S7 — and a completed Header write is relayed to the demo robot at that `GR[n]` the way GRM does
//! (`DemoWorld::grm_opcua_write`).
//! Used by `gr-console --demo-opcua` and the bench tests, never against a real PLC.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use opcua::server::address_space::{AddressSpace, ObjectBuilder, VariableBuilder};
use opcua::server::diagnostics::NamespaceMetadata;
use opcua::server::node_manager::memory::{SimpleNodeManager, simple_node_manager};
use opcua::server::{ServerBuilder, ServerHandle};
use opcua::types::{DataTypeId, DataValue, DateTime, NodeId, ObjectId, ObjectTypeId, StatusCode, VariableTypeId, Variant};
use plc_layout::{Contract, Prim};
use serde_json::Value as Json;

use crate::demo::DemoWorld;

const NS_URI: &str = "urn:gr-console:sim-grm-plc";
const DB: &str = "OPCUA";

pub struct SimOpcua {
    pub endpoint: String,
    pub ns: u16,
    /// Leaf variables created under all roots.
    pub leaves: usize,
    handle: ServerHandle,
}

impl Drop for SimOpcua {
    fn drop(&mut self) {
        self.handle.cancel();
    }
}

/// Starts the server on 127.0.0.1 (ephemeral port) with one `CMD` subtree per root (`GR[2].CMD`, ...).
pub async fn start(world: Arc<DemoWorld>, grm: &Contract, roots: &[String], pki_dir: PathBuf) -> anyhow::Result<SimOpcua> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    std::fs::create_dir_all(&pki_dir)?;
    let (server, handle) = ServerBuilder::new_anonymous("gr-sim-grm")
        .application_uri("urn:gr-sim-grm")
        .product_uri("urn:gr-sim-grm")
        .host("127.0.0.1")
        .port(port)
        .discovery_urls(vec![format!("opc.tcp://127.0.0.1:{port}/")])
        .pki_dir(pki_dir)
        .create_sample_keypair(false)
        .with_node_manager(simple_node_manager(NamespaceMetadata { namespace_uri: NS_URI.to_string(), ..Default::default() }, "sim-grm"))
        .build()
        .map_err(|e| anyhow::anyhow!("sim opcua build: {e}"))?;
    let nm = handle.node_managers().get_of_type::<SimpleNodeManager>().ok_or_else(|| anyhow::anyhow!("sim opcua: node manager missing"))?;
    let ns = handle.get_namespace_index(NS_URI).ok_or_else(|| anyhow::anyhow!("sim opcua: namespace missing"))?;

    let layout = grm.layout_db(DB)?;
    let all: Vec<String> = layout.members.iter().map(|m| m.path.clone()).collect();
    let mut bounds = HashMap::new();
    let mut leaves = 0usize;
    {
        let mut space = nm.address_space().write();
        let mut t = Tree { space: &mut space, nm: &nm, ns, nodes: HashMap::new() };
        let objects: NodeId = ObjectId::ObjectsFolder.into();
        let plc = t.object(&objects, "GRM_PLC", "GRM_PLC");
        let dbs = t.object(&plc, "GRM_PLC.DataBlocksGlobal", "DataBlocksGlobal");
        let db = t.object(&dbs, "\"OPCUA\"", DB);
        t.nodes.insert(String::new(), (db, "\"OPCUA\"".to_string()));
        for root in roots {
            let prefix = format!("{root}.");
            for m in layout.members.iter().filter(|m| m.path.starts_with(&prefix)) {
                let ptr = json_pointer(&all, &m.path, &mut bounds);
                // Elementary-type array element (`...Position[1]` as the leaf): the S7-1500 server names it
                // 0-based from the declared lower bound (`Position[0]`); struct arrays keep the PLC index.
                let elem_lb = match m.path.rfind('[') {
                    Some(i) if m.path.ends_with(']') && !m.path[i..].contains('.') => lower_bound(&all, &m.path[..i]),
                    _ => 0,
                };
                if t.leaf(&m.path, m.prim, elem_lb, ptr, world.clone()) {
                    leaves += 1;
                }
            }
        }
    }
    if leaves == 0 {
        anyhow::bail!("sim opcua: no members under {roots:?} in GRM_PLC.OPCUA");
    }
    tokio::spawn(async move {
        if let Err(e) = server.run_with(listener).await {
            tracing::error!("sim opcua server stopped: {e}");
        }
    });
    Ok(SimOpcua { endpoint: format!("opc.tcp://127.0.0.1:{port}/"), ns, leaves, handle })
}

struct Tree<'a> {
    space: &'a mut AddressSpace,
    nm: &'a SimpleNodeManager,
    ns: u16,
    /// member path → (node id, S7 string id)
    nodes: HashMap<String, (NodeId, String)>,
}

impl Tree<'_> {
    fn object(&mut self, parent: &NodeId, sid: &str, name: &str) -> NodeId {
        let id = NodeId::new(self.ns, sid);
        ObjectBuilder::new(&id, name, name).has_type_definition(ObjectTypeId::FolderType).organized_by(parent.clone()).insert(self.space);
        id
    }

    fn container(&mut self, parent: &NodeId, sid: &str, name: &str) -> NodeId {
        let id = NodeId::new(self.ns, sid);
        VariableBuilder::new(&id, name, name).data_type(DataTypeId::Structure).has_type_definition(VariableTypeId::BaseDataVariableType).component_of(parent.clone()).insert(self.space);
        id
    }

    /// Container chain for `path`; array segments get an array node `Name` with element children
    /// `Name[i]` like the S7-1500 server.
    fn ensure(&mut self, path: &str) -> (NodeId, String) {
        if let Some(n) = self.nodes.get(path) {
            return n.clone();
        }
        let (parent_path, seg) = match path.rfind('.') {
            Some(i) => (&path[..i], &path[i + 1..]),
            None => ("", path),
        };
        let (parent, parent_sid) = self.ensure(parent_path);
        let (name, idx) = split_seg(seg);
        let out = match idx {
            None => {
                let sid = format!("{parent_sid}.\"{name}\"");
                (self.container(&parent, &sid, name), sid)
            }
            Some(i) => {
                let array_path = if parent_path.is_empty() { name.to_string() } else { format!("{parent_path}.{name}") };
                let (array, array_sid) = self.ensure_array(&array_path, &parent, &parent_sid, name);
                let sid = format!("{array_sid}[{i}]");
                (self.container(&array, &sid, &format!("{name}[{i}]")), sid)
            }
        };
        self.nodes.insert(path.to_string(), out.clone());
        out
    }

    fn ensure_array(&mut self, array_path: &str, parent: &NodeId, parent_sid: &str, name: &str) -> (NodeId, String) {
        if let Some(n) = self.nodes.get(array_path) {
            return n.clone();
        }
        let sid = format!("{parent_sid}.\"{name}\"");
        let out = (self.container(parent, &sid, name), sid);
        self.nodes.insert(array_path.to_string(), out.clone());
        out
    }

    /// Leaf variable bound to the world model at JSON pointer `ptr`. Returns false for types the
    /// console never writes and the sim does not model. `elem_lb`: declared lower bound when the leaf
    /// is an array element — its node id / browse name use `index - elem_lb` like the real server.
    fn leaf(&mut self, path: &str, prim: Prim, elem_lb: i64, ptr: String, world: Arc<DemoWorld>) -> bool {
        let Some((dt, init)) = ua_type(prim) else { return false };
        let (parent_path, seg) = match path.rfind('.') {
            Some(i) => (&path[..i], &path[i + 1..]),
            None => ("", path),
        };
        let (parent, parent_sid) = self.ensure(parent_path);
        let (name, idx) = split_seg(seg);
        let (parent, sid, browse) = match idx {
            None => (parent, format!("{parent_sid}.\"{name}\""), name.to_string()),
            Some(i) => {
                let i = i - elem_lb;
                let array_path = if parent_path.is_empty() { name.to_string() } else { format!("{parent_path}.{name}") };
                let (array, array_sid) = self.ensure_array(&array_path, &parent, &parent_sid, name);
                (array, format!("{array_sid}[{i}]"), format!("{name}[{i}]"))
            }
        };
        let id = NodeId::new(self.ns, sid);
        let writable = prim != Prim::Dtl;
        let mut b = VariableBuilder::new(&id, &browse, &browse).data_type(dt).value(init).has_type_definition(VariableTypeId::BaseDataVariableType).component_of(parent);
        if writable {
            b = b.writable();
        }
        b.insert(self.space);

        if writable {
            let expected: NodeId = dt.into();
            let (w, p, rel) = (world.clone(), ptr.clone(), path.to_string());
            self.nm.inner().add_write_callback(id.clone(), move |dv, _range| {
                let Some(v) = dv.value else {
                    return StatusCode::BadNothingToDo;
                };
                if v.data_type().map(|e| e.node_id) != Some(expected.clone()) {
                    return StatusCode::BadTypeMismatch;
                }
                let Some(j) = variant_json(&v) else {
                    return StatusCode::BadTypeMismatch;
                };
                w.grm_opcua_write(&rel, &p, j);
                StatusCode::Good
            });
        }
        self.nm.inner().add_read_callback(id, move |_range, _ts, _age| Ok(DataValue::new_now(json_variant(&world.grm_opcua_get(&ptr), prim))));
        true
    }
}

/// `Position[3]` → (`Position`, Some(3)); `Header` → (`Header`, None).
fn split_seg(seg: &str) -> (&str, Option<i64>) {
    match seg.find('[') {
        Some(i) => (&seg[..i], seg[i + 1..].trim_end_matches(']').parse().ok()),
        None => (seg, None),
    }
}

/// Layout path → JSON pointer into the decoded DB model (`GR[2].CMD.TaskData.Position[1]` →
/// `/GR/1/CMD/TaskData/Position/0`): decoded arrays are 0-based from the declared lower bound.
fn json_pointer(all: &[String], path: &str, bounds: &mut HashMap<String, i64>) -> String {
    let mut ptr = String::new();
    let mut prefix = String::new();
    for seg in path.split('.') {
        let (name, idx) = split_seg(seg);
        if !prefix.is_empty() {
            prefix.push('.');
        }
        prefix.push_str(name);
        ptr.push('/');
        ptr.push_str(name);
        if let Some(i) = idx {
            let lb = *bounds.entry(prefix.clone()).or_insert_with(|| lower_bound(all, &prefix));
            ptr.push('/');
            ptr.push_str(&(i - lb).to_string());
            prefix.push_str(&format!("[{i}]"));
        }
    }
    ptr
}

/// Smallest PLC index of the array `array_path` (`GR[2].CMD.TaskData.Position`) among the layout paths.
fn lower_bound(all: &[String], array_path: &str) -> i64 {
    let head = format!("{array_path}[");
    all.iter().filter_map(|p| p.strip_prefix(&head)).filter_map(|r| r.split(']').next()?.parse::<i64>().ok()).min().unwrap_or(0)
}

fn ua_type(prim: Prim) -> Option<(DataTypeId, Variant)> {
    Some(match prim {
        Prim::Bool => (DataTypeId::Boolean, Variant::Boolean(false)),
        Prim::Byte | Prim::USInt | Prim::Char => (DataTypeId::Byte, Variant::Byte(0)),
        Prim::SInt => (DataTypeId::SByte, Variant::SByte(0)),
        Prim::Word | Prim::UInt | Prim::Date => (DataTypeId::UInt16, Variant::UInt16(0)),
        Prim::Int => (DataTypeId::Int16, Variant::Int16(0)),
        Prim::DWord | Prim::UDInt | Prim::Tod => (DataTypeId::UInt32, Variant::UInt32(0)),
        Prim::DInt | Prim::Time => (DataTypeId::Int32, Variant::Int32(0)),
        Prim::Real => (DataTypeId::Float, Variant::Float(0.0)),
        Prim::LReal => (DataTypeId::Double, Variant::Double(0.0)),
        Prim::LInt | Prim::LTime => (DataTypeId::Int64, Variant::Int64(0)),
        Prim::ULInt | Prim::LWord => (DataTypeId::UInt64, Variant::UInt64(0)),
        Prim::Dtl | Prim::DateAndTime => (DataTypeId::DateTime, Variant::from(DateTime::now())),
        Prim::String(_) => return None,
    })
}

fn variant_json(v: &Variant) -> Option<Json> {
    Some(match v {
        Variant::Boolean(b) => Json::Bool(*b),
        Variant::Byte(x) => Json::from(*x),
        Variant::SByte(x) => Json::from(*x),
        Variant::UInt16(x) => Json::from(*x),
        Variant::Int16(x) => Json::from(*x),
        Variant::UInt32(x) => Json::from(*x),
        Variant::Int32(x) => Json::from(*x),
        Variant::UInt64(x) => Json::from(*x),
        Variant::Int64(x) => Json::from(*x),
        Variant::Float(x) => serde_json::Number::from_f64(f64::from(*x)).map(Json::Number)?,
        Variant::Double(x) => serde_json::Number::from_f64(*x).map(Json::Number)?,
        _ => return None,
    })
}

fn json_variant(j: &Json, prim: Prim) -> Variant {
    let u = || j.as_u64().unwrap_or(0);
    let i = || j.as_i64().unwrap_or(0);
    let f = || j.as_f64().unwrap_or(0.0);
    match prim {
        Prim::Bool => Variant::Boolean(j.as_bool().unwrap_or(false)),
        Prim::Byte | Prim::USInt | Prim::Char => Variant::Byte(u() as u8),
        Prim::SInt => Variant::SByte(i() as i8),
        Prim::Word | Prim::UInt | Prim::Date => Variant::UInt16(u() as u16),
        Prim::Int => Variant::Int16(i() as i16),
        Prim::DWord | Prim::UDInt | Prim::Tod => Variant::UInt32(u() as u32),
        Prim::DInt | Prim::Time => Variant::Int32(i() as i32),
        Prim::Real => Variant::Float(f() as f32),
        Prim::LReal => Variant::Double(f()),
        Prim::LInt | Prim::LTime => Variant::Int64(i()),
        Prim::ULInt | Prim::LWord => Variant::UInt64(u()),
        Prim::Dtl | Prim::DateAndTime | Prim::String(_) => Variant::from(DateTime::now()),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gr_proto::{StockItem, TaskData, TaskType};
    use opcua_cmd::{CmdWriter, OpcState, OpcUaConfig, TaskOp};
    use serde_json::json;

    use super::*;

    fn contracts() -> (Arc<Contract>, Arc<Contract>) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plc/contract");
        (Arc::new(Contract::load_dir(&root.join("GR2_PLC")).expect("GR2 contract")), Arc::new(Contract::load_dir(&root.join("GRM_PLC")).expect("GRM contract")))
    }

    async fn eventually(secs: u64, mut f: impl FnMut() -> bool) -> bool {
        let end = tokio::time::Instant::now() + Duration::from_secs(secs);
        while tokio::time::Instant::now() < end {
            if f() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        f()
    }

    #[test]
    fn pointer_uses_declared_lower_bounds() {
        let all: Vec<String> =
            ["GR[1].CMD.Data[0]", "GR[2].CMD.Data[0]", "GR[2].CMD.Data[1]", "GR[2].CMD.TaskData.Position[1]", "GR[2].CMD.TaskData.Position[4]"].iter().map(|s| s.to_string()).collect();
        let mut b = HashMap::new();
        assert_eq!(json_pointer(&all, "GR[2].CMD.TaskData.Position[1]", &mut b), "/GR/1/CMD/TaskData/Position/0");
        assert_eq!(json_pointer(&all, "GR[2].CMD.Data[1]", &mut b), "/GR/1/CMD/Data/1");
        assert_eq!(json_pointer(&all, "GR[2].CMD.Header.SEQ", &mut b), "/GR/1/CMD/Header/SEQ");
    }

    /// OPC UA command path end to end against the layout-generated GRM address space: browse, submit
    /// (TaskData + Command zeros + Header), GRM relay → GR2 echo with the identical TaskData, GRM CMD
    /// cleared, task op.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn opc_command_relays_through_grm_model() {
        let (gr2, grm) = contracts();
        let robot = crate::demo::DemoRobot { plc_name: "GR2".into(), contract: gr2, dst: 4002, gr_index: 1, machine_id: 2 };
        let world = DemoWorld::start(vec![robot], grm.clone(), 100).await.expect("world");
        let tmp = std::env::temp_dir().join(format!("gr-sim-opcua-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let sim = start(world.clone(), &grm, &["GR[2].CMD".to_string()], tmp.join("server")).await.expect("sim server");
        assert!(sim.leaves > 100, "leaves = {}", sim.leaves);

        let cfg = OpcUaConfig {
            endpoint: sim.endpoint.clone(),
            ns_hint: sim.ns,
            root_path: "GR[2].CMD".into(),
            pki_dir: Some(tmp.join("client")),
            trust_server_cert: true,
            connect_timeout_ms: 5_000,
            write_timeout_ms: 5_000,
            session_timeout_ms: 20_000,
            array_bases: crate::cmd::opcua_array_bases(&grm, DB, "GR[2].CMD"),
            ..OpcUaConfig::default()
        };
        // Drop the state receiver like `main` does: `writer.state()` must still advance to Ready.
        let (writer, rx) = CmdWriter::spawn(cfg);
        drop(rx);
        assert!(eventually(30, || matches!(writer.state(), OpcState::Ready { .. })).await, "state without receivers = {:?}", writer.state());
        let OpcState::Ready { node_count, .. } = writer.state() else { unreachable!() };
        assert_eq!(node_count, sim.leaves, "every layout leaf is browsed");
        // The console used to add `Command.Stop` etc. as bytes; on the real UDT they are bit structs, so the
        // node does not exist and the whole submission failed before writing anything.
        let err = writer.write_members(&[opcua_cmd::MemberValue::new("Command.Stop", opcua_cmd::PlcValue::U8(0))]).await.unwrap_err();
        assert_eq!(err, opcua_cmd::OpcError::NodeMissing("Command.Stop".into()));

        let mut t =
            TaskData { work_id: 77, task_id: 1, task_type: TaskType::Pick.code(), position: [12000.0, 3000.0, 1980.0, 300.0], grip_height: 40, measure_item: true, avoid: true, ..Default::default() };
        t.cell = crate::demo::sample_cell(101, 0);
        t.item = StockItem { code: 1001, count: 1, inner_diameter: 381.0, outer_diameter: 780.0, lower_bid_height: 20.0, upper_bid_height: 220.0, height: 240.0, deflection_factor: 0.0 };
        let wire: Vec<opcua_cmd::MemberValue> = t.to_members().iter().map(crate::cmd::to_opc).collect();
        let h = writer.write_task(&wire, t.task_type, 5000, 4002, 1).await.expect("write_task over layout-generated nodes");

        let echoed = eventually(5, || world.gr2_get("/STAT/RES/Header/SEQ") == json!(h.seq) && world.gr2_get("/STAT/RES/Header/CMD_ID") == json!(h.cmd_id)).await;
        assert!(echoed, "GR2 RES echo; RES = {}", world.gr2_get("/STAT/RES/Header"));
        let res_task = TaskData::from_json(&world.gr2_get("/STAT/RES/Task")).expect("RES task");
        assert_eq!(res_task, t, "TaskData relayed through OPC UA → GRM model → GR2 is identical");
        assert_eq!(world.gr2_get("/STAT/RES/Data/2"), json!(1), "validation code VALID");
        assert!(eventually(2, || world.grm_opcua_get("/GR/1/CMD/Header/CMD") == json!(0) && world.grm_opcua_get("/GR/1/CMD/TaskData/WorkId") == json!(0)).await, "GRM cleared CMD after the echo");

        writer.write_task_op(TaskOp::Complete, 77, 1).await.expect("complete op");
        let done = eventually(8, || (0..10).any(|i| world.gr2_get(&format!("/STAT/Task/Completed/{i}/WorkId")) == json!(77))).await;
        assert!(done, "task 77 reaches the completed ring");
        writer.write_task_op(TaskOp::Complete, 0, 0).await.expect("re-arm");
        writer.shutdown();
    }
}
