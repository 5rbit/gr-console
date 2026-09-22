// gr-console 백엔드 REST/SSE 페이로드 타입 — **공유 계약**이다(리드 소유).
//
// 이름은 백엔드와 1:1이다. PLC를 그대로 비추는 페이로드(`Plc*`·`WebMon`·`MeasLog*`)는 PLC의
// PascalCase 필드명을 **그대로** 둔다(오타처럼 보이는 `LowerBidHeight`·`Lenth`도 PLC DB 이름이다).
// 콘솔이 소유하는 레지스트리(`Item`·`Cell`·`Station`·`Task`…)는 snake_case다.

import type { RecSample } from './record/analysis'

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
  /** 설정 이름(`GR1`) — S7 만. */
  name?: string
  /** `gr` / `grm` — S7 만. */
  role?: 'gr' | 'grm'
  label: string
  kind: 'opcua' | 's7'
  endpoint: string
  connected: boolean
  rtt_ms: number | null
  last_ok_at: string | null
  last_error: string | null
  /** S7 통신 품질 — 폴링 실패·재연결 횟수, 주기별 마지막/최대 한 바퀴(ms). OPC UA 줄·구버전에는 없다. */
  comm?: {
    errors: number
    reconnects: number
    cycle_ms: Record<string, number>
    cycle_max_ms: Record<string, number>
    /** 하트비트(2 Hz)가 마지막으로 바뀐 뒤 ms — 감시하는 PLC 만. 3 s 넘으면 PLC 프로그램 정지. */
    heartbeat_age_ms?: number | null
    /** 느린 주기 작업이 다음 주기까지 못 끝나 건너뛴 횟수. */
    slow_overruns?: number
    /** OPC UA 줄: 등록 노드 수(RegisterNodes) · 서버 한도 · 쓰기 호출·노드 수. */
    registered?: number
    max_nodes_per_write?: number
    /** 세션 읽기 검증을 통과한 구조체 멤버(TaskData) / 실제로 구조체로 쓰는 멤버 / 검증 실패·거절 사유. */
    struct_verified?: string[]
    struct_active?: string[]
    struct_note?: string | null
    struct_writes?: number
    write_calls?: number
    write_nodes?: number
  }
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
  /** 콘솔 소유 부가 규격(PLC 로 가지 않는다) — 백엔드 `registry::spec::ItemSpec`. 옛 응답엔 없을 수 있다. */
  spec?: ItemSpec
  updated_at: string
}
/** `spec` 을 빼고 보내면 서버가 저장된 규격을 그대로 둔다. */
export type ItemUpsert = Omit<Item, 'updated_at'>

/** 곡선 점의 출처. */
export type BeadSource = 'measured' | 'manual' | 'computed'
/** 실제로 쓴 값의 출처(보간까지). */
export type ResolvedSource = BeadSource | 'interpolated'

/**
 * 하중(`Above` = 위에 얹힌 개수) 기준 비드 곡선의 한 점 — **정본**.
 * 비드·높이는 모두 **그 타이어 자기 바닥 기준**(mm)이다.
 */
export interface AboveRow {
  above: number
  lower_bead: number | null
  upper_bead: number | null
  /** 그 하중에서 타이어 하나가 차지하는 높이(mm) */
  pressed_height: number | null
  source: BeadSource
  sample_plc: string
  sample_seq: number
  updated_at: string
}

/**
 * 잰 스택 한 단의 **셀 바닥 기준 절대** 측정값 — 정본(백엔드 `spec::ProfileRow`).
 */
export interface ProfileRow {
  /** 1-based 단 번호(맨 아래가 1) */
  level: number
  lower_bead: number | null
  upper_bead: number | null
  /** 이 단 타이어 윗면까지(셀 바닥 기준). 맨 윗단은 표본의 TotalHeight */
  stack_height: number | null
  /** `measured` | `manual`(손으로 고친 행 — 다시 재도 덮지 않는다) */
  source: BeadSource
}

