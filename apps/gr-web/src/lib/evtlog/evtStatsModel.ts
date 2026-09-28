// 알람 · 스텝 통계 — 기간, 쿼리, 파레토 줄, 목록으로 넘기는 필터.
//
// 기간은 **달력 날**로 끊는다(오늘 = 로컬 자정부터). 보고서·현장 교대가 날 단위라 "지난 24시간"보다 읽기 쉽다.
import type { AlarmStatRow } from './api'
import { EMPTY_FILTER, type EvtFilter } from './evtFilterModel'

export type StatsPeriod = 'today' | '7d' | '30d' | 'custom'

export const STATS_PERIODS: { id: Exclude<StatsPeriod, 'custom'>; label: string; days: number }[] =
  [
    { id: 'today', label: '오늘', days: 1 },
    { id: '7d', label: '7일', days: 7 },
    { id: '30d', label: '30일', days: 30 },
  ]

export interface StatsState {
  period: StatsPeriod
  /** `custom` 일 때만 — epoch ms. */
  from: number | null
  to: number | null
  plcs: string[]
  /** 알람 통계의 ErrorList 유형 — 비면 서버 기본(Alarm, Warn). */
  types: string[]
  /** 스텝 통계를 Task 종류로 나눈다. */
  split: boolean
}

export const EMPTY_STATS: StatsState = {
  period: 'today',
  from: null,
  to: null,
  plcs: [],
  types: [],
  split: false,
}

export function localMidnight(ms: number): number {
  const d = new Date(ms)
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()
}

/** 기간 → epoch ms. 프리셋은 오늘을 포함한 N 일(자정 기준), 직접 지정은 빈 쪽을 오늘 자정 / 지금으로. */
export function periodRange(s: StatsState, now: number): { from: number; to: number } {
  if (s.period === 'custom') return { from: s.from ?? localMidnight(now), to: s.to ?? now }
  const days = STATS_PERIODS.find((p) => p.id === s.period)?.days ?? 1
  const d = new Date(localMidnight(now))
  d.setDate(d.getDate() - (days - 1))
  return { from: d.getTime(), to: now }
}

/** `?from=…&to=…[&plc=…][&split=type]` — 스텝 통계(`split`)에는 유형을 싣지 않는다. */
export function statsQuery(s: StatsState, now: number, opts: { split?: boolean } = {}): string {
  const r = periodRange(s, now)
  const q = new URLSearchParams({ from: String(r.from), to: String(r.to) })
  if (s.plcs.length) q.set('plc', s.plcs.join(','))
  if (!opts.split && s.types.length) q.set('type', s.types.join(','))
  if (opts.split && s.split) q.set('split', 'type')
  return `?${q.toString()}`
}

export type ParetoMetric = 'count' | 'duration'

export interface ParetoRow extends AlarmStatRow {
  /** 고른 척도의 값(건수 또는 ms). */
  value: number
  /** 전체 중 몫(%). */
  share: number
  /** 위에서부터 누적 몫(%). */
  cum: number
  /** 막대 길이(가장 큰 값 = 100). */
  bar: number
}

/** 큰 것부터 — 같으면 다른 척도, 그다음 코드 순. */
export function pareto(rows: readonly AlarmStatRow[], metric: ParetoMetric): ParetoRow[] {
  const val = (r: AlarmStatRow) => (metric === 'count' ? r.count : r.total_ms)
  const other = (r: AlarmStatRow) => (metric === 'count' ? r.total_ms : r.count)
  const sorted = [...rows].sort(
    (a, b) =>
      val(b) - val(a) ||
      other(b) - other(a) ||
      a.plc.localeCompare(b.plc) ||
      a.label.localeCompare(b.label),
  )
  const total = sorted.reduce((s, r) => s + val(r), 0)
  const max = sorted.length ? val(sorted[0]) : 0
  let cum = 0
  return sorted.map((r) => {
    const v = val(r)
    cum += v
    return {
      ...r,
      value: v,
      share: total ? (v / total) * 100 : 0,
      cum: total ? (cum / total) * 100 : 0,
      bar: max ? (v / max) * 100 : 0,
    }
  })
}

/** ms → 초(한 자리). 표의 머리글이 단위(s)를 맡는다. */
export function secs(ms: number): string {
  return (ms / 1000).toFixed(1)
}

/** 머리 숫자 띠의 길이 — 1 분 미만 s, 1 시간 미만 m s, 그 위 h m. */
export function fmtDuration(ms: number): string {
  const s = Math.round(ms / 1000)
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`
  if (s < 3600) return `${Math.floor(s / 60)} m ${String(s % 60).padStart(2, '0')} s`
  return `${Math.floor(s / 3600)} h ${String(Math.floor((s % 3600) / 60)).padStart(2, '0')} m`
}

/** 알람 한 줄 → 그 알람의 행만 남는 목록 필터(같은 기간). v2 행은 ErrorList 코드로, 옛 행은 문구 검색으로. */
export function alarmListFilter(
  row: Pick<AlarmStatRow, 'plc' | 'search' | 'by_code'>,
  range: { from: number; to: number },
): EvtFilter {
  const base = { ...EMPTY_FILTER, plcs: [row.plc], range: 'custom' as const, ...range }
  return row.by_code ? { ...base, code: row.search } : { ...base, cats: ['ALARM'], q: row.search }
}

/** Task 종류로 나눈 표에서 같은 (PLC, proc, step) 줄이 모이게 — 종류 없는 줄이 먼저. */
export function stepRowKey(r: {
  plc: string
  proc: number
  step: number
  task_type: number | null
}): string {
  return `${r.plc}/${r.proc}/${r.step}/${r.task_type ?? '-'}`
}
