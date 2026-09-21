// MOVE 작성 방식 — 백엔드 `issue::MoveMode` 와 같은 뜻(요청 `params.move_mode` / `params.move_clearance`).
//
// GR2 근거(export/GR2_PLC): `TaskType = MOVE AND Position.Z = 9999` 이면
//   isValidTaskData  — Z 범위 검사 면제(34행), MOVE 는 G 범위·ItemCode 검사도 면제(38·85행)
//   isValidTaskArea  — 셀/스테이션 영역 검사 전체 면제(83행)
//   PL_Task_V2       — isTaskMove(299행): Z 는 HomePos 유지, XY 이동 뒤 바로 999(하강·그립 없음)
// `Avoid` 플래그도 isTaskMove 를 켜고, 300 스텝에서 Y 를 지금 위치로 둔다(X 만 이동, 859–865행).
// 화면에서 새로 고르는 MOVE 의 기본은 `top`(하강 없는 이동) — 모드를 늘 명시해 보낸다.
import type { MoveMode, TaskRequestParams, TaskType } from '../types'

export const MOVE_TOP_Z = 9999
/** `stack` 여유 기본값(mm) — 백엔드 `MOVE_CLEARANCE`(옛 "바닥 + 500" 과 같은 값). */
export const MOVE_CLEARANCE_DEFAULT = 500
export const MOVE_CLEARANCE_MAX = 5000
export const MOVE_MODE_DEFAULT: MoveMode = 'top'

export interface MoveOpts {
  mode: MoveMode
  /** `stack` 의 여유(mm). 비우면 기본값. */
  clearance?: number | null
}

export const MOVE_MODES: readonly { id: MoveMode; label: string; title: string }[] = [
  { id: 'top', label: 'Top', title: 'Z 9999 — 상단 유지, 대상 XY 로 이동만 (품목·수량 불필요)' },
  { id: 'avoid', label: 'Avoid', title: 'Avoid + Z 9999 — 상단에서 X 만 이동, Y 유지 (회피)' },
  { id: 'stack', label: 'Stack', title: '스택 윗면 + Clearance 까지 내려갔다 올라옴' },
]

/** 스텝·초안의 MOVE 옵션 — 없으면 화면 기본(`top`). */
export function moveOf(x: { move?: MoveOpts | null }): MoveOpts {
  return x.move ?? { mode: MOVE_MODE_DEFAULT }
}

/** `stack` 만 품목(스택 높이)을 쓴다 — 그마저 선택(없으면 셀 재고의 품목). */
export function moveUsesItem(o: MoveOpts): boolean {
  return o.mode === 'stack'
}

/** 요청에 실을 품목 — `top`/`avoid` MOVE 는 화면에서 품목을 감추므로 보내지도 않는다. */
export function sentItem(x: {
  type: TaskType
  item_code: number | null
  move?: MoveOpts | null
}): number | null {
  return x.type === 'MOVE' && !moveUsesItem(moveOf(x)) ? null : x.item_code
}

/** 요청 `params` 에 얹을 키. */
export function moveParams(o: MoveOpts): TaskRequestParams {
  const out: TaskRequestParams = { move_mode: o.mode }
  if (o.mode === 'stack' && o.clearance !== null && o.clearance !== undefined)
    out.move_clearance = o.clearance
  return out
}

export function moveErrors(o: MoveOpts): string[] {
  if (o.mode !== 'stack' || o.clearance === null || o.clearance === undefined) return []
  const c = o.clearance
  return Number.isFinite(c) && c >= 0 && c <= MOVE_CLEARANCE_MAX
    ? []
    : [`Clearance: 0..${MOVE_CLEARANCE_MAX} mm`]
}

/**
 * 미리보기 Z — `top`/`avoid` 는 9999, `stack` 은 바닥 + 스택 높이 + 여유.
 * 스택이 있는데 높이를 모르면 `null`(계산 불가).
 */
export function moveZ(o: MoveOpts, floor: number, stackHeight: number | null): number | null {
  if (o.mode !== 'stack') return MOVE_TOP_Z
  if (stackHeight === null) return null
  return floor + stackHeight + (o.clearance ?? MOVE_CLEARANCE_DEFAULT)
}