/**
 * **정본** — 스택 크기 `count` 를 통째로 잰 결과. 같은 크기를 다시 재면 통째로 바뀐다(manual 행만 남는다).
 * 크기가 다른 프로파일은 나란히 산다(n = 3 · 5 · 8 …).
 */
export interface BeadProfile {
  count: number
  rows: ProfileRow[]
  total_height: number | null
  each_height: number | null
  sample_plc: string
  sample_seq: number
  at: string
}

export interface ItemSpec {
  /** 셀 최대 단수(0 = 제한 없음, ≤ 20) */
  stack_max: number
  /** 팔레트 최대 개수(0 = 제한 없음) */
  pallet_max: number
  weight_kg: number | null
  /** 잰 스택 크기별 절대 프로파일(정본). 하중 곡선은 여기서 파생된다 */
  profiles: BeadProfile[]
  bead_source: string
  /** pick_bead 그립에서 상부 비드 아래로 내려잡는 양(mm). null = 기본 30 */
  pick_bead_offset: number | null
  /** 잰 적 없는 크기를 위한 **대체** 눌림양(mm/개). null = 0 */
  compression: number | null
  compression_source: string
  /** SKU 측정이 들어오면 프로파일을 자동으로 갱신한다. null = 켬(기본) */
  auto_apply_measured: boolean | null
}

/** 어느 SKU 표본이 그 점을 채웠는지. */
export interface SampleRef {
  plc: string
  seq: number
}

/** 하중 하나에 대해 실제로 쓰는 값(측정 → 보간 → 계산). */
export interface CurveValue {
  above: number
  upper_bead: number | null
  lower_bead: number | null
  pressed_height: number
  source: ResolvedSource
  /** **상부 비드만** 의 출처 — `computed` 면 공칭값을 지어낸 것이라 그립은 mid 로 내려간다 */
  upper_bead_source: ResolvedSource
  sample: SampleRef | null
}

/** `GET /api/items/{code}/bead-samples` 의 한 줄. */
export interface BeadSample {
  plc: string
  seq: number
  code: number
  at: string
  status: number
  sku_status: number
  diag_flags: number
  layer_offset: number
  /** 잰 스택의 단수 — k 단 타이어 위에는 total_count − k 개가 있었다 */
  total_count: number
  each_height: number
  total_height: number
  levels: LevelValues[]
  /** 값이 있는 단 수 */
  filled: number
  /** 규격에 반영했는가 */
  applied: boolean
  /** 반영 내용 또는 거부 사유 */
  reason: string
  recorded_at: string
  /** 지금 규칙으로 봐도 쓸 수 있는 표본인가 */
  valid: boolean
}

/** 측정 스택 하나의 단별 프로파일(`GET .../levels` 의 `profile`). */
export interface ProfilePoint {
  level: number
  above: number
  bottom: number | null
  pressed_height: number | null
  lower_bead: number | null
  upper_bead: number | null
  compression_at: number | null
}

export interface StackProfile {
  count: number
  points: ProfilePoint[]
  pitch_sum: number | null
  total_height: number | null
  warnings: string[]
}

export interface LevelValues {
  lower_bead: number | null
  upper_bead: number | null
  stack_height: number | null
}

/** 미리보기 스택 `n` 에서 본 한 단 — 행의 키는 `above`, `level` 은 `n − above` 파생값. */
export interface LevelView {
  level: number
  above: number
  /** 곡선에 저장된 그대로(점이 없으면 null) */
  configured: AboveRow | null
  /** 실제로 쓰는 값 */
  effective: CurveValue
  /** 그 하중에서 먹은 총 눌림(mm) = Height − PressedHeight */
  compression_at: number | null
  /** 미리보기 스택에서 이 타이어 바닥이 서는 높이(셀 바닥 기준) */
  bottom: number
  abs_lower_bead: number | null
  abs_upper_bead: number | null
  /** 눌림 없는 공칭 절대값 */
  computed: LevelValues
  measured: LevelValues | null
  deviation: LevelValues | null
  /** 이 단을 집을 때 실제로 쓸 그립 Z(셀 바닥 기준) */
  pick_z: number | null
  /** 그 Z 가 쓴 기준 — `pick_bead`(잰 비드 − PickBeadOffset) | `mid`(측정 없음 → Height/2) */
  pick_z_ref: GripRef
  source: ResolvedSource
  sample: SampleRef | null
  /** 이 크기의 스택을 통째로 잰 프로파일의 값인가(아니면 환산·보간값 — 화면은 흐리게) */
  from_profile: boolean
}

