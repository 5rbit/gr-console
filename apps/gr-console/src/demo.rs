//! In-process demo world: two fake S7 servers (GR2 / GRM) whose DB bytes are regenerated from JSON models
//! every tick, plus a fake command path (GRM CMD → echo → queue → running → completed) so the whole
//! console can be exercised without a PLC.

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

pub struct DemoWorld {
    inner: Mutex<Inner>,
    gr2_store: DbStore,
    grm_store: DbStore,
    gr2: Arc<Contract>,
    grm: Arc<Contract>,
    pub gr2_addr: SocketAddr,
    pub grm_addr: SocketAddr,
    _servers: Vec<FakeS7Server>,
}

struct Running {
    task: TaskData,
    step_idx: usize,
    step_at: Instant,
}

struct Inner {
    gr2: HashMap<String, Json>,
    grm: HashMap<String, Json>,
    pending: Option<(Header, TaskData)>,
    last_header: Header,
    queue: Vec<TaskData>,
    now: Option<Running>,
    completed: VecDeque<TaskData>,
    canceled: VecDeque<TaskData>,
    rejected: VecDeque<TaskData>,
    cmd_id: u8,
    seq: u16,
    tick: u64,
    meas_total: u32,
    axis: [f32; 4],
    target: [f32; 4],
    rng: StdRng,
    /// JSON pointer of the GRM `GR[n].CMD` a relayed OPC UA command came from (cleared after the echo).
    relay_root: Option<String>,
    /// Zeroed `GR[n].CMD` subtree used to clear Header/TaskData/Data like GRM does.
    cmd_zero: Json,
    /// Bytes last encoded per (store, DB number) for S7-writable tables; a difference means a client wrote them.
    encoded: HashMap<(u8, u16), Vec<u8>>,
    op_echo: Option<(TaskOp, u32, u32)>,
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

impl DemoWorld {
    pub async fn start(gr2: Arc<Contract>, grm: Arc<Contract>, tick_ms: u64) -> anyhow::Result<Arc<DemoWorld>> {
        let gr2_store = FakeS7Server::store();
        let grm_store = FakeS7Server::store();
        let s1 = FakeS7Server::spawn(gr2_store.clone(), 960).await?;
        let s2 = FakeS7Server::spawn(grm_store.clone(), 960).await?;
        let mut inner = Inner {
            gr2: HashMap::new(),
            grm: HashMap::new(),
            pending: None,
            last_header: Header::default(),
            queue: Vec::new(),
            now: None,
            completed: VecDeque::new(),
            canceled: VecDeque::new(),
            rejected: VecDeque::new(),
            cmd_id: 1,
            seq: 1,
            tick: 0,
            meas_total: 0,
            axis: [12000.0, 3000.0, 2500.0, 300.0],
            target: [12000.0, 3000.0, 2500.0, 300.0],
            rng: StdRng::seed_from_u64(7),
            op_echo: None,
            relay_root: None,
            cmd_zero: Json::Null,
            encoded: HashMap::new(),
        };
        // zero models from the contract, then seed constants / registries
        for db in ["OPCUA", "TASK", "CELL", "STATION", "PARA", "ALARM", "Interface_GRM", "WEBMON", "MEASLOG", "MEASLOG_HIST"] {
            let size = gr2.size_of_db(db)?;
            inner.gr2.insert(db.into(), gr2.decode_db(db, &vec![0u8; size as usize])?);
        }
        for db in ["OPCUA", "STATION", "CELL", "MACHINE"] {
            let size = grm.size_of_db(db)?;
            inner.grm.insert(db.into(), grm.decode_db(db, &vec![0u8; size as usize])?);
        }
        for db in ["WEBMON", "MEASLOG", "MEASLOG_HIST"] {
            let sig = gr2.layout_sig(db)?;
            set(inner.gr2.get_mut(db).unwrap(), "/LayoutSig", json!(sig));
        }
        set(inner.gr2.get_mut("OPCUA").unwrap(), "/STAT/ComponentID", json!(4002));
        set(inner.gr2.get_mut("PARA").unwrap(), "/Machine/ID", json!(2));
        set(inner.grm.get_mut("OPCUA").unwrap(), "/GR/1/STAT/ComponentID", json!(4002));
        set(inner.grm.get_mut("MACHINE").unwrap(), "/Communication/NextCmdID", json!(1));
        inner.cmd_zero = inner.grm["OPCUA"].pointer("/GR/1/CMD").cloned().unwrap_or(Json::Null);
        // registries
        let cells: Vec<CellInfo> = (0..12).map(|i| sample_cell(101 + i as u16, i)).collect();
        for (store, contract_name) in [(&mut inner.gr2, "GR2"), (&mut inner.grm, "GRM")] {
            let db = store.get_mut("CELL").unwrap();
            set(db, "/Count", json!(cells.len()));
            for (i, c) in cells.iter().enumerate() {
                set(db, &format!("/Cell/{i}"), serde_json::to_value(c)?);
            }
            let _ = contract_name;
        }
        let stations: Vec<StationPara> = (0..2).map(|i| sample_station(2101 + i as u16, i)).collect();
        {
            let db = inner.gr2.get_mut("STATION").unwrap();
            set(db, "/Count", json!(stations.len()));
            for (i, s) in stations.iter().enumerate() {
                set(db, &format!("/Station/{i}"), serde_json::to_value(s)?);
            }
            let db = inner.grm.get_mut("STATION").unwrap();
            set(db, "/Count", json!(stations.len()));
            for (i, s) in stations.iter().enumerate() {
                set(db, &format!("/Station/{i}/Para"), serde_json::to_value(s)?);
            }
        }
        set(inner.gr2.get_mut("WEBMON").unwrap(), "/Mode", json!(32));
        set(inner.gr2.get_mut("OPCUA").unwrap(), "/STAT/Mode/Auto", json!(true));
        set(inner.gr2.get_mut("OPCUA").unwrap(), "/STAT/Status/Online", json!(true));
        set(inner.gr2.get_mut("OPCUA").unwrap(), "/STAT/Status/Normal", json!(true));
        set(inner.gr2.get_mut("OPCUA").unwrap(), "/STAT/Status/PowerOn", json!(true));
        let world = Arc::new(DemoWorld { inner: Mutex::new(inner), gr2_store, grm_store, gr2, grm, gr2_addr: s1.addr, grm_addr: s2.addr, _servers: vec![s1, s2] });
        // seed some history so the measurement screens have content
        {
            let mut g = world.inner.lock().unwrap_or_else(PoisonError::into_inner);
            for i in 0..40 {
                let mut t = TaskData::default();
                t.work_id = 4000 + i;
                t.task_id = 1;
                t.task_type = if i % 3 == 0 { TaskType::Measure.code() } else { TaskType::Pick.code() };
                t.measure_item = i % 3 == 0;
                t.cell = cells[(i as usize) % cells.len()].clone();
                t.item = StockItem { code: 1001 + (i % 3), count: 3, inner_diameter: 381.0 + (i % 3) as f32 * 50.0, outer_diameter: 780.0, height: 240.0, ..Default::default() };
                t.position = [t.cell.position[0], t.cell.position[1], 1500.0 + 480.0, 300.0];
                world.push_measure(&mut g, &t);
            }
            world.encode_all(&mut g);
        }
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

    fn next_header(&self, g: &mut Inner, cmd: u8, src: u16, dst: u16, protocol: u8) -> Header {
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

    /// Like the GCS writing GRM `GR[2].CMD`: returns the header used; the tick processes it.
    pub fn submit(&self, task: TaskData, cmd: u8, src: u16, dst: u16, protocol: u8) -> Header {
        let mut g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let h = self.next_header(&mut g, cmd, src, dst, protocol);
        g.pending = Some((h.clone(), task));
        h
    }

    pub fn task_op(&self, op: TaskOp, work_id: u32, task_id: u32) {
        let mut g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if work_id == 0 {
            g.op_echo = None;
            return;
        }
        g.op_echo = Some((op, work_id, task_id));
    }

    #[allow(dead_code)]
    pub fn clear_header(&self) {
        let mut g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        g.pending = None;
    }

    /// Value at a JSON pointer of the GR2 `OPCUA` model (bench / tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn gr2_get(&self, ptr: &str) -> Json {
        let g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        g.gr2.get("OPCUA").and_then(|d| d.pointer(ptr)).cloned().unwrap_or(Json::Null)
    }

    /// OPC UA read of the GRM `OPCUA` model (sim server).
    pub fn grm_opcua_get(&self, ptr: &str) -> Json {
        let g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        g.grm.get("OPCUA").and_then(|d| d.pointer(ptr)).cloned().unwrap_or(Json::Null)
    }

    /// OPC UA write into GRM `"OPCUA".GR[n].CMD` (sim server), emulating GRM: the value lands in the DB the
    /// console reads over S7; a Header whose six fields are all non-zero (completed by the SEQ write) is
    /// relayed to GR2 with the TaskData currently in CMD; a Complete/Delete TaskId write with a non-zero
    /// WorkId becomes a task op, `(0,0)` re-arms.
    pub fn grm_opcua_write(&self, path: &str, ptr: &str, value: Json) {
        let mut g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let written = value.as_u64().unwrap_or(0) as u32;
        if let Some(d) = g.grm.get_mut("OPCUA") {
            set(d, ptr, value);
        }
        let Some(i) = ptr.find("/CMD/") else { return };
        let root = ptr[..i + 4].to_string();
        let opc = &g.grm["OPCUA"];
        if path.ends_with(".Header.SEQ") {
            let header: Header = opc.pointer(&format!("{root}/Header")).and_then(|h| serde_json::from_value(h.clone()).ok()).unwrap_or_default();
            let fields = [u32::from(header.protocol), u32::from(header.cmd_id), u32::from(header.cmd), u32::from(header.src), u32::from(header.dst), u32::from(header.seq)];
            if fields.iter().all(|v| *v != 0) {
                let task = opc.pointer(&format!("{root}/TaskData")).and_then(|t| TaskData::from_json(t).ok()).unwrap_or_default();
                g.pending = Some((header, task));
                g.relay_root = Some(root);
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
            g.op_echo = if work_id == 0 { None } else { Some((op, work_id, written)) };
        }
    }

    fn tick(&self) {
        let mut g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        g.tick += 1;
        // ---- command intake (GR2 UL_ParseOpcUaCommand semantics, simplified)
        if let Some((h, task)) = g.pending.take() {
            let mut data = [0u8; 16];
            let mut validation: u16 = 1;
            if h.dst != 4002 {
                data[0] |= 0x01;
            }
            if h == g.last_header {
                data[0] |= 0x02;
            }
            let cells = g.gr2["CELL"].clone();
            let stations = g.gr2["STATION"].clone();
            let known = (0..cells["Count"].as_u64().unwrap_or(0) as usize).any(|i| cells["Cell"][i]["Id"].as_u64() == Some(task.cell.id as u64))
                || (0..stations["Count"].as_u64().unwrap_or(0) as usize).any(|i| stations["Station"][i]["Info"]["Id"].as_u64() == Some(task.cell.id as u64));
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
            if g.queue.len() >= QUEUE {
                data[0] |= 0x08;
                validation = 110;
            }
            data[1] = (validation >> 8) as u8;
            data[2] = validation as u8;
            data[5] = 1;
            data[6] = 1;
            let accepted = RejectInfo::from_data(&data).accepted();
            g.last_header = h.clone();
            let res_h = serde_json::to_value(&h).unwrap_or(Json::Null);
            let res_t = serde_json::to_value(&task).unwrap_or(Json::Null);
            let res_d = Json::Array(data.iter().map(|b| json!(b)).collect());
            let opc = g.gr2.get_mut("OPCUA").unwrap();
            set(opc, "/STAT/RES/Header", res_h.clone());
            set(opc, "/STAT/RES/Task", res_t.clone());
            set(opc, "/STAT/RES/Data", res_d.clone());
            let grm = g.grm.get_mut("OPCUA").unwrap();
            set(grm, "/GR/1/STAT/RES/Header", res_h);
            set(grm, "/GR/1/STAT/RES/Task", res_t);
            set(grm, "/GR/1/STAT/RES/Data", res_d);
            if accepted {
                g.queue.push(task);
            } else {
                ring_push(&mut g.rejected, task);
            }
        }
        // ---- GRM clears CMD Header/TaskData/Data once RES.Header == CMD.Header (OPC UA relay only)
        if let Some(root) = g.relay_root.take() {
            let zero = g.cmd_zero.clone();
            let opc = g.grm.get_mut("OPCUA").unwrap();
            for part in ["Header", "TaskData", "Data"] {
                if let Some(z) = zero.get(part) {
                    set(opc, &format!("{root}/{part}"), z.clone());
                }
            }
        }
        // ---- task ops
        if let Some((op, work_id, task_id)) = g.op_echo.take() {
            match op {
                TaskOp::Delete => {
                    let mut removed = Vec::new();
                    g.queue.retain(|t| {
                        let hit = t.work_id == work_id && (task_id == 0 || t.task_id == task_id);
                        if hit {
                            removed.push(t.clone());
                        }
                        !hit
                    });
                    if let Some(r) = g.now.as_ref()
                        && r.task.work_id == work_id
                        && (task_id == 0 || r.task.task_id == task_id)
                    {
                        let r = g.now.take().unwrap();
                        removed.push(r.task);
                    }
                    for t in removed {
                        ring_push(&mut g.canceled, t);
                    }
                }
                TaskOp::Complete => {
                    if let Some(r) = g.now.as_ref()
                        && r.task.work_id == work_id
                        && r.task.task_id == task_id
                    {
                        let r = g.now.take().unwrap();
                        ring_push(&mut g.completed, r.task);
                    } else if let Some(i) = g.queue.iter().position(|t| t.work_id == work_id && t.task_id == task_id) {
                        let t = g.queue.remove(i);
                        ring_push(&mut g.completed, t);
                    }
                }
            }
        }
        // ---- execution
        if g.now.is_none() && !g.queue.is_empty() {
            let t = g.queue[0].clone();
            g.target = [t.position[0], t.position[1], t.position[2], t.position[3]];
            g.now = Some(Running { task: t, step_idx: 0, step_at: Instant::now() });
        }
        let mut finished: Option<TaskData> = None;
        if let Some(r) = g.now.as_mut()
            && r.step_at.elapsed() > Duration::from_millis(700)
        {
            r.step_idx += 1;
            r.step_at = Instant::now();
            if r.step_idx >= STEPS.len() {
                finished = Some(r.task.clone());
            }
        }
        if let Some(t) = finished {
            g.now = None;
            g.queue.retain(|q| q.key() != t.key());
            self.push_measure(&mut g, &t);
            ring_push(&mut g.completed, t);
            g.target[2] = 2500.0;
            g.target[3] = 300.0;
        }
        // ---- axes
        for i in 0..4 {
            let d = g.target[i] - g.axis[i];
            g.axis[i] += d * 0.25;
        }
        // ---- publish status
        let step = g.now.as_ref().map(|r| STEPS[r.step_idx.min(STEPS.len() - 1)]).unwrap_or(0);
        let now_json = serde_json::to_value(g.now.as_ref().map(|r| r.task.clone()).unwrap_or_default()).unwrap_or(Json::Null);
        let mut queue: Vec<Json> = g.queue.iter().map(|t| serde_json::to_value(t).unwrap_or(Json::Null)).collect();
        while queue.len() < QUEUE {
            queue.push(serde_json::to_value(TaskData::default()).unwrap_or(Json::Null));
        }
        let queue = Json::Array(queue);
        let (completed, canceled, rejected) = (ring_json(&g.completed), ring_json(&g.canceled), ring_json(&g.rejected));
        let accept = g.queue.len() < QUEUE && g.pending.is_none();
        let status = json!({ "Accept": accept, "Idle": g.now.is_none() && g.queue.is_empty(), "Assigned": !g.queue.is_empty(), "Inprogress": g.now.is_some(),
            "AvoidReq": false, "HoldItem": (500..999).contains(&step), "Complete": false, "Canceled": false, "Reserved": 0, "Step": step });
        let axis = g.axis;
        let (now_none, now_some, tick, target) = (g.now.is_none(), g.now.is_some(), g.tick, g.target);
        let ntp = now_str().replace('T', " ").chars().take(23).collect::<String>();
        {
            let opc = g.gr2.get_mut("OPCUA").unwrap();
            set(opc, "/STAT/NTP", json!(ntp));
            set(opc, "/STAT/Task/Status", status.clone());
            set(opc, "/STAT/Task/Now", now_json.clone());
            set(opc, "/STAT/Task/Queue", queue.clone());
            set(opc, "/STAT/Task/Completed", completed.clone());
            set(opc, "/STAT/Task/Canceled", canceled.clone());
            set(opc, "/STAT/Task/Rejected", rejected.clone());
            set(opc, "/STAT/Status/Idle", json!(now_none));
            set(opc, "/STAT/Status/Busy", json!(now_some));
            set(opc, "/STAT/Status/HeartBeat", json!(tick % 2 == 0));
            set(opc, "/STAT/Status/ItemDectect", json!((500..999).contains(&step)));
            for (i, a) in axis.iter().enumerate() {
                set(opc, &format!("/STAT/Drive/{i}/Position"), json!(a));
            }
        }
        let stat = g.gr2["OPCUA"]["STAT"].clone();
        {
            let task = g.gr2.get_mut("TASK").unwrap();
            set(task, "/Status", status.clone());
            set(task, "/Now", now_json.clone());
            set(task, "/Buff", queue.clone());
            set(task, "/Last", completed.clone());
            set(task, "/Canceled", canceled.clone());
            set(task, "/Rejected", rejected.clone());
            set(task, "/Proc/Step/Now", json!(step));
        }
        {
            let grm = g.grm.get_mut("OPCUA").unwrap();
            set(grm, "/GR/1/STAT", stat.clone());
            // GRM overrides Accept while a command sits in CMD
            set(grm, "/GR/1/STAT/Task/Status/Accept", json!(accept));
        }
        let meas_total = g.meas_total;
        let meas_count = meas_total.min(200);
        let measure_json = {
            let t = g.now.as_ref().map(|r| r.task.clone());
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
        {
            let w = g.gr2.get_mut("WEBMON").unwrap();
            set(w, "/UpdateTime", json!(ntp));
            set(w, "/Stat", stat);
            set(w, "/Proc/Step/Now", json!(step));
            set(w, "/Proc/Msg", json!(format!("Step {step}")));
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
            set(w, "/Gripper/GID", json!([120.1, 121.7, 119.4, 120.9]));
            set(w, "/Gripper/FLD", json!(800.0 - (axis[2] - 1500.0)));
            set(w, "/Measure", measure_json);
            set(w, "/MeasLog/Total", json!(meas_total));
            set(w, "/MeasLog/Count", json!(meas_count));
        }
        self.encode_all(&mut g);
    }

    /// Appends a MEASLOG_HIST entry for a finished task and updates MEASLOG counters / stats.
    fn push_measure(&self, g: &mut Inner, t: &TaskData) {
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
        let n = |g: &mut Inner, sd: f32| -> f32 { (g.rng.random::<f32>() + g.rng.random::<f32>() + g.rng.random::<f32>() - 1.5) * sd };
        let mut data = [0f32; 20];
        let mut delta = gr_proto::measure::Delta::default();
        let mut status = 2u16;
        match kind {
            1 => {
                let in_id = id + n(g, 1.2);
                let out_id = id + n(g, 1.2);
                let tq = id + 1.5 + n(g, 0.8);
                let bead = zrel + h - 20.0 + n(g, 1.0);
                let th = zrel + h + n(g, 1.5);
                let (ox, oy) = (n(g, 3.0), n(g, 3.0));
                let rid = if ((in_id + out_id) / 2.0 - tq).abs() <= 10.0 { ((in_id + out_id) / 2.0 + tq) / 2.0 } else { tq };
                data[..20].copy_from_slice(&[2.0, rid, bead, th, 0.0, 0.0, 0.0, tq, 0.35, 0.0, in_id, bead, th, ox, oy, out_id, bead, th, ox, oy]);
                delta = gr_proto::measure::Delta { inner_dia: rid - id, height: th - zrel - h, z: bead - zrel, offset: (ox * ox + oy * oy).sqrt(), count: 0.0 };
                if g.rng.random::<f32>() < 0.1 {
                    status = 4;
                    data[0] = 4.0;
                }
            }
            2 => {
                let cnt = t.item.count.max(1) as usize;
                let each = h - 3.0 + n(g, 1.0);
                for k in 1..=cnt.min(7) {
                    data[2 * k - 1] = (k - 1) as f32 * each + 20.0;
                    data[2 * k] = (k - 1) as f32 * each + h - 20.0;
                }
                data[0] = 2.0;
                data[17] = cnt as f32;
                data[18] = each;
                data[19] = cnt as f32 * each;
                delta = gr_proto::measure::Delta { inner_dia: 0.0, height: each - h, z: data[19] - zrel, offset: 0.0, count: 0.0 };
            }
            3 => {
                data[0] = 2.0;
                data[1] = 1500.0 + n(g, 1.5);
                data[2] = 1700.0;
                data[3] = 200.0;
                delta.z = data[1] - 1500.0;
            }
            _ => {
                let last = 25.0 + n(g, 2.0);
                let valid = g.rng.random::<f32>() > 0.2;
                data[0] = if valid { 2.0 } else { 3.0 };
                data[1] = last;
                data[2] = if last - 20.0 > 0.0 && last - 20.0 < h * 0.3 { 1500.0 + zrel + last - 20.0 } else { 0.0 };
                data[3] = if valid { 1.0 } else { 0.0 };
                data[5] = if valid { id + n(g, 2.0) } else { 0.0 };
                data[6] = if valid { n(g, 4.0) } else { 0.0 };
                data[7] = if valid { n(g, 4.0) } else { 0.0 };
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
        g.meas_total += 1;
        let seq = g.meas_total;
        let entry = MeasureLogEntry { time_stamp: now_str().replace('T', " ").chars().take(23).collect(), seq, kind, status, cmd: t.clone(), data: data.to_vec(), delta };
        let head = ((seq - 1) % 200) as usize;
        let ej = serde_json::to_value(&entry).unwrap_or(Json::Null);
        set(g.gr2.get_mut("MEASLOG_HIST").unwrap(), &format!("/Entry/{head}"), ej.clone());
        let m = g.gr2.get_mut("MEASLOG").unwrap();
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

    /// Writes every model DB into the fake S7 stores. Tables a client may write over S7 (`CELL`, `STATION`) are
    /// absorbed first: if their bytes changed since the last encode (a push from the console), the model is
    /// re-decoded from them, like a real PLC keeping the written values. Check and encode run under the store
    /// lock, so a concurrent S7 write cannot slip between them.
    fn encode_all(&self, g: &mut Inner) {
        const ABSORB: [&str; 2] = ["CELL", "STATION"];
        for (side, store, contract) in [(0u8, &self.gr2_store, &self.gr2), (1u8, &self.grm_store, &self.grm)] {
            let mut bytes = store.lock().unwrap_or_else(PoisonError::into_inner);
            let models = if side == 0 { &mut g.gr2 } else { &mut g.grm };
            for (db, json) in models.iter_mut() {
                let (Some(n), Ok(size)) = (contract.db_number(db), contract.size_of_db(db)) else { continue };
                let buf = bytes.entry(n).or_insert_with(|| vec![0u8; size as usize]);
                let absorb = ABSORB.contains(&db.as_str());
                if absorb && g.encoded.get(&(side, n)).is_some_and(|last| last != buf) {
                    match contract.decode_db(db, buf) {
                        Ok(j) => *json = j,
                        Err(e) => tracing::error!(db, "demo absorb: {e}"),
                    }
                }
                if let Err(e) = contract.encode_db(db, json, buf) {
                    tracing::error!(db, "demo encode: {e}");
                }
                if absorb {
                    g.encoded.insert((side, n), buf.clone());
                }
            }
        }
    }

    /// Current queue keys (used by tests).
    #[allow(dead_code)]
    pub fn queue_keys(&self) -> Vec<TaskKey> {
        let g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        g.queue.iter().map(|t| t.key()).collect()
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
