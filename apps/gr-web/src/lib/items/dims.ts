// 측정 반영 — MeasureItem 기록으로 화물 규격 치수(InnerDiameter · UpperBeadHeight · Height)를 제안하고,
// 사람이 골라 적용하고, 이력으로 되돌린다. 계산은 전부 백엔드(`registry/dims.rs`)가 하고 여기는 타입 ·
// API · 화면이 쓰는 순수 규칙만 둔다.
//
// 자동 적용은 없다(사용자 결정 2026-09-21) — 이 셋은 PLC 로 가는 품목 값이라 로봇 위치(G = 내경 − 30,
// 적층 Z)를 바로 바꾼다. 기본 선택은 "바뀌고 안정한" 필드만이다.
import { getJson, postJson } from '../api'

export type DimField = 'inner_diameter' | 'upper_bead_height' | 'height'

export const DIM_FIELDS: readonly DimField[] = ['inner_diameter', 'upper_bead_height', 'height']

/** 화면 라벨 — PLC 멤버 이름 그대로(영문). */
export const DIM_LABEL: Record<DimField, string> = {
  inner_diameter: 'InnerDiameter',
  upper_bead_height: 'UpperBeadHeight',
  height: 'Height',
}

/** 어느 측정값에서 오나(설명 줄). */
export const DIM_SOURCE: Record<DimField, string> = {
  inner_diameter: 'MeasureItem Data[1] — 레이저·토크 조정 내경',
  upper_bead_height: 'MeasureItem Data[2] — 상부 비드(1 단일 때만)',
  height: 'MeasureItem Data[3] — 타이어 높이(1 단일 때만)',
}

export interface DimSample {
  plc: string
  seq: number
  at: string
  value: number
  cell_id: number
}

export interface DimSuggestion {
  field: DimField
  current: number
  suggested: number | null
  n: number
  spread: number | null
  spread_limit: number
  delta: number | null
  unstable: boolean
  /** 등록값(0 이 아닌)과 한도 넘게 다르다 — 재고 품목이 틀렸을(다른 타이어를 잰) 가능성. */
  outlier: boolean
  outlier_limit: number
  changed: boolean
  samples: DimSample[]
}

export interface ItemDims {
  code: number
  name: string
  window: number
  fields: DimSuggestion[]
  stack_max: { current: number; observed_max: number | null; samples: number }
  has_changes: boolean
}

export interface DimChange {
  id: number
  code: number
  field: DimField | string
  before: number
  after: number
  source: 'measured' | 'revert' | string
  samples: { plc: string; seq: number }[]
  note: string
  at: string
  reverted: boolean
}

export interface BulkResult {
  applied: number
  failed: number
  results: { code: number; ok: boolean; changes?: number[]; error?: string }[]
}

export const WINDOWS = [3, 5, 10, 20] as const
export const DEFAULT_WINDOW = 5

export const dimsApi = {
  item: (code: number, window = DEFAULT_WINDOW) =>
    getJson<ItemDims>(`/api/items/${code}/dims?window=${window}`),
  suggest: (window = DEFAULT_WINDOW) =>
    getJson<ItemDims[]>(`/api/items/dims/suggest?window=${window}`),
  apply: (code: number, fields: DimField[], window = DEFAULT_WINDOW, note = '') =>
    postJson<{ code: number; changes: number[] }>(`/api/items/${code}/dims/apply`, {
      fields,
      window,
      note,
    }),
  applyBulk: (items: { code: number; fields: DimField[] }[], window = DEFAULT_WINDOW) =>
    postJson<BulkResult>('/api/items/dims/apply-bulk', { items, window }),
  changes: (code: number, limit = 50) =>
    getJson<DimChange[]>(`/api/items/${code}/dims/changes?limit=${limit}`),
  revert: (code: number, id: number, force = false) =>
    postJson<{ changes: number[] }>(
      `/api/items/${code}/dims/changes/${id}/revert${force ? '?force=1' : ''}`,
    ),
}

/** 적용할 수 있는 제안인가 — 표본이 있고 지금 값과 다르다. */
export function applicable(s: DimSuggestion): boolean {
  return s.suggested !== null && s.changed
}

/** 기본 선택 — 적용할 수 있고 **안정하며 등록값과 크게 다르지 않은** 필드만. 나머지는 사람이 직접 고른다. */
export function defaultPick(d: ItemDims): Set<DimField> {
  return new Set(
    d.fields.filter((s) => applicable(s) && !s.unstable && !s.outlier).map((s) => s.field),
  )
}

/** 상태 칸 — 가장 급한 하나. */
export function dimState(s: DimSuggestion): {
  tone: 'warn' | 'fault' | null
  text: string
  title?: string
} {
  if (s.suggested === null) return { tone: null, text: '표본 없음' }
  if (s.outlier)
    return {
      tone: 'fault',
      text: '차이 큼',
      title: `등록값과 ${Math.abs(s.delta ?? 0).toFixed(1)} mm 다름(한도 ${s.outlier_limit}) — 그 칸의 재고 품목이 맞는지 먼저 확인하세요`,
    }
  if (s.unstable)
    return {
      tone: 'warn',
      text: '흔들림',
      title: `표본 편차 ${s.spread} mm > 한도 ${s.spread_limit}`,
    }
  return { tone: null, text: s.changed ? '제안' : '같음' }
}

/** 부호를 붙인 1자리 mm(0 은 "0.0"). */
export function signed(v: number | null): string {
  if (v === null || !Number.isFinite(v)) return ''
  const r = Math.round(v * 10) / 10
  const s = (r === 0 ? 0 : r).toFixed(1)
  return r > 0 ? `+${s}` : s
}

/** 적용 확인 문구 한 줄 — `InnerDiameter 372.0 → 370.5 (−1.5)`. */
export function changeLine(s: DimSuggestion): string {
  return `${DIM_LABEL[s.field]} ${s.current.toFixed(1)} → ${s.suggested?.toFixed(1) ?? '-'} (${signed(s.delta)})`
}

export function fieldLabel(f: string): string {
  return (DIM_LABEL as Record<string, string>)[f] ?? f
}
