import { describe, expect, it } from 'vitest'
import type { DerivedRuleStatus, SchedState } from '../sched'
import {
  candTargets,
  normalizeState,
  originCounts,
  originOf,
  simSummary,
  skippedLine,
  sortDerived,
  termsSummary,
} from './decisionModel'

const d = (rule_id: string, origin: DerivedRuleStatus['origin'], name = rule_id): DerivedRuleStatus => ({
  rule_id,
  origin,
  name,
  request_id: null,
  station: null,
  fires: false,
  inputs: '',
  terms: [],
  state: 'idle',
  reason: null,
  age_min: 0,
  generated: 0,
  last_generated_at: null,
})

describe('decision helpers', () => {
  it('fills missing arrays of an old backend', () => {
    const s = normalizeState({
      config: { version: 1, auto: false, weights: { target: {}, item: {}, robot: {} }, rules: [] },
      rules: [],
      candidates: [],
      skipped: [],
      queue: [],
      note: null,
      separation_mm: 0,
    } as unknown as SchedState)
    expect(s?.derived).toEqual([])
    expect(s?.cooldowns).toEqual([])
    expect(s?.hand_alerts).toEqual([])
    expect(s?.metrics.waits).toEqual({})
    expect(normalizeState(null)).toBeNull()
  })

  it('groups derived rules request → policy → user', () => {
    const rows = [d('u1', 'user', 'b'), d('p1', 'policy'), d('r1', 'request'), d('u2', 'user', 'a')]
    expect(sortDerived(rows).map((r) => r.rule_id)).toEqual(['r1', 'p1', 'u2', 'u1'])
    expect(originCounts(rows)).toEqual({ request: 1, policy: 1, user: 2 })
    expect(originOf('p1', rows)).toBe('policy')
    expect(originOf('missing', rows)).toBe('user')
  })

  it('formats candidate targets', () => {
    expect(candTargets({ first: { kind: 'station', id: 2101 }, second: { kind: 'cell', id: 401 } })).toBe(
      'Station 2101 → Cell 401',
    )
    expect(candTargets({ first: { kind: 'cell', id: 5 }, second: null })).toBe('Cell 5')
    expect(candTargets({})).toBe('—')
  })

  it('summarizes terms, skipped and simulation', () => {
    const t = termsSummary([
      { label: 'Req', value: '1', ok: true },
      { label: 'CVOK', value: '0', ok: false },
    ])
    expect(t).toMatchObject({ label: '1/2', ok: false })
    expect(t.title).toContain('아직 · CVOK: 0')
    expect(termsSummary(undefined).label).toBe('—')
    expect(skippedLine([])).toBeNull()
    expect(skippedLine([{ rule: 'a', reason: 'x' }, { rule: 'b', reason: 'y' }])?.text).toBe('후보 못 됨 2건 — a: x')
    expect(
      simSummary({
        derived: [d('a', 'policy')],
        candidates: [
          { reason: null } as never,
          { reason: '영역' } as never,
        ],
        skipped: [],
      }),
    ).toEqual({ derived: 1, candidates: 2, generate: 1, skipped: 0 })
  })
})
