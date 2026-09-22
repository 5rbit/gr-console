//! LGR_Command_Header / LGR_Stock_Item / LGR_Cell_Info / LGR_Task_Data.

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::consts::*;
use crate::wire::{MemberValue, WireValue};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskKey {
    pub work_id: u32,
    pub task_id: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum TaskType {
    Up,
    Pick,
    Drop,
    Move,
    Measure,
}

impl TaskType {
    pub fn code(self) -> u8 {
        match self {
            TaskType::Up => CMD_TASK_UP,
            TaskType::Pick => CMD_TASK_PICK,
            TaskType::Drop => CMD_TASK_DROP,
            TaskType::Move => CMD_TASK_MOVE,
            TaskType::Measure => CMD_TASK_MEASURE,
        }
    }
    pub fn from_code(code: u8) -> Option<TaskType> {
        Some(match code {
            CMD_TASK_UP => TaskType::Up,
            CMD_TASK_PICK => TaskType::Pick,
            CMD_TASK_DROP => TaskType::Drop,
            CMD_TASK_MOVE => TaskType::Move,
            CMD_TASK_MEASURE => TaskType::Measure,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            TaskType::Up => "UP",
            TaskType::Pick => "PICK",
            TaskType::Drop => "DROP",
            TaskType::Move => "MOVE",
            TaskType::Measure => "MEASURE",
        }
    }
}

pub fn task_type_name(code: u8) -> String {
    match TaskType::from_code(code) {
        Some(t) => t.name().to_string(),
        None => match code {
            CMD_TASK_AVOID_REF => "AVOID_REF".into(),
            CMD_TASK_AVOID_ABS => "AVOID_ABS".into(),
            CMD_TASK_AVOID_REL => "AVOID_REL".into(),
            0 => "-".into(),
            c => format!("0x{c:02X}"),
        },
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Header {
    #[serde(rename = "Protocol")]
    pub protocol: u8,
    #[serde(rename = "CMD_ID")]
    pub cmd_id: u8,
    #[serde(rename = "CMD")]
    pub cmd: u8,
    #[serde(rename = "SRC")]
    pub src: u16,
    #[serde(rename = "DST")]
    pub dst: u16,
    #[serde(rename = "SEQ")]
    pub seq: u16,
}

impl Header {
    pub fn is_zero(&self) -> bool {
        self.protocol == 0 && self.cmd_id == 0 && self.cmd == 0 && self.src == 0 && self.dst == 0 && self.seq == 0
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StockItem {
    #[serde(rename = "Code")]
    pub code: u32,
    #[serde(rename = "Count")]
    pub count: u8,
    #[serde(rename = "InnerDiameter")]
    #[serde(deserialize_with = "crate::nan::f32")]
    pub inner_diameter: f32,
    #[serde(rename = "OuterDiameter")]
    #[serde(deserialize_with = "crate::nan::f32")]
    pub outer_diameter: f32,
    #[serde(rename = "LowerBidHeight")]
    #[serde(deserialize_with = "crate::nan::f32")]
    pub lower_bid_height: f32,
    #[serde(rename = "UpperBidHeight")]
    #[serde(deserialize_with = "crate::nan::f32")]
    pub upper_bid_height: f32,
    #[serde(rename = "Height")]
    #[serde(deserialize_with = "crate::nan::f32")]
    pub height: f32,
    #[serde(rename = "DeflectionFactor")]
    #[serde(deserialize_with = "crate::nan::f32")]
    pub deflection_factor: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CellInfo {
    #[serde(rename = "Use")]
    pub use_: bool,
    #[serde(rename = "BlendUse")]
    pub blend_use: bool,
    #[serde(rename = "Id")]
    pub id: u16,
    #[serde(rename = "Section")]
    pub section: u16,
    #[serde(rename = "Row")]
    pub row: u16,
    #[serde(rename = "Col")]
    pub col: u16,
    #[serde(rename = "Lenth")]
    #[serde(deserialize_with = "crate::nan::f32")]
    pub length: f32,
    #[serde(rename = "Width")]
    #[serde(deserialize_with = "crate::nan::f32")]
    pub width: f32,
    /// X, Y, Z
    #[serde(rename = "Position")]
    #[serde(deserialize_with = "crate::nan::arr")]
    pub position: [f32; 3],
}

impl CellInfo {
    pub fn is_station(&self) -> bool {
        is_station_id(self.id)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskData {
    #[serde(rename = "WorkId")]
    pub work_id: u32,
    #[serde(rename = "TaskId")]
    pub task_id: u32,
    #[serde(rename = "TaskType")]
    pub task_type: u8,
    /// X, Y, Z, G
    #[serde(rename = "Position")]
    #[serde(deserialize_with = "crate::nan::arr")]
    pub position: [f32; 4],
    #[serde(rename = "Item")]
    pub item: StockItem,
    #[serde(rename = "Cell")]
    pub cell: CellInfo,
    #[serde(rename = "BlendUpDistance")]
    pub blend_up_distance: u16,
    #[serde(rename = "BlendDownDistance")]
    pub blend_down_distance: u16,
    #[serde(rename = "DragOutHeight")]
    pub drag_out_height: u16,
    #[serde(rename = "DragOutDist")]
    pub drag_out_dist: u16,
    #[serde(rename = "DragOutDir")]
    pub drag_out_dir: u8,
    #[serde(rename = "DragInHeight")]
    pub drag_in_height: u16,
    #[serde(rename = "DragInDist")]
    pub drag_in_dist: u16,
    #[serde(rename = "DragInDir")]
    pub drag_in_dir: u8,
    #[serde(rename = "LiftUpCreepDistance")]
    pub lift_up_creep_distance: u16,
    #[serde(rename = "LiftDownCreepDistance")]
    pub lift_down_creep_distance: u16,
    #[serde(rename = "LiftUpHeight")]
    pub lift_up_height: u16,
    #[serde(rename = "PreGripDelta")]
    pub pre_grip_delta: u16,
    #[serde(rename = "GripBackDelta")]
    pub grip_back_delta: u16,
    #[serde(rename = "GripHeight")]
    pub grip_height: u16,
    #[serde(rename = "UseDragOut")]
    pub use_drag_out: bool,
    #[serde(rename = "UseDragIn")]
    pub use_drag_in: bool,
    #[serde(rename = "LiftUpAfterComplete")]
    pub lift_up_after_complete: bool,
    #[serde(rename = "LiftUpPartial")]
    pub lift_up_partial: bool,
    #[serde(rename = "MeasureFloor")]
    pub measure_floor: bool,
    #[serde(rename = "MeasureItem")]
    pub measure_item: bool,
    #[serde(rename = "MeasureSku")]
    pub measure_sku: bool,
    #[serde(rename = "AdjustCenter")]
    pub adjust_center: bool,
    #[serde(rename = "FindStationItem")]
    pub find_station_item: bool,
    #[serde(rename = "Avoid")]
    pub avoid: bool,
    #[serde(rename = "Outbound")]
    pub outbound: bool,
}

impl TaskData {
    pub fn from_json(v: &Json) -> Result<Self, serde_json::Error> {
        serde_json::from_value(v.clone())
    }
    pub fn key(&self) -> TaskKey {
        TaskKey { work_id: self.work_id, task_id: self.task_id }
    }
    pub fn is_zero(&self) -> bool {
        self.work_id == 0 && self.task_id == 0 && self.task_type == 0
    }
    pub fn task_type(&self) -> Option<TaskType> {
        TaskType::from_code(self.task_type)
    }

    /// Leaf members for an OPC UA write of `CMD.TaskData` (array indices are PLC indices: X..G = 1..4).
    pub fn to_members(&self) -> Vec<MemberValue> {
        let mut m = Vec::with_capacity(60);
        let p = |s: &str| format!("TaskData.{s}");
        m.push(MemberValue::new(p("WorkId"), WireValue::U32(self.work_id)));
        m.push(MemberValue::new(p("TaskId"), WireValue::U32(self.task_id)));
        m.push(MemberValue::new(p("TaskType"), WireValue::U8(self.task_type)));
        for (i, v) in self.position.iter().enumerate() {
            m.push(MemberValue::new(p(&format!("Position[{}]", i + 1)), WireValue::F32(*v)));
        }
        m.push(MemberValue::new(p("Item.Code"), WireValue::U32(self.item.code)));
        m.push(MemberValue::new(p("Item.Count"), WireValue::U8(self.item.count)));
        m.push(MemberValue::new(p("Item.InnerDiameter"), WireValue::F32(self.item.inner_diameter)));
        m.push(MemberValue::new(p("Item.OuterDiameter"), WireValue::F32(self.item.outer_diameter)));
        m.push(MemberValue::new(p("Item.LowerBidHeight"), WireValue::F32(self.item.lower_bid_height)));
        m.push(MemberValue::new(p("Item.UpperBidHeight"), WireValue::F32(self.item.upper_bid_height)));
        m.push(MemberValue::new(p("Item.Height"), WireValue::F32(self.item.height)));
        m.push(MemberValue::new(p("Item.DeflectionFactor"), WireValue::F32(self.item.deflection_factor)));
        m.push(MemberValue::new(p("Cell.Use"), WireValue::Bool(self.cell.use_)));
        m.push(MemberValue::new(p("Cell.BlendUse"), WireValue::Bool(self.cell.blend_use)));
        m.push(MemberValue::new(p("Cell.Id"), WireValue::U16(self.cell.id)));
        m.push(MemberValue::new(p("Cell.Section"), WireValue::U16(self.cell.section)));
        m.push(MemberValue::new(p("Cell.Row"), WireValue::U16(self.cell.row)));
        m.push(MemberValue::new(p("Cell.Col"), WireValue::U16(self.cell.col)));
        m.push(MemberValue::new(p("Cell.Lenth"), WireValue::F32(self.cell.length)));
        m.push(MemberValue::new(p("Cell.Width"), WireValue::F32(self.cell.width)));
        for (i, v) in self.cell.position.iter().enumerate() {
            m.push(MemberValue::new(p(&format!("Cell.Position[{}]", i + 1)), WireValue::F32(*v)));
        }
        m.push(MemberValue::new(p("BlendUpDistance"), WireValue::U16(self.blend_up_distance)));
        m.push(MemberValue::new(p("BlendDownDistance"), WireValue::U16(self.blend_down_distance)));
        m.push(MemberValue::new(p("DragOutHeight"), WireValue::U16(self.drag_out_height)));
        m.push(MemberValue::new(p("DragOutDist"), WireValue::U16(self.drag_out_dist)));
        m.push(MemberValue::new(p("DragOutDir"), WireValue::U8(self.drag_out_dir)));
        m.push(MemberValue::new(p("DragInHeight"), WireValue::U16(self.drag_in_height)));
        m.push(MemberValue::new(p("DragInDist"), WireValue::U16(self.drag_in_dist)));
        m.push(MemberValue::new(p("DragInDir"), WireValue::U8(self.drag_in_dir)));
        m.push(MemberValue::new(p("LiftUpCreepDistance"), WireValue::U16(self.lift_up_creep_distance)));
        m.push(MemberValue::new(p("LiftDownCreepDistance"), WireValue::U16(self.lift_down_creep_distance)));
        m.push(MemberValue::new(p("LiftUpHeight"), WireValue::U16(self.lift_up_height)));
        m.push(MemberValue::new(p("PreGripDelta"), WireValue::U16(self.pre_grip_delta)));
        m.push(MemberValue::new(p("GripBackDelta"), WireValue::U16(self.grip_back_delta)));
        m.push(MemberValue::new(p("GripHeight"), WireValue::U16(self.grip_height)));
        m.push(MemberValue::new(p("UseDragOut"), WireValue::Bool(self.use_drag_out)));
        m.push(MemberValue::new(p("UseDragIn"), WireValue::Bool(self.use_drag_in)));
        m.push(MemberValue::new(p("LiftUpAfterComplete"), WireValue::Bool(self.lift_up_after_complete)));
        m.push(MemberValue::new(p("LiftUpPartial"), WireValue::Bool(self.lift_up_partial)));
        m.push(MemberValue::new(p("MeasureFloor"), WireValue::Bool(self.measure_floor)));
        m.push(MemberValue::new(p("MeasureItem"), WireValue::Bool(self.measure_item)));
        m.push(MemberValue::new(p("MeasureSku"), WireValue::Bool(self.measure_sku)));
        m.push(MemberValue::new(p("AdjustCenter"), WireValue::Bool(self.adjust_center)));
        m.push(MemberValue::new(p("FindStationItem"), WireValue::Bool(self.find_station_item)));
        m.push(MemberValue::new(p("Avoid"), WireValue::Bool(self.avoid)));
        m.push(MemberValue::new(p("Outbound"), WireValue::Bool(self.outbound)));
        m
    }
}

/// Members that zero `CMD.Command` (all bytes + the Task Complete/Delete ids) and `CMD.Data[0..15]`.
pub fn command_zero_members() -> Vec<MemberValue> {
    let mut m = Vec::new();
    for b in ["Stop", "Common", "Jog", "B3_Spare", "Home", "MaintPos", "CellOrigin", "ChangeMode"] {
        m.push(MemberValue::new(format!("Command.{b}"), WireValue::U8(0)));
    }
    for p in ["Complete", "Delete"] {
        m.push(MemberValue::new(format!("Command.Task.{p}.WorkId"), WireValue::U32(0)));
        m.push(MemberValue::new(format!("Command.Task.{p}.TaskId"), WireValue::U32(0)));
    }
    for i in 0..16 {
        m.push(MemberValue::new(format!("Data[{i}]"), WireValue::U8(0)));
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn task_from_plc_json() {
        let j = json!({"WorkId": 5001, "TaskId": 2, "TaskType": 0x41, "Position": [1.0, 2.0, 3.0, 4.0],
            "Item": {"Code": 1001, "Count": 3, "InnerDiameter": 381.0},
            "Cell": {"Use": true, "Id": 2101, "Position": [10.0, 20.0, 30.0]}, "GripHeight": 40, "MeasureItem": true});
        let t = TaskData::from_json(&j).unwrap();
        assert_eq!(t.key(), TaskKey { work_id: 5001, task_id: 2 });
        assert_eq!(t.task_type(), Some(TaskType::Pick));
        assert!(t.cell.is_station());
        assert_eq!(t.to_members().len(), 51);
        assert_eq!(t.to_members()[3].path, "TaskData.Position[1]");
    }
}
