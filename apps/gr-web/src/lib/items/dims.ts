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

/** 적용된 필드 하나 — 서버가 실제로 쓴 이전 · 이후 값과 변경 id(되돌리기용). */
export interface AppliedDim {
  id: number
  field: DimField
  before: number
  after: number
}

export interface BulkResult {
  applied: number
  failed: number
  results: {
    code: number
    ok: boolean
    changes?: number[]
    applied?: AppliedDim[]
    error?: string
  }[]
}

export const WINDOWS = [3, 5, 10, 20] as const
export const DEFAULT_WINDOW = 5

export const dimsApi = {
  item: (code: number, window = DEFAULT_WINDOW) =>
    getJson<ItemDims>(`/api/items/${code}/dims?window=${window}`),
  /** `onlyChanges=false` 면 적용할 것이 없는 품목도(반영된 칸을 보이려고) 온다. */
  suggest: (window = DEFAULT_WINDOW, onlyChanges = true) =>
    getJson<ItemDims[]>(`/api/items/dims/suggest?window=${window}&only_changes=${onlyChanges}`),
  allChanges: (limit = 500) => getJson<DimChange[]>(`/api/items/dims/changes?limit=${limit}`),
  apply: (code: number, fields: DimField[], window = DEFAULT_WINDOW, note = '') =>
    postJson<{ code: number; changes: number[]; applied?: AppliedDim[] }>(
      `/api/items/${code}/dims/apply`,
      {
        fields,
        window,
        note,
      },
    ),
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

/** 기본 선택 — 바로 적용해도 되는(적용 가능 · 새 값) 필드만. 표본 부족 · 흔들림 · 차이 큼 은 사람이 직접 고른다. */
export function defaultPick(d: ItemDims): Set<DimField> {
  return new Set(d.fields.filter((s) => safeTone(dimTone(s))).map((s) => s.field))
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

// ── 화면 표현(매트릭스 칸 · 신뢰도) ────────────────────────────────────────────

/** 이만큼은 재야 "바로 적용" 이 된다 — 두 개로는 중앙값이 평균일 뿐이고 편차도 믿기 어렵다. */
export const MIN_SAMPLES = 3

/**
 * 카드 색 — 한 필드가 지금 어떤 결정을 요구하나.
 *  - `ok`    : 바뀌고 안정 — 바로 적용해도 되는 제안
 *  - `info`  : 등록값 0(미입력)을 처음 채우는 제안
 *  - `thin`  : 표본 부족 — `MIN_SAMPLES` 개 미만(더 재 보기)
 *  - `warn`  : 흔들림 — 표본끼리 편차가 한도를 넘음(창을 늘리거나 더 재 보기)
 *  - `fault` : 차이 큼 — 다른 타이어를 잰 것일 수 있음(재고 품목 확인)
 *  - `muted` : 같음 · 표본 없음 — 할 일 없음
 */
export type DimTone = 'ok' | 'info' | 'thin' | 'warn' | 'fault' | 'muted'

export function dimTone(s: DimSuggestion): DimTone {
  if (!applicable(s)) return 'muted'
  // 차이 큼이 먼저다 — 두 개뿐이라도 다른 타이어를 잰 것이면 그게 가장 급하다.
  if (s.outlier) return 'fault'
  if (s.n < MIN_SAMPLES) return 'thin'
  if (s.unstable) return 'warn'
  if (s.current === 0) return 'info'
  return 'ok'
}

/** 톤의 한 단어 — 배지 · 필터 · 요약이 같은 말을 쓴다. */
export const TONE_LABEL: Record<DimTone, string> = {
  ok: '적용 가능',
  info: '새 값',
  thin: '표본 부족',
  warn: '흔들림',
  fault: '차이 큼',
  muted: '할 일 없음',
}

/** 바로 적용해도 되는 톤(기본 선택과 같은 규칙). */
export function safeTone(t: DimTone): boolean {
  return t === 'ok' || t === 'info'
}

/** 이 필드에 **지금 걸려 있는** 측정 반영 — 되돌리지 않았고, 그 뒤 값이 바뀌지 않았다(되돌리기 가능). */
export function liveChange(
  changes: readonly DimChange[],
  field: DimField,
  current: number,
): DimChange | null {
  const last = changes.find((c) => c.field === field && !c.reverted)
  if (!last || last.source !== 'measured') return null
  return Math.abs(last.after - current) < 0.05 ? last : null
}

/** 일괄 검토 필터 — 전체 · 바로 적용해도 되는 것 · 사람이 봐야 하는 것. */
export type ReviewFilter = 'all' | 'safe' | 'check'

export function passFilter(s: DimSuggestion, f: ReviewFilter): boolean {
  const t = dimTone(s)
  if (t === 'muted') return false
  if (f === 'safe') return safeTone(t)
  if (f === 'check') return !safeTone(t)
  return true
}

/**
 * 신뢰도 0..5 — 표본 수(창 대비)와 편차(한도 대비)를 합친 한 숫자. 칸의 점 ●●●○○ 로 보인다.
 *  - 표본: min(n, window) / window
 *  - 편차: 한도 안이면 1 → 0.5(한도에서), 넘으면 0.3
 * 표본이 없으면 0, 있으면 최소 1.
 */
export function confidence(s: DimSuggestion, window: number): number {
  if (!s.n) return 0
  const w = Math.max(window, 1)
  const sample = Math.min(s.n, w) / w
  const spread =
    s.spread === null ? 1 : s.spread <= s.spread_limit ? 1 - (0.5 * s.spread) / s.spread_limit : 0.3
  return Math.max(1, Math.min(5, Math.round(5 * sample * spread)))
}
