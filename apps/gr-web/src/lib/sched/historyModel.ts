// 기록 탭의 순수 도우미 — KPI 타일 · 시간 · 판정 기록 종류.
import type { Kpi, LogRow } from '../sched'

export const KPI_HOURS = [1, 8, 24] as const
export type KpiHours = (typeof KPI_HOURS)[number]

/** 판정 기록 종류 — 백엔드 `taskgen_log.kind`. */
export const LOG_KINDS = [
  'generated',
  'issued',
  'completed',
  'aborted',
  'cooldown',
  'request',
  'hand_remove',
  'wait_alert',
] as const

const KIND_LABEL: Record<string, string> = {
  generated: '생성',
  issued: '발행',
  completed: '완료',
  aborted: '중단',
  cooldown: '냉각',
  request: '요청',
  hand_remove: 'Hand 정리',
  wait_alert: '대기 경고',
}

export function kindLabel(k: string): string {
  return KIND_LABEL[k] ?? k
}

export type KindTone = 'ok' | 'warn' | 'fault' | 'neutral'

export function kindTone(k: string): KindTone {
  switch (k) {
    case 'completed':
      return 'ok'
    case 'aborted':
    case 'hand_remove':
      return 'fault'
    case 'cooldown':
    case 'wait_alert':
      return 'warn'
    default:
      return 'neutral'
  }
}

/** 초 → `12.3`(단위는 머리글), 없으면 `—`. 100 초 이상은 소수점을 버린다. */
export function fmtSeconds(v: number | null | undefined): string {
  if (v === null || v === undefined || !Number.isFinite(v)) return '—'
  return Math.abs(v) >= 100 ? v.toFixed(0) : v.toFixed(1)
}

const pad = (n: number) => String(n).padStart(2, '0')

/** ISO 시각 → 로컬 `MM-DD HH:mm:ss`(오늘이면 `HH:mm:ss`). 못 읽으면 원문. */
export function fmtAt(iso: string | null | undefined, now = new Date()): string {
  if (!iso) return '—'
  const d = new Date(iso)
  if (!Number.isFinite(d.getTime())) return iso
  const hms = `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
  const today =
    d.getFullYear() === now.getFullYear() &&
    d.getMonth() === now.getMonth() &&
    d.getDate() === now.getDate()
  return today ? hms : `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${hms}`
}

/** 백엔드가 필드를 빼먹어도 표가 서게 — 배열은 빈 것으로. */
export function normalizeKpi(k: Partial<Kpi> | null | undefined, hours: number): Kpi {
  return {
    hours: k?.hours ?? hours,
    generated: k?.generated ?? 0,
    completed: k?.completed ?? 0,
    aborted: k?.aborted ?? 0,
    cycle_avg_s: k?.cycle_avg_s ?? null,
    cycle_p90_s: k?.cycle_p90_s ?? null,
    stations: k?.stations ?? [],
    robots: k?.robots ?? [],
    requests: k?.requests ?? { done: 0, open: 0, failed: 0 },
  }
}

export interface KpiTile {
  key: string
  label: string
  value: string
  tone: 'neutral' | 'warn' | 'fault'
}

/** KPI 머리 줄 — 생성 · 완료 · 중단 · Cycle avg/p90. 중단이 완료의 1/10 을 넘으면 경고색. */
export function kpiTiles(k: Kpi | null): KpiTile[] {
  const n = (v: number | undefined) => (k ? String(v ?? 0) : '—')
  const aborted = k?.aborted ?? 0
  const completed = k?.completed ?? 0
  const abortTone: KpiTile['tone'] =
    aborted === 0 ? 'neutral' : aborted * 10 > Math.max(completed, 1) ? 'fault' : 'warn'
  return [
    { key: 'generated', label: '생성', value: n(k?.generated), tone: 'neutral' },
    { key: 'completed', label: '완료', value: n(k?.completed), tone: 'neutral' },
    { key: 'aborted', label: '중단', value: n(k?.aborted), tone: k ? abortTone : 'neutral' },
    {
      key: 'cycle',
      label: 'Cycle avg/p90 (s)',
      value: k ? `${fmtSeconds(k.cycle_avg_s)} / ${fmtSeconds(k.cycle_p90_s)}` : '—',
      tone: 'neutral',
    },
    {
      key: 'requests',
      label: '요청 완료/열림/실패',
      value: k ? `${k.requests.done} / ${k.requests.open} / ${k.requests.failed}` : '—',
      tone: k && k.requests.failed ? 'warn' : 'neutral',
    },
  ]
}

/** 기록 줄 정렬 — 최신 먼저(백엔드 순서를 믿지 않는다). */
export function sortLog(rows: readonly LogRow[] | null | undefined): LogRow[] {
  return [...(rows ?? [])].sort((a, b) => b.id - a.id)
}
