import { describe, expect, it } from 'vitest'
import {
  applicable,
  changeLine,
  defaultPick,
  dimState,
  fieldLabel,
  signed,
  type DimSuggestion,
  type ItemDims,
} from './dims'

const sug = (patch: Partial<DimSuggestion>): DimSuggestion => ({
  field: 'inner_diameter',
  current: 372,
  suggested: 370.5,
  n: 5,
  spread: 1.2,
  spread_limit: 3,
  delta: -1.5,
  unstable: false,
  outlier: false,
  outlier_limit: 15,
  changed: true,
  samples: [],
  ...patch,
})

const dims = (fields: DimSuggestion[]): ItemDims => ({
  code: 1501,
  name: '225/45ZR15',
  window: 5,
  fields,
  stack_max: { current: 0, observed_max: null, samples: 0 },
  has_changes: true,
})

describe('dims', () => {
  it('applicable — 표본이 있고 값이 다를 때만', () => {
    expect(applicable(sug({}))).toBe(true)
    expect(applicable(sug({ suggested: null, changed: false }))).toBe(false)
    expect(applicable(sug({ changed: false, delta: 0 }))).toBe(false)
  })

  it('defaultPick — 흔들리는 필드는 기본으로 고르지 않는다', () => {
    const d = dims([
      sug({}),
      sug({ field: 'height', unstable: true, spread: 9 }),
      sug({ field: 'upper_bead_height', changed: false, delta: 0 }),
    ])
    expect([...defaultPick(d)]).toEqual(['inner_diameter'])
    expect([...defaultPick(dims([sug({ outlier: true, delta: 55.9 })]))]).toEqual([])
  })

  it('dimState — 차이 큼 > 흔들림 > 제안', () => {
    expect(dimState(sug({ outlier: true, unstable: true, delta: 55.9 })).text).toBe('차이 큼')
    expect(dimState(sug({ unstable: true })).tone).toBe('warn')
    expect(dimState(sug({})).text).toBe('제안')
    expect(dimState(sug({ suggested: null })).text).toBe('표본 없음')
  })

  it('signed · changeLine · fieldLabel', () => {
    expect(signed(1.25)).toBe('+1.3')
    expect(signed(-0.04)).toBe('0.0')
    expect(signed(null)).toBe('')
    expect(changeLine(sug({}))).toBe('InnerDiameter 372.0 → 370.5 (-1.5)')
    expect(fieldLabel('height')).toBe('Height')
    expect(fieldLabel('weird')).toBe('weird')
  })
})
