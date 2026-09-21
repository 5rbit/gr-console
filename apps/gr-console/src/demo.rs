//! In-process demo world: one fake GRM S7 server plus one fake S7 server per robot PLC (GR1, GR2, ...). Every side
//! keeps JSON models that are regenerated into DB bytes each tick, and each robot has its own fake command path
//! (GRM `GR[n].CMD` → echo → queue → running → completed), so the whole console — robot switching included — can be
//! exercised without a PLC.
//!
//! The robots are deliberately made to look different (axis positions, `PARA.Machine.ID` / `XLength`, ComponentID,
//! measurement history, WEBMON message): with identical fake data a view that reads the wrong robot's PLC would go
//! unnoticed in the demo. A robot contract may lack a DB or its number (GR1 has no LASERDIAG `.xml`): that DB is
//! simply not modelled or not served.

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use gr_proto::{CellInfo, Header, MeasureLogEntry, RejectInfo, StationPara, StockItem, TaskData, TaskKey, TaskType};
use plc_layout::Contract;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use s7::testing::{DbStore, FakeS7Server};
use serde_json::{Value as Json, json};

use crate::cmd::TaskOp;
use crate::util::now_str;

const RING: usize = 10;
const QUEUE: usize = 4;
const STEPS: [u16; 7] = [100, 200, 300, 400, 500, 600, 999];
/// DBs modelled on a robot PLC (skipped one by one when the contract lacks them).
const GR_DBS: [&str; 11] = ["OPCUA", "TASK", "CELL", "STATION", "PARA", "ALARM", "Interface_GRM", "WEBMON", "MEASLOG", "MEASLOG_HIST", "LASERDIAG"];
const GRM_DBS: [&str; 4] = ["OPCUA", "STATION", "CELL", "MACHINE"];
/// Tables a client may write over S7: absorbed back into the model (see `encode_models`).
const ABSORB: [&str; 3] = ["CELL", "STATION", "LASERDIAG"];

/// One robot of the demo: its status PLC and where it sits behind GRM.
#[derive(Clone)]
pub struct DemoRobot {
    /// `[[plcs]]` name of the robot's status PLC (e.g. `GR1`).
    pub plc_name: String,
    pub contract: Arc<Contract>,
    /// Header DST this robot accepts (4001 / 4002).
    pub dst: u16,
    /// 0-based index into GRM `OPCUA.GR` (declared `Array[1..3]`): `GR[n]` → `n - 1`.
    pub gr_index: usize,
    /// `PARA.Machine.ID` — the connect-time semantic check compares it with the config.
    pub machine_id: u8,
}

impl DemoRobot {
    /// `GR[2].CMD` → `Some(1)`; `None` when the root does not start with `GR[n]`.
    pub fn gr_index_of(opcua_root: &str) -> Option<usize> {
        let n: usize = opcua_root.strip_prefix("GR[")?.split(']').next()?.trim().parse().ok()?;
        n.checked_sub(1)
    }
}

pub struct DemoWorld {
    inner: Mutex<Inner>,
    grm_store: DbStore,
    grm: Arc<Contract>,
    pub grm_addr: SocketAddr,
    /// (plc name, fake server address) per robot side, in side order — readable without the model lock.
    addrs: Vec<(String, SocketAddr)>,
    _servers: Vec<FakeS7Server>,
}

struct Running {
    task: TaskData,
    step_idx: usize,
    step_at: Instant,
}

/// One robot PLC: its models, S7 store and command/task state.
struct Side {
    plc: String,
    gr_index: usize,
    dst: u16,
    contract: Arc<Contract>,
    store: DbStore,
    models: HashMap<String, Json>,
    /// Bytes last encoded per DB number for S7-writable tables; a difference means a client wrote them.
    encoded: HashMap<u16, Vec<u8>>,
    /// LASERDIAG modelled and served (the contract has its layout and DB number).
    has_laser: bool,
    pending: Option<(Header, TaskData)>,
    last_header: Header,
    queue: Vec<TaskData>,
    now: Option<Running>,
    completed: VecDeque<TaskData>,
    canceled: VecDeque<TaskData>,
    rejected: VecDeque<TaskData>,
    meas_total: u32,
    axis: [f32; 4],
    target: [f32; 4],
    rng: StdRng,
    /// JSON pointer of the GRM `GR[n].CMD` a relayed OPC UA command came from (cleared after the echo).
    relay_root: Option<String>,
    op_echo: Option<(TaskOp, u32, u32)>,
    /// A task that just ended: stays in `Now` for one tick with `Status.Complete`/`Canceled` raised,
    /// then moves to its ring — the real PLC's order, which the ledger sync relies on.
    ended: Option<(TaskData, bool)>,
}

struct Inner {
    sides: Vec<Side>,
    grm: HashMap<String, Json>,
    grm_encoded: HashMap<u16, Vec<u8>>,
    cmd_id: u8,
    seq: u16,
    tick: u64,
    /// Zeroed `GR[n].CMD` subtree used to clear Header/TaskData/Data like GRM does.
    cmd_zero: Json,
}

fn ring_push(ring: &mut VecDeque<TaskData>, t: TaskData) {
    ring.push_front(t);
    while ring.len() > RING {
        ring.pop_back();
    }
}

fn ring_json(ring: &VecDeque<TaskData>) -> Json {
    let mut v: Vec<Json> = ring.iter().map(|t| serde_json::to_value(t).unwrap_or(Json::Null)).collect();
    while v.len() < RING {
        v.push(serde_json::to_value(TaskData::default()).unwrap_or(Json::Null));
    }
    Json::Array(v)
}

/// PARA.Sensor Z offsets in LASERDIAG direction order (L, F, R, B).
const LASER_OFFSETS: [&str; 4] = ["GIDL_ZOffset", "GIDF_ZOffset", "GIDR_ZOffset", "GIDB_ZOffset"];
/// Measurements per demo Z-offset calibration.
const LASER_CAL_COUNT: i64 = 5;
/// Demo mounting height error per direction (PCR L / F / R, mm): gives the bias trend and the calibration something to find.
const LASER_MOUNT: [f64; 3] = [1.5, -0.5, -1.0];
/// Laser distance when nothing is in the beam (`PARA.Sensor.ItemDetectDistance_H` clamp).
const LASER_FAR: f64 = 800.0;

/// Simulated gripper laser distances (L, F, R, B) at gripper height `z` / opening `g` over the task's tire: nothing
/// above or below the tire, the bead (≈ inner diameter) in the top and bottom bead bands, the wider cavity in between.
/// The tire stands on the cell floor (`Cell.Position[Z]`, else target Z - lower bead height); B only for TBR sizes (OD ≥ 900).
fn laser_gid(t: &TaskData, z: f64, g: f64, tick: u64) -> [f64; 4] {
    let id = f64::from(t.item.inner_diameter);
    let od = f64::from(t.item.outer_diameter);
    let mut out = [LASER_FAR; 4];
    if id <= 0.0 || od <= id {
        return out;
    }
    let height = if t.item.height > 0.0 { f64::from(t.item.height) } else { 200.0 };
    let floor = if t.cell.position[2] > 0.0 { f64::from(t.cell.position[2]) } else { f64::from(t.position[2]) - f64::from(t.item.lower_bid_height) };
    let bead = (height * 0.12).clamp(12.0, 30.0);
    let half = bead / 2.0;
    for (k, slot) in out.iter_mut().enumerate() {
        if k == 3 && od < 900.0 {
            continue;
        }
        let r = z + LASER_MOUNT.get(k).copied().unwrap_or(0.0) - floor;
        if !(0.0..=height).contains(&r) {
            continue;
        }
        let dia = if r >= height - bead {
            id + 6.0 * ((r - (height - half)) / half).powi(2)
        } else if r <= bead {
            id + 6.0 * ((r - half) / half).powi(2)
        } else {
            od - 0.15 * (od - id)
        };
        let jitter = ((tick as f64 * 12.9898 + k as f64 * 78.233).sin() * 43_758.545).fract() * 0.6;
        *slot = ((dia - g) / 2.0 + jitter).clamp(0.0, LASER_FAR);
    }
    out
}

