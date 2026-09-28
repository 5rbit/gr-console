import { describe, expect, it } from 'vitest'
import { condAll, condText, condTokens } from './condExprModel'

const expr = [
  { name: 'CVOK', neg: false, ok: true },
  { name: 'Req', neg: false, ok: false },
  { name: 'ItemExist', neg: false, ok: true },
  { name: 'CVNO', neg: true, ok: true },
  { name: 'Comp', neg: true, ok: false },
]

describe('condExprModel', () => {
  it('writes negated terms as "0" and marks unmet ones', () => {
    expect(condTokens(expr).map((t) => t.text)).toEqual([
      'CVOK',
      'Req',
      'ItemExist',
      'CVNO 0',
      'Comp 0',
    ])
    expect(condText(expr)).toBe('CVOK & Req(미충족) & ItemExist & CVNO 0 & Comp 0(미충족)')
  })
  it('all-met needs at least one term', () => {
    expect(condAll(expr)).toBe(false)
    expect(condAll(expr.map((c) => ({ ...c, ok: true })))).toBe(true)
    expect(condAll(undefined)).toBe(false)
    expect(condText(null)).toBe('')
  })
})
