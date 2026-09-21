// Task 생성 엔진 — 백엔드 `taskgen` 의 타입과 화면용 순수 도우미(규칙 문구 · 가중치 편집 · 점수 근거).
import { getJson, postJson, putJson, del } from './api'
import type { Target } from './types'

export type GenTrigger =
  | { kind: 'manual' }
  | { kind: 'station_req'; station: number; require_cvok?: boolean }
  | { kind: 'station_item'; station: number; require_cvok?: boolean }
  | { kind: 'cell_stock'; cell: number; item?: number | null; min?: number }

/** 셀 자동 선택 — 구역·행·열 필터와 순서(출발 oldest/nearest, 도착 같은 품목 먼저). */
export interface CellPick {
  section?: number | null
  row_min?: number | null
  row_max?: number | null
  col_min?: number | null
  col_max?: number | null
  order?: string
  same_item_first?: boolean
}

export type GenAction =
  | {
      kind: 'transfer'
      from: Target
      to: Target
      item?: number | null
      count?: number
      from_auto?: CellPick | null
      to_auto?: CellPick | null
      pallet_auto?: boolean
    }
  | { kind: 'move'; to: Target }
  | { kind: 'measure'; target: Target; item?: number | null }

export interface GenRule {
  id: string
  name: string
  enabled: boolean
  trigger: GenTrigger
  action: GenAction
  robots: number[]
  priority: number
  manual_requests?: number
}

/** 규칙 설정과 함께 저장하는 상황별 가중(대기 가점·거리 감점 같은 전역 값은 Parameters). */
export interface GenWeights {
  target: Record<string, number>
  item: Record<string, number>
  robot: Record<string, number>
}

export interface GenConfig {
  version: number
  auto: boolean
  weights: GenWeights
  rules: GenRule[]
}

export interface GenCandidate {
  rule_id: string
  rule_name: string
  robot: number
  robot_name: string
  score: number
  breakdown: [string, number][]
  area: { lo: number; hi: number }
  first?: Target
  second?: Target | null
  age_min: number
  order: number
  /** 없으면 이번에 생성 */
  reason: string | null
}

export interface GenItem {
  id: string
  rule_id: string
  rule_name: string
  robot: number
  steps: { type: string; target: Target; item_code: number | null; count: number }[]
  next: number
  task_ids: string[]
  transfer_order_id: string | null
  score: number
  created_at: string
  note: string | null
}

export interface GenMetrics {
  started_at: string
  generated: number
  issued: number
  completed_items: number
  aborted: number
  submit_failures: number
  waits: Record<string, number>
}

export interface GenState {
  config: GenConfig
  candidates: GenCandidate[]
  /** 후보조차 못 된 규칙(자동 셀 없음 · 팔렛 자리 없음 · 거리 초과) */
  skipped: { rule: string; reason: string }[]
  queue: GenItem[]
  note: string | null
  metrics: GenMetrics
  separation_mm: number
}

export const taskgenApi = {
  get: () => getJson<GenState>('/api/taskgen'),
  save: (c: GenConfig) => putJson<GenConfig>('/api/taskgen/config', c),
  request: (id: string) =>
    postJson<{ rule: string; requests: number }>(
      `/api/taskgen/rules/${encodeURIComponent(id)}/request`,
    ),
  removeQueued: (id: string) => del(`/api/taskgen/queue/${encodeURIComponent(id)}`),
}

export const EMPTY_WEIGHTS: GenWeights = { target: {}, item: {}, robot: {} }

/** 지표 한 줄. */
export function metricsLine(m: GenMetrics): string {
  const waits = Object.entries(m.waits)
    .sort((a, b) => b[1] - a[1])
    .slice(0, 3)
    .map(([k, v]) => `${k} ${v}`)
    .join(' · ')
  return `생성 ${m.generated} · 발행 ${m.issued} · 끝남 ${m.completed_items} · 중단 ${m.aborted} · 제출 실패 ${m.submit_failures}${waits ? ` — 대기 ${waits}` : ''}`
}

const where = (t: Target) => `${t.kind === 'station' ? 'Station' : 'Cell'} ${t.id}`

export function triggerLabel(t: GenTrigger): string {
  switch (t.kind) {
    case 'manual':
      return 'Manual'
    case 'station_req':
      return `Station ${t.station} Req${t.require_cvok === false ? '' : ' + CVOK'}`
    case 'station_item':
      return `Station ${t.station} ItemExist${t.require_cvok === false ? '' : ' + CVOK'}`
    case 'cell_stock':
      return `Cell ${t.cell} ≥ ${t.min ?? 1}${t.item ? ` (Item ${t.item})` : ''}`
  }
}

export function actionLabel(a: GenAction): string {
  switch (a.kind) {
    case 'transfer': {
      const from = a.from_auto ? `Auto(${a.from_auto.order || 'oldest'})` : where(a.from)
      const to = a.to_auto ? 'Auto' : where(a.to)
      return `PICK ${from} → DROP ${to}${a.pallet_auto ? ' (Pallet)' : ''}${a.item ? ` · ${a.item}` : ''} ×${a.count ?? 1}`
    }
    case 'move':
      return `MOVE ${where(a.to)}`
    case 'measure':
      return `MEASURE ${where(a.target)}`
  }
}

/** `{2101: 10, 2102: 5}` ↔ `"2101:10, 2102:5"` — 가중치 한 줄 편집. 잘못된 조각은 사유와 함께. */
export function formatMap(m: Record<string, number>): string {
  return Object.entries(m)
    .map(([k, v]) => `${k}:${v}`)
    .join(', ')
}

export function parseMap(s: string): { ok: Record<string, number> } | { error: string } {
  const out: Record<string, number> = {}
  for (const part of s
    .split(',')
    .map((p) => p.trim())
    .filter(Boolean)) {
    const [k, v] = part.split(':').map((x) => x.trim())
    const key = Number(k)
    const val = Number(v)
    if (!Number.isInteger(key) || key < 0 || v === undefined || !Number.isFinite(val))
      return { error: `'${part}' — id:값 형식` }
    out[String(key)] = val
  }
  return { ok: out }
}

/** 점수 근거 한 줄(툴팁). */
export function breakdownText(c: Pick<GenCandidate, 'score' | 'breakdown'>): string {
  const parts = c.breakdown.map(([k, v]) => `${k} ${v >= 0 ? '+' : ''}${Number(v.toFixed(2))}`)
  return `Score ${Number(c.score.toFixed(2))} = ${parts.join(' ')}`
}

export function newRule(existing: readonly GenRule[]): GenRule {
  let n = existing.length + 1
  while (existing.some((r) => r.id === `rule-${n}`)) n++
  return {
    id: `rule-${n}`,
    name: `Rule ${n}`,
    enabled: false,
    trigger: { kind: 'manual' },
    action: {
      kind: 'transfer',
      from: { kind: 'cell', id: 0 },
      to: { kind: 'cell', id: 0 },
      item: null,
      count: 1,
    },
    robots: [],
    priority: 0,
  }
}
