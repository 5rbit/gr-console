import { describe, expect, it } from 'vitest'
import type { WebMon } from '../types'
import { cargoMismatch, cargoVisible, plcHoldOf, robotCargo } from './robotCargoModel'

describe('robotCargo', () => {
  it('empty on both sides', () => {
    const c = robotCargo(null, false, false)
    expect(c.state).toBe('none')
    expect(cargoVisible(c)).toBe(false)
  })
  it('console and PLC agree', () => {
    const c = robotCargo({ item_code: 1001, count: 2 }, true, true)
    expect(c).toMatchObject({ state: 'ok', itemCode: 1001, count: 2 })
    expect(cargoMismatch(c)).toBe(false)
  })
  it('PLC holds, console empty', () => {
    const c = robotCargo({ item_code: 0, count: 0 }, true, true)
    expect(c.state).toBe('plc_only')
    expect(cargoVisible(c)).toBe(true)
    expect(cargoMismatch(c)).toBe(true)
  })
  it('console holds, PLC empty', () => {
    expect(robotCargo({ item_code: 7, count: 1 }, false, false).state).toBe('console_only')
  })
  it('falls back to the sensor when HoldItem is missing', () => {
    expect(robotCargo(null, null, true).state).toBe('plc_only')
  })
  it('unknown PLC keeps the console hand', () => {
    expect(robotCargo({ item_code: 7, count: 1 }, null, null).state).toBe('unknown_plc')
  })
  it('zero count ignores a stale item code', () => {
    expect(robotCargo({ item_code: 9, count: 0 }, false, false).itemCode).toBe(0)
  })
  it('notes a HoldItem/sensor disagreement', () => {
    expect(robotCargo({ item_code: 7, count: 1 }, true, false).text).toContain('ItemDetect 0')
  })
})

describe('plcHoldOf', () => {
  it('reads HoldItem and ItemDetect', () => {
    const wm = {
      Stat: { Task: { Status: { HoldItem: true } } },
      Gripper: { ItemDetect: false },
    } as unknown as WebMon
    expect(plcHoldOf(wm)).toEqual({ hold: true, detect: false })
  })
  it('null without a stream', () => {
    expect(plcHoldOf(null)).toEqual({ hold: null, detect: null })
  })
})
