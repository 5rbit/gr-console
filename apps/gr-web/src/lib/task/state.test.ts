import { describe, expect, it } from 'vitest'
import {
  cascadeAfter,
  ALL_STATES,
  EMPTY_FILTER,
  TERMINAL,
  allowedActions,
  autoBlock,
  deriveState,
  elapsed,
  endedToday,
  fmtElapsed,
  isRobotAction,
  isStationId,
  locate,
  matchesFilter,
  targetOf,
} from './state'
import type { PlcTaskArea } from './state'
import type { PlcTask, Task, TaskState } from '../types'

function plc(work: number, task: number, type = 0x41): PlcTask {
  return { WorkId: work, TaskId: task, TaskType: type, Cell: { Id: 101 } } as unknown as PlcTask
}
const ZERO = plc(0, 0, 0)

function area(over: Partial<PlcTaskArea> = {}): PlcTaskArea {
  return {
    Now: ZERO,
    Queue: [ZERO, ZERO, ZERO, ZERO],
    Completed: Array.from({ length: 10 }, () => ZERO),
    Canceled: Array.from({ length: 10 }, () => ZERO),
    Rejected: Array.from({ length: 10 }, () => ZERO),
    ...over,
  }
}

function task(over: Partial<Task> = {}): Task {
  return {
    id: 't1',
    seq: 1,
    work_id: 260912001,
    task_id: 7,
    origin: 'console',
    request: null,
    resolved: null,
    position: [0, 0, 0, 0],
    plc_task: plc(260912001, 7),
    state: 'queued',
    state_at: '2026-09-12T10:00:00+09:00',
    created_at: '2026-09-12T10:00:00+09:00',
    submitted_at: '2026-09-12T10:00:01+09:00',
    ended_at: null,
    ack: null,
    plc: null,
    error: null,
    history: [],
    ...over,
  }
}

describe('allowedActions', () => {
  it('covers every state', () => {
    for (const s of ALL_STATES) expect(allowedActions(s).length).toBeGreaterThan(0)
  })
  it('terminal states only allow resubmit / delete', () => {
    for (const s of TERMINAL) expect(allowedActions(s)).toEqual(['resubmit', 'delete'])
  })
  it('running task can be canceled, completed or failed but not deleted', () => {
    const a = allowedActions('running')
    expect(a).toContain('cancel')
    expect(a).toContain('complete')
    expect(a).toContain('fail')
    expect(a).not.toContain('delete')
    expect(a).not.toContain('resubmit')
  })
  it('submitted has no complete (no PLC slot yet); lost may be resubmitted', () => {
    expect(allowedActions('submitted')).not.toContain('complete')
    expect(allowedActions('lost')).toContain('resubmit')
    expect(allowedActions('draft')).toContain('submit')
  })
  it('cancel / complete touch the robot', () => {
    expect(isRobotAction('cancel')).toBe(true)
    expect(isRobotAction('complete')).toBe(true)
    expect(isRobotAction('resubmit')).toBe(false)
    expect(isRobotAction('delete')).toBe(false)
  })
})

