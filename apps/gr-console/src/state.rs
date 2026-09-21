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
use crate::record::Recorder;
use crate::registry::Registry;
use crate::scenario::Runner;
use crate::status::StatusBus;
use crate::stock::Stock;
use crate::trace::TraceStore;

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
    /// This robot's WEBMON status stream (`/api/status?robot=`).
    pub status: StatusBus,
    /// This robot's MEASLOG mirror (`/api/measlog/*?robot=`).
    pub measure: Arc<MeasureStore>,
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
    pub scenario: Arc<Runner>,
    pub stock: Arc<Stock>,
    pub recorder: Arc<Recorder>,
    /// Cycle-accurate trace over the PLC socket link; `None` when the contract has no `LNK_Trace`.
    pub trace: Option<Arc<TraceStore>>,
    pub events: broadcast::Sender<ConsoleEvent>,
    /// 안전 종료 조정자 — PLC 쓰기 구간은 `shutdown.enter(…)` 로 감싼다(`crate::shutdown`).
    pub shutdown: crate::shutdown::Shutdown,
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

    /// Status PLC of the default robot (`robots[0]`). Every read without a robot resolves here, so a label from
    /// `robot(None)` and the data next to it never disagree; `cmd.status_plc` only feeds `robots[0].plc` when
    /// `[[robots]]` is empty (`Config::robots_effective`).
    pub fn status_plc(&self) -> Result<&PlcHandle, ApiError> {
        self.plc(self.default_plc_name())
    }

    /// Config name of the default robot's status PLC.
    pub fn default_plc_name(&self) -> &str {
        self.robots.first().map(|r| r.plc.as_str()).unwrap_or(&self.cfg.cmd.status_plc)
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

    /// Status PLC of a robot by id (`None` = default robot) together with the robot.
    /// PLC 에 **쓰는** 요청용 — 로봇이 둘 이상이면 `None` 을 받지 않는다. `robot` 이 빠진 요청이 조용히
    /// 첫 로봇(GR1)으로 가던 사고(2026-09-21: 작업 제출·Task 취소)를 경로마다 막는다. 로봇이 하나면 그 로봇.
    pub fn robot_required(&self, id: Option<u8>, what: &str) -> Result<&RobotCtx, ApiError> {
        if id.is_none() && self.robots.len() > 1 {
            return Err(ApiError::BadRequest(format!("{what}: 대상 로봇을 지정해야 합니다 (robot: {})", self.robot_choices())));
        }
        self.robot(id)
    }

    /// `robot_required` + 그 로봇의 상태 PLC.
    pub fn robot_and_plc_required(&self, id: Option<u8>, what: &str) -> Result<(&RobotCtx, &PlcHandle), ApiError> {
        let r = self.robot_required(id, what)?;
        Ok((r, self.robot_plc(r)?))
    }

    /// 거부 문구용 로봇 목록 — `1=GR1, 2=GR2`.
    pub fn robot_choices(&self) -> String {
        self.robots.iter().map(|r| format!("{}={}", r.id, r.name)).collect::<Vec<_>>().join(", ")
    }

    pub fn robot_and_plc(&self, id: Option<u8>) -> Result<(&RobotCtx, &PlcHandle), ApiError> {
        let r = self.robot(id)?;
        Ok((r, self.robot_plc(r)?))
    }

    /// Status PLC handle of a robot.
    pub fn robot_plc(&self, r: &RobotCtx) -> Result<&PlcHandle, ApiError> {
        self.plc(&r.plc)
    }

    pub fn emit(&self, topic: &'static str, data: Json) {
        let _ = self.events.send(ConsoleEvent { topic, at: crate::util::now_str(), data });
    }
}
