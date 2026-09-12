// gr-console 백엔드 REST/SSE 페이로드 타입 — **공유 계약**이다(리드 소유).
//
// 이름은 백엔드와 1:1이다. PLC를 그대로 비추는 페이로드(`Plc*`·`WebMon`·`MeasLog*`)는 PLC의
// PascalCase 필드명을 **그대로** 둔다(오타처럼 보이는 `LowerBidHeight`·`Lenth`도 PLC DB 이름이다).
// 콘솔이 소유하는 레지스트리(`Item`·`Cell`·`Station`·`Task`…)는 snake_case다.

// ── 콘솔·PLC 연결 ─────────────────────────────────────────────────────────────

export interface ConsoleInfo {
  app_id: string
  app_name: string
  tabs: string[]
  default_tab: string
}

export type PlcId = 'grm_opcua' | 'gr2_s7' | 'grm_s7'

export interface LayoutCheck {
  ok: boolean | null
  detail: string | null
  checked_at: string | null
  mismatches: { db: string; kind: string; expected: string; actual: string }[]
}

export interface PlcStatus {
  id: PlcId
  label: string
  kind: 'opcua' | 's7'
  endpoint: string
  connected: boolean
  rtt_ms: number | null
  last_ok_at: string | null
  last_error: string | null
  layout: LayoutCheck
}

// ── 레지스트리(품목·셀·스테이션) ──────────────────────────────────────────────

export interface Item {
  code: number
  name: string
  count: number
  inner_diameter: number
  outer_diameter: number
  lower_bead_height: number
  upper_bead_height: number
  height: number
  deflection_factor: number
  note: string
  updated_at: string
}
export type ItemUpsert = Omit<Item, 'updated_at'>

export interface Cell {
  id: number
  use: boolean
  blend_use: boolean
  section: number
  row: number
  col: number
  length: number
  width: number
  position: [number, number, number]
  source: 'plc' | 'local'
  dirty: boolean
  updated_at: string
}
export type CellUpsert = Omit<Cell, 'updated_at' | 'source' | 'dirty'>

export interface SensorSettings {
  io_link_master_module: number
  io_link_master_port_l: number
  io_link_master_port_r: number
  detection_factor: number
  allow_range: number
  l_sensor_offset: number
  r_sensor_offset: number
}

export interface Station {
  id: number
  conv_no: number
  task_type: number
  rotate_type: number
  group: number
  group_index: number
  connection_prev: number
  connection_next: number
  info: Omit<Cell, 'source' | 'dirty' | 'updated_at'>
  sensor: SensorSettings
  io_block_no: number
  source: 'plc' | 'local'
  dirty: boolean
  updated_at: string
}
export type StationUpsert = Omit<Station, 'updated_at' | 'source' | 'dirty'>

export interface ImportResult {
  imported: number
  updated: number
  removed: number
  skipped: number
  errors: { row: number; message: string }[]
}

export interface DiffRow<T> {
  id: number
  status: 'same' | 'local_only' | 'plc_only' | 'changed'
  local: T | null
  plc: T | null
}

// ── 작업(Task) ────────────────────────────────────────────────────────────────

export type TaskType = 'UP' | 'PICK' | 'DROP' | 'MOVE' | 'MEASURE'
export type TargetKind = 'cell' | 'station'

export interface Target {
  kind: TargetKind
  id: number
}

export interface TaskParams {
  lift_up_height: number
  grip_height: number
  pre_grip_delta: number
  grip_back_delta: number
  blend_up_distance: number
  blend_down_distance: number
  lift_up_creep_distance: number
  lift_down_creep_distance: number
  lift_up_after_complete: boolean
  lift_up_partial: boolean
  measure_floor: boolean
  measure_item: boolean
  measure_sku: boolean
  adjust_center: boolean
  find_station_item: boolean
  avoid: boolean
  outbound: boolean
  use_drag_out: boolean
  drag_out_height: number
  drag_out_dist: number
  drag_out_dir: number
  use_drag_in: boolean
  drag_in_height: number
  drag_in_dist: number
  drag_in_dir: number
}

/** 그립 기준 — 타이어 바닥에서 그리퍼가 잡는 높이: mid = Height/2, bead = UpperBidHeight(없으면 mid) */
export type GripRef = 'mid' | 'bead'

export interface Defaults {
  version: number
  updated_at: string
  base: TaskParams
  by: Record<'PICK' | 'DROP', Record<TargetKind, Partial<TaskParams>>>
  grip_ref: GripRef
}

/** GRM 뒤의 로봇 한 대 (GET /api/robots). */
export interface Robot {
  id: number
  name: string
  plc: string
  opcua_root: string
  dst: number
  default: boolean
  cmd_ready: boolean
  cmd_error: string | null
  plc_connected: boolean
  layout_ok: boolean | null
  gate: Gate
  active_tasks: number
}

