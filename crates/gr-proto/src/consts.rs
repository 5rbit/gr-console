//! Protocol constants (from the PLC constant tables).

pub const COMP_ID_GRM: u16 = 4000;
pub const COMP_ID_GR1: u16 = 4001;
pub const COMP_ID_GR2: u16 = 4002;
pub const COMP_ID_GR3: u16 = 4003;
pub const COMP_ID_GCS: u16 = 5000;

pub const CMD_TASK_UP: u8 = 0x40;
pub const CMD_TASK_PICK: u8 = 0x41;
pub const CMD_TASK_DROP: u8 = 0x42;
pub const CMD_TASK_MOVE: u8 = 0x43;
pub const CMD_TASK_MEASURE: u8 = 0x44;
pub const CMD_TASK_AVOID_REF: u8 = 0x4A;
pub const CMD_TASK_AVOID_ABS: u8 = 0x4B;
pub const CMD_TASK_AVOID_REL: u8 = 0x4C;

pub const VALID_TASK_DATA: u16 = 1;

pub const MODE_BOOT: u16 = 0x0000;
pub const MODE_INIT: u16 = 0x0001;
pub const MODE_MAINT: u16 = 0x0002;
pub const MODE_STANDBY: u16 = 0x0004;
pub const MODE_MANUAL: u16 = 0x0008;
pub const MODE_READY: u16 = 0x0010;
pub const MODE_AUTO: u16 = 0x0020;
pub const MODE_FAULT: u16 = 0x0080;
pub const MODE_TEACH: u16 = 0x0100;

pub const CELL_ID_MIN: u16 = 1;
pub const CELL_ID_MAX: u16 = 1000;
pub const CELL_MAX: usize = 500;
pub const STATION_ID_MIN: u16 = 2001;
pub const STATION_ID_MAX: u16 = 2999;
pub const STATION_MAX: usize = 32;

pub const MEAS_LOG_KIND_ITEM: u8 = 1;
pub const MEAS_LOG_KIND_SKU: u8 = 2;
pub const MEAS_LOG_KIND_FLOOR: u8 = 3;
pub const MEAS_LOG_KIND_PICK: u8 = 4;
pub const MEAS_LOG_KIND_MANUAL: u8 = 5;

pub fn mode_name(mode: u16) -> &'static str {
    match mode {
        MODE_BOOT => "BOOT",
        MODE_INIT => "INIT",
        MODE_MAINT => "MAINT",
        MODE_STANDBY => "STANDBY",
        MODE_MANUAL => "MANUAL",
        MODE_READY => "READY",
        MODE_AUTO => "AUTO",
        MODE_FAULT => "FAULT",
        MODE_TEACH => "TEACH",
        _ => "?",
    }
}

/// Task validation codes returned in `STAT.RES.Data[1..2]` (from LGR_Const_Interface_Validation).
pub fn validation_text(code: u16) -> &'static str {
    match code {
        1 => "VALID_TASK_DATA",
        100 => "INVALID_PROTOCOL",
        101 => "INVALID_CMD",
        102 => "INVALID_SRC",
        103 => "INVALID_DST",
        104 => "INVALID_DUPLICATED",
        110 => "INVALID_BUFFERFULL",
        200 => "INVALID_TASK_WORKID",
        201 => "INVALID_TASK_TASKID",
        202 => "INVALID_TASK_WORKCELL",
        203 => "INVALID_TASK_TYPE",
        301 => "INVALID_TASK_POSX",
        302 => "INVALID_TASK_POSY",
        303 => "INVALID_TASK_POSZ",
        304 => "INVALID_TASK_POSG",
        305 => "INVALID_CELL_POSZ",
        310 => "INVALID_TASK_BLENDUP",
        311 => "INVALID_TASK_BLENDDOWN",
        320 => "INVALID_DRAGOUT_USE",
        321 => "INVALID_DRAGOUT_HEIGHT",
        322 => "INVALID_DRAGOUT_DIST",
        323 => "INVALID_DRAGOUT_DIR",
        330 => "INVALID_DRAGIN_USE",
        331 => "INVALID_DRAGIN_HEIGHT",
        332 => "INVALID_DRAGIN_DIST",
        333 => "INVALID_DRAGIN_DIR",
        340 => "INVALID_GRIP_DELTA",
        401 => "INVALID_CELL_ID_EMPTY",
        409 => "INVALID_AREA_UNDEFINE",
        410 => "INVALID_STATION_SECTION",
        411 => "INVALID_STATION_RANGE_X",
        412 => "INVALID_STATION_RANGE_Y",
        413 => "INVALID_STATION_RANGE_Z",
        418 => "INVALID_STATION_USE",
        419 => "INVALID_STATION_NO_RESISTRY",
        420 => "INVALID_CELL_SECTION",
        421 => "INVALID_CELL_RANGE_X",
        422 => "INVALID_CELL_RANGE_Y",
        423 => "INVALID_CELL_RANGE_Z",
        428 => "INVALID_CELL_USE",
        429 => "INVALID_CELL_NO_RESISTRY",
        501 => "INVALID_ITEM_CODE",
        502 => "INVALID_ITEM_COUNT",
        900 => "INVALID_UNDEFINED",
        _ => "UNKNOWN",
    }
}

/// Station ids are 2001..2999 with `id MOD 100 ∈ 1..32`.
pub fn is_station_id(id: u16) -> bool {
    (STATION_ID_MIN..=STATION_ID_MAX).contains(&id) && (1..=STATION_MAX as u16).contains(&(id % 100))
}