fn set(j: &mut Json, ptr: &str, v: Json) {
    if let Some(slot) = j.pointer_mut(ptr) {
        *slot = v;
    } else {
        tracing::debug!(ptr, "demo: pointer missing");
    }
}

fn get_f(j: &Json, ptr: &str) -> f64 {
    j.pointer(ptr).and_then(|v| v.as_f64()).unwrap_or(0.0)
}

/// `set` on a model DB that may not exist for this contract.
fn set_db(models: &mut HashMap<String, Json>, db: &str, ptr: &str, v: Json) {
    if let Some(d) = models.get_mut(db) {
        set(d, ptr, v);
    }
}

/// Whether the first `Count` entries of a CELL / STATION table hold `id` at `id_ptr`.
fn table_has(d: Option<&Json>, list: &str, id_ptr: &str, id: u16) -> bool {
    let Some(d) = d else { return false };
    let n = d["Count"].as_u64().unwrap_or(0) as usize;
    d[list].as_array().is_some_and(|a| a.iter().take(n).any(|e| e.pointer(id_ptr).and_then(Json::as_u64) == Some(u64::from(id))))
}

/// Zero models decoded from the contract. A DB the contract cannot lay out is skipped with a warning, so an older
/// robot contract still gets a demo PLC for everything it does have.
fn zero_models(contract: &Contract, dbs: &[&str], plc: &str) -> HashMap<String, Json> {
    let mut out = HashMap::new();
    for db in dbs {
        match contract.size_of_db(db).and_then(|size| contract.decode_db(db, &vec![0u8; size as usize])) {
            Ok(j) => {
                out.insert((*db).to_string(), j);
            }
            Err(e) => tracing::warn!(plc, db, "demo: DB not modelled: {e}"),
        }
    }
    out
}

/// Writes every model DB into a fake S7 store. Tables a client may write over S7 (`ABSORB`) are absorbed first: if
/// their bytes changed since the last encode (a push from the console), the model is re-decoded from them, like a
/// real PLC keeping the written values. Check and encode run under the store lock, so a concurrent S7 write cannot
/// slip between them. DBs without a number in the contract are not served.
fn encode_models(contract: &Contract, store: &DbStore, models: &mut HashMap<String, Json>, encoded: &mut HashMap<u16, Vec<u8>>) {
    let mut bytes = store.lock().unwrap_or_else(PoisonError::into_inner);
    for (db, json) in models.iter_mut() {
        let (Some(n), Ok(size)) = (contract.db_number(db), contract.size_of_db(db)) else { continue };
        let buf = bytes.entry(n).or_insert_with(|| vec![0u8; size as usize]);
        let absorb = ABSORB.contains(&db.as_str());
        if absorb && encoded.get(&n).is_some_and(|last| last != buf) {
            match contract.decode_db(db, buf) {
                Ok(j) => *json = j,
                Err(e) => tracing::error!(db, "demo absorb: {e}"),
            }
        }
        if let Err(e) = contract.encode_db(db, json, buf) {
            tracing::error!(db, "demo encode: {e}");
        }
        if absorb {
            encoded.insert(n, buf.clone());
        }
    }
}

pub(crate) fn sample_cell(id: u16, i: usize) -> CellInfo {
    CellInfo {
        use_: true,
        blend_use: false,
        id,
        section: 2,
        row: (i % 4 + 1) as u16,
        col: (i / 4 + 1) as u16,
        length: 1200.0,
        width: 1200.0,
        position: [12000.0 + (i % 4) as f32 * 1300.0, 3000.0 + (i / 4) as f32 * 1300.0, 1500.0],
    }
}

fn sample_station(id: u16, i: usize) -> StationPara {
    StationPara {
        conv_no: (i + 1) as u16,
        task_type: 1,
        rotate_type: 1,
        group: 1,
        group_index: (i + 1) as u8,
        info: CellInfo { use_: true, blend_use: false, id, section: 3, row: (i + 1) as u16, col: 1, length: 1500.0, width: 1500.0, position: [20000.0 + i as f32 * 2000.0, 1000.0, 1450.0] },
        ..Default::default()
    }
}

fn side_index(sides: &[Side], dst: u16) -> usize {
    // an unknown DST lands on the first robot, which then rejects it as the wrong robot — like a real GR would
    sides.iter().position(|s| s.dst == dst).unwrap_or(0)
}