/** 측정에서 되짚은 눌림양 제안(`GET /api/items/{code}/levels`). */
export interface CompressionSuggestion {
  value: number | null
  method: 'beads' | 'each_height' | null
  from_beads: number | null
  from_each_height: number | null
  samples: number
  measured_count: number
}

/** `GET /api/items/{code}/levels?robot=` */
export interface ItemLevels {
  code: number
  name: string
  height: number
  eff_height: number
  lower_bead_height: number
  upper_bead_height: number
  deflection_factor: number
  /** 콘솔 계산에 처짐 계수를 적용했는가(지금은 항상 false) */
  deflection_applied: boolean
  spec: ItemSpec
  rows: LevelView[]
  warnings: string[]
  /** 실제로 쓰는 값(빈 칸의 기본값이 적용된 뒤) */
  pick_bead_offset: number
  compression: number
  /** 단별 보기가 "가득 쌓았다"고 보는 개수(StackMax) */
  reference_stack: number
  suggestion: CompressionSuggestion
  /** SKU 측정 자동 반영 스위치(실제로 쓰는 값) */
  auto_apply_measured: boolean
  /** 최신 표본으로 푼 스택 프로파일 */
  profile: StackProfile
  /** `rows` 가 몇 개 스택 기준인가 */
  preview: number
  /** 파생 곡선이 값을 든 하중 목록(오름차순) */
  measured_aboves: number[]
  /** 통째로 잰 스택 크기 목록(오름차순) */
  measured_counts: number[]
  /** 최근 SKU 측정 표본(최신 순) */
  bead_samples: BeadSample[]
  /** 이 코드의 최신 SKU 측정(MEASLOG_HIST) — 없으면 null */
  measured: {
    plc: string
    seq: number
    at: string
    count: number
    each_height: number
    stack_height: number
  } | null
  /** MEASLOG.ByCode 추세(스냅샷이 있을 때) */
  measured_summary: {
    plc: string
    count: number
    last_time: string
    upper_bead: Trend | null
    tire_height: Trend | null
    each_height: Trend | null
    stack_height: Trend | null
    pick_bead_pos: Trend | null
  } | null
}

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

/**
 * `/api/cells/bulk` 병합 모드 — 기본은 `append`(아무것도 지우지 않고 충돌만 건너뛴다).
 * `replace_section` 만 그 구역의 기존 셀을 먼저 지우고, `overwrite` 는 충돌해도 id 가 이긴다.
 */
export type CellBulkMode = 'append' | 'replace_section' | 'overwrite'
export type CellBulkOutcome = 'added' | 'updated' | 'unchanged' | 'skipped' | 'remapped'

/** 요청 한 줄의 결과 — `id` 는 보낸 id, `new_id` 는 `auto_id` 로 옮긴 id. */
export interface CellBulkRow {
  id: number
  outcome: CellBulkOutcome
  reason?: string
  new_id?: number
}

export interface CellBulkResult {
  mode: CellBulkMode
  auto_id: boolean
  created: number
  updated: number
  unchanged: number
  skipped: number
  remapped: number
  removed: number
  total: number
  rows: CellBulkRow[]
  warnings?: string[]
}

export interface CellBulkOptions {
  mode?: CellBulkMode
  /** `replace_section` 대상 구역. */
  section?: number | null
  /** id 가 겹치면 그 구역의 빈 id 로 옮긴다(`append` 에서만 뜻이 있다). */
  autoId?: boolean
  /** 겹침 판정 지름(mm) — 없으면 서버가 셀의 `max(length, width)` 를 쓴다. */
  diameter?: number
}

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

