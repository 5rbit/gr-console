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
export const STATUS = ['None', 'Busy', 'Done', 'Error', 'Mismatch'] as const

/** 축 이름(인덱스 = `WebMon.Axis` 순서). */
export const AXIS = ['X', 'Y', 'Z', 'G'] as const

/** 작업 파라미터의 한글 라벨. */
export const PARAM_LABELS: Record<keyof TaskParams, string> = {
  lift_up_height: '리프트 상승 높이',
  grip_height: '그립 높이',
  pre_grip_delta: '그립 전 오프셋',
  grip_back_delta: '그립 후 후퇴',
  blend_up_distance: '블렌드 상승 거리',
  blend_down_distance: '블렌드 하강 거리',
  lift_up_creep_distance: '상승 크리프 거리',
  lift_down_creep_distance: '하강 크리프 거리',
  lift_up_after_complete: '완료 후 상승',
  lift_up_partial: '부분 상승',
  measure_floor: '바닥 측정',
  measure_item: '품목 측정',
  measure_sku: 'SKU 측정',
  adjust_center: '중심 보정',
  find_station_item: '스테이션 품목 탐색',
  avoid: '회피',
  outbound: '출고',
  use_drag_out: '드래그 아웃 사용',
  drag_out_height: '드래그 아웃 높이',
  drag_out_dist: '드래그 아웃 거리',
  drag_out_dir: '드래그 아웃 방향',
  use_drag_in: '드래그 인 사용',
  drag_in_height: '드래그 인 높이',
  drag_in_dist: '드래그 인 거리',
  drag_in_dir: '드래그 인 방향',
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