impl DemoWorld {
    /// Starts one fake S7 server per robot plus one for GRM. `robots` must not be empty; give each PLC one entry.
    pub async fn start(robots: Vec<DemoRobot>, grm: Arc<Contract>, tick_ms: u64) -> anyhow::Result<Arc<DemoWorld>> {
        anyhow::ensure!(!robots.is_empty(), "demo: no robot PLC");
        let grm_store = FakeS7Server::store();
        let grm_server = FakeS7Server::spawn(grm_store.clone(), 960).await?;
        let mut grm_models = zero_models(&grm, &GRM_DBS, "GRM");
        anyhow::ensure!(grm_models.contains_key("OPCUA"), "demo: GRM contract has no OPCUA DB");
        set_db(&mut grm_models, "MACHINE", "/Communication/NextCmdID", json!(1));
        // registries: the GCS cell / station tables are plant-wide, so every PLC gets the same ones
        let cells: Vec<CellInfo> = (0..12).map(|i| sample_cell(101 + i as u16, i)).collect();
        let stations: Vec<StationPara> = (0..2).map(|i| sample_station(2101 + i as u16, i)).collect();
        if let Some(db) = grm_models.get_mut("CELL") {
            set(db, "/Count", json!(cells.len()));
            for (i, c) in cells.iter().enumerate() {
                set(db, &format!("/Cell/{i}"), serde_json::to_value(c)?);
            }
        }
        if let Some(db) = grm_models.get_mut("STATION") {
            set(db, "/Count", json!(stations.len()));
            for (i, s) in stations.iter().enumerate() {
                set(db, &format!("/Station/{i}/Para"), serde_json::to_value(s)?);
            }
            // 스테이션 보정 미리보기용 트래킹(슬롯 1 = 2101, RotateType 1 → TY = -OD/2, TX = 측정 오프셋)
            set(db, "/Station/0/Tracking/Now", json!({ "OutterDiameter": 640.0, "TaskOffset": [35.0, 0.0] }));
        }
        let cmd_zero = grm_models["OPCUA"].pointer(&format!("/GR/{}/CMD", robots[0].gr_index)).or_else(|| grm_models["OPCUA"].pointer("/GR/0/CMD")).cloned().unwrap_or(Json::Null);

        let mut sides = Vec::new();
        let mut servers = vec![grm_server];
        let mut addrs = Vec::new();
        for r in robots {
            let store = FakeS7Server::store();
            let server = FakeS7Server::spawn(store.clone(), 960).await?;
            addrs.push((r.plc_name.clone(), server.addr));
            servers.push(server);
            let mut models = zero_models(&r.contract, &GR_DBS, &r.plc_name);
            for db in ["WEBMON", "MEASLOG", "MEASLOG_HIST", "LASERDIAG"] {
                if let (Some(m), Ok(sig)) = (models.get_mut(db), r.contract.layout_sig(db)) {
                    set(m, "/LayoutSig", json!(sig));
                }
            }
            let has_laser = models.contains_key("LASERDIAG") && r.contract.db_number("LASERDIAG").is_some();
            let gi = r.gr_index as f32;
            set_db(&mut models, "OPCUA", "/STAT/ComponentID", json!(r.dst));
            set_db(&mut models, "PARA", "/Machine/ID", json!(r.machine_id));
            // a per-robot PARA value so the PARA page visibly changes with the robot
            set_db(&mut models, "PARA", "/Machine/XLength", json!(28_000 + 4_000 * r.gr_index as u32));
            set_db(&mut grm_models, "OPCUA", &format!("/GR/{}/STAT/ComponentID", r.gr_index), json!(r.dst));
            if let Some(db) = models.get_mut("CELL") {
                set(db, "/Count", json!(cells.len()));
                for (i, c) in cells.iter().enumerate() {
                    set(db, &format!("/Cell/{i}"), serde_json::to_value(c)?);
                }
            }
            if let Some(db) = models.get_mut("STATION") {
                set(db, "/Count", json!(stations.len()));
                for (i, s) in stations.iter().enumerate() {
                    set(db, &format!("/Station/{i}"), serde_json::to_value(s)?);
                }
            }
            set_db(&mut models, "WEBMON", "/Mode", json!(32));
            for p in ["/STAT/Mode/Auto", "/STAT/Status/Online", "/STAT/Status/Normal", "/STAT/Status/PowerOn"] {
                set_db(&mut models, "OPCUA", p, json!(true));
            }
            // GR[1] parks around X 3000, GR[2] around X 12000: the two gantries share one rail
            let home = [3000.0 + 9000.0 * gi, 3000.0, 2500.0, 300.0];
            let mut side = Side {
                plc: r.plc_name,
                gr_index: r.gr_index,
                dst: r.dst,
                contract: r.contract,
                store,
                models,
                encoded: HashMap::new(),
                has_laser,
                pending: None,
                last_header: Header::default(),
                queue: Vec::new(),
                now: None,
                completed: VecDeque::new(),
                canceled: VecDeque::new(),
                rejected: VecDeque::new(),
                meas_total: 0,
                axis: home,
                target: home,
                rng: StdRng::seed_from_u64(7 + 1000 * r.gr_index as u64),
                relay_root: None,
                op_echo: None,
                ended: None,
            };
            // seed some history so the measurement screens have content — a different amount and tire size per robot
            let (count, id_base, work_base) = (25 + 15 * r.gr_index as u32, 355.6 + 25.4 * gi, 3000 + 1000 * r.gr_index as u32);
            for i in 0..count {
                let mut t = TaskData::default();
                t.work_id = work_base + i;
                t.task_id = 1;
                t.task_type = if i % 3 == 0 { TaskType::Measure.code() } else { TaskType::Pick.code() };
                t.measure_item = i % 3 == 0;
                t.cell = cells[(i as usize) % cells.len()].clone();
                t.item = StockItem { code: 1001 + (i % 3), count: 3, inner_diameter: id_base + (i % 3) as f32 * 50.0, outer_diameter: 780.0, height: 240.0, ..Default::default() };
                t.position = [t.cell.position[0], t.cell.position[1], 1500.0 + 480.0, 300.0];
                side.push_measure(&t);
            }
            side.encode();
            sides.push(side);
        }
        let mut inner = Inner { sides, grm: grm_models, grm_encoded: HashMap::new(), cmd_id: 1, seq: 1, tick: 0, cmd_zero };
        encode_models(&grm, &grm_store, &mut inner.grm, &mut inner.grm_encoded);
        let world = Arc::new(DemoWorld { inner: Mutex::new(inner), grm_store, grm, grm_addr: servers[0].addr, addrs, _servers: servers });
        let w = world.clone();
        tokio::spawn(async move {
            let mut iv = tokio::time::interval(Duration::from_millis(tick_ms.max(50)));
            loop {
                iv.tick().await;
                w.tick();
            }
        });
        Ok(world)
    }

    /// Fake S7 server address of a robot PLC.
    pub fn addr_of(&self, plc_name: &str) -> Option<SocketAddr> {
        self.addrs.iter().find(|(n, _)| n == plc_name).map(|(_, a)| *a)
    }

    /// (plc name, address) of every robot side, first robot first.
    pub fn robot_addrs(&self) -> &[(String, SocketAddr)] {
        &self.addrs
    }

    fn next_header(g: &mut Inner, cmd: u8, src: u16, dst: u16, protocol: u8) -> Header {
        g.cmd_id = g.cmd_id.wrapping_add(1);
        if g.cmd_id == 0 {
            g.cmd_id = 1;
        }
        g.seq = g.seq.wrapping_add(1);
        if g.seq == 0 {
            g.seq = 1;
        }
        Header { protocol, cmd_id: g.cmd_id, cmd, src, dst, seq: g.seq }
    }

    /// Like the GCS writing GRM `GR[n].CMD` of the robot with this DST: returns the header used; the tick processes it.
    pub fn submit(&self, task: TaskData, cmd: u8, src: u16, dst: u16, protocol: u8) -> Header {
        let mut g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let h = Self::next_header(&mut g, cmd, src, dst, protocol);
        let i = side_index(&g.sides, dst);
        g.sides[i].pending = Some((h.clone(), task));
        h
    }

    /// Complete / Delete task op for the robot with this DST (`work_id == 0` re-arms).
    pub fn task_op(&self, op: TaskOp, dst: u16, work_id: u32, task_id: u32) {
        let mut g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let i = side_index(&g.sides, dst);
        g.sides[i].op_echo = if work_id == 0 { None } else { Some((op, work_id, task_id)) };
    }

    #[allow(dead_code)]
    pub fn clear_header(&self, dst: u16) {
        let mut g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let i = side_index(&g.sides, dst);
        g.sides[i].pending = None;
    }

