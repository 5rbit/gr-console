// 레이아웃 맵의 도형 표현 규칙(DOM 없는 순수 함수) — 채널마다 뜻 하나(2026-09-21 사용자와 조율).
//
//   채움  = 칸 상태: 사용가능(빈 칸 · 재고 있음) / Max(단수 Max 도달) / 비활성(Use=false)
//   윤곽  = **한 줄**, 겹치면 우선순위가 높은 하나만: 선택 > 로봇 작업 > 계획 > 품목 강조 > 못 쓰는 칸 >
//           로컬 수정(편집 모드) > 호버
//   구역  = 칸 색이 아니라 레이아웃 바탕의 옅은 영역 + 이름
//
// 셀 바닥 Z ≤ 0 은 정상이다(바닥 평탄도 보정). PLC `isValidTaskData` 가 INVALID_CELL_POSZ 로 거부하는 것은
// 대상 Id > 2000(스테이션)일 때뿐이라, "못 쓰는 칸"의 Z 조건은 스테이션에만 붙는다.

export type FillState = 'empty' | 'stocked' | 'full' | 'disabled'

export interface FillInput {
  use: boolean
  count: number
  /** 담긴 품목의 단수 Max(0 = 모름 → Max 판정 안 함). */
  stackMax: number
}

export function fillState(i: FillInput): FillState {
  if (!i.use) return 'disabled'
  if (i.count <= 0) return 'empty'
  if (i.stackMax > 0 && i.count >= i.stackMax) return 'full'
  return 'stocked'
}

/** PLC 가 작업을 거부하는 칸 — Use=false, 또는 스테이션(Id > 2000) 바닥 Z ≤ 0. */
export function unusable(s: { kind: 'cell' | 'station'; id: number; use: boolean; z: number }): string | null {
  if (!s.use) return 'Use = false — 사용 안 함'
  if (s.id > 2000 && s.z <= 0) return `Z ${s.z} ≤ 0 — GR2 가 INVALID_CELL_POSZ 로 거부`
  return null
}

export type OutlineKind =
  | 'selected'
  | 'work'
  | 'planned'
  | 'highlight'
  | 'unusable'
  | 'dirty'
  | 'hover'

/** 우선순위(앞이 높다). */
export const OUTLINE_ORDER: readonly OutlineKind[] = [
  'selected',
  'work',
  'planned',
  'highlight',
  'unusable',
  'dirty',
  'hover',
]

/** 켜진 상태들 중 윤곽으로 그릴 하나. 없으면 null(기본 윤곽). */
export function outlineOf(on: Partial<Record<OutlineKind, boolean>>): OutlineKind | null {
  return OUTLINE_ORDER.find((k) => on[k]) ?? null
}
