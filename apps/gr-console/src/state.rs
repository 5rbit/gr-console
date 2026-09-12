use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value as Json;
use tokio::sync::broadcast;

use crate::cmd::CommandPort;
use crate::config::Config;
use crate::db::Db;
use crate::error::ApiError;
use crate::ledger::{Ledger, LedgerEntry, LedgerEvent};
use crate::measure::MeasureStore;
use crate::plc::PlcHandle;
use crate::registry::Registry;
use crate::scenario::Runner;
use crate::status::StatusBus;
use crate::stock::Stock;

/// Unified console event (SSE `/api/events`).
#[derive(Clone, Debug, Serialize)]
pub struct ConsoleEvent {
    pub topic: &'static str,
    pub at: String,
    pub data: Json,
}

/// One robot: its command port (OPC UA `GR[n].CMD` via GRM) + ledger + status PLC name.
pub struct RobotCtx {
    pub id: u8,
    pub name: String,
    pub plc: String,
    pub opcua_root: String,
    pub dst: u16,
    pub cmd: Arc<CommandPort>,
    pub ledger: Arc<Ledger>,
}

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub plcs: Arc<HashMap<String, PlcHandle>>,
    /// Default robot's command port (= `robots[0].cmd`).
    pub cmd: Arc<CommandPort>,
    /// All robots (≥ 1). Task events of every ledger go through `task_events`.
    pub robots: Arc<Vec<RobotCtx>>,
    pub task_events: broadcast::Sender<LedgerEvent>,
    #[allow(dead_code)] // 슬라이스들이 각자 Arc<Db> 사본을 들고 있음; 관리용 접근 경로로 유지
    pub db: Db,
    pub ledger: Arc<Ledger>,
    pub registry: Arc<Registry>,
    pub measure: Arc<MeasureStore>,
    pub scenario: Arc<Runner>,
    pub stock: Arc<Stock>,
    pub status: StatusBus,
    pub events: broadcast::Sender<ConsoleEvent>,
}

impl AppState {
    pub fn plc(&self, name: &str) -> Result<&PlcHandle, ApiError> {
        self.plcs.get(name).or_else(|| self.plcs.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v)).ok_or_else(|| ApiError::NotFound(format!("plc {name}")))
    }

    /// Resolves a frontend id like `gr2_s7` to a PLC handle.
    pub fn plc_by_id(&self, id: &str) -> Option<&PlcHandle> {
        let name = id.strip_suffix("_s7").unwrap_or(id);
        self.plcs.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v)
    }

    /// The PLC whose `OPCUA` DB carries the echo / task arrays (GR2).
    pub fn status_plc(&self) -> Result<&PlcHandle, ApiError> {
        self.plc(&self.cfg.cmd.status_plc)
    }

    pub fn grm_plc(&self) -> Option<&PlcHandle> {
        self.plcs.get(&self.cfg.cmd.grm_plc)
    }

    /// Robot by id; `None` = default robot (first configured).
    pub fn robot(&self, id: Option<u8>) -> Result<&RobotCtx, ApiError> {
        match id {
            None => self.robots.first().ok_or_else(|| ApiError::Internal("no robot configured".into())),
            Some(i) => self.robots.iter().find(|r| r.id == i).ok_or_else(|| ApiError::NotFound(format!("robot {i}"))),
        }
    }

    /// The ledger entry `id` and the robot that owns it.
    pub fn find_task(&self, id: &str) -> Option<(&RobotCtx, LedgerEntry)> {
        self.robots.iter().find_map(|r| r.ledger.get(id).map(|e| (r, e)))
    }

    /// Status PLC handle of a robot.
    pub fn robot_plc(&self, r: &RobotCtx) -> Result<&PlcHandle, ApiError> {
        self.plc(&r.plc)
    }

    pub fn emit(&self, topic: &'static str, data: Json) {
        let _ = self.events.send(ConsoleEvent { topic, at: crate::util::now_str(), data });
    }
}
