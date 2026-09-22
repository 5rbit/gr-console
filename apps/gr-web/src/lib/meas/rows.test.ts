import { describe, expect, it } from 'vitest'
import type { MeasLogEntry, PlcTask } from '../types'
import { CSV_COLUMNS, toCsv } from './csv'
import { METRICS } from './const'
import { f1, f2, flagStr, tt } from './format'
import { codesOf, filt, flatten, summary } from './rows'
import { ema, nearest, stats, trendPoints } from './trend'

function task(over: Partial<PlcTask> = {}): PlcTask {
  return {
    WorkId: 5001,
    TaskId: 2,
    TaskType: 0x41,
    Position: [12000, 3000, 1980, 300],
    Item: { Code: 1001, Count: 3, InnerDiameter: 381, OuterDiameter: 780, LowerBidHeight: 20, UpperBidHeight: 220, Height: 240, DeflectionFactor: 0 },
    Cell: { Use: true, BlendUse: false, Id: 101, Section: 2, Row: 1, Col: 1, Lenth: 1200, Width: 1200, Position: [12000, 3000, 1500] },
    BlendUpDistance: 300, BlendDownDistance: 300, DragOutHeight: 0, DragOutDist: 0, DragOutDir: 0, DragInHeight: 0, DragInDist: 0, DragInDir: 0,
    LiftUpCreepDistance: 30, LiftDownCreepDistance: 30, LiftUpHeight: 2500, PreGripDelta: 10, GripBackDelta: 5, GripHeight: 40,
    UseDragOut: false, UseDragIn: false, LiftUpAfterComplete: true, LiftUpPartial: false, MeasureFloor: false, MeasureItem: true, MeasureSku: false,
    AdjustCenter: false, FindStationItem: false, Avoid: false, Outbound: false,
    ...over,
  }
}

function entry(seq: number, kind: number, data: number[], delta = { InnerDia: 1.5, Height: 0, Z: 2, Offset: 0.5, Count: 0 }): MeasLogEntry {
  return { TimeStamp: '2026-09-12 10:00:00.000', Seq: seq, Kind: kind, Status: 2, Cmd: task(), Data: data, Delta: delta }
}

describe('rows', () => {
  it('flattens command and delta fields and keeps backend order', () => {
    const rows = flatten([entry(3, 1, [2, 382.5, 258, 480]), entry(2, 4, [2, 24.6])])
    expect(rows.map((r) => r.seq)).toEqual([3, 2])
    expect(rows[0].kindName).toBe('Item')
    expect(rows[0].cmdZRel).toBe(480)
    expect(rows[0].flags).toBe('IL')
    expect(rows[0].dInnerDia).toBe(1.5)
    expect(summary(rows[0])).toContain('InnerDia 382.5')
    expect(summary(rows[1])).toContain('LastBeadPos−Target 24.6')
  })
  it('filters by kind and code', () => {
    const rows = flatten([entry(1, 1, []), entry(2, 4, [])])
    expect(filt(rows, '1', '').length).toBe(1)
    expect(filt(rows, '', 1001).length).toBe(2)
    expect(filt(rows, '', 9999).length).toBe(0)
    expect(codesOf([], rows)).toEqual([1001])
  })
})

describe('format', () => {
  it('formats numbers and codes', () => {
    expect(f1(1234.56)).toBe('1235')
    expect(f1(12.34)).toBe('12.3')
    expect(f2(null)).toBe('')
    expect(tt(0x44)).toBe('MEASURE')
    expect(tt(0x99)).toBe('0x99')
    expect(flagStr(task({ MeasureItem: false, LiftUpAfterComplete: false }))).toBe('-')
  })
})

describe('trend', () => {
  it('builds points oldest first and drops zero deltas for common metrics', () => {
    const rows = flatten([entry(3, 1, [], { InnerDia: 0, Height: 0, Z: 0, Offset: 0, Count: 0 }), entry(2, 1, []), entry(1, 1, [])])
    const m = METRICS.find((x) => x.id === 'dInnerDia')!
    const pts = trendPoints(rows, m)
    expect(pts.map((p) => p.x)).toEqual([1, 2])
    expect(ema([1, 1, 3])).toEqual([1, 1, 1.4])
    const s = stats([1, 2, 3])
    expect(s.avg).toBe(2)
    expect(s.min).toBe(1)
    expect(nearest(pts, 100, (x) => x * 50)).toEqual(pts[1])
    expect(nearest(pts, 500, (x) => x * 50)).toBeNull()
  })
})

describe('csv', () => {
  it('has header, BOM and 20 data columns', () => {
    const rows = flatten([entry(1, 1, [2, 381])])
    const text = toCsv(rows)
    expect(text.charCodeAt(0)).toBe(0xfeff)
    const lines = text.slice(1).trimEnd().split('\n')
    expect(lines[0].split(',').length).toBe(CSV_COLUMNS.length + 20)
    expect(lines[1].split(',').length).toBe(CSV_COLUMNS.length + 20)
    expect(lines[1]).toContain('381')
  })
})
