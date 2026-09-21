import { describe, expect, it } from 'vitest'
import {
  ROBOT_ACTIONS,
  robotActionDisabled,
  robotActionDone,
  robotActionSpec,
} from './robotCommandModel'

describe('robotCommandModel', () => {
  it('lists the five commands in menu order', () => {
    expect(ROBOT_ACTIONS.map((s) => s.label)).toEqual([
      'Start',
      'Stop',
      'Reset',
      'Buzzer Stop',
      'Complete',
      'Clear',
    ])
  })

  it('never puts Stop behind a confirmation', () => {
    expect(robotActionSpec('stop').confirm).toBe(false)
    expect(robotActionSpec('buzzerstop').confirm).toBe(false)
    expect(robotActionSpec('clear').confirm).toBe(true)
  })

  it('blocks everything without a command path', () => {
    const r = { cmd_ready: false, cmd_error: 'OPC UA 세션 재연결 중' }
    for (const s of ROBOT_ACTIONS) {
      expect(robotActionDisabled(s.action, r, 'READY')).toContain('OPC UA 세션 재연결 중')
    }
  })

  it('blocks Start when already AUTO', () => {
    const r = { cmd_ready: true, cmd_error: null }
    expect(robotActionDisabled('start', r, 'AUTO')).toBe('이미 AUTO')
    expect(robotActionDisabled('start', r, 'READY')).toBeUndefined()
    expect(robotActionDisabled('stop', r, 'AUTO')).toBeUndefined()
  })

  it('blocks Complete / Clear in AUTO only', () => {
    const r = { cmd_ready: true, cmd_error: null }
    expect(robotActionDisabled('complete', r, 'AUTO')).toContain('AUTO')
    expect(robotActionDisabled('clear', r, 'AUTO')).toContain('AUTO')
    expect(robotActionDisabled('complete', r, 'READY')).toBeUndefined()
    expect(robotActionDisabled('clear', r, 'MANUAL')).toBeUndefined()
  })

  it('reports what the backend did', () => {
    expect(robotActionDone('clear', 'GR2', { deleted: ['5/1', '6/1'] })).toBe(
      'GR2 Clear 요청 — 2건 (5/1, 6/1)',
    )
    expect(robotActionDone('complete', 'GR2', { task: '5/1' })).toBe('GR2 Complete 요청 — Task 5/1')
    expect(robotActionDone('reset', 'GR1', {})).toBe('GR1 Reset 보냄')
  })
})
