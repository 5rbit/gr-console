//! LGR_Station_Para (GR2 `STATION.Station[i]`, GRM `STATION.Station[i].Para` / `OPCUA.STATION[i].Para`).

use serde::{Deserialize, Serialize};

use crate::task::CellInfo;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SensorSettings {
    #[serde(rename = "IOLinkMasterModule")]
    pub io_link_master_module: u8,
    #[serde(rename = "IOLinkMasterPort_L")]
    pub io_link_master_port_l: u8,
    #[serde(rename = "IOLinkMasterPort_R")]
    pub io_link_master_port_r: u8,
    #[serde(rename = "DetectionFactor")]
    pub detection_factor: f32,
    #[serde(rename = "AllowRange")]
    pub allow_range: f32,
    #[serde(rename = "L_SensorOffset")]
    pub l_sensor_offset: f32,
    #[serde(rename = "R_SensorOffset")]
    pub r_sensor_offset: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StationPara {
    #[serde(rename = "ConvNo")]
    pub conv_no: u16,
    #[serde(rename = "TaskType")]
    pub task_type: u8,
    #[serde(rename = "RotateType")]
    pub rotate_type: u8,
    #[serde(rename = "Group")]
    pub group: u8,
    #[serde(rename = "GroupIndex")]
    pub group_index: u8,
    #[serde(rename = "ConnectionPrev")]
    pub connection_prev: u8,
    #[serde(rename = "ConnectionNext")]
    pub connection_next: u8,
    #[serde(rename = "Info")]
    pub info: CellInfo,
    #[serde(rename = "SensorSettings")]
    pub sensor_settings: SensorSettings,
    #[serde(rename = "IOBlockNo")]
    pub io_block_no: u8,
}