    /// Value at a JSON pointer of the GR2 `OPCUA` model (the first robot when there is no `GR2`) — bench / tests.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn gr2_get(&self, ptr: &str) -> Json {
        let g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let side = g.sides.iter().find(|s| s.plc == "GR2").or(g.sides.first());
        side.and_then(|s| s.models.get("OPCUA")).and_then(|d| d.pointer(ptr)).cloned().unwrap_or(Json::Null)
    }

    /// Value at a JSON pointer of a robot PLC's model DB (tests).
    #[cfg(test)]
    pub fn robot_get(&self, plc: &str, db: &str, ptr: &str) -> Json {
        let g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        g.sides.iter().find(|s| s.plc == plc).and_then(|s| s.models.get(db)).and_then(|d| d.pointer(ptr)).cloned().unwrap_or(Json::Null)
    }

    /// OPC UA read of the GRM `OPCUA` model (sim server).
    pub fn grm_opcua_get(&self, ptr: &str) -> Json {
        let g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        g.grm.get("OPCUA").and_then(|d| d.pointer(ptr)).cloned().unwrap_or(Json::Null)
    }

    /// OPC UA write into GRM `"OPCUA".GR[n].CMD` (sim server), emulating GRM: the value lands in the DB the
    /// console reads over S7; a Header whose six fields are all non-zero (completed by the SEQ write) is
    /// relayed to the robot at `GR[n]` with the TaskData currently in CMD; a Complete/Delete TaskId write with a
    /// non-zero WorkId becomes a task op for that robot, `(0,0)` re-arms. `GR[n]` without a demo robot only stores.
    pub fn grm_opcua_write(&self, path: &str, ptr: &str, value: Json) {
        let mut g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let written = value.as_u64().unwrap_or(0) as u32;
        let Inner { sides, grm, .. } = &mut *g;
        let Some(opc) = grm.get_mut("OPCUA") else { return };
        set(opc, ptr, value);
        let Some(i) = ptr.find("/CMD/") else { return };
        let root = ptr[..i + 4].to_string();
        let gr_index: Option<usize> = root.strip_prefix("/GR/").and_then(|r| r.split('/').next()).and_then(|n| n.parse().ok());
        let Some(side) = sides.iter_mut().find(|s| Some(s.gr_index) == gr_index) else { return };
        let opc = &*opc;
        if path.ends_with(".Header.SEQ") {
            let header: Header = opc.pointer(&format!("{root}/Header")).and_then(|h| serde_json::from_value(h.clone()).ok()).unwrap_or_default();
            let fields = [u32::from(header.protocol), u32::from(header.cmd_id), u32::from(header.cmd), u32::from(header.src), u32::from(header.dst), u32::from(header.seq)];
            if fields.iter().all(|v| *v != 0) {
                let task = opc.pointer(&format!("{root}/TaskData")).and_then(|t| TaskData::from_json(t).ok()).unwrap_or_default();
                side.pending = Some((header, task));
                side.relay_root = Some(root);
            }
        } else {
            let op = if path.ends_with(".Command.Task.Complete.TaskId") {
                TaskOp::Complete
            } else if path.ends_with(".Command.Task.Delete.TaskId") {
                TaskOp::Delete
            } else {
                return;
            };
            let base = if op == TaskOp::Complete { "Complete" } else { "Delete" };
            let work_id = opc.pointer(&format!("{root}/Command/Task/{base}/WorkId")).and_then(Json::as_u64).unwrap_or(0) as u32;
            side.op_echo = if work_id == 0 { None } else { Some((op, work_id, written)) };
        }
    }

    fn tick(&self) {
        let mut g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        g.tick += 1;
        let tick = g.tick;
        let ntp = now_str().replace('T', " ").chars().take(23).collect::<String>();
        let Inner { sides, grm, grm_encoded, cmd_zero, .. } = &mut *g;
        for s in sides.iter_mut() {
            s.tick(tick, &ntp, grm, cmd_zero);
            s.encode();
        }
        encode_models(&self.grm, &self.grm_store, grm, grm_encoded);
    }

    /// Current queue keys of the robot with this DST (used by tests).
    #[allow(dead_code)]
    pub fn queue_keys(&self, dst: u16) -> Vec<TaskKey> {
        let g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        g.sides[side_index(&g.sides, dst)].queue.iter().map(|t| t.key()).collect()
    }
}

impl Side {
    fn encode(&mut self) {
        encode_models(&self.contract, &self.store, &mut self.models, &mut self.encoded);
    }

