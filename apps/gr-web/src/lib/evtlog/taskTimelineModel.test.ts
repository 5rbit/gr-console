import { describe, expect, it } from 'vitest'
import type { TaskTransition } from '../types'
import type { EventRow } from './api'
import { mergeTimeline } from './taskTimelineModel'

const T0 = Date.parse('2026-09-27T03:00:00Z')

function h(at: string, to: TaskTransition['to']): TaskTransition {
  return { from: null, to, at, by: 'plc', note: null }
}

function e(id: number, ts_ms: number): EventRow {
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
    lvl: 2,
    lvl_name: 'INFO',
    src: 20,
    code: 1,
    name: null,
    a: 0,
    b: 0,
    ctx: 1,
    detail: null,
    text: '',
  }
}

describe('mergeTimeline', () => {
  it('interleaves transitions and events by time, states first on ties', () => {
    const history = [h('2026-09-27T03:00:00Z', 'queued'), h('2026-09-27T03:00:05Z', 'running')]
    const events = [e(2, T0 + 7000), e(1, T0 + 5000), e(1, T0 + 5000)]
    const out = mergeTimeline(history, events)
    expect(out.map((x) => (x.kind === 'state' ? x.h.to : `e${x.e.id}`))).toEqual([
      'queued',
      'running',
      'e1',
      'e2',
    ])
  })

  it('keeps an unparsable transition at its place', () => {
    const history = [h('2026-09-27T03:00:00Z', 'queued'), h('bad', 'running')]
    const out = mergeTimeline(history, [e(1, T0 + 1)])
    expect(out.map((x) => x.kind)).toEqual(['state', 'state', 'evt'])
  })
})
