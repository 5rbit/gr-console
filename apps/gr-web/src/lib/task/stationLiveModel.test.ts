import { describe, expect, it } from 'vitest'
import { interlockLamps, stationFill, stationMatch, stationRing, tireOf } from './stationLiveModel'
import type { StationLive } from './types'

const live = (p: Partial<StationLive> = {}): StationLive => ({
  id: 2101,
  live: true,
  why: null,
  source: 'OPCUA',
  age_ms: null,
  disconnected: false,
  stale: false,
  rotate_type: 6,
  sensor: false,
  state: 0,
  error_code: 0,
  item_detect: false,
  has_tracking: false,
  measuring: false,
  measuring_error: false,
  data_mismatch: false,
  od: 0,
  tx: 0,
  ty: 0,
  prev: 0,
  next: 0,
  next_id: null,
  pi: { cvok: true, req: false, meas_req: false, item_exist: false },
  po: { cvno: false, comp: false, meas_comp: false, meas_err: false },
  ...p,
})
const item = { ...live().pi, item_exist: true }
const code = { item_code: 1001, count: 1 }

describe('stationFill', () => {
  it('Use=false, missing or stale GRM', () => {
    expect(stationFill(false, live())).toBe('disabled')
    expect(stationFill(true, undefined)).toBe('unknown')
    expect(stationFill(true, live({ stale: true }))).toBe('unknown')
    expect(stationFill(true, live())).toBe('normal')
  })
})

describe('stationMatch', () => {
  it('no GRM → no mark; empty → no mark', () => {
    expect(stationMatch(undefined, code).state).toBeNull()
    expect(stationMatch(live(), null).state).toBeNull()
  })

  it('item + code (+ tracking where expected) → green', () => {
    expect(stationMatch(live({ pi: item }), code).state).toBe('ok')
    expect(stationMatch(live({ pi: item, sensor: true, od: 640 }), code).state).toBe('ok')
    expect(stationMatch(live({ pi: item, prev: 2, has_tracking: true }), code).state).toBe('ok')
  })

  it('item without code, or without expected tracking → red', () => {
    const m = stationMatch(live({ pi: item }), null)
    expect(m.state).toBe('mismatch')
    expect(m.reasons.join()).toContain('화물 코드 없음')
    expect(stationMatch(live({ pi: item, sensor: true }), code).reasons.join()).toContain('트래킹')
    // 측정 중에는 트래킹이 아직 없어도 빨강이 아니다
    expect(stationMatch(live({ pi: item, sensor: true, state: 2 }), code).state).toBe('ok')
  })

  it('leftovers without item → red, but a code in transit is fine', () => {
    expect(stationMatch(live({ od: 640 }), null).state).toBe('mismatch')
    expect(stationMatch(live({ next_id: 0 }), code).state).toBe('mismatch')
    expect(stationMatch(live({ next_id: null }), code).state).toBe('mismatch')
    expect(stationMatch(live({ next_id: 2101 }), code).state).toBeNull()
  })

  it('GRM errors are red even when empty', () => {
    expect(stationMatch(live({ error_code: 3 }), null).state).toBe('mismatch')
    expect(stationMatch(live({ po: { ...live().po, meas_err: true } }), null).state).toBe(
      'mismatch',
    )
  })
})

describe('stationRing', () => {
  it('none without GRM or when idle', () => {
    expect(stationRing(undefined)).toBeNull()
    expect(stationRing(live())).toBeNull()
  })

  it('priority: cv_not_ready > robot_in > comp > req > meas_req', () => {
    const all = live({
      pi: { cvok: false, req: true, meas_req: true, item_exist: true },
      po: { cvno: true, comp: true, meas_comp: false, meas_err: false },
    })
    expect(stationRing(all)).toBe('cv_not_ready')
    const ok = { ...all.pi, cvok: true }
    expect(stationRing({ ...all, pi: ok })).toBe('robot_in')
    expect(stationRing({ ...all, pi: ok, po: { ...all.po, cvno: false } })).toBe('comp')
    expect(stationRing(live({ pi: { ...live().pi, req: true, meas_req: true } }))).toBe('req')
    expect(stationRing(live({ pi: { ...live().pi, meas_req: true } }))).toBe('meas_req')
  })
})

it('interlock lamps list IN and OUT with state', () => {
  const l = interlockLamps(live({ po: { ...live().po, cvno: true } }))
  expect(l.pi.map((x) => [x.name, x.on])).toEqual([
    ['CVOK', true],
    ['Req', false],
    ['MeasReq', false],
    ['ItemExist', false],
  ])
  expect(l.po.find((x) => x.name === 'CVNO')?.on).toBe(true)
})

describe('tireOf', () => {
  it('tracking: wall side ±OD/2 plus cross TaskOffset', () => {
    expect(tireOf(6, 640, 35, 9, 780, 500)).toEqual({ dx: 35, dy: 320, d: 640, source: 'tracking' })
    expect(tireOf(1, 640, 35, 9, 0, 500)).toEqual({ dx: 35, dy: -320, d: 640, source: 'tracking' })
    expect(tireOf(3, 640, 35, 9, 0, 500)).toEqual({ dx: 320, dy: 9, d: 640, source: 'tracking' })
    expect(tireOf(4, 640, 35, 9, 0, 500)).toEqual({ dx: -320, dy: 9, d: 640, source: 'tracking' })
    expect(tireOf(0, 640, 35, 9, 0, 500)).toEqual({ dx: 35, dy: 9, d: 640, source: 'tracking' })
  })

  it('falls back to item OD (no cross offset), then nominal', () => {
    expect(tireOf(6, 0, 35, 9, 780, 500)).toEqual({ dx: 0, dy: 390, d: 780, source: 'item' })
    expect(tireOf(0, 0, 0, 0, 0, 500)).toEqual({ dx: 0, dy: 0, d: 500, source: 'nominal' })
  })
})
