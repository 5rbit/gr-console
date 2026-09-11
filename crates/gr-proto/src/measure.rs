//! MEASLOG / MEASLOG_HIST structures.

use serde::{Deserialize, Serialize};

use crate::task::{StockItem, TaskData};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Trend {
    #[serde(rename = "Count")]
    pub count: u32,
    #[serde(rename = "Last")]
    pub last: f32,
    #[serde(rename = "Avg")]
    pub avg: f32,
    #[serde(rename = "Ema")]
    pub ema: f32,
    #[serde(rename = "Min")]
    pub min: f32,
    #[serde(rename = "Max")]
    pub max: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MeasStat {
    #[serde(rename = "Count")]
    pub count: u32,
    #[serde(rename = "ErrorCount")]
    pub error_count: u32,
    #[serde(rename = "InnerDia")]
    pub inner_dia: Trend,
    #[serde(rename = "Height")]
    pub height: Trend,
    #[serde(rename = "Z")]
    pub z: Trend,
    #[serde(rename = "Offset")]
    pub offset: Trend,
    #[serde(rename = "StackCount")]
    pub stack_count: Trend,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Delta {
    #[serde(rename = "InnerDia")]
    pub inner_dia: f32,
    #[serde(rename = "Height")]
    pub height: f32,
    #[serde(rename = "Z")]
    pub z: f32,
    #[serde(rename = "Offset")]
    pub offset: f32,
    #[serde(rename = "Count")]
    pub count: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MeasureLogEntry {
    #[serde(rename = "TimeStamp")]
    pub time_stamp: String,
    #[serde(rename = "Seq")]
    pub seq: u32,
    #[serde(rename = "Kind")]
    pub kind: u8,
    #[serde(rename = "Status")]
    pub status: u16,
    #[serde(rename = "Cmd")]
    pub cmd: TaskData,
    #[serde(rename = "Data")]
    pub data: Vec<f32>,
    #[serde(rename = "Delta")]
    pub delta: Delta,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ByCode {
    #[serde(rename = "Code")]
    pub code: u32,
    #[serde(rename = "Item")]
    pub item: StockItem,
    #[serde(rename = "LastTime")]
    pub last_time: String,
    #[serde(rename = "Count")]
    pub count: u32,
    #[serde(rename = "Stat")]
    pub stat: Vec<MeasStat>,
    #[serde(rename = "Meas")]
    pub meas: serde_json::Value,
}