    /// One tick of this robot; mirrors its STAT into GRM `OPCUA.GR[gr_index]`.
    fn tick(&mut self, tick: u64, ntp: &str, grm: &mut HashMap<String, Json>, cmd_zero: &Json) {
        let gr = format!("/GR/{}", self.gr_index);
        // ---- command intake (GR UL_ParseOpcUaCommand semantics, simplified)
        if let Some((h, task)) = self.pending.take() {
            let mut data = [0u8; 16];
            let mut validation: u16 = 1;
            if h.dst != self.dst {
                data[0] |= 0x01;
            }
            if h == self.last_header {
                data[0] |= 0x02;
            }
            let known = table_has(self.models.get("CELL"), "Cell", "/Id", task.cell.id) || table_has(self.models.get("STATION"), "Station", "/Info/Id", task.cell.id);
            if task.cell.id == 0 {
                validation = 401;
            } else if !known {
                validation = if task.cell.is_station() { 419 } else { 429 };
            } else if task.item.code == 0 && task.task_type != TaskType::Move.code() {
                validation = 501;
            } else if TaskType::from_code(task.task_type).is_none() {
                validation = 203;
            }
            if validation != 1 {
                data[0] |= 0x04;
            }
            if self.queue.len() >= QUEUE {
                data[0] |= 0x08;
                validation = 110;
            }
            data[1] = (validation >> 8) as u8;
            data[2] = validation as u8;
            data[5] = 1;
            data[6] = 1;
            let accepted = RejectInfo::from_data(&data).accepted();
            self.last_header = h.clone();
            let res_h = serde_json::to_value(&h).unwrap_or(Json::Null);
            let res_t = serde_json::to_value(&task).unwrap_or(Json::Null);
            let res_d = Json::Array(data.iter().map(|b| json!(b)).collect());
            for (models, prefix) in [(&mut self.models, String::new()), (&mut *grm, gr.clone())] {
                set_db(models, "OPCUA", &format!("{prefix}/STAT/RES/Header"), res_h.clone());
                set_db(models, "OPCUA", &format!("{prefix}/STAT/RES/Task"), res_t.clone());
                set_db(models, "OPCUA", &format!("{prefix}/STAT/RES/Data"), res_d.clone());
            }
            if accepted {
                self.queue.push(task);
            } else {
                ring_push(&mut self.rejected, task);
            }
        }
        // ---- GRM clears CMD Header/TaskData/Data once RES.Header == CMD.Header (OPC UA relay only)
        if let Some(root) = self.relay_root.take()
            && let Some(opc) = grm.get_mut("OPCUA")
        {
            for part in ["Header", "TaskData", "Data"] {
                if let Some(z) = cmd_zero.get(part) {
                    set(opc, &format!("{root}/{part}"), z.clone());
                }
            }
        }
        // ---- the task that ended last tick (bit was up) now goes to its ring
        if let Some((t, canceled)) = self.ended.take() {
            if canceled {
                ring_push(&mut self.canceled, t);
            } else {
                self.push_measure(&t);
                ring_push(&mut self.completed, t);
            }
        }
        // ---- task ops
        if let Some((op, work_id, task_id)) = self.op_echo.take() {
            match op {
                TaskOp::Delete => {
                    let mut removed = Vec::new();
                    self.queue.retain(|t| {
                        let hit = t.work_id == work_id && (task_id == 0 || t.task_id == task_id);
                        if hit {
                            removed.push(t.clone());
                        }
                        !hit
                    });
                    if let Some(r) = self.now.take_if(|r| r.task.work_id == work_id && (task_id == 0 || r.task.task_id == task_id)) {
                        // the running task is also Queue[0]: one key, one ring entry
                        removed.retain(|t| t.key() != r.task.key());
                        self.ended = Some((r.task, true));
                    }
                    for t in removed {
                        ring_push(&mut self.canceled, t);
                    }
                }
                TaskOp::Complete => {
                    if let Some(r) = self.now.take_if(|r| r.task.work_id == work_id && r.task.task_id == task_id) {
                        self.queue.retain(|q| q.key() != r.task.key());
                        self.ended = Some((r.task, false));
                    } else if let Some(i) = self.queue.iter().position(|t| t.work_id == work_id && t.task_id == task_id) {
                        let t = self.queue.remove(i);
                        ring_push(&mut self.completed, t);
                    }
                }
            }
        }
        // ---- execution
        if self.now.is_none() && !self.queue.is_empty() {
            let t = self.queue[0].clone();
            self.target = [t.position[0], t.position[1], t.position[2], t.position[3]];
            self.now = Some(Running { task: t, step_idx: 0, step_at: Instant::now() });
        }
        let mut finished: Option<TaskData> = None;
        if let Some(r) = self.now.as_mut()
            && r.step_at.elapsed() > Duration::from_millis(700)
        {
            r.step_idx += 1;
            r.step_at = Instant::now();
            if r.step_idx >= STEPS.len() {
                finished = Some(r.task.clone());
            }
        }
        if let Some(t) = finished {
            self.now = None;
            self.queue.retain(|q| q.key() != t.key());
            self.ended = Some((t, false));
            self.target[2] = 2500.0;
            self.target[3] = 300.0;
        }
        // ---- axes
        for i in 0..4 {
            let d = self.target[i] - self.axis[i];
            self.axis[i] += d * 0.25;
        }
        // ---- publish status
        let step = self.now.as_ref().map(|r| STEPS[r.step_idx.min(STEPS.len() - 1)]).unwrap_or(0);
        // an ended task is still `Now` for this tick, with its terminal bit raised
        let now_task = self.now.as_ref().map(|r| r.task.clone()).or_else(|| self.ended.as_ref().map(|(t, _)| t.clone())).unwrap_or_default();
        let now_json = serde_json::to_value(now_task).unwrap_or(Json::Null);
        let (bit_complete, bit_canceled) = match &self.ended {
            Some((_, canceled)) => (!canceled, *canceled),
            None => (false, false),
        };
        let mut queue: Vec<Json> = self.queue.iter().map(|t| serde_json::to_value(t).unwrap_or(Json::Null)).collect();
        while queue.len() < QUEUE {
            queue.push(serde_json::to_value(TaskData::default()).unwrap_or(Json::Null));
        }
        let queue = Json::Array(queue);
        let (completed, canceled, rejected) = (ring_json(&self.completed), ring_json(&self.canceled), ring_json(&self.rejected));
        let accept = self.queue.len() < QUEUE && self.pending.is_none();
        let status = json!({ "Accept": accept, "Idle": self.now.is_none() && self.queue.is_empty(), "Assigned": !self.queue.is_empty(), "Inprogress": self.now.is_some(),
            "AvoidReq": false, "HoldItem": (500..999).contains(&step), "Complete": bit_complete, "Canceled": bit_canceled, "Reserved": 0, "Step": step });
        let (axis, target, running) = (self.axis, self.target, self.now.is_some());
        if let Some(opc) = self.models.get_mut("OPCUA") {
            set(opc, "/STAT/NTP", json!(ntp));
            set(opc, "/STAT/Task/Status", status.clone());
            set(opc, "/STAT/Task/Now", now_json.clone());
            set(opc, "/STAT/Task/Queue", queue.clone());
            set(opc, "/STAT/Task/Completed", completed.clone());
            set(opc, "/STAT/Task/Canceled", canceled.clone());
            set(opc, "/STAT/Task/Rejected", rejected.clone());
            set(opc, "/STAT/Status/Idle", json!(!running));
            set(opc, "/STAT/Status/Busy", json!(running));
            set(opc, "/STAT/Status/HeartBeat", json!(tick.is_multiple_of(2)));
            set(opc, "/STAT/Status/ItemDectect", json!((500..999).contains(&step)));
            for (i, a) in axis.iter().enumerate() {
                set(opc, &format!("/STAT/Drive/{i}/Position"), json!(a));
            }
        }
        let stat = self.models.get("OPCUA").and_then(|o| o.get("STAT")).cloned();
        if let Some(task) = self.models.get_mut("TASK") {
            set(task, "/Status", status);
            set(task, "/Now", now_json);
            set(task, "/Buff", queue);
            set(task, "/Last", completed);
            set(task, "/Canceled", canceled);
            set(task, "/Rejected", rejected);
            set(task, "/Proc/Step/Now", json!(step));
        }
        if let (Some(opc), Some(stat)) = (grm.get_mut("OPCUA"), stat.as_ref()) {
            set(opc, &format!("{gr}/STAT"), stat.clone());
            // GRM overrides Accept while a command sits in CMD
            set(opc, &format!("{gr}/STAT/Task/Status/Accept"), json!(accept));
        }
        let meas_total = self.meas_total;
        let meas_count = meas_total.min(200);
        let measure_json = {
            let t = self.now.as_ref().map(|r| r.task.clone());
            json!({
                "Item": { "Busy": t.as_ref().is_some_and(|t| t.measure_item) && (400..999).contains(&step), "Done": false, "Reported": false, "WorkCell": t.as_ref().map(|t| t.cell.id).unwrap_or(0),
                          "Code": t.as_ref().map(|t| t.item.code).unwrap_or(0), "Status": if step > 0 { 1 } else { 0 }, "InnerDia": 0.0, "OffsetX": 0.0, "OffsetY": 0.0, "UpperBeadHeight": 0.0, "TireHeight": 0.0,
                          "Torq_InnerDia": 0.0, "InBusy": step == 400, "InDone": step >= 500, "InValid": step >= 500, "InInnerDia": t.as_ref().map(|t| t.item.inner_diameter + 0.9).unwrap_or(0.0),
                          "OutBusy": step == 600, "OutDone": false, "OutValid": false, "OutInnerDia": 0.0 },
                "Sku": { "Enable": false, "Busy": false, "Done": false, "WorkCell": 0, "StackCount": 0, "StackHeight": 0.0, "EachHeight": 0.0, "BeadPos": vec![0.0f32; 20] },
                "Floor": { "Enable": false, "Busy": false, "Done": false, "Height": 1500.3 },
                "LastBead": { "Enable": step == 400, "Busy": step == 400, "Done": step > 400, "LastBidPos": 24.6, "PickZTarget": 0.0 }
            })
        };
        // laser distances over the running task's tire; the gripper stalls on the tire while gripping (step 500)
        let gid = self.now.as_ref().map(|r| laser_gid(&r.task, f64::from(axis[2]), f64::from(axis[3]), tick)).unwrap_or([LASER_FAR; 4]);
        let step_secs = self.now.as_ref().map(|r| r.step_at.elapsed().as_secs_f64()).unwrap_or(0.0);
        let msg = format!("{} Step {step}", self.plc);
        if let Some(w) = self.models.get_mut("WEBMON") {
            set(w, "/UpdateTime", json!(ntp));
            if let Some(stat) = stat {
                set(w, "/Stat", stat);
            }
            set(w, "/Proc/Step/Now", json!(step));
            // PLC name in the message: shows at a glance which robot's stream is on screen
            set(w, "/Proc/Msg", json!(msg));
            set(w, "/Alarm/FaultCode/0", json!(0));
            for (i, a) in axis.iter().enumerate() {
                set(w, &format!("/Axis/{i}/Position"), json!(a));
                set(w, &format!("/Axis/{i}/Target"), json!(target[i]));
                set(w, &format!("/Axis/{i}/Running"), json!((target[i] - a).abs() > 1.0));
                set(w, &format!("/Axis/{i}/StandStill"), json!((target[i] - a).abs() <= 1.0));
                set(w, &format!("/Axis/{i}/Ready"), json!(true));
                set(w, &format!("/Axis/{i}/Enabled"), json!(true));
            }
            set(w, "/Gripper/ItemDetect", json!((500..999).contains(&step)));
            set(w, "/Gripper/GID", json!(gid));
            set(w, "/Gripper/FLD", json!(800.0 - (axis[2] - 1500.0)));
            let g_settled = (target[3] - axis[3]).abs() <= 1.0;
            let gripping = (500..999).contains(&step);
            set(
                w,
                "/Gripper/State",
                json!({ "Commanded": step == 500 || !g_settled, "Stopped": g_settled, "AtCommand": g_settled && !gripping, "TorqueReached": gripping,
                        "TorqueStop": step == 500, "Stall": step == 500, "StallNoTorque": false, "StallTime": if step == 500 { step_secs } else { 0.0 } }),
            );
            set(w, "/Measure", measure_json);
            set(w, "/MeasLog/Total", json!(meas_total));
            set(w, "/MeasLog/Count", json!(meas_count));
        }
        self.laser_tick();
    }