export type TaskState =
  | 'draft'
  | 'submitted'
  | 'accepted'
  | 'rejected'
  | 'queued'
  | 'running'
  | 'completed'
  | 'canceled'
  | 'failed'
  | 'lost'

export interface TaskAck {
  accepted: boolean
  code: number
  reason: string
  reject_bits: number
  at: string
}

export interface TaskTransition {
  from: TaskState | null
  to: TaskState
  at: string
  by: 'ui' | 'plc' | 'scenario' | 'system'
  note: string | null
}

export interface TaskRequest {
  type: TaskType
  target: Target | null
  item_code: number | null
  count: number
  params: Partial<TaskParams>
  position_override: [number, number, number, number] | null
  note: string
  source: { scenario_id: string; run_id: string; iteration: number; step_index: number } | null
  /** 보낼 로봇(없으면 기본 로봇) */
  robot?: number | null
  /** 그립 기준 덮어쓰기(없으면 Defaults.grip_ref) */
  grip_ref?: GripRef | null
}

export interface Task {
  id: string
  seq: number
  work_id: number
  task_id: number
  origin: 'console' | 'scenario' | 'external'
  /** 로봇 상태 PLC 이름(= 로봇 이름) */
  plc_name?: string
  request: TaskRequest | null
  resolved: TaskParams | null
  position: [number, number, number, number]
  plc_task: PlcTask | null
  state: TaskState
  state_at: string
  created_at: string
  submitted_at: string | null
  ended_at: string | null
  ack: TaskAck | null
  plc: { step: number; queue_index: number | null; last_seen_at: string } | null
  error: string | null
  history: TaskTransition[]
}

export interface TaskPage {
  items: Task[]
  total: number
  limit: number
  offset: number
}

export interface TaskQuery {
  robot?: number
  state?: TaskState[] | 'active' | 'terminal'
  type?: TaskType
  q?: string
  limit?: number
  offset?: number
}

export type TasksEvent =
  | { kind: 'snapshot'; tasks: Task[] }
  | { kind: 'upsert'; task: Task }
  | { kind: 'remove'; id: string }

export interface Gate {
  can_submit: boolean
  reasons: string[]
}

// ── 시나리오 ──────────────────────────────────────────────────────────────────

export interface ScenarioStep {
  id: string
  label: string
  type: TaskType
  target: Target | null
  item_code: number | null
  count: number
  params: Partial<TaskParams>
  wait_for: 'accepted' | 'completed'
  wait_after_ms: number
  on_failure: 'stop' | 'skip' | 'retry'
  note: string
  /** 보낼 로봇(없으면 기본 로봇) */
  robot?: number | null
}

export interface Scenario {
  id: string
  name: string
  description: string
  steps: ScenarioStep[]
  repeat: number
  created_at: string
  updated_at: string
}
export type ScenarioUpsert = Omit<Scenario, 'id' | 'created_at' | 'updated_at'> & { id?: string }

export type RunState = 'idle' | 'running' | 'paused' | 'stopping' | 'stopped' | 'done' | 'failed'

export interface StepResult {
  iteration: number
  step_index: number
  task_id: string | null
  state: TaskState
  ack: TaskAck | null
  started_at: string
  ended_at: string | null
}

export interface ScenarioRun {
  run_id: string
  scenario_id: string
  scenario_name: string
  state: RunState
  iteration: number
  total_iterations: number | null
  step_index: number
  step_count: number
  current_task_id: string | null
  started_at: string
  ended_at: string | null
  error: string | null
  results: StepResult[]
}

// ── PLC 미러 페이로드 — PLC PascalCase 그대로 ─────────────────────────────────

export interface PlcItem {
  Code: number
  Count: number
  InnerDiameter: number
  OuterDiameter: number
  LowerBidHeight: number
  UpperBidHeight: number
  Height: number
  DeflectionFactor: number
}

export interface PlcCell {
  Use: boolean
  BlendUse: boolean
  Id: number
  Section: number
  Row: number
  Col: number
  Lenth: number
  Width: number
  Position: number[]
}

export interface PlcTask {
  WorkId: number
  TaskId: number
  TaskType: number
  Position: number[]
  Item: PlcItem
  Cell: PlcCell
  BlendUpDistance: number
  BlendDownDistance: number
  DragOutHeight: number
  DragOutDist: number
  DragOutDir: number
  DragInHeight: number
  DragInDist: number
  DragInDir: number
  LiftUpCreepDistance: number
  LiftDownCreepDistance: number
  LiftUpHeight: number
  PreGripDelta: number
  GripBackDelta: number
  GripHeight: number
  UseDragOut: boolean
  UseDragIn: boolean
  LiftUpAfterComplete: boolean
  LiftUpPartial: boolean
  MeasureFloor: boolean
  MeasureItem: boolean
  MeasureSku: boolean
  AdjustCenter: boolean
  FindStationItem: boolean
  Avoid: boolean
  Outbound: boolean
}

