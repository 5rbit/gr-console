//! Command port: how a task command reaches the robot. `Opc` writes GRM `"OPCUA".GR[2].CMD` through
//! OPC UA; `Demo` feeds the in-process demo world. Both expose the same async surface.

use std::sync::{Arc, Mutex, PoisonError};

use gr_proto::{Header, MemberValue, TaskData, WireValue};
use serde::Serialize;
use serde_json::{Value as Json, json};

use crate::config::CmdCfg;
use crate::demo::DemoWorld;
use crate::error::ApiError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskOp {
    Complete,
    Delete,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CmdStatus {
    pub kind: String,
    pub endpoint: String,
    pub ready: bool,
    pub state: String,
    pub error: Option<String>,
    pub detail: Option<String>,
    pub last_ok_at: Option<String>,
    pub last_header: Option<Header>,
    pub endpoints: Vec<String>,
}

pub enum CommandPort {
    Opc { writer: opcua_cmd::CmdWriter, cfg: CmdCfg, last: Mutex<Option<Header>>, endpoint: String },
    Demo { world: Arc<DemoWorld>, cfg: CmdCfg, last: Mutex<Option<Header>> },
}

/// Declared lower bound of every array under `root` (e.g. `GR[2].CMD`) in the GRM `db` (contract
/// layout), keyed relative to the root without indices: `TaskData.Position` → 1, `Data` → 0.
/// The S7-1500 OPC UA server exposes arrays of elementary types 0-based, the writer addresses
/// members by PLC index — `opcua_cmd` rebases browsed/cached keys with this table.
pub(crate) fn opcua_array_bases(grm: &plc_layout::Contract, db: &str, root: &str) -> std::collections::BTreeMap<String, i64> {
    let layout = match grm.layout_db(db) {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(db, error = %e, "OPC UA array bases: GRM layout unavailable — 0-based arrays will not resolve");
            return Default::default();
        }
    };
    let prefix = format!("{root}.");
    opcua_cmd::array_bases_from_paths(layout.members.iter().filter_map(|m| m.path.strip_prefix(&prefix)))
}

pub(crate) fn to_opc(m: &MemberValue) -> opcua_cmd::MemberValue {
    let value = match m.value {
        WireValue::Bool(b) => opcua_cmd::PlcValue::Bool(b),
        WireValue::U8(v) => opcua_cmd::PlcValue::U8(v),
        WireValue::I8(v) => opcua_cmd::PlcValue::I8(v),
        WireValue::U16(v) => opcua_cmd::PlcValue::U16(v),
        WireValue::I16(v) => opcua_cmd::PlcValue::I16(v),
        WireValue::U32(v) => opcua_cmd::PlcValue::U32(v),
        WireValue::I32(v) => opcua_cmd::PlcValue::I32(v),
        WireValue::F32(v) => opcua_cmd::PlcValue::F32(v),
    };
    opcua_cmd::MemberValue { path: m.path.clone(), value }
}

impl CommandPort {
    pub fn cfg(&self) -> &CmdCfg {
        match self {
            CommandPort::Opc { cfg, .. } | CommandPort::Demo { cfg, .. } => cfg,
        }
    }