/** MOVE 작성 방식 — 요청 `params.move_mode`(백엔드 `issue::MoveMode`). stack = 스택 윗면 + 여유까지 하강, top = Z 9999(상단 유지 XY 이동), avoid = Avoid + Z 9999(상단에서 X 만). */
export type MoveMode = 'stack' | 'top' | 'avoid'

/** MEASURE 측정 종류 — 요청 `params.measure_item` / `params.measure_sku`. 순차 계획은 비우면 재고로 고른다. */
export type MeasureMode = 'item' | 'sku'

/** 작업 요청 `params` — 튜닝 값(부분) + MOVE 옵션(`TaskParams` 밖의 키라 백엔드가 원본 JSON 에서 읽는다). */
export type TaskRequestParams = Partial<TaskParams> & {
  move_mode?: MoveMode
  move_clearance?: number
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

/**
 * 그립 기준 — 타이어 바닥에서 그리퍼가 잡는 높이: `mid` = Height/2(기본),
 * `pick_bead`(화면 이름 `bead+offset`) = UpperBead − PickBeadOffset(기본 30 mm).
 *
 * `pick_bead` 는 **잰 비드가 있을 때만** 쓴다 — 한 번도 안 잰 품목은 mid(Height/2)로 내려간다.
 * 비드는 그 타이어 **위에 얹힌 개수**만큼 Compression 이 반영된 값이다.
 * 옛 값 `bead`(비드를 그대로 잡기)는 없어졌고, 읽을 때 `pick_bead` 로 옮겨진다.
 */
export type GripRef = 'mid' | 'pick_bead'

export interface Defaults {
  version: number
  updated_at: string
  base: TaskParams
  by: Record<'PICK' | 'DROP', Record<TargetKind, Partial<TaskParams>>>
  grip_ref: GripRef
  /** 상황별 덮어쓰기 — 종류·대상별 위, 작성 카드 덮어쓰기 아래(백엔드 `Defaults.situations`). 구버전 응답엔 없다. */
  situations?: Partial<Record<Situation, Partial<TaskParams>>>
  /** 읽을 때 서버가 걷어 낸 것(깨진 값·모르는 키) — 있으면 대화상자가 알린다. 저장하면 사라진다. */
  load_warnings?: string[]
}

/** 기본값 변경 이력 한 줄(`GET /api/defaults/history`). */
export interface DefaultsHistoryRow {
  id: number
  version: number
  at: string
  note: string
  changes: string[]
}

/** 기본값 가져오기 결과(`POST /api/defaults/import`) — 문제·버린 키가 있으면 저장하지 않는다. */
export interface DefaultsImportResult {
  saved: boolean
  changes: string[]
  problems: string[]
  dropped: string[]
  version?: number
}

/** 상황 키 — 적용 순서(뒤가 이긴다). 백엔드 `registry::SITUATIONS` 와 같다. */
export type Situation = 'measure_item' | 'measure_sku' | 'pallet_station' | 'multi_pick'

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

/** 로봇 운전 명령 (POST /api/robots/{id}/command/{action}). */
export type RobotAction = 'start' | 'stop' | 'reset' | 'buzzerstop' | 'complete' | 'clear'

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
  params: TaskRequestParams
  position_override: [number, number, number, number] | null
  note: string
  source: { scenario_id: string; run_id: string; iteration: number; step_index: number } | null
  /** 보낼 로봇(없으면 기본 로봇) */
  robot?: number | null
  /** 그립 기준 덮어쓰기(없으면 Defaults.grip_ref) */
  grip_ref?: GripRef | null
  /** 스테이션 보정(GRM StationCenterAdjust 재현) — 없으면 켜짐. `'off'`/`false` 로 끈다. */
  station_offset?: 'auto' | 'off' | boolean | null
  /** 셀 단수 Max 를 넘는 DROP 도 제출(기본은 409 거부) */
  ignore_stack_max?: boolean
  /** 팔렛 슬롯 — 켜진 팔렛 프로파일이 있는 스테이션 대상에만(백엔드 `pallet::compose::PalletRef`). */
  pallet?: PalletRef | null
  /** Multi-Picking 상황(스테이션 PICK/DROP) — 기본값 `situations.multi_pick` 층을 얹는다. */
  multi_pick?: boolean | null
}