describe('locate / deriveState', () => {
  const t = task()
  it('finds the key in each array', () => {
    expect(locate(t, area({ Now: plc(260912001, 7) }))).toEqual({ location: 'now', index: 0 })
    expect(locate(t, area({ Queue: [ZERO, plc(260912001, 7), ZERO, ZERO] }))).toEqual({
      location: 'queue',
      index: 1,
    })
    const comp = area()
    comp.Completed[3] = plc(260912001, 7)
    expect(locate(t, comp)).toEqual({ location: 'completed', index: 3 })
    expect(locate(t, area())).toEqual({ location: 'absent', index: null })
  })
  it('agrees when the ledger and the PLC match', () => {
    expect(deriveState(task({ state: 'running' }), area({ Now: plc(260912001, 7) })).mismatch).toBe(
      false,
    )
    expect(
      deriveState(task({ state: 'queued' }), area({ Queue: [plc(260912001, 7), ZERO, ZERO, ZERO] }))
        .mismatch,
    ).toBe(false)
    const comp = area()
    comp.Completed[0] = plc(260912001, 7)
    expect(deriveState(task({ state: 'completed' }), comp).mismatch).toBe(false)
    // rolled off the 10-deep ring is still consistent
    expect(deriveState(task({ state: 'completed' }), area()).mismatch).toBe(false)
    expect(deriveState(task({ state: 'lost' }), area()).mismatch).toBe(false)
  })
  it('flags running-but-not-Now and queued-but-in-Now', () => {
    const d = deriveState(task({ state: 'running' }), area())
    expect(d.mismatch).toBe(true)
    expect(d.reason).toContain('실행 중')
    const q = deriveState(task({ state: 'queued' }), area({ Now: plc(260912001, 7) }))
    expect(q.mismatch).toBe(true)
    expect(q.location).toBe('now')
  })
  it('flags a completed ledger entry that the PLC put in Rejected', () => {
    const rej = area()
    rej.Rejected[0] = plc(260912001, 7)
    expect(deriveState(task({ state: 'completed' }), rej).mismatch).toBe(true)
    expect(deriveState(task({ state: 'rejected' }), rej).mismatch).toBe(false)
  })
  it('never flags transitional states, nor without a PLC snapshot', () => {
    for (const s of ['submitted', 'accepted'] as TaskState[]) {
      expect(deriveState(task({ state: s }), area({ Now: plc(260912001, 7) })).mismatch).toBe(false)
      expect(deriveState(task({ state: s }), area()).mismatch).toBe(false)
    }
    expect(deriveState(task({ state: 'running' }), null).mismatch).toBe(false)
  })
  it('flags a lost task that reappeared', () => {
    expect(deriveState(task({ state: 'lost' }), area({ Now: plc(260912001, 7) })).mismatch).toBe(
      true,
    )
  })
})

describe('elapsed', () => {
  const t0 = Date.parse('2026-09-12T10:00:01+09:00')
  it('measures from submission to now while active', () => {
    expect(elapsed(task(), t0 + 12_500)).toBeCloseTo(12.5)
  })
  it('measures to ended_at once terminal', () => {
    const t = task({ state: 'completed', ended_at: '2026-09-12T10:01:06+09:00' })
    expect(elapsed(t, t0 + 999_999)).toBe(65)
  })
  it('falls back to created_at without submitted_at and null on garbage', () => {
    expect(elapsed(task({ submitted_at: null }), t0 + 1000)).toBe(2)
    expect(elapsed(task({ created_at: 'x', submitted_at: null }))).toBeNull()
    expect(elapsed(task({ state: 'failed', ended_at: null }))).toBeNull()
  })
  it('formats', () => {
    expect(fmtElapsed(null)).toBe('')
    expect(fmtElapsed(3.14)).toBe('3.1s')
    expect(fmtElapsed(65)).toBe('1m 05s')
    expect(fmtElapsed(3725)).toBe('1h 02m')
  })
  it('endedToday compares local dates', () => {
    const now = Date.parse('2026-09-12T23:00:00+09:00')
    expect(endedToday(task({ ended_at: '2026-09-12T01:00:00+09:00' }), now)).toBe(true)
    expect(endedToday(task({ ended_at: '2026-09-11T23:59:00+09:00' }), now)).toBe(false)
    expect(endedToday(task({ ended_at: null }), now)).toBe(false)
  })
})

describe('targetOf / isStationId', () => {
  it('prefers the request target, else derives from Cell.Id', () => {
    expect(
      targetOf(task({ request: { target: { kind: 'station', id: 2101 } } as Task['request'] })),
    ).toEqual({
      kind: 'station',
      id: 2101,
    })
    expect(targetOf(task())).toEqual({ kind: 'cell', id: 101 })
    expect(
      targetOf(
        task({
          plc_task: plc(1, 1) && ({ ...plc(1, 1), Cell: { Id: 2102 } } as unknown as PlcTask),
        }),
      ),
    ).toEqual({
      kind: 'station',
      id: 2102,
    })
    expect(targetOf(task({ plc_task: null }))).toBeNull()
  })
  it('station id rule 2001..2999 with MOD 100 in 1..32', () => {
    expect(isStationId(2101)).toBe(true)
    expect(isStationId(2132)).toBe(true)
    expect(isStationId(2133)).toBe(false)
    expect(isStationId(2100)).toBe(false)
    expect(isStationId(101)).toBe(false)
  })
})