    /// 종료 절차: OPC UA 세션을 정상적으로 닫는다(서버에 세션이 떠 있지 않게). 닫혔으면 true.
    pub async fn close(&self, timeout: std::time::Duration) -> bool {
        match self {
            CommandPort::Opc { writer, .. } => {
                writer.shutdown();
                let t0 = std::time::Instant::now();
                while t0.elapsed() < timeout {
                    if matches!(writer.state(), opcua_cmd::OpcState::Disconnected) {
                        return true;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                false
            }
            CommandPort::Demo { .. } => true,
        }
    }

    /// 제출 게이트용 명령 경로 사유 — 준비됐으면 None. 매번 writer 의 현재 상태에서 만든다(캐시 없음):
    /// 서버가 세션을 무효화하면 writer 가 곧바로 Ready 를 벗어나므로 게이트도 바로 막힌다.
    pub fn not_ready_reason(&self) -> Option<String> {
        match self {
            CommandPort::Opc { writer, .. } => match writer.state() {
                opcua_cmd::OpcState::Ready { .. } => None,
                opcua_cmd::OpcState::Failed { error, .. } => Some(format!("OPC UA 세션 재연결 중: {error}")),
                opcua_cmd::OpcState::Connecting | opcua_cmd::OpcState::Browsing => Some("OPC UA 세션 재연결 중".into()),
                opcua_cmd::OpcState::Disconnected => Some("OPC UA 명령 경로가 준비되지 않음".into()),
            },
            CommandPort::Demo { .. } => None,
        }
    }

    pub fn status(&self) -> CmdStatus {
        match self {
            CommandPort::Opc { writer, endpoint, last, .. } => {
                let st = writer.state();
                let (state, error, ready, detail) = match &st {
                    opcua_cmd::OpcState::Ready { node_count, ns } => ("ready".to_string(), None, true, Some(format!("{node_count} nodes, ns={ns}"))),
                    opcua_cmd::OpcState::Failed { error, retry_in_ms } => ("failed".to_string(), Some(error.clone()), false, Some(format!("retry in {retry_in_ms} ms"))),
                    opcua_cmd::OpcState::Connecting => ("connecting".into(), None, false, None),
                    opcua_cmd::OpcState::Browsing => ("browsing".into(), None, false, None),
                    opcua_cmd::OpcState::Disconnected => ("disconnected".into(), None, false, None),
                };
                CmdStatus {
                    kind: "opcua".into(),
                    endpoint: endpoint.clone(),
                    ready,
                    state,
                    error,
                    detail,
                    last_ok_at: None,
                    last_header: last.lock().unwrap_or_else(PoisonError::into_inner).clone(),
                    endpoints: writer.endpoints(),
                }
            }
            CommandPort::Demo { last, .. } => CmdStatus {
                kind: "demo".into(),
                endpoint: "demo://grm".into(),
                ready: true,
                state: "ready".into(),
                error: None,
                detail: Some("in-process demo PLC".into()),
                last_ok_at: Some(crate::util::now_str()),
                last_header: last.lock().unwrap_or_else(PoisonError::into_inner).clone(),
                endpoints: vec![],
            },
        }
    }

    pub fn nodes(&self) -> Json {
        match self {
            CommandPort::Opc { writer, .. } => writer.nodes().map(|n| json!({ "ns": n.ns, "count": n.count, "sample": n.sample })).unwrap_or(Json::Null),
            CommandPort::Demo { .. } => json!({ "ns": 0, "count": 0, "sample": [] }),
        }
    }

    pub async fn rebrowse(&self) {
        if let CommandPort::Opc { writer, .. } = self {
            let _ = writer.rebrowse().await;
        }
    }

    /// Writes TaskData (+ zeroed Command/Data) then the Header. Returns the header used.
    pub async fn write_task(&self, task: &TaskData) -> Result<Header, ApiError> {
        let cmd_code = task.task_type;
        let cfg = self.cfg();
        // `Command.*` and `Data[]` zeros come from the browsed node map inside `CmdWriter::write_task`: on the PLC the
        // Command bytes are Bool bit structs (`Command.Stop.Normal`, ...), so a fixed byte list names nodes that do not exist.
        let members = task.to_members();
        let h = match self {
            CommandPort::Opc { writer, .. } => {
                let m: Vec<opcua_cmd::MemberValue> = members.iter().map(to_opc).collect();
                let hw = writer.write_task(&m, cmd_code, cfg.src, cfg.dst, cfg.protocol).await.map_err(|e| ApiError::OpcNotReady(e.to_string()))?;
                Header { protocol: hw.protocol, cmd_id: hw.cmd_id, cmd: hw.cmd, src: hw.src, dst: hw.dst, seq: hw.seq }
            }
            CommandPort::Demo { world, .. } => world.submit(task.clone(), cmd_code, cfg.src, cfg.dst, cfg.protocol),
        };
        match self {
            CommandPort::Opc { last, .. } | CommandPort::Demo { last, .. } => *last.lock().unwrap_or_else(PoisonError::into_inner) = Some(h.clone()),
        }
        Ok(h)
    }

    pub async fn write_task_op(&self, op: TaskOp, work_id: u32, task_id: u32) -> Result<(), ApiError> {
        match self {
            CommandPort::Opc { writer, .. } => {
                let o = match op {
                    TaskOp::Complete => opcua_cmd::TaskOp::Complete,
                    TaskOp::Delete => opcua_cmd::TaskOp::Delete,
                };
                writer.write_task_op(o, work_id, task_id).await.map_err(|e| ApiError::OpcNotReady(e.to_string()))
            }
            CommandPort::Demo { world, cfg, .. } => {
                world.task_op(op, cfg.dst, work_id, task_id);
                Ok(())
            }
        }
    }

    #[allow(dead_code)] // 에코 후 재무장용 — 실기 M4 에서 사용 예정
    pub async fn clear_header(&self) -> Result<(), ApiError> {
        match self {
            CommandPort::Opc { writer, .. } => writer.clear_header().await.map_err(|e| ApiError::OpcNotReady(e.to_string())),
            CommandPort::Demo { world, cfg, .. } => {
                world.clear_header(cfg.dst);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod opc_path_tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use gr_proto::{StockItem, TaskData};

    /// Member keys and node ids exactly as the real GRM S7-1500 server was browsed (GR[2].CMD, node cache).
    const REAL_CACHE: &str = include_str!("testdata/opcua-nodes.gr2.real.json");

    fn real_members() -> BTreeMap<String, String> {
        let j: serde_json::Value = serde_json::from_str(REAL_CACHE).expect("fixture json");
        serde_json::from_value(j["members"].clone()).expect("members map")
    }

    fn grm_bases() -> BTreeMap<String, i64> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plc/contract/GRM_PLC");
        let grm = plc_layout::Contract::load_dir(&root).expect("GRM contract");
        super::opcua_array_bases(&grm, "OPCUA", "GR[2].CMD")
    }

    #[test]
    fn contract_bases_for_cmd_arrays() {
        let b = grm_bases();
        assert_eq!(b.get("TaskData.Position"), Some(&1), "Array[\"X\"..\"G\"] of Real");
        assert_eq!(b.get("TaskData.Cell.Position"), Some(&1), "Array[\"X\"..\"Z\"] of Real");
        assert_eq!(b.get("Data"), Some(&0));
    }

    #[test]
    fn real_server_keys_resolve_by_plc_index() {
        let raw = real_members();
        assert!(raw.contains_key("TaskData.Position[0]") && !raw.contains_key("TaskData.Position[4]"), "fixture is the 0-based real shape");
        let (m, n) = opcua_cmd::rebase_array_keys(&raw, &grm_bases());
        assert_eq!(n, 7, "Position[0..3] + Cell.Position[0..2]");
        let id = |p: &str| m.get(p).cloned().unwrap_or_else(|| panic!("missing {p}"));
        for i in 1..=4 {
            assert_eq!(id(&format!("TaskData.Position[{i}]")), format!("ns=3;s=\"OPCUA\".\"GR\"[2].\"CMD\".\"TaskData\".\"Position\"[{}]", i - 1));
        }
        for i in 1..=3 {
            assert_eq!(id(&format!("TaskData.Cell.Position[{i}]")), format!("ns=3;s=\"OPCUA\".\"GR\"[2].\"CMD\".\"TaskData\".\"Cell\".\"Position\"[{}]", i - 1));
        }
        for i in 0..16 {
            assert_eq!(id(&format!("Data[{i}]")), format!("ns=3;s=\"OPCUA\".\"GR\"[2].\"CMD\".\"Data\"[{i}]"));
        }
        // Idempotent (a cache saved after rebasing stays as is).
        assert_eq!(opcua_cmd::rebase_array_keys(&m, &grm_bases()).1, 0);

        // Every member the writer addresses by name exists after rebasing.
        let mut t = TaskData { position: [1.0, 2.0, 3.0, 4.0], ..Default::default() };
        t.item = StockItem { code: 1, ..Default::default() };
        let mut paths: Vec<String> = t.to_members().iter().map(|m| m.path.clone()).collect();
        paths.extend(["Header.Protocol", "Header.CMD_ID", "Header.CMD", "Header.SRC", "Header.DST", "Header.SEQ"].map(String::from));
        for op in ["Complete", "Delete"] {
            paths.extend(["WorkId", "TaskId"].map(|f| format!("Command.Task.{op}.{f}")));
        }
        let missing: Vec<&String> = paths.iter().filter(|p| !m.contains_key(p.as_str())).collect();
        assert!(missing.is_empty(), "missing on the real server: {missing:?}");
    }
}
