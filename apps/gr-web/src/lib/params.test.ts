import { describe, expect, it } from 'vitest'
import { fromText, toText, type ParamSpec } from './params'
import { metricsLine } from './taskgen'

const spec = (key: string, min: number | null, max: number | null): ParamSpec => ({
  key,
  group: 'g',
  unit: 'mm',
  min,
  max,
  locked: false,
  source: null,
  help: '',
})

describe('params text round trip', () => {
  it('numbers, ranges, maps, booleans, optional', () => {
    expect(fromText('2500', spec('anticol_separation_mm', 0, 20000), 2403)).toEqual({ ok: 2500 })
    expect(fromText('-1', spec('anticol_separation_mm', 0, 20000), 2403)).toMatchObject({
      error: expect.stringContaining('0..20000'),
    })
    expect(fromText('x', spec('gen_tick_ms', 200, 10000), 1000)).toMatchObject({
      error: expect.any(String),
    })
    expect(fromText('1:200, 2:150', spec('robot_margin_mm', 0, 3000), {})).toEqual({
      ok: { '1': 200, '2': 150 },
    })
    expect(fromText('1:9000', spec('robot_margin_mm', 0, 3000), {})).toMatchObject({
      error: expect.any(String),
    })
    expect(fromText('true', spec('stack_max_enforce', null, null), true)).toEqual({ ok: true })
    expect(fromText('', spec('echo_timeout_ms', 500, 60000), null)).toEqual({ ok: null })
    expect(toText({ '1': 200 })).toBe('1:200')
    expect(toText(null)).toBe('')
  })

  it('metrics line shows the top wait reasons', () => {
    expect(
      metricsLine({
        started_at: '',
        generated: 3,
        issued: 5,
        completed_items: 2,
        aborted: 0,
        submit_failures: 1,
        waits: { area: 9, gate: 2, busy: 4, pair: 1 },
      }),
    ).toBe('생성 3 · 발행 5 · 끝남 2 · 중단 0 · 제출 실패 1 — 대기 area 9 · busy 4 · gate 2')
  })
})