/** `{seq, level}`(1-based) 또는 `{auto: true}`(스테이션 재고로 다음 슬롯). */
export interface PalletRef {
  seq?: number | null
  level?: number | null
  auto?: boolean
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
  /** 출처 — 콘솔·시나리오·외부(PLC 에서 처음 본 Task). */
  origin?: Task['origin']
  /** RFC 3339 — `created_at` 하한. */
  since?: string
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
  /** 막는 사유 — 백엔드가 `GR1: AUTO 모드가 아님` 처럼 **로봇 이름을 달아** 낸다(다시 꾸미지 않는다). */
  reasons: string[]
  /** 이 게이트가 말하는 로봇과 그 상태 PLC(`/api/tasks/gate`·`/api/robots`). 구버전 응답에는 없다. */
  robot?: string
  plc?: string
}

// ── 시나리오 ──────────────────────────────────────────────────────────────────

export interface ScenarioStep {
  id: string
  label: string
  type: TaskType
  target: Target | null
  item_code: number | null
  count: number
  params: TaskRequestParams
  wait_for: 'accepted' | 'completed'
  wait_after_ms: number
  on_failure: 'stop' | 'skip' | 'retry'
  note: string
  /** 보낼 로봇(없으면 기본 로봇) */
  robot?: number | null
  /** 팔렛 슬롯(`TaskRequest.pallet` 그대로) — JSON 에만 실린다(CSV 열 없음). */
  pallet?: PalletRef | null
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
  /** 실행 로봇 — 로봇을 지정하지 않은 스텝이 여기로 간다 */
  robot?: number | null
  robot_name?: string | null
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
  Gripper: {
    ItemDetect: boolean
    GID: number[]
    FLD: number
    TorqueReachedPosition: number
    State?: GripperState
  }
  Measure: {
    Item: Record<string, number | boolean>
    Sku: Record<string, number | boolean | number[]>
    Floor: Record<string, number | boolean>
    LastBead: Record<string, number | boolean>
  }
  MeasLog: { Total: number; Count: number }
}

/** "MACHINE".Gripper.State (WEBMON 복사) — GripperState FC 가 매 스캔 갱신. */
export interface GripperState {
  Commanded: boolean
  Stopped: boolean
  AtCommand: boolean
  TorqueReached: boolean
  TorqueStop: boolean
  Stall: boolean
  StallNoTorque: boolean
  StallTime: number
}

export interface StatusEvent {
  at: string
  source: 'plc' | 'demo'
  seq: number
  /** 이 이벤트를 낸 상태 PLC 이름(`GR1`) */
  plc?: string
  /** 로봇 id — 스트림은 로봇마다 따로다(`/api/status/stream?robot=`) */
  robot?: number
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
  /** 상태 PLC 이름 */
  plc?: string
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
  plc?: string
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
  /** `profile`(잰 절대값 그대로) | `curve`(환산·보간) | `computed` */
  z_source?: string
  level?: number
  stack_max?: number | null
}

/** LASERDIAG.Sensor[k] (LGR_LaserSensorHealth) */
export interface LaserSensorHealth {
  BiasEma: number
  BiasSamples: number
  LastDev: number
  NoRespConsec: number
  UnstableConsec: number
  BiasAlarm: boolean
  NoRespAlarm: boolean
  UnstableAlarm: boolean
}

/** LASERDIAG.ZCal (LGR_LaserZCal) */
export interface LaserZCal {
  Enable: boolean
  Busy: boolean
  Done: boolean
  Error: boolean
  ErrorCode: number
  Target: number
  Count: number
  Skipped: number
  Sum: number[]
  SumSq: number[]
  Mean: number[]
  StdDev: number[]
  OldOffset: number[]
  NewOffset: number[]
  EnableMm: boolean
}

