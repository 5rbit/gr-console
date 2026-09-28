import { describe, expect, it } from 'vitest'
import type { EventRow, EvtCatalogEvent } from './api'
import {
  EMPTY_FILTER,
  activeCount,
  buildQuery,
  codeSuggestions,
  codeTokens,
  localInputToMs,
  matchesFilter,
  msToLocalInput,
  parseCtx,
  type EvtFilter,
} from './evtFilterModel'

const NOW = Date.UTC(2026, 8, 27, 3, 0, 0) // 12:00 KST

function row(p: Partial<EventRow> = {}): EventRow {
  return {
    id: 1,
    plc: 'GR2',
    origin: 'plc',
    epoch: 1,
    seq: 10,
    ts: '2026-09-27 11:59:00.000',
    ts_ms: NOW - 60_000,
    rx_ts: '2026-09-27 11:59:00.010',
    cat: 3,
    cat_name: 'GRIP',
    lvl: 2,
    lvl_name: 'INFO',
    src: 20,
    code: 701,
    name: 'GRIP_REQ',
    a: 0,
    b: 0,
    ctx: 42,
    detail: null,
    text: 'Gripper request close',
    ...p,
  }
}

const f = (p: Partial<EvtFilter>): EvtFilter => ({ ...EMPTY_FILTER, ...p })

function params(q: string): Record<string, string> {
  expect(q.startsWith('?') || q === '').toBe(true)
  return Object.fromEntries(new URLSearchParams(q.slice(1)))
}

describe('buildQuery', () => {
  it('is empty for the empty filter', () => {
    expect(buildQuery(EMPTY_FILTER, { now: NOW })).toBe('')
  })

  it('carries every filter, limit and cursor', () => {
    const q = buildQuery(
      f({
        plcs: ['GR2', 'console'],
        cats: ['GRIP', 'ALARM'],
        minLvl: 3,
        code: ' 701, grip_error ',
        ctx: '42',
        range: '1h',
        q: '  close ',
      }),
      { now: NOW, limit: 200, before: 'c:123' },
    )
    expect(params(q)).toEqual({
      plc: 'GR2,console',
      cat: 'GRIP,ALARM',
      lvl: '3',
      code: '701,GRIP_ERROR',
      ctx: '42',
      from: String(NOW - 3_600_000),
      q: 'close',
      limit: '200',
      before: 'c:123',
    })
  })

  it('uses from/to for a custom range and nothing for all', () => {
    expect(params(buildQuery(f({ range: 'custom', from: 1000, to: 2000 }), { now: NOW }))).toEqual({
      from: '1000',
      to: '2000',
    })
    expect(params(buildQuery(f({ range: 'custom', from: 1000, to: null }), { now: NOW }))).toEqual({
      from: '1000',
    })
    expect(buildQuery(f({ range: 'all' }), { now: NOW })).toBe('')
  })

  it('drops an invalid ctx and a null cursor', () => {
    expect(buildQuery(f({ ctx: 'abc' }), { now: NOW, before: null })).toBe('')
    expect(parseCtx('-1')).toBeNull()
    expect(parseCtx(' 7 ')).toBe(7)
  })
})

describe('codeTokens / activeCount', () => {
  it('normalizes numbers and names', () => {
    expect(codeTokens('0701, grip_req  ALARM_ON')).toEqual(['701', 'GRIP_REQ', 'ALARM_ON'])
    expect(codeTokens(' , ')).toEqual([])
  })

  it('counts active conditions (all is not a condition)', () => {
    expect(activeCount(EMPTY_FILTER)).toBe(0)
    expect(activeCount(f({ plcs: ['GR2'], range: '15m', ctx: 'x', q: ' ' }))).toBe(2)
  })
})

describe('ErrorList types and codes', () => {
  const alarm = row({
    cat: 6,
    cat_name: 'ALARM',
    code: 13119,
    name: null,
    etype: 'Alarm',
    ecode: 'F3119',
    trans: 'raise',
    text: '스테이션 인터록 타임아웃 발생',
    text_en: 'Station Interlock Timeout raised',
  })
  it('the type filter goes to the query and counts as a condition', () => {
    expect(params(buildQuery(f({ types: ['Alarm', 'Task'] }), { now: NOW }))).toEqual({
      type: 'Alarm,Task',
    })
    expect(activeCount(f({ types: ['Warn'] }))).toBe(1)
  })
  it('live rows match by type — rows outside the ErrorList never do', () => {
    expect(matchesFilter(alarm, f({ types: ['Alarm'] }), NOW)).toBe(true)
    expect(matchesFilter(alarm, f({ types: ['Warn', 'Task'] }), NOW)).toBe(false)
    expect(matchesFilter(row(), f({ types: ['Info'] }), NOW)).toBe(false)
  })
  it('an ErrorList code in the code box matches ecode, the search sees ecode and the English text', () => {
    expect(codeTokens('f3119, 701')).toEqual(['F3119', '701'])
    expect(matchesFilter(alarm, f({ code: 'f3119' }), NOW)).toBe(true)
    expect(matchesFilter(alarm, f({ code: 'F3118' }), NOW)).toBe(false)
    expect(matchesFilter(row(), f({ code: 'F3119,701' }), NOW)).toBe(true)
    expect(matchesFilter(alarm, f({ q: 'f3119' }), NOW)).toBe(true)
    expect(matchesFilter(alarm, f({ q: 'interlock' }), NOW)).toBe(true)
  })
})