export interface PlcAxis {
  Position: number
  Target: number
  Speed: number
  Torque: number
  LagError: number
  Ready: boolean
  Enabled: boolean
  Running: boolean
  StandStill: boolean
  MainCode: number
  SubCode: number
}

export interface PlcDrive {
  MainCode: number
  SubCode: number
  OpMode: number
  Speed: number
  Torque: number
  Position: number
  JerkTime: number
  TorqLimit: number
  StartPos: number
  Direction: number
}

export interface WebMon {
  UpdateTime: string
  Mode: number
  Stat: {
    NTP: string
    ComponentID: number
    RES: {
      Header: {
        Protocol: number
        CMD_ID: number
        CMD: number
        SRC: number
        DST: number
        SEQ: number
      }
      Data: number[]
    }
    Mode: Record<string, boolean | number>
    Status: Record<string, boolean | number>
    StatusCode: { Main: number; Sub: number }
    Task: {
      Status: Record<string, boolean | number>
      Now: PlcTask
      Queue: PlcTask[]
      Completed: PlcTask[]
      Canceled: PlcTask[]
      Rejected: PlcTask[]
    }
    Measuring: {
      SKU: { WorkCell: number; Code: number; Data: number[] }
      Item: { WorkCell: number; Code: number; Data: number[] }
      Floor: { WorkCell: number; Data: number }
    }
    Interlock: {
      WorkCell: number
      WorkType: number
      PI: Record<string, boolean>
      PO: Record<string, boolean>
    }
    Drive: PlcDrive[]
  }
  Proc: {
    ProcNo: number
    Step: { Now: number; Prev: number; ElapseTime: number }
    Option: { Logging: boolean }
    Msg: string
  }
  Alarm: {
    Fault: boolean
    Warn: boolean
    FaultCode: number[]
    WarnCode: number[]
    EventCode: number[]
    TaskCode: number[]
  }
  Axis: PlcAxis[]
  Gripper: { ItemDetect: boolean; GID: number[]; FLD: number; TorqueReachedPosition: number }
  Measure: {
    Item: Record<string, number | boolean>
    Sku: Record<string, number | boolean | number[]>
    Floor: Record<string, number | boolean>
    LastBead: Record<string, number | boolean>
  }
  MeasLog: { Total: number; Count: number }
}

export interface StatusEvent {
  at: string
  source: 'plc' | 'demo'
  seq: number
  webmon: WebMon
}

// ── 측정 로그 ─────────────────────────────────────────────────────────────────

export interface Trend {
  Count: number
  Last: number
  Avg: number
  Ema: number
  Min: number
  Max: number
}

export interface MeasStat {
  Count: number
  ErrorCount: number
  InnerDia: Trend
  Height: Trend
  Z: Trend
  Offset: Trend
  StackCount: Trend
}

export type MeasKey =
  | 'InnerDia'
  | 'LaserInnerDia'
  | 'TorqInnerDia'
  | 'UpperBeadHeight'
  | 'TireHeight'
  | 'Offset'
  | 'PickBeadPos'
  | 'PickInnerDia'
  | 'SkuEachHeight'
  | 'SkuStackHeight'

export interface ByCode {
  Code: number
  Item: PlcItem
  LastTime: string
  Count: number
  Stat: MeasStat[]
  Meas: Record<MeasKey, Trend>
}

export interface MeasLogEntry {
  TimeStamp: string
  Seq: number
  Kind: number
  Status: number
  Cmd: PlcTask
  Data: number[]
  Delta: { InnerDia: number; Height: number; Z: number; Offset: number; Count: number }
}

export interface MeasLogSnapshot {
  at: string
  head: number
  count: number
  total: number
  capacity: number
  stat: MeasStat[]
  last: (MeasLogEntry | null)[]
  by_code: ByCode[]
}

export interface MeasLogEntries {
  total: number
  entries: MeasLogEntry[]
}

// ── 셀 재고(콘솔 소유) ───────────────────────────────────────────────────────

export interface StockEntry {
  cell_id: number
  /** 0 = 비어 있음/품목 미상 */
  item_code: number
  count: number
  note: string
  updated_at: string
}

export type StockEvent =
  | { kind: 'snapshot'; stock: StockEntry[] }
  | { kind: 'upsert'; entry: StockEntry; reason: string }
  | { kind: 'remove'; cell_id: number }

/** `GET /api/stock/z` — 지금 재고 기준으로 백엔드가 쓸 Z */
export interface StockZ {
  cell: number
  type: TaskType
  item_code: number | null
  height: number
  stock: number
  floor: number
  z: number
}
