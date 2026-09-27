import { describe, expect, it } from 'vitest'
import type { EventRow, SwimPoint } from './api'
import {
  EMPTY_SWIM,
  groupLanes,
  highSegs,
  isStationId,
  segments,
  stepPath,
  swimEntry,
  swimQuery,
  swimRange,
  ticks,
  valueAt,
  zoomTo,
} from './swimlaneModel'

const pts: SwimPoint[] = [
  { t: 0, v: 1 },
  { t: 100, v: 0 },
  { t: 300, v: null },
  { t: 500, v: 1 },
]

const evt = (p: Partial<EventRow>): EventRow => ({
  id: 1,
  plc: 'GR2',
  origin: 'plc',
  epoch: 1,
  seq: 1,
  ts: '',
  ts_ms: 1_000_000,
  rx_ts: '',
  cat: 12,
  cat_name: 'ILOCK',
  lvl: 2,
  lvl_name: 'INFO',
  src: 0,
  code: 1203,
  name: 'ILK_PO',
  a: 1,
  b: 0,
  ctx: 7,
  detail: null,
  text: '',
  ...p,
})

describe('segments and values', () => {
  it('each point lasts until the next, clipped to the window', () => {
    expect(segments(pts, 50, 600)).toEqual([
      { t0: 50, t1: 100, v: 1 },
      { t0: 100, t1: 300, v: 0 },
      { t0: 300, t1: 500, v: null },
      { t0: 500, t1: 600, v: 1 },
    ])
    expect(segments(pts, 120, 250)).toEqual([{ t0: 120, t1: 250, v: 0 }])
  })
  it('value at a time', () => {
    expect(valueAt(pts, -1)).toBeNull()
    expect(valueAt(pts, 100)).toBe(0)
    expect(valueAt(pts, 400)).toBeNull()
    expect(valueAt(pts, 9999)).toBe(1)
  })
  it('step path breaks at null and draws high at the top', () => {
    const d = stepPath(pts, { t0: 0, t1: 1000 }, 100, 20)
    expect(d).toBe('M0 3H10V17H30M50 3H100')
    expect(highSegs(pts, { t0: 0, t1: 1000 }).map((s) => s.t0)).toEqual([0, 500])
    expect(stepPath(pts, { t0: 0, t1: 1000 }, 100, 20, 700)).toBe('M0 3H10V17H30M50 3H70')
    expect(highSegs(pts, { t0: 0, t1: 1000 }, 400).map((s) => s.t0)).toEqual([0])
  })
})

describe('view math', () => {
  it('drag range zooms, a click does not', () => {
    const v = { t0: 0, t1: 10_000 }
    expect(zoomTo(20, 70, v, 100)).toEqual({ t0: 2000, t1: 7000 })
    expect(zoomTo(70, 20, v, 100)).toEqual({ t0: 2000, t1: 7000 })
    expect(zoomTo(20, 22, v, 100)).toBeNull()
    expect(zoomTo(20, 25, { t0: 0, t1: 1000 }, 100)).toBeNull()
  })
  it('ticks land on round steps', () => {
    expect(ticks({ t0: 0, t1: 600_000 })).toEqual([0, 120_000, 240_000, 360_000, 480_000, 600_000])
    expect(ticks({ t0: 1500, t1: 6500 })).toEqual([2000, 3000, 4000, 5000, 6000])
  })
  it('groups keep their order', () => {
    const l = (group: string, key: string) => ({ key, group, label: key, points: [] })
    expect(
      groupLanes([l('GR1', 'a'), l('GRM', 'b'), l('GR1', 'c')]).map((g) => g.lanes.length),
    ).toEqual([2, 1])
  })
})

describe('query and entry', () => {
  it('window defaults to the last 10 minutes', () => {
    expect(swimRange(EMPTY_SWIM, 1_000_000)).toEqual({ from: 400_000, to: 1_000_000 })
    expect(swimRange({ ...EMPTY_SWIM, from: 1, to: 2 }, 9)).toEqual({ from: 1, to: 2 })
  })
  it('station first, then slot, then event', () => {
    expect(swimQuery(EMPTY_SWIM, 0)).toBeNull()
    expect(swimQuery({ ...EMPTY_SWIM, station: 2101, slot: 3 }, 600_000)).toBe(
      '?station=2101&from=0&to=600000',
    )
    expect(swimQuery({ ...EMPTY_SWIM, event: 42, from: 5, to: 6 }, 0)).toBe('?event=42&from=5&to=6')
  })
  it('entries from ILOCK and STATION rows', () => {
    expect(swimEntry(evt({}))).toMatchObject({ event: 1, from: 700_000, to: 1_300_000 })
    expect(swimEntry(evt({ name: 'ILK_TARGET', code: 1201, a: 2101 }))).toMatchObject({
      station: 2101,
    })
    expect(swimEntry(evt({ name: 'ILK_TARGET', code: 1201, a: 105 }))).toMatchObject({ event: 1 })
    expect(swimEntry(evt({ plc: 'GRM', cat: 16, cat_name: 'STATION', src: 2 }))).toMatchObject({
      slot: 2,
    })
    expect(swimEntry(evt({ cat: 3, cat_name: 'STEP' }))).toBeNull()
    expect([isStationId(2101), isStationId(2100), isStationId(2133), isStationId(101)]).toEqual([
      true,
      false,
      false,
      false,
    ])
  })
})
