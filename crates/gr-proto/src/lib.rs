//! Typed views of the GR PLC interface. Field names mirror the PLC (PascalCase) so the JSON produced by
//! `plc-layout` deserializes directly and the frontend sees PLC names verbatim.

pub mod consts;
pub mod measure;
mod nan;
pub mod params;
pub mod station;
pub mod status;
pub mod task;
pub mod wire;

pub use consts::*;
pub use measure::{ByCode, MeasStat, MeasureLogEntry, Trend};
pub use params::{DEFAULT_DRAG_DIST, PARAM_SPECS, ParamKind, ParamSpec, TaskParams, check_partial};
pub use station::{SensorSettings, StationPara};
pub use status::{RejectInfo, ResponseView, StatusView, TaskStatusBits};
pub use task::{CellInfo, Header, StockItem, TaskData, TaskKey, TaskType, command_zero_members, task_type_name};
pub use wire::{MemberValue, WireValue};

#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
}
