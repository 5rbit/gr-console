import { describe, expect, it } from 'vitest'
import { candidateRoute, nextCandidates, parseSchedTab } from './pageModel'
import type { GenCandidate } from '../taskgen'

const cand = (p: Partial<GenCandidate>): GenCandidate => ({
  rule_id: 'r',
  rule_name: 'R',
  robot: 1,
  robot_name: 'GR1',
  score: 0,
  breakdown: [],
  area: { lo: 0, hi: 0 },
  age_min: 0,
  order: 0,
  reason: null,
  ...p,
})

describe('pageModel', () => {
  it('parses the remembered tab', () => {
    expect(parseSchedTab('policy')).toBe('policy')
    expect(parseSchedTab('bogus')).toBe('requests')
    expect(parseSchedTab(null)).toBe('requests')
  })

  it('next candidates: generating first, then score', () => {
    const r = nextCandidates([
      cand({ rule_id: 'a', score: 90, reason: '영역' }),
      cand({ rule_id: 'b', score: 10 }),
      cand({ rule_id: 'c', score: 50 }),
      cand({ rule_id: 'd', score: 5, reason: 'x' }),
    ])
    expect(r.map((c) => c.rule_id)).toEqual(['c', 'b', 'a'])
    expect(nextCandidates(undefined)).toEqual([])
  })

  it('candidate route', () => {
    expect(
      candidateRoute(
        cand({ first: { kind: 'station', id: 2101 }, second: { kind: 'cell', id: 401 } }),
      ),
    ).toBe('PICK Station 2101 → DROP Cell 401')
    expect(candidateRoute(cand({ first: { kind: 'station', id: 2101 }, second: null }))).toBe(
      'Station 2101 · R',
    )
    expect(candidateRoute(cand({}))).toBe('R')
  })
})
