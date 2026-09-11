//! Command port: how a task command reaches the robot. `Opc` writes GRM `"OPCUA".GR[2].CMD` through
//! OPC UA; `Demo` feeds the in-process demo world. Both expose the same async surface.

use std::sync::{Arc, Mutex, PoisonError};

use gr_proto::{Header, MemberValue, TaskData, WireValue, command_zero_members};
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

fn to_opc(m: &MemberValue) -> opcua_cmd::MemberValue {
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

    pub fn is_ready(&self) -> bool {
        match self {
            CommandPort::Opc { writer, .. } => matches!(writer.state(), opcua_cmd::OpcState::Ready { .. }),
            CommandPort::Demo { .. } => true,
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
        let mut members = task.to_members();
        members.extend(command_zero_members());
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
            CommandPort::Demo { world, .. } => {
                world.task_op(op, work_id, task_id);
                Ok(())
            }
        }
    }

    #[allow(dead_code)] // 에코 후 재무장용 — 실기 M4 에서 사용 예정
    pub async fn clear_header(&self) -> Result<(), ApiError> {
        match self {
            CommandPort::Opc { writer, .. } => writer.clear_header().await.map_err(|e| ApiError::OpcNotReady(e.to_string())),
            CommandPort::Demo { world, .. } => {
                world.clear_header();
                Ok(())
            }
        }
    }
}
