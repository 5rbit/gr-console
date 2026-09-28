import { describe, expect, it } from 'vitest'
import type { AlarmStatRow } from './api'
import {
  EMPTY_STATS,
  alarmListFilter,
  fmtDuration,
  localMidnight,
  pareto,
  periodRange,
  secs,
  statsQuery,
  stepRowKey,
} from './evtStatsModel'

const row = (p: Partial<AlarmStatRow>): AlarmStatRow => ({
  plc: 'GR2',
  type: 'Warn',
  area: 2,
  area_name: 'WARN',
  bit: 320,
  code: 1101,
  label: 'W1101',
  text: 'X Axis - Lag Error',
  count: 1,
  total_ms: 1000,
  mean_ms: 1000,
  max_ms: 1000,
  open: 0,
  flicker: 0,
  last_ts: 0,
  last: '',
  search: 'W1101 X Axis - Lag Error',
  by_code: false,
  ...p,
})

describe('periodRange', () => {
  const now = new Date(2026, 8, 27, 15, 30).getTime()
  it('today starts at local midnight', () => {
    expect(periodRange(EMPTY_STATS, now)).toEqual({
      from: new Date(2026, 8, 27).getTime(),
      to: now,
    })
  })
  it('7 days include today', () => {
    expect(periodRange({ ...EMPTY_STATS, period: '7d' }, now).from).toBe(
      new Date(2026, 8, 21).getTime(),
    )
    expect(periodRange({ ...EMPTY_STATS, period: '30d' }, now).from).toBe(
      new Date(2026, 7, 29).getTime(),
    )
  })
  it('custom fills the missing ends', () => {
    expect(periodRange({ ...EMPTY_STATS, period: 'custom', from: 5, to: null }, now)).toEqual({
      from: 5,
      to: now,
    })
    expect(periodRange({ ...EMPTY_STATS, period: 'custom', from: null, to: 9 }, now).from).toBe(
      localMidnight(now),
    )
  })
  it('query carries plc and split only when asked', () => {
    const s = { ...EMPTY_STATS, plcs: ['GR1', 'GR2'], split: true }
    const q = statsQuery(s, now)
    expect(q).toContain('plc=GR1%2CGR2')
    expect(q).not.toContain('split')
    expect(statsQuery(s, now, { split: true })).toContain('split=type')
  })
  it('alarm types go to the alarm query only', () => {
    const s = { ...EMPTY_STATS, types: ['Alarm', 'Operator'] }
    expect(statsQuery(s, now)).toContain('type=Alarm%2COperator')
    expect(statsQuery(s, now, { split: true })).not.toContain('type=')
    expect(statsQuery(EMPTY_STATS, now)).not.toContain('type=')
  })
})

describe('pareto', () => {
  const rows = [
    row({ label: 'W1', count: 2, total_ms: 9000 }),
    row({ label: 'W2', count: 5, total_ms: 1000 }),
    row({ label: 'W3', count: 3, total_ms: 0 }),
  ]
  it('by count: biggest first with cumulative share', () => {
    const p = pareto(rows, 'count')
    expect(p.map((r) => r.label)).toEqual(['W2', 'W3', 'W1'])
    expect(p.map((r) => Math.round(r.cum))).toEqual([50, 80, 100])
    expect(p[0].bar).toBe(100)
    expect(p[2].bar).toBe(40)
  })
  it('by duration, ties by count', () => {
    const p = pareto([...rows, row({ label: 'W4', count: 9, total_ms: 1000 })], 'duration')
    expect(p.map((r) => r.label)).toEqual(['W1', 'W4', 'W2', 'W3'])
    expect(p[3].share).toBe(0)
  })
  it('empty is empty', () => {
    expect(pareto([], 'count')).toEqual([])
  })
})

describe('formats and links', () => {
  it('durations', () => {
    expect(secs(1234)).toBe('1.2')
    expect(fmtDuration(4200)).toBe('4.2 s')
    expect(fmtDuration(185_000)).toBe('3 m 05 s')
    expect(fmtDuration(7_560_000)).toBe('2 h 06 m')
  })
  it('an alarm row opens the list on its rows in the same period', () => {
    const f = alarmListFilter(row({ plc: 'GRM', search: 'F0501 Station 01' }), { from: 1, to: 2 })
    expect(f).toMatchObject({
      plcs: ['GRM'],
      cats: ['ALARM'],
      q: 'F0501 Station 01',
      range: 'custom',
      from: 1,
      to: 2,
    })
  })
  it('a v2 alarm row opens the list by its ErrorList code', () => {
    const f = alarmListFilter(row({ plc: 'GR2', search: 'W1101', by_code: true }), {
      from: 1,
      to: 2,
    })
    expect(f).toMatchObject({ plcs: ['GR2'], code: 'W1101', q: '', cats: [], range: 'custom' })
  })
  it('step keys separate task types', () => {
    const a = stepRowKey({ plc: 'GR2', proc: 20, step: 300, task_type: null })
    const b = stepRowKey({ plc: 'GR2', proc: 20, step: 300, task_type: 1 })
    expect(a).not.toBe(b)
  })
})
