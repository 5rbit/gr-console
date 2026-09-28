// 이벤트 화면 상태 ↔ URL 쿼리(`e.*`) — 복사한 링크와 저장된 필터가 같은 화면을 다시 연다.
//
// 셸이 `?tab=` 을 쓰므로 이 화면은 `e.` 접두 키만 만진다(다른 키는 그대로 둔다). 기본값은 싣지 않는다 —
// 링크가 짧고, 빈 화면의 URL 은 `?tab=events` 그대로다.
import { EMPTY_FILTER, RANGE_PRESETS, type EvtFilter, type RangePreset } from './evtFilterModel'
import { EMPTY_STATS, type StatsPeriod, type StatsState } from './evtStatsModel'
import { parseTypes } from './evtTypeModel'
import { EMPTY_SWIM, type SwimState } from './swimlaneModel'

export type EvtViewId = 'list' | 'alarms' | 'steps' | 'ilock'

export const EVT_VIEWS: { id: EvtViewId; label: string }[] = [
  { id: 'list', label: '목록' },
  { id: 'alarms', label: '알람 통계' },
  { id: 'steps', label: '스텝 통계' },
  { id: 'ilock', label: '인터록' },
]

export interface EvtUrlState {
  view: EvtViewId
  filter: EvtFilter
  stats: StatsState
  swim: SwimState
}

export const EMPTY_URL_STATE: EvtUrlState = {
  view: 'list',
  filter: EMPTY_FILTER,
  stats: EMPTY_STATS,
  swim: EMPTY_SWIM,
}

const P = 'e.'

const intOrNull = (s: string | null): number | null => {
  if (s === null || !/^-?\d+$/.test(s)) return null
  const n = Number(s)
  return Number.isSafeInteger(n) ? n : null
}

const listOf = (s: string | null): string[] =>
  s
    ? s
        .split(',')
        .map((x) => x.trim())
        .filter(Boolean)
    : []

/** `e.*` 만 — 앞에 `?` 없음, 기본값이면 빈 문자열. */
export function toQuery(st: EvtUrlState): string {
  const q = new URLSearchParams()
  const set = (k: string, v: string | number | null | undefined) => {
    if (v !== null && v !== undefined && v !== '') q.set(P + k, String(v))
  }
  if (st.view !== 'list') set('view', st.view)
  const f = st.filter
  if (f.plcs.length) set('plc', f.plcs.join(','))
  if (f.cats.length) set('cat', f.cats.join(','))
  if (f.types.length) set('type', f.types.join(','))
  set('lvl', f.minLvl)
  set('code', f.code.trim())
  set('ctx', f.ctx.trim())
  if (f.range !== 'all') set('range', f.range)
  if (f.range === 'custom') {
    set('from', f.from)
    set('to', f.to)
  }
  set('q', f.q.trim())
  const s = st.stats
  if (s.period !== EMPTY_STATS.period) set('sp', s.period)
  if (s.period === 'custom') {
    set('sfrom', s.from)
    set('sto', s.to)
  }
  if (s.plcs.length) set('splc', s.plcs.join(','))
  if (s.types.length) set('stype', s.types.join(','))
  if (s.split) set('split', 1)
  const w = st.swim
  set('st', w.station)
  if (w.station === null) {
    set('slot', w.slot)
    set('ev', w.event)
  }
  set('wfrom', w.from)
  set('wto', w.to)
  return q.toString()
}

/** URL 쿼리 → 상태. `e.*` 가 하나도 없으면 `null`(화면 상태를 그대로 둔다). */
export function fromQuery(search: string): EvtUrlState | null {
  const q = new URLSearchParams(search)
  const g = (k: string) => q.get(P + k)
  if (![...q.keys()].some((k) => k.startsWith(P))) return null
  const view = (g('view') ?? 'list') as EvtViewId
  const range = (g('range') ?? 'all') as RangePreset
  const validRange = range === 'custom' || RANGE_PRESETS.some((p) => p.id === range)
  const filter: EvtFilter = {
    plcs: listOf(g('plc')),
    cats: listOf(g('cat')),
    types: parseTypes(listOf(g('type'))),
    minLvl: intOrNull(g('lvl')),
    code: g('code') ?? '',
    ctx: g('ctx') ?? '',
    range: validRange ? range : 'all',
    from: range === 'custom' ? intOrNull(g('from')) : null,
    to: range === 'custom' ? intOrNull(g('to')) : null,
    q: g('q') ?? '',
  }
  const sp = (g('sp') ?? EMPTY_STATS.period) as StatsPeriod
  const validSp = ['today', '7d', '30d', 'custom'].includes(sp)
  const stats: StatsState = {
    period: validSp ? sp : EMPTY_STATS.period,
    from: sp === 'custom' ? intOrNull(g('sfrom')) : null,
    to: sp === 'custom' ? intOrNull(g('sto')) : null,
    plcs: listOf(g('splc')),
    types: parseTypes(listOf(g('stype'))),
    split: g('split') === '1',
  }
  const station = intOrNull(g('st'))
  const swim: SwimState = {
    station,
    slot: station === null ? intOrNull(g('slot')) : null,
    event: station === null ? intOrNull(g('ev')) : null,
    from: intOrNull(g('wfrom')),
    to: intOrNull(g('wto')),
  }
  return {
    view: EVT_VIEWS.some((v) => v.id === view) ? view : 'list',
    filter,
    stats,
    swim,
  }
}

/** 지금 주소의 쿼리에서 `e.*` 를 `query` 로 갈아 끼운다(`tab` 같은 다른 키는 남는다). `?` 로 시작. */
export function mergeSearch(search: string, query: string): string {
  const q = new URLSearchParams(search)
  for (const k of [...q.keys()]) if (k.startsWith(P)) q.delete(k)
  for (const [k, v] of new URLSearchParams(query)) q.append(k, v)
  const s = q.toString()
  return s ? `?${s}` : ''
}