describe('matchesFilter', () => {
  const typeName = (c: number) => (c === 0x41 ? 'PICK' : c === 0x42 ? 'DROP' : '')
  it('hides terminal by default and shows them with includeTerminal or an explicit chip', () => {
    const done = task({ state: 'completed' })
    expect(matchesFilter(done, EMPTY_FILTER, typeName)).toBe(false)
    expect(matchesFilter(done, { ...EMPTY_FILTER, includeTerminal: true }, typeName)).toBe(true)
    expect(matchesFilter(done, { ...EMPTY_FILTER, states: ['completed'] }, typeName)).toBe(true)
    expect(matchesFilter(task(), { ...EMPTY_FILTER, states: ['completed'] }, typeName)).toBe(false)
  })
  it('filters by type and free text', () => {
    expect(matchesFilter(task(), { ...EMPTY_FILTER, type: 'PICK' }, typeName)).toBe(true)
    expect(matchesFilter(task(), { ...EMPTY_FILTER, type: 'DROP' }, typeName)).toBe(false)
    expect(matchesFilter(task(), { ...EMPTY_FILTER, q: '101' }, typeName)).toBe(true)
    expect(matchesFilter(task(), { ...EMPTY_FILTER, q: 'zzz' }, typeName)).toBe(false)
    expect(
      matchesFilter(
        task({
          ack: { accepted: false, code: 429, reason: 'INVALID_CELL', reject_bits: 4, at: '' },
        }),
        { ...EMPTY_FILTER, q: 'invalid_cell' },
        typeName,
      ),
    ).toBe(true)
  })
})

describe('cascadeAfter', () => {
  const t = (id: string, work: number, tid: number, state: TaskState) =>
    task({ id, seq: tid, work_id: work, task_id: tid, state })
  it('같은 WorkId의 뒤 Task 중 살아 있는 것만, TaskId 순으로', () => {
    const me = t('me', 7, 2, 'running')
    const all = [
      t('a', 7, 1, 'completed'),
      me,
      t('d', 7, 4, 'queued'),
      t('c', 7, 3, 'accepted'),
      t('e', 7, 5, 'draft'),
      t('f', 7, 6, 'canceled'),
      t('g', 8, 3, 'queued'),
    ]
    expect(cascadeAfter(all, me).map((x) => x.id)).toEqual(['c', 'd'])
    expect(cascadeAfter(all, t('d', 7, 4, 'queued'))).toEqual([])
  })
  it('WorkId가 없는(제출 전) Task는 꼬리가 없다', () => {
    expect(cascadeAfter([t('x', 7, 3, 'queued')], t('me', 0, 0, 'draft'))).toEqual([])
  })
})

describe('autoBlock', () => {
  it('blocks PLC complete / delete only in AUTO', () => {
    expect(autoBlock('complete', 'running', 'AUTO')).toContain('AUTO')
    expect(autoBlock('cancel', 'queued', 'AUTO')).toContain('AUTO')
    expect(autoBlock('complete', 'running', 'READY')).toBeUndefined()
    expect(autoBlock('cancel', 'queued', 'MANUAL')).toBeUndefined()
    expect(autoBlock('cancel', 'running', null)).toBeUndefined()
  })
  it('never blocks console-only actions', () => {
    expect(autoBlock('cancel', 'draft', 'AUTO')).toBeUndefined()
    expect(autoBlock('delete', 'completed', 'AUTO')).toBeUndefined()
    expect(autoBlock('fail', 'running', 'AUTO')).toBeUndefined()
  })
})
