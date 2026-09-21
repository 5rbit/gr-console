import { describe, expect, it } from 'vitest'
import { OUTLINE_ORDER, fillState, outlineOf, unusable } from './mapStyleModel'

describe('fillState', () => {
  it('비활성 > 빈 칸 > Max > 재고 있음', () => {
    expect(fillState({ use: false, count: 5, stackMax: 5 })).toBe('disabled')
    expect(fillState({ use: true, count: 0, stackMax: 5 })).toBe('empty')
    expect(fillState({ use: true, count: 5, stackMax: 5 })).toBe('full')
    expect(fillState({ use: true, count: 6, stackMax: 5 })).toBe('full')
    expect(fillState({ use: true, count: 4, stackMax: 5 })).toBe('stocked')
  })
  it('단수 Max 를 모르면 Max 로 보지 않는다', () => {
    expect(fillState({ use: true, count: 9, stackMax: 0 })).toBe('stocked')
  })
})

describe('unusable', () => {
  it('셀 바닥 Z ≤ 0 은 정상, 스테이션 Z ≤ 0 · Use=false 는 못 쓰는 칸', () => {
    expect(unusable({ kind: 'cell', id: 401, use: true, z: 0 })).toBeNull()
    expect(unusable({ kind: 'cell', id: 401, use: true, z: -8 })).toBeNull()
    expect(unusable({ kind: 'station', id: 2021, use: true, z: 0 })).toContain('INVALID_CELL_POSZ')
    expect(unusable({ kind: 'station', id: 2021, use: true, z: 1 })).toBeNull()
    expect(unusable({ kind: 'cell', id: 401, use: false, z: 100 })).toContain('Use')
  })
})

describe('outlineOf', () => {
  it('윤곽은 하나 — 우선순위가 높은 것', () => {
    expect(outlineOf({})).toBeNull()
    expect(outlineOf({ hover: true, unusable: true })).toBe('unusable')
    expect(outlineOf({ planned: true, work: true, selected: true })).toBe('selected')
    expect(outlineOf({ planned: true, highlight: true })).toBe('planned')
    expect(outlineOf({ dirty: true, hover: true })).toBe('dirty')
  })
  it('우선순위 표', () => {
    expect(OUTLINE_ORDER).toEqual([
      'selected',
      'work',
      'planned',
      'highlight',
      'unusable',
      'dirty',
      'hover',
    ])
  })
})
