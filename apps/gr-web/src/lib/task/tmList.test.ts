import { describe, expect, it } from 'vitest'
import {
  EMPTY_HISTORY_FILTER,
  filterCount,
  historyQuery,
  liveQueue,
  sinceIso,
  startedAt,
  terminalCount,
} from './tmList'
import type { Task, TaskState } from '../types'

let n = 0
function task(state: TaskState, over: Partial<Task> = {}): Task {
  n++
  return {
    id: `t${n}`,
    seq: n,
    work_id: 90000 + n,
    task_id: 1,
    origin: 'external',
    plc_name: 'GR2',
    request: null,
    resolved: null,
    position: [0, 0, 0, 0],
    plc_task: null,
    state,
    state_at: '2026-09-21T10:00:00+09:00',
    created_at: '2026-09-21T10:00:00+09:00',
    submitted_at: null,
    ended_at: null,
    ack: null,
    plc: null,
    error: null,
    history: [],
    ...over,
  }
}
const slot = (i: number) => ({ step: 0, queue_index: i, last_seen_at: '' })

describe('liveQueue', () => {
  it('keeps only active tasks of the selected robot: running, queue slot order, then the rest', () => {
    const q2 = task('queued', { plc: slot(2) })
    const q0 = task('queued', { plc: slot(0) })
    const run = task('running')
    const sub = task('submitted')
    const lost = task('lost')
    const done = task('completed')
    const other = task('running', { plc_name: 'GR1' })
    const got = liveQueue([lost, q2, done, sub, other, q0, run], 'GR2')
    expect(got.map((t) => t.id)).toEqual([run.id, q0.id, q2.id, sub.id, lost.id])
  })
  it('null robot = every robot', () => {
    const a = task('running', { plc_name: 'GR1' })
    const b = task('running', { plc_name: 'GR2' })
    expect(liveQueue([a, b], null)).toHaveLength(2)
  })
})

describe('terminalCount', () => {
  it('counts terminal tasks of the robot only', () => {
    const list = [
      task('completed'),
      task('rejected'),
      task('running'),
      task('canceled', { plc_name: 'GR1' }),
    ]
    expect(terminalCount(list, 'GR2')).toBe(2)
    expect(terminalCount(list, null)).toBe(3)
  })
})

describe('startedAt', () => {
  it('is the first transition to running', () => {
    const t = task('completed', {
      history: [
        { from: null, to: 'queued', at: 'a', by: 'plc', note: null },
        { from: 'queued', to: 'running', at: 'b', by: 'plc', note: null },
        { from: 'running', to: 'completed', at: 'c', by: 'plc', note: null },
      ],
    })
    expect(startedAt(t)).toBe('b')
    expect(startedAt(task('queued'))).toBeNull()
  })
})

describe('history query', () => {
  it('defaults to every terminal state of the robot', () => {
    expect(historyQuery(EMPTY_HISTORY_FILTER, 2, 0, 20)).toEqual({
      robot: 2,
      state: 'terminal',
      type: undefined,
      origin: undefined,
      since: undefined,
      offset: 0,
      limit: 20,
    })
  })
  it('passes filters and drops non-terminal states', () => {
    const q = historyQuery(
      { states: ['rejected', 'running'], type: 'PICK', origin: 'external', since: '2026-09-20' },
      null,
      40,
      20,
    )
    expect(q.state).toEqual(['rejected'])
    expect(q.type).toBe('PICK')
    expect(q.origin).toBe('external')
    expect(q.robot).toBeUndefined()
    expect(q.since).toBe(new Date(2026, 8, 20).toISOString())
    expect(q.offset).toBe(40)
  })
  it('only non-terminal states picked = all terminal', () => {
    expect(historyQuery({ ...EMPTY_HISTORY_FILTER, states: ['queued'] }, 1, 0, 20).state).toBe(
      'terminal',
    )
  })
  it('sinceIso rejects junk', () => {
    expect(sinceIso('')).toBeUndefined()
    expect(sinceIso('2026/09/20')).toBeUndefined()
  })
  it('filterCount counts set fields', () => {
    expect(filterCount(EMPTY_HISTORY_FILTER)).toBe(0)
    expect(
      filterCount({ states: ['completed'], type: 'DROP', origin: 'console', since: '2026-01-01' }),
    ).toBe(4)
  })
})
