import { describe, expect, it } from 'vitest'
import type { PlanStep } from '../task/plan'
import type { TaskRequest } from '../types'
import {
  groupOf,
  placeJobs,
  route,
  stepsToJobs,
  targetCode,
  taskLabel,
  whoOf,
  type Job,
} from './model'

const req = (
  type: TaskRequest['type'],
  id: number,
  kind: 'cell' | 'station' = 'cell',
): TaskRequest => ({
  type,
  target: { kind, id },
  item_code: 1001,
  count: 1,
  params: {},
  position_override: null,
  note: '',
  source: null,
})

const job = (
  id: string,
  stage: Job['stage'],
  work: number | null,
  seq: number,
  steps: Job['steps'],
): Job => ({
  id,
  seq,
  robot: 2,
  stage,
  origin: 'manual',
  via: null,
  priority: 50,
  work_id: work,
  transfer_order_id: null,
  steps,
  wait_reason: null,
  error: null,
  note: '',
  created_at: '',
  updated_at: '',
  ended_at: null,
  history: [],
})

const pair = (pick: Job['steps'][0]['state'], drop: Job['steps'][0]['state']) => [
  { request: req('PICK', 101), task: pick ? 't1' : null, state: pick },
  { request: req('DROP', 2102, 'station'), task: drop ? 't2' : null, state: drop },
]

describe('job model', () => {
  it('codes cells and stations', () => {
    expect(targetCode({ kind: 'cell', id: 103 })).toBe('C103')
    expect(targetCode({ kind: 'station', id: 2102 })).toBe('S2102')
    expect(route({ steps: pair(null, null) })).toEqual({ from: 'C101', to: 'S2102' })
  })

  it('groups by stage', () => {
    expect(groupOf('wait')).toBe('queue')
    expect(groupOf('pre')).toBe('queue')
    expect(groupOf('assigned')).toBe('live')
    expect(groupOf('canceled')).toBe('ended')
  })

  it('task label follows the live step', () => {
    expect(taskLabel(job('a', 'wait', 7, 1, pair(null, null)))).toBe('-1 PICK 전')
    expect(taskLabel(job('a', 'running', 7, 1, pair('running', null)))).toBe('-1 PICK')
    expect(taskLabel(job('a', 'running', 7, 1, pair('completed', 'queued')))).toBe('-2 DROP')
    expect(taskLabel(job('a', 'done', 7, 1, pair('completed', 'completed')))).toBe('-1 · -2')
    expect(taskLabel(job('a', 'pre', null, 1, pair(null, null)))).toBe('')
  })

  it('places rows in a fixed order and keeps the held row where it is', () => {
    const jobs = [
      job('b', 'wait', 12, 5, pair(null, null)),
      job('a', 'assigned', 11, 9, pair('submitted', null)),
      job('c', 'pre', null, 3, pair(null, null)),
      job('d', 'wait', 10, 7, pair(null, null)),
    ]
    const p = placeJobs(jobs, null, new Map())
    expect(p.queue.map((j) => j.id)).toEqual(['d', 'b', 'c'])
    expect(p.live.map((j) => j.id)).toEqual(['a'])
    const held = placeJobs(jobs, 'a', new Map([['a', 'queue']]))
    expect(held.queue.map((j) => j.id)).toEqual(['d', 'a', 'b', 'c'])
    expect(held.live).toEqual([])
  })

  it('who', () => {
    expect(whoOf({ origin: 'manual', via: null })).toBe('수동')
    expect(whoOf({ origin: 'auto', via: 'rule:R2' })).toBe('자동 · 규칙 R2')
    expect(whoOf({ origin: 'auto', via: 'gen:R5' })).toBe('자동 · 규칙 R5')
    expect(whoOf({ origin: 'auto', via: 'req:WMS-7@S2101' })).toBe('자동 · 요청 WMS-7')
  })
})

describe('stepsToJobs', () => {
  const s = (id: string, type: PlanStep['type'], robot?: number): PlanStep => ({
    id,
    type,
    target: { kind: 'cell', id: 101 },
    item_code: 1001,
    count: 1,
    note: '',
    robot,
  })
  const toReq = (p: PlanStep) => req(p.type, p.target.id)

  it('pairs PICK with the next DROP of the same robot and leaves a lone PICK', () => {
    const r = stepsToJobs(
      [s('1', 'PICK'), s('2', 'DROP'), s('3', 'MEASURE'), s('4', 'PICK')],
      2,
      toReq,
    )
    expect(r.jobs.map((j) => j.ids)).toEqual([['1', '2'], ['3']])
    expect(r.rest.map((x) => x.id)).toEqual(['4'])
  })

  it('pairs across another robot step in between', () => {
    const r = stepsToJobs([s('1', 'PICK', 2), s('2', 'MOVE', 1), s('3', 'DROP', 2)], 2, toReq)
    expect(r.jobs.map((j) => j.ids)).toEqual([['1', '3'], ['2']])
    expect(r.jobs[1].new.robot).toBe(1)
  })
})