    /// Appends a MEASLOG_HIST entry for a finished task and updates MEASLOG counters / stats.
    fn push_measure(&mut self, t: &TaskData) {
        let kind: u8 = match TaskType::from_code(t.task_type) {
            Some(TaskType::Pick) => 4,
            Some(TaskType::Measure) if t.measure_sku => 2,
            Some(TaskType::Measure) if t.measure_floor => 3,
            Some(TaskType::Measure) => 1,
            _ => return,
        };
        let id = t.item.inner_diameter;
        let h = t.item.height;
        let zrel = t.position[2] - t.cell.position[2];
        let n = |rng: &mut StdRng, sd: f32| -> f32 { (rng.random::<f32>() + rng.random::<f32>() + rng.random::<f32>() - 1.5) * sd };
        let rng = &mut self.rng;
        let mut data = [0f32; 20];
        let mut delta = gr_proto::measure::Delta::default();
        let mut status = 2u16;
        match kind {
            1 => {
                let in_id = id + n(rng, 1.2);
                let out_id = id + n(rng, 1.2);
                let tq = id + 1.5 + n(rng, 0.8);
                let bead = zrel + h - 20.0 + n(rng, 1.0);
                let th = zrel + h + n(rng, 1.5);
                let (ox, oy) = (n(rng, 3.0), n(rng, 3.0));
                let rid = if ((in_id + out_id) / 2.0 - tq).abs() <= 10.0 { ((in_id + out_id) / 2.0 + tq) / 2.0 } else { tq };
                data[..20].copy_from_slice(&[2.0, rid, bead, th, 0.0, 0.0, 0.0, tq, 0.35, 0.0, in_id, bead, th, ox, oy, out_id, bead, th, ox, oy]);
                delta = gr_proto::measure::Delta { inner_dia: rid - id, height: th - zrel - h, z: bead - zrel, offset: (ox * ox + oy * oy).sqrt(), count: 0.0 };
                if rng.random::<f32>() < 0.1 {
                    status = 4;
                    data[0] = 4.0;
                }
            }
            2 => {
                let cnt = t.item.count.max(1) as usize;
                let each = h - 3.0 + n(rng, 1.0);
                for k in 1..=cnt.min(7) {
                    data[2 * k - 1] = (k - 1) as f32 * each + 20.0;
                    data[2 * k] = (k - 1) as f32 * each + h - 20.0;
                }
                data[0] = 2.0;
                // 스택 정렬 진단 — 실기는 단이 어긋나면 DiagFlags 를 세우고 콘솔은 그런 표본을 규격에 안 넣는다
                data[16] = (n(rng, 5.0)).abs();
                data[15] = if data[16] > 5.0 { 33.0 } else { 0.0 };
                data[17] = cnt as f32;
                data[18] = each;
                data[19] = cnt as f32 * each;
                delta = gr_proto::measure::Delta { inner_dia: 0.0, height: each - h, z: data[19] - zrel, offset: 0.0, count: 0.0 };
            }
            3 => {
                data[0] = 2.0;
                data[1] = 1500.0 + n(rng, 1.5);
                data[2] = 1700.0;
                data[3] = 200.0;
                delta.z = data[1] - 1500.0;
            }
            _ => {
                let last = 25.0 + n(rng, 2.0);
                let valid = rng.random::<f32>() > 0.2;
                data[0] = if valid { 2.0 } else { 3.0 };
                data[1] = last;
                data[2] = if last - 20.0 > 0.0 && last - 20.0 < h * 0.3 { 1500.0 + zrel + last - 20.0 } else { 0.0 };
                data[3] = if valid { 1.0 } else { 0.0 };
                data[5] = if valid { id + n(rng, 2.0) } else { 0.0 };
                data[6] = if valid { n(rng, 4.0) } else { 0.0 };
                data[7] = if valid { n(rng, 4.0) } else { 0.0 };
                data[8] = zrel + h - 20.0;
                delta.z = last;
                if valid {
                    delta.inner_dia = data[5] - id;
                    delta.offset = (data[6] * data[6] + data[7] * data[7]).sqrt();
                } else {
                    status = 3;
                }
            }
        }
        match kind {
            1 => {
                self.push_laser(2, json!(t.item.code));
                self.push_laser(3, json!(t.item.code));
            }
            4 => self.push_laser(1, json!(t.item.code)),
            _ => {}
        }
        self.meas_total += 1;
        let seq = self.meas_total;
        let entry = MeasureLogEntry { time_stamp: now_str().replace('T', " ").chars().take(23).collect(), seq, kind, status, cmd: t.clone(), data: data.to_vec(), delta };
        let head = ((seq - 1) % 200) as usize;
        let ej = serde_json::to_value(&entry).unwrap_or(Json::Null);
        set_db(&mut self.models, "MEASLOG_HIST", &format!("/Entry/{head}"), ej.clone());
        let Some(m) = self.models.get_mut("MEASLOG") else { return };
        set(m, "/Head", json!((seq % 200) as i64));
        set(m, "/Count", json!(seq.min(200)));
        set(m, "/Total", json!(seq));
        set(m, &format!("/Last/{}", kind as usize - 1), ej);
        let stat_ptr = format!("/Stat/{}", kind as usize - 1);
        let count = get_f(m, &format!("{stat_ptr}/Count")) as u32 + 1;
        set(m, &format!("{stat_ptr}/Count"), json!(count));
        if status >= 3 {
            let ec = get_f(m, &format!("{stat_ptr}/ErrorCount")) as u32 + 1;
            set(m, &format!("{stat_ptr}/ErrorCount"), json!(ec));
        }
        for (name, v) in [("InnerDia", entry.delta.inner_dia), ("Height", entry.delta.height), ("Z", entry.delta.z), ("Offset", entry.delta.offset)] {
            if v != 0.0 {
                trend(m, &format!("{stat_ptr}/{name}"), v);
            }
        }
        // by code
        if t.item.code != 0 && (kind == 1 || kind == 2 || kind == 4) {
            let mut slot = None;
            for i in 0..32 {
                let c = get_f(m, &format!("/ByCode/{i}/Code")) as u32;
                if c == t.item.code || (slot.is_none() && c == 0) {
                    slot = Some(i);
                    if c == t.item.code {
                        break;
                    }
                }
            }
            if let Some(i) = slot {
                let p = format!("/ByCode/{i}");
                set(m, &format!("{p}/Code"), json!(t.item.code));
                set(m, &format!("{p}/Item"), serde_json::to_value(&t.item).unwrap_or(Json::Null));
                set(m, &format!("{p}/LastTime"), json!(entry.time_stamp));
                let c = get_f(m, &format!("{p}/Count")) as u32 + 1;
                set(m, &format!("{p}/Count"), json!(c));
                let sp = format!("{p}/Stat/{}", kind as usize - 1);
                let sc = get_f(m, &format!("{sp}/Count")) as u32 + 1;
                set(m, &format!("{sp}/Count"), json!(sc));
                if entry.delta.inner_dia != 0.0 {
                    trend(m, &format!("{sp}/InnerDia"), entry.delta.inner_dia);
                }
                if entry.delta.z != 0.0 {
                    trend(m, &format!("{sp}/Z"), entry.delta.z);
                }
                match kind {
                    1 => {
                        trend(m, &format!("{p}/Meas/InnerDia"), data[1]);
                        trend(m, &format!("{p}/Meas/LaserInnerDia"), (data[10] + data[15]) / 2.0);
                        trend(m, &format!("{p}/Meas/TorqInnerDia"), data[7]);
                        trend(m, &format!("{p}/Meas/UpperBeadHeight"), data[2]);
                        trend(m, &format!("{p}/Meas/TireHeight"), data[3]);
                        trend(m, &format!("{p}/Meas/Offset"), entry.delta.offset);
                    }
                    4 => {
                        trend(m, &format!("{p}/Meas/PickBeadPos"), data[1]);
                        if data[3] > 0.0 {
                            trend(m, &format!("{p}/Meas/PickInnerDia"), data[5]);
                        }
                    }
                    _ => {
                        trend(m, &format!("{p}/Meas/SkuEachHeight"), data[18]);
                        trend(m, &format!("{p}/Meas/SkuStackHeight"), data[19]);
                    }
                }
            }
        }
    }

