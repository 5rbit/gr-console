use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value as Json;
use tokio::sync::broadcast;

use crate::cmd::CommandPort;
use crate::config::Config;
use crate::db::Db;
use crate::error::ApiError;
use crate::ledger::Ledger;
use crate::measure::MeasureStore;
use crate::plc::PlcHandle;
use crate::registry::Registry;
use crate::scenario::Runner;
use crate::status::StatusBus;

/// Unified console event (SSE `/api/events`).
#[derive(Clone, Debug, Serialize)]
pub struct ConsoleEvent {
    pub topic: &'static str,
    pub at: String,
    pub data: Json,
}

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub plcs: Arc<HashMap<String, PlcHandle>>,
    pub cmd: Arc<CommandPort>,
    pub db: Db,
    pub ledger: Arc<Ledger>,
    pub registry: Arc<Registry>,
    pub measure: Arc<MeasureStore>,
    pub scenario: Arc<Runner>,
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

    pub fn emit(&self, topic: &'static str, data: Json) {
        let _ = self.events.send(ConsoleEvent { topic, at: crate::util::now_str(), data });
    }
}
