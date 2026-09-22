import { describe, expect, it } from 'vitest'
import {
  autoDecision,
  awaitingEcho,
  clampLimit,
  robotQueueCount,
  type AutoInput,
} from './autoSubmitModel'

const base: AutoInput = {
  on: true,
  busy: false,
  remaining: 3,
  queue: 0,
  limit: 1,
  gateOk: true,
  awaitingEcho: false,
}

describe('robotQueueCount', () => {
  it('counts only this robot’s tasks that went to the PLC and are not over', () => {
    const t = [
      { plc_name: 'GR2', state: 'submitted' as const },
      { plc_name: 'GR2', state: 'running' as const },
      { plc_name: 'GR2', state: 'queued' as const },
      { plc_name: 'GR2', state: 'completed' as const },
      { plc_name: 'GR2', state: 'draft' as const },
      { plc_name: 'GR2', state: 'lost' as const },
      { plc_name: 'GR1', state: 'queued' as const },
    ]
    expect(robotQueueCount(t, 'GR2')).toBe(3)
    expect(robotQueueCount(t, 'GR1')).toBe(1)
    expect(robotQueueCount(t, null)).toBe(0)
  })
})

describe('autoDecision', () => {
  it('sends while the queue is at or under N', () => {
    expect(autoDecision({ ...base, queue: 1, limit: 1 })).toEqual({ go: true })
    expect(autoDecision({ ...base, queue: 2, limit: 1 })).toEqual({ go: false, wait: '큐 2 > 1' })
  })
  it('never sends twice before the last submission shows up', () => {
    expect(autoDecision({ ...base, awaitingEcho: true }).go).toBe(false)
    expect(autoDecision({ ...base, busy: true }).go).toBe(false)
  })
  it('waits on a closed gate with its reason and ends on an empty plan', () => {
    expect(autoDecision({ ...base, gateOk: false, gateReason: 'GR2: AUTO 모드가 아님' })).toEqual({
      go: false,
      wait: '게이트 — GR2: AUTO 모드가 아님',
    })
    expect(autoDecision({ ...base, remaining: 0 })).toEqual({ go: false, done: true })
    expect(autoDecision({ ...base, on: false }).go).toBe(false)
  })
  it('keeps N inside the PLC buffer', () => {
    expect(clampLimit(9)).toBe(4)
    expect(clampLimit(-1)).toBe(0)
    expect(clampLimit(Number.NaN)).toBe(1)
  })
})

describe('awaitingEcho', () => {
  const t = (id: string, state: 'submitted' | 'accepted' | 'draft', plc = 'GR2') => ({
    id,
    plc_name: plc,
    state,
  })
  it('waits until the last auto task left "submitted" (server gate refuses otherwise)', () => {
    expect(awaitingEcho([], 'a', 'GR2')).toBe(true) // 아직 SSE 에 없음
    expect(awaitingEcho([t('a', 'submitted')], 'a', 'GR2')).toBe(true)
    expect(awaitingEcho([t('a', 'accepted')], 'a', 'GR2')).toBe(false)
  })
  it('also waits on any other echo-pending submission of the same robot only', () => {
    expect(awaitingEcho([t('x', 'submitted')], null, 'GR2')).toBe(true)
    expect(awaitingEcho([t('x', 'submitted', 'GR1')], null, 'GR2')).toBe(false)
  })
})