/** LASERDIAG.Entry[i] (LGR_LaserDiagEntry) */
export interface LaserDiagEntry {
  TimeStamp: string
  Seq: number
  Source: number
  Code: number
  Valid: boolean
  FitError: number
  DiagFlags: number
  BeadDev: number[]
  EdgeHeight: number[]
  Samples: number[]
  Jumps: number[]
  Spread: number
  PlanarResidual: number
  ZOffset: number[]
}

/** GET /api/laser — 최신 진단 이력이 앞, para 는 레이저 관련 PARA.Sensor 멤버만. */
export interface LaserSnapshot {
  robot?: number
  plc: string
  at: string
  total: number
  count: number
  sensor: LaserSensorHealth[]
  zcal: LaserZCal
  entries: LaserDiagEntry[]
  para: Record<string, number>
}

// ── PARA (/api/para?robot=) ─────────────────────────────────────────────────────
// 선택한 로봇 PLC 의 PARA DB — 계약 선언 주석·형식과 함께. 읽기 전용.

export type ParaScalar = number | boolean | string

export interface ParaRow {
  /** `Sensor.GIDL_ZOffset` */
  path: string
  name: string
  ty: string
  comment: string
  /** 주석의 `[pNNN]` 번호 */
  param: number | null
  /** 그룹 안 깊이(0 = 최상위 구조체의 직접 멤버) */
  depth: number
  /** false = 하위 구조체 머리줄(값 없음) */
  leaf: boolean
  value: ParaScalar | ParaScalar[] | Record<string, unknown> | Record<string, unknown>[] | null
}

export interface ParaGroup {
  name: string
  ty: string
  comment: string
  rows: ParaRow[]
}

export interface ParaSnapshot {
  robot: number
  robot_name: string
  plc: string
  contract: string
  at: string
  seq: number
  groups: ParaGroup[]
}

// ── 측정 기록 (/api/record) ─────────────────────────────────────────────────────
// 콘솔 소유 레코드라 snake_case. 스냅샷 안의 PLC 값(PARA 멤버, LASERDIAG, MEASLOG Last)은 PLC 이름 그대로.

export interface RecordStart {
  label: string
  kind: string
  note: string
  rate_ms: number
  /** 기록할 로봇(없으면 기본 로봇). 기록은 전체에서 한 번에 하나. */
  robot?: number | null
}

/** MEASLOG.Last 항목 (LGR_MeasureLog) 중 기록 분석에 쓰는 부분 */
export interface RecordMeasLog {
  Seq: number
  Kind: number
  Status: number
  Data: number[]
  Delta: { InnerDia: number; Height: number; Z: number; Offset: number; Count: number }
  Cmd?: { Item?: { Code: number; InnerDiameter: number } }
}

export interface RecordSnapshot {
  at: string
  para_task: Record<string, number>
  para_sensor: Record<string, number>
  laser: { Total: number; Sensor: LaserSensorHealth[]; ZCal: LaserZCal } | null
  /** 끝 스냅샷만 : 기록 중 새로 생긴 LASERDIAG.Entry (오래된 것부터) */
  laser_entries: LaserDiagEntry[] | null
  measlog_total: number | null
  /** 끝 스냅샷만 : 기록 중 갱신된 MEASLOG.Last 종류별 항목 */
  measlog_last: RecordMeasLog[] | null
}

export interface RecordMeta {
  id: string
  label: string
  kind: string
  note: string
  plc: string
  /** 기록한 로봇 id(여러 대 지원 이전 기록은 없음) */
  robot?: number | null
  rate_ms: number
  started_at: string
  stopped_at: string | null
  samples: number
  marks: number
  read_errors: number
  start: RecordSnapshot | null
  end: RecordSnapshot | null
}

export interface RecordOverview {
  active: RecordMeta | null
  sessions: RecordMeta[]
}

export interface RecordSession {
  meta: RecordMeta
  samples: RecSample[]
}
