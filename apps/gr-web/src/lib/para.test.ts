import { describe, expect, it } from 'vitest'
import { filterPara, formatParaValue, leafCount, matchRow } from './para'
import type { ParaGroup, ParaRow } from './types'

function row(p: Partial<ParaRow> & { path: string }): ParaRow {
  return {
    name: p.path.split('.').pop() ?? p.path,
    ty: 'Real',
    comment: '',
    param: null,
    depth: 0,
    leaf: true,
    value: 0,
    ...p,
  }
}

const GROUPS: ParaGroup[] = [
  {
    name: 'Machine',
    ty: 'Struct',
    comment: '[p001 - p099] 장비 파라미터',
    rows: [
      row({ path: 'Machine.ID', ty: 'USInt', comment: '[p1] Machine ID', param: 1, value: 2 }),
      row({ path: 'Machine.MotorRefTorque', ty: 'Array[1..4] of Real', comment: '[p50-p53]', param: 50 }),
    ],
  },
  {
    name: 'Sensor',
    ty: 'Struct',
    comment: '[p400 - p499] 센서',
    rows: [
      row({ path: 'Sensor.Laser', ty: 'Struct', leaf: false, value: null }),
      row({ path: 'Sensor.LaserInnerDia_Offset', comment: '[p428] 레이저 내경 보정', param: 428, depth: 1 }),
    ],
  },
]

describe('matchRow', () => {
  it('matches parameter numbers with or without p', () => {
    const r = GROUPS[1].rows[1]
    expect(matchRow(r, 'p428')).toBe(true)
    expect(matchRow(r, '428')).toBe(true)
    expect(matchRow(r, 'p42')).toBe(false)
  })
  it('finds numbers inside a range comment', () => {
    expect(matchRow(GROUPS[0].rows[1], 'p52')).toBe(true)
  })
  it('matches name, path, comment and type', () => {
    const r = GROUPS[1].rows[1]
    expect(matchRow(r, 'innerdia')).toBe(true)
    expect(matchRow(r, '내경')).toBe(true)
    expect(matchRow(r, 'sensor.')).toBe(true)
    expect(matchRow(r, 'real')).toBe(true)
    expect(matchRow(r, 'torque')).toBe(false)
  })
})

describe('filterPara', () => {
  it('keeps everything for an empty query', () => {
    expect(filterPara(GROUPS, '  ')).toHaveLength(2)
  })
  it('keeps whole group when the group matches', () => {
    const g = filterPara(GROUPS, '장비')
    expect(g.map((x) => x.name)).toEqual(['Machine'])
    expect(g[0].rows).toHaveLength(2)
  })
  it('keeps only matching value rows otherwise', () => {
    const g = filterPara(GROUPS, 'p428')
    expect(g.map((x) => x.name)).toEqual(['Sensor'])
    expect(g[0].rows.map((r) => r.path)).toEqual(['Sensor.LaserInnerDia_Offset'])
  })
  it('counts leaves only', () => {
    expect(leafCount(GROUPS)).toBe(3)
  })
})

describe('formatParaValue', () => {
  it('formats scalars', () => {
    expect(formatParaValue(0.10000000149011612, 'Real')).toBe('0.1')
    expect(formatParaValue(2, 'USInt')).toBe('2')
    expect(formatParaValue(true, 'Bool')).toBe('TRUE')
    expect(formatParaValue(null, 'Real')).toBe('-')
  })
  it('formats arrays and structs', () => {
    expect(formatParaValue([1.5, 2.25], 'Array[1..2] of Real')).toBe('1.5, 2.25')
    expect(formatParaValue([{ A: 1 }], 'Array[1..1] of U')).toBe('{"A":1}')
    expect(formatParaValue({ A: 1 }, 'U')).toBe('{"A":1}')
  })
})
