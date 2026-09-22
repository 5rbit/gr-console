//! LGR_Interface_GR_Status (the `STAT` half of the OPC UA interface) and the response/reject decoding.

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::consts::{VALID_TASK_DATA, validation_text};
use crate::task::{Header, TaskData, TaskKey};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResponseView {
    #[serde(rename = "Header")]
    pub header: Header,
    #[serde(rename = "Command")]
    pub command: Json,
    #[serde(rename = "Task")]
    pub task: TaskData,
    #[serde(rename = "Data")]
    pub data: Vec<u8>,
}

/// Decoded `STAT.RES.Data`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RejectInfo {
    pub wrong_robot: bool,
    pub duplicate: bool,
    pub invalid_task: bool,
    pub buffer_full: bool,
    pub offline: bool,
    pub validation: u16,
    pub validation_text: String,
    pub complete_allowed: bool,
    pub delete_allowed: bool,
}

impl RejectInfo {
    pub fn from_data(data: &[u8]) -> RejectInfo {
        let b0 = data.first().copied().unwrap_or(0);
        let validation = ((data.get(1).copied().unwrap_or(0) as u16) << 8) | data.get(2).copied().unwrap_or(0) as u16;
        RejectInfo {
            wrong_robot: b0 & 0x01 != 0,
            duplicate: b0 & 0x02 != 0,
            invalid_task: b0 & 0x04 != 0,
            buffer_full: b0 & 0x08 != 0,
            offline: b0 & 0x80 != 0,
            validation,
            validation_text: validation_text(validation).to_string(),
            complete_allowed: data.get(5).copied().unwrap_or(0) != 0,
            delete_allowed: data.get(6).copied().unwrap_or(0) != 0,
        }
    }
    pub fn accepted(&self) -> bool {
        !(self.wrong_robot || self.duplicate || self.invalid_task || self.buffer_full || self.offline) && self.validation == VALID_TASK_DATA
    }
    pub fn reason(&self) -> String {
        let mut r = Vec::new();
        if self.wrong_robot {
            r.push("wrong robot (DST)");
        }
        if self.duplicate {
            r.push("duplicate header");
        }
        if self.invalid_task {
            r.push("invalid task data");
        }
        if self.buffer_full {
            r.push("task buffer full");
        }
        if self.offline {
            r.push("robot offline");
        }
        if self.validation != VALID_TASK_DATA {
            return format!("{} ({}{})", self.validation_text, self.validation, if r.is_empty() { String::new() } else { format!("; {}", r.join(", ")) });
        }
        if r.is_empty() { "accepted".into() } else { r.join(", ") }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskStatusBits {
    #[serde(rename = "Accept")]
    pub accept: bool,
    #[serde(rename = "Idle")]
    pub idle: bool,
    #[serde(rename = "Assigned")]
    pub assigned: bool,
    #[serde(rename = "Inprogress")]
    pub inprogress: bool,
    #[serde(rename = "AvoidReq")]
    pub avoid_req: bool,
    #[serde(rename = "HoldItem")]
    pub hold_item: bool,
    #[serde(rename = "Complete")]
    pub complete: bool,
    #[serde(rename = "Canceled")]
    pub canceled: bool,
    #[serde(rename = "Step")]
    pub step: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskArea {
    #[serde(rename = "Status")]
    pub status: TaskStatusBits,
    #[serde(rename = "Now")]
    pub now: TaskData,
    #[serde(rename = "Queue")]
    pub queue: Vec<TaskData>,
    #[serde(rename = "Completed")]
    pub completed: Vec<TaskData>,
    #[serde(rename = "Canceled")]
    pub canceled: Vec<TaskData>,
    #[serde(rename = "Rejected")]
    pub rejected: Vec<TaskData>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EquipMode {
    #[serde(rename = "Maint")]
    pub maint: bool,
    #[serde(rename = "Manual")]
    pub manual: bool,
    #[serde(rename = "AutoReady")]
    pub auto_ready: bool,
    #[serde(rename = "Auto")]
    pub auto: bool,
    #[serde(rename = "Init")]
    pub init: bool,
    #[serde(rename = "Fault")]
    pub fault: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EquipStatus {
    #[serde(rename = "Normal")]
    pub normal: bool,
    #[serde(rename = "Idle")]
    pub idle: bool,
    #[serde(rename = "Busy")]
    pub busy: bool,
    #[serde(rename = "Stopped")]
    pub stopped: bool,
    #[serde(rename = "Online")]
    pub online: bool,
    #[serde(rename = "Warn")]
    pub warn: bool,
    #[serde(rename = "Fault")]
    pub fault: bool,
    #[serde(rename = "PowerOn")]
    pub power_on: bool,
    #[serde(rename = "ItemDectect")]
    pub item_detect: bool,
    #[serde(rename = "HeartBeat")]
    pub heart_beat: bool,
    #[serde(rename = "Speed")]
    pub speed: i8,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StatusDetail {
    #[serde(rename = "Main")]
    pub main: u16,
    #[serde(rename = "Sub")]
    pub sub: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DriveView {
    #[serde(rename = "MainCode")]
    pub main_code: u16,
    #[serde(rename = "SubCode")]
    pub sub_code: u16,
    #[serde(rename = "OpMode")]
    pub op_mode: u16,
    #[serde(rename = "Speed")]
    #[serde(deserialize_with = "crate::nan::f32")]
    pub speed: f32,
    #[serde(rename = "Torque")]
    #[serde(deserialize_with = "crate::nan::f32")]
    pub torque: f32,
    #[serde(rename = "Position")]
    #[serde(deserialize_with = "crate::nan::f32")]
    pub position: f32,
    #[serde(rename = "JerkTime")]
    #[serde(deserialize_with = "crate::nan::f32")]
    pub jerk_time: f32,
    #[serde(rename = "TorqLimit")]
    #[serde(deserialize_with = "crate::nan::f32")]
    pub torq_limit: f32,
    #[serde(rename = "StartPos")]
    #[serde(deserialize_with = "crate::nan::f32")]
    pub start_pos: f32,
    #[serde(rename = "Direction")]
    pub direction: i16,
}

/// `LGR_Interface_GR_Status`. Unknown / rarely used blocks stay as raw JSON.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StatusView {
    #[serde(rename = "NTP")]
    pub ntp: String,
    #[serde(rename = "ComponentID")]
    pub component_id: u16,
    #[serde(rename = "RES")]
    pub res: ResponseView,
    #[serde(rename = "Mode")]
    pub mode: EquipMode,
    #[serde(rename = "Status")]
    pub status: EquipStatus,
    #[serde(rename = "StatusCode")]
    pub status_code: StatusDetail,
    #[serde(rename = "Task")]
    pub task: TaskArea,
    #[serde(rename = "Measuring")]
    pub measuring: Json,
    #[serde(rename = "Interlock")]
    pub interlock: Json,
    #[serde(rename = "Signal")]
    pub signal: Json,
    #[serde(rename = "Drive")]
    pub drive: Vec<DriveView>,
}

impl StatusView {
    pub fn from_json(v: &Json) -> Result<Self, serde_json::Error> {
        serde_json::from_value(v.clone())
    }
    pub fn reject_info(&self) -> RejectInfo {
        RejectInfo::from_data(&self.res.data)
    }
    /// Position of a task key in the PLC arrays.
    pub fn locate(&self, key: TaskKey) -> TaskLocation {
        if !self.task.now.is_zero() && self.task.now.key() == key {
            return TaskLocation::Now;
        }
        if let Some(i) = self.task.queue.iter().position(|t| !t.is_zero() && t.key() == key) {
            return TaskLocation::Queue(i);
        }
        if let Some(i) = self.task.completed.iter().position(|t| !t.is_zero() && t.key() == key) {
            return TaskLocation::Completed(i);
        }
        if let Some(i) = self.task.canceled.iter().position(|t| !t.is_zero() && t.key() == key) {
            return TaskLocation::Canceled(i);
        }
        if let Some(i) = self.task.rejected.iter().position(|t| !t.is_zero() && t.key() == key) {
            return TaskLocation::Rejected(i);
        }
        TaskLocation::Absent
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum TaskLocation {
    Now,
    Queue(usize),
    Completed(usize),
    Canceled(usize),
    Rejected(usize),
    Absent,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_bits_and_validation() {
        let ok = RejectInfo::from_data(&[0, 0, 1, 0, 0, 1, 0]);
        assert!(ok.accepted());
        assert!(ok.complete_allowed);
        let bad = RejectInfo::from_data(&[0x04, 0x01, 0xA5, 0, 0, 0, 0]); // 0x01A5 = 421
        assert!(!bad.accepted());
        assert_eq!(bad.validation, 421);
        assert!(bad.reason().contains("INVALID_CELL_RANGE_X"));
    }
}