    fn laser_offsets(&self) -> Vec<f64> {
        LASER_OFFSETS.iter().map(|n| self.models.get("PARA").map(|p| get_f(p, &format!("/Sensor/{n}"))).unwrap_or(0.0)).collect()
    }

    /// LASERDIAG : `Reset` and Z-offset calibration start / cancel. The console writes `ZCal.Enable` / `Reset` over S7
    /// (absorbed in `encode_models`); this mirrors `UL_LaserDiag` on the PLC.
    fn laser_tick(&mut self) {
        if !self.has_laser {
            return;
        }
        let offsets = self.laser_offsets();
        let Some(d) = self.models.get_mut("LASERDIAG") else { return };
        let flag = |d: &Json, p: &str| d.pointer(p).and_then(Json::as_bool).unwrap_or(false);
        if flag(d, "/Reset") {
            for k in 0..4 {
                for (m, v) in [
                    ("BiasEma", json!(0.0)),
                    ("BiasSamples", json!(0)),
                    ("LastDev", json!(0.0)),
                    ("NoRespConsec", json!(0)),
                    ("UnstableConsec", json!(0)),
                    ("BiasAlarm", json!(false)),
                    ("NoRespAlarm", json!(false)),
                    ("UnstableAlarm", json!(false)),
                ] {
                    set(d, &format!("/Sensor/{k}/{m}"), v);
                }
            }
            set(d, "/Head", json!(0));
            set(d, "/Count", json!(0));
            set(d, "/Reset", json!(false));
        }
        let (enable, mm, busy) = (flag(d, "/ZCal/Enable"), flag(d, "/ZCal/EnableMm"), flag(d, "/ZCal/Busy"));
        if enable && !mm {
            for (m, v) in
                [("Busy", json!(true)), ("Done", json!(false)), ("Error", json!(false)), ("ErrorCode", json!(0)), ("Target", json!(LASER_CAL_COUNT)), ("Count", json!(0)), ("Skipped", json!(0))]
            {
                set(d, &format!("/ZCal/{m}"), v);
            }
            for m in ["Sum", "SumSq", "Mean", "StdDev", "NewOffset"] {
                set(d, &format!("/ZCal/{m}"), json!([0.0, 0.0, 0.0, 0.0]));
            }
            set(d, "/ZCal/OldOffset", json!(offsets));
        } else if !enable && mm && busy {
            set(d, "/ZCal/Busy", json!(false));
            set(d, "/ZCal/Error", json!(true));
            set(d, "/ZCal/ErrorCode", json!(1));
        }
        set(d, "/ZCal/EnableMm", json!(enable));
    }

    /// One LASERDIAG entry per laser measurement (1 PICK 하강, 2 들어갈 때, 3 나갈 때): sensor bias EMA and, while
    /// `ZCal.Busy`, the calibration sums (sources 2 / 3). A finished calibration writes the new offsets to PARA.
    fn push_laser(&mut self, source: u8, code: Json) {
        if !self.has_laser {
            return;
        }
        let offsets = self.laser_offsets();
        // bead height seen by k = true bead - mount error + offset -> deviation from the mean
        let mut dev = [0f64; 4];
        for ((slot, mount), off) in dev.iter_mut().zip(LASER_MOUNT).zip(&offsets) {
            *slot = off - mount + (self.rng.random::<f64>() - 0.5) * 1.2;
        }
        let mean = dev[..3].iter().sum::<f64>() / 3.0;
        for slot in dev.iter_mut().take(3) {
            *slot -= mean;
        }
        let spread = dev[..3].iter().copied().fold(f64::MIN, f64::max) - dev[..3].iter().copied().fold(f64::MAX, f64::min);
        let stamp: String = now_str().replace('T', " ").chars().take(23).collect();
        let Some(d) = self.models.get_mut("LASERDIAG") else { return };
        let total = get_f(d, "/Total") as u64 + 1;
        let head = get_f(d, "/Head") as usize % 50;
        let count = (get_f(d, "/Count") as usize + 1).min(50);
        let entry = json!({
            "TimeStamp": stamp, "Seq": total, "Source": source, "Code": code, "Valid": true, "FitError": 0, "DiagFlags": 0,
            "BeadDev": dev, "EdgeHeight": [240.2, 240.6, 239.9, 0.0], "Samples": [38, 41, 40, 0], "Jumps": [0, 0, 0, 0],
            "Spread": spread, "PlanarResidual": 0.0, "ZOffset": offsets,
        });
        set(d, &format!("/Entry/{head}"), entry);
        set(d, "/Head", json!((head + 1) % 50));
        set(d, "/Count", json!(count));
        set(d, "/Total", json!(total));
        for (k, dv) in dev.iter().copied().enumerate().take(3) {
            let ema = get_f(d, &format!("/Sensor/{k}/BiasEma"));
            let n = get_f(d, &format!("/Sensor/{k}/BiasSamples")) as i64;
            let next = if n <= 0 { dv } else { ema + 0.1 * (dv - ema) };
            set(d, &format!("/Sensor/{k}/BiasEma"), json!(next));
            set(d, &format!("/Sensor/{k}/BiasSamples"), json!((n + 1).min(30000)));
            set(d, &format!("/Sensor/{k}/LastDev"), json!(dv));
            set(d, &format!("/Sensor/{k}/BiasAlarm"), json!(n + 1 >= 20 && next.abs() > 5.0));
        }
        let busy = d.pointer("/ZCal/Busy").and_then(Json::as_bool).unwrap_or(false);
        let mut apply: Option<[f64; 4]> = None;
        if source >= 2 && busy {
            for (k, dv) in dev.iter().copied().enumerate().take(3) {
                let s = get_f(d, &format!("/ZCal/Sum/{k}")) + dv;
                let sq = get_f(d, &format!("/ZCal/SumSq/{k}")) + dv * dv;
                set(d, &format!("/ZCal/Sum/{k}"), json!(s));
                set(d, &format!("/ZCal/SumSq/{k}"), json!(sq));
            }
            let cnt = get_f(d, "/ZCal/Count") as i64 + 1;
            set(d, "/ZCal/Count", json!(cnt));
            if cnt >= (get_f(d, "/ZCal/Target") as i64).max(2) {
                let mut new = [0f64; 4];
                for (k, slot) in new.iter_mut().enumerate() {
                    let old = get_f(d, &format!("/ZCal/OldOffset/{k}"));
                    *slot = old;
                    if k < 3 {
                        let mean = get_f(d, &format!("/ZCal/Sum/{k}")) / cnt as f64;
                        let std = (get_f(d, &format!("/ZCal/SumSq/{k}")) / cnt as f64 - mean * mean).max(0.0).sqrt();
                        set(d, &format!("/ZCal/Mean/{k}"), json!(mean));
                        set(d, &format!("/ZCal/StdDev/{k}"), json!(std));
                        *slot = old - mean;
                    }
                    set(d, &format!("/ZCal/NewOffset/{k}"), json!(*slot));
                }
                for (m, v) in [("Busy", false), ("Done", true), ("Enable", false), ("EnableMm", false)] {
                    set(d, &format!("/ZCal/{m}"), json!(v));
                }
                apply = Some(new);
            }
        }
        if let Some(new) = apply {
            for (name, v) in LASER_OFFSETS.iter().zip(new) {
                set_db(&mut self.models, "PARA", &format!("/Sensor/{name}"), json!(v));
            }
        }
    }
}