describe('matchesFilter (live matcher)', () => {
  it('passes everything with the empty filter', () => {
    expect(matchesFilter(row(), EMPTY_FILTER, NOW)).toBe(true)
  })

  it('filters by plc, cat (name or id) and min level', () => {
    expect(matchesFilter(row(), f({ plcs: ['GRM'] }), NOW)).toBe(false)
    expect(matchesFilter(row({ plc: 'console' }), f({ plcs: ['console'] }), NOW)).toBe(true)
    expect(matchesFilter(row(), f({ cats: ['ALARM'] }), NOW)).toBe(false)
    expect(matchesFilter(row(), f({ cats: ['GRIP'] }), NOW)).toBe(true)
    expect(matchesFilter(row(), f({ cats: ['3'] }), NOW)).toBe(true)
    expect(matchesFilter(row({ lvl: 2 }), f({ minLvl: 3 }), NOW)).toBe(false)
    expect(matchesFilter(row({ lvl: 3 }), f({ minLvl: 3 }), NOW)).toBe(true)
  })

  it('matches code by number or by event name (case-insensitive)', () => {
    expect(matchesFilter(row(), f({ code: '701' }), NOW)).toBe(true)
    expect(matchesFilter(row(), f({ code: '702' }), NOW)).toBe(false)
    expect(matchesFilter(row(), f({ code: 'grip_req' }), NOW)).toBe(true)
    expect(matchesFilter(row(), f({ code: '999,GRIP_REQ' }), NOW)).toBe(true)
    expect(matchesFilter(row({ name: null }), f({ code: 'GRIP_REQ' }), NOW)).toBe(false)
  })

  it('filters by ctx and free text over text/name/detail', () => {
    expect(matchesFilter(row(), f({ ctx: '42' }), NOW)).toBe(true)
    expect(matchesFilter(row(), f({ ctx: '43' }), NOW)).toBe(false)
    expect(matchesFilter(row(), f({ q: 'CLOSE' }), NOW)).toBe(true)
    expect(matchesFilter(row(), f({ q: 'grip_r' }), NOW)).toBe(true)
    expect(matchesFilter(row({ detail: 'torque 12' }), f({ q: 'torque' }), NOW)).toBe(true)
    expect(matchesFilter(row(), f({ q: 'open' }), NOW)).toBe(false)
  })

  it('respects the time window', () => {
    expect(matchesFilter(row({ ts_ms: NOW - 20 * 60_000 }), f({ range: '15m' }), NOW)).toBe(false)
    expect(matchesFilter(row({ ts_ms: NOW - 10 * 60_000 }), f({ range: '15m' }), NOW)).toBe(true)
    const custom = f({ range: 'custom', from: NOW - 5000, to: NOW - 1000 })
    expect(matchesFilter(row({ ts_ms: NOW - 3000 }), custom, NOW)).toBe(true)
    expect(matchesFilter(row({ ts_ms: NOW }), custom, NOW)).toBe(false)
  })
})

describe('codeSuggestions', () => {
  const ev = (code: number | null, name: string): EvtCatalogEvent => ({
    code,
    name,
    cat: 0,
    cat_name: 'X',
    lvl: 2,
    text: '',
    console: false,
  })
  const events = [
    ev(701, 'GRIP_REQ'),
    ev(702, 'GRIP_ERROR'),
    ev(100, 'BOOT'),
    ev(null, 'CON_START'),
  ]

  it('suggests by name part or code prefix for the last token', () => {
    expect(codeSuggestions('grip', events)).toEqual(['GRIP_REQ', 'GRIP_ERROR'])
    expect(codeSuggestions('70', events)).toEqual(['GRIP_REQ', 'GRIP_ERROR'])
    expect(codeSuggestions('701, bo', events)).toEqual(['701,BOOT'])
    expect(codeSuggestions('', events, 2)).toEqual(['GRIP_REQ', 'GRIP_ERROR'])
  })
})

describe('datetime-local conversion', () => {
  it('round-trips local time', () => {
    const ms = localInputToMs('2026-09-27T12:34:56')
    expect(ms).toBe(new Date(2026, 8, 27, 12, 34, 56).getTime())
    expect(msToLocalInput(ms)).toBe('2026-09-27T12:34:56')
    expect(localInputToMs('2026-09-27T12:34')).toBe(new Date(2026, 8, 27, 12, 34).getTime())
    expect(localInputToMs('bad')).toBeNull()
    expect(localInputToMs('')).toBeNull()
  })
})
