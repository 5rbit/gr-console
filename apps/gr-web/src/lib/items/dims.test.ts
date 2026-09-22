import { describe, expect, it } from 'vitest'
import {
  applicable,
  changeLine,
  defaultPick,
  dimState,
  dimTone,
  fieldLabel,
  confidence,
  liveChange,
  passFilter,
  safeTone,
  signed,
  type DimChange,
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

describe('tone and live change', () => {
  const base = (p: Partial<DimSuggestion>): DimSuggestion => ({
    field: 'height',
    current: 225,
    suggested: 231.1,
    n: 3,
    spread: 0.5,
    spread_limit: 5,
    delta: 6.1,
    unstable: false,
    outlier: false,
    outlier_limit: 30,
    changed: true,
    samples: [
      { plc: 'GR2', seq: 1, at: '', value: 230.9, cell_id: 408 },
      { plc: 'GR2', seq: 2, at: '', value: 231.3, cell_id: 408 },
    ],
    ...p,
  })
  it('one tone per decision', () => {
    expect(dimTone(base({}))).toBe('ok')
    expect(dimTone(base({ current: 0 }))).toBe('info')
    expect(dimTone(base({ unstable: true }))).toBe('warn')
    expect(dimTone(base({ unstable: true, outlier: true }))).toBe('fault')
    expect(dimTone(base({ changed: false }))).toBe('muted')
    expect(dimTone(base({ suggested: null, samples: [] }))).toBe('muted')
    expect(safeTone('info') && safeTone('ok') && !safeTone('warn')).toBe(true)
  })
  it('live change = last measured change still in place', () => {
    const ch = (id: number, after: number, reverted = false, source = 'measured'): DimChange => ({
      id,
      code: 1,
      field: 'height',
      before: 225,
      after,
      source,
      samples: [],
      note: '',
      at: '',
      reverted,
    })
    expect(liveChange([ch(2, 231.1)], 'height', 231.1)?.id).toBe(2)
    expect(liveChange([ch(2, 231.1)], 'height', 240)).toBeNull()
    expect(liveChange([ch(2, 231.1, true)], 'height', 231.1)).toBeNull()
    expect(liveChange([ch(3, 225, false, 'revert')], 'height', 225)).toBeNull()
    expect(liveChange([ch(2, 231.1)], 'inner_diameter', 231.1)).toBeNull()
  })
})

describe('thin samples and confidence', () => {
  const s = (p: Partial<DimSuggestion>): DimSuggestion => ({
    field: 'inner_diameter',
    current: 372,
    suggested: 371.4,
    n: 5,
    spread: 1,
    spread_limit: 3,
    delta: -0.6,
    unstable: false,
    outlier: false,
    outlier_limit: 15,
    changed: true,
    samples: [],
    ...p,
  })
  it('under 3 samples is thin (not auto-selected) unless it is an outlier', () => {
    expect(dimTone(s({ n: 2 }))).toBe('thin')
    expect(dimTone(s({ n: 2, outlier: true }))).toBe('fault')
    expect(dimTone(s({ n: 2, unstable: true }))).toBe('thin')
    expect(dimTone(s({ n: 3 }))).toBe('ok')
    expect(safeTone('thin')).toBe(false)
  })
  it('confidence grows with samples and falls with spread', () => {
    expect(confidence(s({ n: 0 }), 5)).toBe(0)
    expect(confidence(s({ n: 5, spread: 0 }), 5)).toBe(5)
    expect(confidence(s({ n: 2, spread: 0 }), 5)).toBe(2)
    expect(confidence(s({ n: 5, spread: 3 }), 5)).toBe(3)
    expect(confidence(s({ n: 5, spread: 9, unstable: true }), 5)).toBe(2)
    expect(confidence(s({ n: 1, spread: 9 }), 20)).toBe(1)
  })
})

describe('review filter', () => {
  const f = (p: Partial<DimSuggestion>): DimSuggestion => ({
    field: 'height',
    current: 100,
    suggested: 101,
    n: 3,
    spread: 1,
    spread_limit: 3,
    delta: 1,
    unstable: false,
    outlier: false,
    outlier_limit: 15,
    changed: true,
    samples: [],
    ...p,
  })
  it('safe = ok · info, check = thin · warn · fault, nothing for same values', () => {
    expect(passFilter(f({}), 'safe')).toBe(true)
    expect(passFilter(f({ current: 0 }), 'safe')).toBe(true)
    expect(passFilter(f({ n: 2 }), 'check')).toBe(true)
    expect(passFilter(f({ n: 2 }), 'safe')).toBe(false)
    expect(passFilter(f({ outlier: true }), 'check')).toBe(true)
    expect(passFilter(f({ changed: false }), 'all')).toBe(false)
  })
})