fn trend(m: &mut Json, ptr: &str, v: f32) {
    let count = get_f(m, &format!("{ptr}/Count")) as u32 + 1;
    let (avg, ema, min, max) = if count == 1 {
        (v, v, v, v)
    } else {
        let avg = get_f(m, &format!("{ptr}/Avg")) as f32;
        let ema = get_f(m, &format!("{ptr}/Ema")) as f32;
        (avg + (v - avg) / count as f32, ema + 0.2 * (v - ema), (get_f(m, &format!("{ptr}/Min")) as f32).min(v), (get_f(m, &format!("{ptr}/Max")) as f32).max(v))
    };
    set(m, &format!("{ptr}/Count"), json!(count));
    set(m, &format!("{ptr}/Last"), json!(v));
    set(m, &format!("{ptr}/Avg"), json!(avg));
    set(m, &format!("{ptr}/Ema"), json!(ema));
    set(m, &format!("{ptr}/Min"), json!(min));
    set(m, &format!("{ptr}/Max"), json!(max));
}

#[cfg(test)]
mod laser_profile_tests {
    use super::*;

    #[test]
    fn gid_sees_bead_bands_and_cavity_only_inside_the_tire() {
        let t = TaskData {
            item: gr_proto::StockItem { inner_diameter: 400.0, outer_diameter: 700.0, height: 200.0, ..Default::default() },
            cell: gr_proto::CellInfo { position: [0.0, 0.0, 1500.0], ..Default::default() },
            ..Default::default()
        };
        let at = |z: f64| laser_gid(&t, z - LASER_MOUNT[0], 300.0, 1)[0];
        assert_eq!(at(1750.0), LASER_FAR);
        assert!((at(1688.0) - 50.0).abs() < 1.0, "top bead {}", at(1688.0));
        assert!((at(1600.0) - 177.5).abs() < 1.0, "cavity {}", at(1600.0));
        assert_eq!(laser_gid(&t, 1600.0, 300.0, 1)[3], LASER_FAR);
    }
}

#[cfg(test)]
mod two_robot_tests {
    use std::path::PathBuf;

    use super::*;

    fn contract(name: &str) -> Arc<Contract> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plc/contract");
        Arc::new(Contract::load_dir(&root.join(name)).expect(name))
    }

    #[test]
    fn gr_index_from_root() {
        assert_eq!(DemoRobot::gr_index_of("GR[1].CMD"), Some(0));
        assert_eq!(DemoRobot::gr_index_of("GR[2].CMD"), Some(1));
        assert_eq!(DemoRobot::gr_index_of("GR[0].CMD"), None);
        assert_eq!(DemoRobot::gr_index_of("CMD"), None);
    }

    /// Each robot gets its own PLC (GR1 without a LASERDIAG number still starts), commands route by DST and land in
    /// their own GRM `GR[n]` mirror, and an unknown DST is rejected as the wrong robot.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn robots_have_distinct_plcs_and_route_by_dst() {
        let robots = vec![
            DemoRobot { plc_name: "GR1".into(), contract: contract("GR1_PLC"), dst: 4001, gr_index: 0, machine_id: 1 },
            DemoRobot { plc_name: "GR2".into(), contract: contract("GR2_PLC"), dst: 4002, gr_index: 1, machine_id: 2 },
        ];
        let world = DemoWorld::start(robots, contract("GRM_PLC"), 50).await.expect("world");
        assert_ne!(world.addr_of("GR1"), world.addr_of("GR2"));
        assert!(world.addr_of("GR1").is_some() && world.addr_of("GRM").is_none());
        assert_eq!(world.robot_get("GR1", "PARA", "/Machine/ID"), json!(1));
        assert_eq!(world.robot_get("GR2", "PARA", "/Machine/ID"), json!(2));
        assert_eq!(world.robot_get("GR1", "MEASLOG", "/Total"), json!(25));
        assert_eq!(world.robot_get("GR2", "MEASLOG", "/Total"), json!(40));

        let mut t = TaskData { work_id: 91, task_id: 1, task_type: TaskType::Pick.code(), position: [3000.0, 3000.0, 1980.0, 300.0], ..Default::default() };
        t.cell = sample_cell(101, 0);
        t.item = StockItem { code: 1001, count: 1, inner_diameter: 381.0, outer_diameter: 780.0, height: 240.0, ..Default::default() };
        let accepted = world.submit(t.clone(), t.task_type, 5000, 4001, 1);
        let mut ok = false;
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            ok = world.robot_get("GR1", "OPCUA", "/STAT/RES/Header/SEQ") == json!(accepted.seq) && world.grm_opcua_get("/GR/0/STAT/RES/Header/SEQ") == json!(accepted.seq);
            if ok {
                break;
            }
        }
        assert!(ok, "GR1 echoes and its GRM mirror is GR[1]");
        assert_eq!(world.robot_get("GR1", "OPCUA", "/STAT/RES/Data/2"), json!(1), "valid on GR1");
        assert_eq!(world.robot_get("GR2", "OPCUA", "/STAT/RES/Header/SEQ"), json!(0), "GR2 saw nothing");

        let wrong = world.submit(TaskData { work_id: 92, ..t }, 1, 5000, 4009, 1);
        let mut ok = false;
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            ok = world.robot_get("GR1", "OPCUA", "/STAT/RES/Header/SEQ") == json!(wrong.seq);
            if ok {
                break;
            }
        }
        assert!(ok, "unknown DST lands on the first robot");
        assert_eq!(world.robot_get("GR1", "OPCUA", "/STAT/RES/Data/0").as_u64().unwrap_or(0) & 0x01, 1, "rejected as wrong robot");
        assert_eq!(world.grm_opcua_get("/GR/1/STAT/ComponentID"), json!(4002));
    }
}
