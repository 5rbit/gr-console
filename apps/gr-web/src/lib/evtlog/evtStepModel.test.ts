import { describe, expect, it } from 'vitest'
import type { EventRow } from './api'
import { barGeometry, enumLabel, stepBars } from './evtStepModel'

function step(
  id: number,
  ts_ms: number,
  proc: number,
  from: number,
  to: number,
  dwell: number,
): EventRow {
  return {
    id,
    plc: 'GR2',
    origin: 'plc',
    epoch: 1,
    seq: id,
    ts: '',
    ts_ms,
    rx_ts: '',
    cat: 5,
    cat_name: 'STEP',
    lvl: 1,
    lvl_name: 'DEBUG',
    src: proc,
    code: to,
    name: 'STEP',
    a: dwell,
    b: from,
    ctx: 42,
    detail: null,
    text: '',
  }
}

const enums = { proc: { '20': 'Task', '30': 'Grip' }, task_step: { '100': 'Init' } }

describe('stepBars', () => {
  it('turns a STEP row into the dwell bar of the step that was left', () => {
    const c = stepBars([step(1, 10_000, 20, 100, 200, 1500)], enums)
    expect(c.lanes).toHaveLength(1)
    const b = c.lanes[0].bars[0]
    expect(b).toMatchObject({
      step: 100,
      stepName: 'Init',
      next: 200,
      start: 8500,
      dur: 1500,
      proc: 20,
    })
    expect(b.procName).toBe('Task')
    expect(c.t0).toBe(8500)
    expect(c.t1).toBe(10_000)
    expect(c.total).toBe(1500)
  })

  it('sorts out-of-order input by start and splits lanes per proc', () => {
    const rows = [
      step(3, 13_000, 20, 300, 400, 1000), // 12000..13000
      step(2, 12_000, 30, 10, 20, 4000), // 8000..12000
      step(1, 11_000, 20, 200, 300, 2000), // 9000..11000
    ]
    const c = stepBars(rows, enums)
    expect(c.lanes.map((l) => l.procName)).toEqual(['Grip', 'Task'])
    expect(c.lanes[1].bars.map((b) => b.step)).toEqual([200, 300])
    expect(c.t0).toBe(8000)
    expect(c.t1).toBe(13_000)
  })

  it('ignores non-STEP rows and falls back to numbers without enums', () => {
    const other = { ...step(9, 5000, 20, 1, 2, 10), cat_name: 'GRIP' }
    const c = stepBars([other, step(1, 1000, 99, 7, 8, 100)])
    expect(c.lanes).toHaveLength(1)
    expect(c.lanes[0].procName).toBe('99')
    expect(c.lanes[0].bars[0].stepName).toBe('7')
  })

  it('is empty for no rows', () => {
    expect(stepBars([])).toEqual({ lanes: [], t0: 0, t1: 0, total: 0 })
  })
})

describe('geometry and labels', () => {
  it('places bars in percent with a minimum width', () => {
    const c = stepBars([step(1, 2000, 20, 1, 2, 1000), step(2, 2000, 20, 2, 3, 0)])
    const [a, z] = c.lanes[0].bars
    expect(barGeometry(a, c.t0, c.t1)).toEqual({ left: 0, width: 100 })
    const g = barGeometry(z, c.t0, c.t1)
    expect(g.width).toBeGreaterThan(0)
    expect(g.left + g.width).toBeLessThanOrEqual(100)
  })
  it('resolves enum labels', () => {
    expect(enumLabel(enums, 'proc', 30)).toBe('Grip')
    expect(enumLabel(null, 'proc', 30)).toBe('30')
  })
})
