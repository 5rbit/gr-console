//! Console-side tunables of a task (snake_case, used by defaults profiles / overrides) and their
//! application onto `TaskData`.

use serde::{Deserialize, Serialize};

use crate::task::TaskData;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskParams {
    pub lift_up_height: u16,
    pub grip_height: u16,
    pub pre_grip_delta: u16,
    pub grip_back_delta: u16,
    pub blend_up_distance: u16,
    pub blend_down_distance: u16,
    pub lift_up_creep_distance: u16,
    pub lift_down_creep_distance: u16,
    pub lift_up_after_complete: bool,
    pub lift_up_partial: bool,
    pub measure_floor: bool,
    pub measure_item: bool,
    pub measure_sku: bool,
    pub adjust_center: bool,
    pub find_station_item: bool,
    pub avoid: bool,
    pub outbound: bool,
    pub use_drag_out: bool,
    pub drag_out_height: u16,
    pub drag_out_dist: u16,
    pub drag_out_dir: u8,
    pub use_drag_in: bool,
    pub drag_in_height: u16,
    pub drag_in_dist: u16,
    pub drag_in_dir: u8,
}

impl Default for TaskParams {
    fn default() -> Self {
        Self {
            lift_up_height: 2500,
            grip_height: 40,
            pre_grip_delta: 10,
            grip_back_delta: 5,
            blend_up_distance: 300,
            blend_down_distance: 300,
            lift_up_creep_distance: 30,
            lift_down_creep_distance: 30,
            lift_up_after_complete: true,
            lift_up_partial: false,
            measure_floor: false,
            measure_item: false,
            measure_sku: false,
            adjust_center: false,
            find_station_item: false,
            avoid: false,
            outbound: false,
            use_drag_out: false,
            drag_out_height: 0,
            drag_out_dist: 0,
            drag_out_dir: 0,
            use_drag_in: false,
            drag_in_height: 0,
            drag_in_dist: 0,
            drag_in_dir: 0,
        }
    }
}

impl TaskParams {
    /// Overlays a partial JSON object (snake_case keys) onto these params.
    pub fn overlay(&self, partial: &serde_json::Value) -> Result<TaskParams, serde_json::Error> {
        let mut base = serde_json::to_value(self)?;
        if let (Some(b), Some(p)) = (base.as_object_mut(), partial.as_object()) {
            for (k, v) in p {
                if !v.is_null() {
                    b.insert(k.clone(), v.clone());
                }
            }
        }
        serde_json::from_value(base)
    }

    pub fn apply(&self, t: &mut TaskData) {
        t.lift_up_height = self.lift_up_height;
        t.grip_height = self.grip_height;
        t.pre_grip_delta = self.pre_grip_delta;
        t.grip_back_delta = self.grip_back_delta;
        t.blend_up_distance = self.blend_up_distance;
        t.blend_down_distance = self.blend_down_distance;
        t.lift_up_creep_distance = self.lift_up_creep_distance;
        t.lift_down_creep_distance = self.lift_down_creep_distance;
        t.lift_up_after_complete = self.lift_up_after_complete;
        t.lift_up_partial = self.lift_up_partial;
        t.measure_floor = self.measure_floor;
        t.measure_item = self.measure_item;
        t.measure_sku = self.measure_sku;
        t.adjust_center = self.adjust_center;
        t.find_station_item = self.find_station_item;
        t.avoid = self.avoid;
        t.outbound = self.outbound;
        t.use_drag_out = self.use_drag_out;
        t.drag_out_height = self.drag_out_height;
        t.drag_out_dist = self.drag_out_dist;
        t.drag_out_dir = self.drag_out_dir;
        t.use_drag_in = self.use_drag_in;
        t.drag_in_height = self.drag_in_height;
        t.drag_in_dist = self.drag_in_dist;
        t.drag_in_dir = self.drag_in_dir;
    }

    pub fn from_task(t: &TaskData) -> TaskParams {
        TaskParams {
            lift_up_height: t.lift_up_height,
            grip_height: t.grip_height,
            pre_grip_delta: t.pre_grip_delta,
            grip_back_delta: t.grip_back_delta,
            blend_up_distance: t.blend_up_distance,
            blend_down_distance: t.blend_down_distance,
            lift_up_creep_distance: t.lift_up_creep_distance,
            lift_down_creep_distance: t.lift_down_creep_distance,
            lift_up_after_complete: t.lift_up_after_complete,
            lift_up_partial: t.lift_up_partial,
            measure_floor: t.measure_floor,
            measure_item: t.measure_item,
            measure_sku: t.measure_sku,
            adjust_center: t.adjust_center,
            find_station_item: t.find_station_item,
            avoid: t.avoid,
            outbound: t.outbound,
            use_drag_out: t.use_drag_out,
            drag_out_height: t.drag_out_height,
            drag_out_dist: t.drag_out_dist,
            drag_out_dir: t.drag_out_dir,
            use_drag_in: t.use_drag_in,
            drag_in_height: t.drag_in_height,
            drag_in_dist: t.drag_in_dist,
            drag_in_dir: t.drag_in_dir,
        }
    }
}
