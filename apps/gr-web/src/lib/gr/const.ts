// GR PLC 상수 — 코드↔이름 표. PLC 쪽 정의(작업 종류·모드 비트·측정 종류)와 1:1이다.
import type { TaskParams, TaskState, TaskType } from '../types'
import type { Status } from '../ui/status'

/** 작업 종류 → PLC TaskType 코드. */
export const TASK_TYPE_CODE: Record<TaskType, number> = {
  UP: 0x40,
  PICK: 0x41,
  DROP: 0x42,
  MOVE: 0x43,
  MEASURE: 0x44,
}

/** PLC TaskType 코드 → 작업 종류(모르는 코드는 `undefined`). */
export const TASK_TYPE_OF_CODE: Record<number, TaskType | undefined> = {
  0x40: 'UP',
  0x41: 'PICK',
  0x42: 'DROP',
  0x43: 'MOVE',
  0x44: 'MEASURE',
}

export const TASK_TYPES: readonly TaskType[] = ['UP', 'PICK', 'DROP', 'MOVE', 'MEASURE']

/** 운전 모드(`WebMon.Mode`) 비트 → 이름. */
export const MODE_NAME: Record<number, string> = {
  0: 'BOOT',
  1: 'INIT',
  2: 'MAINT',
  4: 'STANDBY',
  8: 'MANUAL',
  16: 'READY',
  32: 'AUTO',
  64: 'RESERVED',
  128: 'FAULT',
  256: 'TEACH',
}

/** 모드 이름 — 표에 없는 값은 16진수로 그대로 보인다(모르는 값을 이름으로 꾸미지 않는다). */
export function modeName(mode: number): string {
  return MODE_NAME[mode] ?? `0x${mode.toString(16).toUpperCase()}`
}

/** 측정 로그 `Kind` 코드 → 이름(0은 미정의). */
export const KIND = ['', 'Item', 'Sku', 'Floor', 'Pick', 'Manual'] as const

/** 측정 로그 `Status` 코드 → 이름. */
export const STATUS = [
  'None',
  'Busy',
  'Done',
  'Error',
  'Mismatch',
  // 5~9 : Sku 스택 정렬 진단 무효 세부 (PLC MEAS_STAT_SKU_*)
  'SkuCountMismatch',
  'SkuHeightSpread',
  'SkuLayerOffset',
  'SkuLiftEnd',
  'SkuNoEdge',
  'SkuCmdCount',
] as const

/** 축 이름(인덱스 = `WebMon.Axis` 순서). */
export const AXIS = ['X', 'Y', 'Z', 'G'] as const

/** 작업 파라미터의 표시 이름 — PLC `LGR_Task_Data` 멤버 이름 그대로. */
export const PARAM_LABELS: Record<keyof TaskParams, string> = {
  lift_up_height: 'LiftUpHeight',
  grip_height: 'GripHeight',
  pre_grip_delta: 'PreGripDelta',
  grip_back_delta: 'GripBackDelta',
  blend_up_distance: 'BlendUpDistance',
  blend_down_distance: 'BlendDownDistance',
  lift_up_creep_distance: 'LiftUpCreepDistance',
  lift_down_creep_distance: 'LiftDownCreepDistance',
  lift_up_after_complete: 'LiftUpAfterComplete',
  lift_up_partial: 'LiftUpPartial',
  measure_floor: 'MeasureFloor',
  measure_item: 'MeasureItem',
  measure_sku: 'MeasureSku',
  adjust_center: 'AdjustCenter',
  find_station_item: 'FindStationItem',
  avoid: 'Avoid',
  outbound: 'Outbound',
  use_drag_out: 'UseDragOut',
  drag_out_height: 'DragOutHeight',
  drag_out_dist: 'DragOutDist',
  drag_out_dir: 'DragOutDir',
  use_drag_in: 'UseDragIn',
  drag_in_height: 'DragInHeight',
  drag_in_dist: 'DragInDist',
  drag_in_dir: 'DragInDir',
}

/** Task 상태의 한글 라벨. */
export const STATE_LABEL: Record<TaskState, string> = {
  draft: '초안',
  submitted: '제출됨',
  accepted: '수락됨',
  rejected: '거부됨',
  queued: '대기',
  running: '실행 중',
  completed: '완료',
  canceled: '취소됨',
  failed: '실패',
  lost: '유실',
}

/** Task 상태의 시맨틱 톤(`StatusDot`·`StatusBadge`). */
export const STATE_TONE: Record<TaskState, Status> = {
  draft: 'neutral',
  submitted: 'info',
  accepted: 'info',
  rejected: 'fault',
  queued: 'info',
  running: 'ok',
  completed: 'ok',
  canceled: 'neutral',
  failed: 'fault',
  lost: 'warn',
}
