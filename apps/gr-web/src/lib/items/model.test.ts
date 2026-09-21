import { describe, expect, it } from 'vitest'
import type { ItemUpsert, StockEntry } from '../types'
import { duplicateItem, itemErrors, itemLabel, matchesItem, stockUsage, usedCodes } from './model'

const ITEM: ItemUpsert = {
  code: 1001,
  name: '225/45R17',
  count: 4,
  inner_diameter: 381,
  outer_diameter: 780.5,
  lower_bead_height: 20,
  upper_bead_height: 220,
  height: 240,
  deflection_factor: 0.1,
  note: '시험',
}

const st = (cell_id: number, item_code: number, count: number): StockEntry => ({
  cell_id,
  item_code,
  count,
  note: '',
  updated_at: '',
})

describe('matchesItem', () => {
  it('코드·이름·비고 어디에 들어도 찾는다(대소문자 무시)', () => {
    expect(matchesItem(ITEM, '')).toBe(true)
    expect(matchesItem(ITEM, '1001')).toBe(true)
    expect(matchesItem(ITEM, 'r17')).toBe(true)
    expect(matchesItem(ITEM, '시험')).toBe(true)
    expect(matchesItem(ITEM, '999')).toBe(false)
  })
})

describe('stockUsage', () => {
  it('코드별 칸 수와 개수 — 빈 칸과 0개는 세지 않는다', () => {
    const u = stockUsage([st(1, 1001, 3), st(2, 1001, 2), st(3, 1002, 1), st(4, 0, 5), st(5, 1002, 0)])
    expect(u.get(1001)).toEqual({ cells: 2, count: 5 })
    expect(u.get(1002)).toEqual({ cells: 1, count: 1 })
    expect(u.has(0)).toBe(false)
  })
  it('usedCodes는 사용 중인 코드만 오름차순으로', () => {
    const u = stockUsage([st(1, 1002, 1), st(2, 1001, 1)])
    expect(usedCodes([1003, 1002, 1001], u).map((x) => x.code)).toEqual([1001, 1002])
  })
})

describe('duplicateItem', () => {
  it('가장 큰 코드 + 1 과 (복사) 이름', () => {
    const d = duplicateItem(ITEM, [{ code: 1001 }, { code: 2000 }, { code: 5 }])
    expect(d.code).toBe(2001)
    expect(d.name).toBe('225/45R17 (복사)')
    expect(d.height).toBe(240)
  })
})

describe('itemErrors', () => {
  it('정상 품목은 통과', () => {
    expect(itemErrors(ITEM)).toEqual([])
  })
  it('코드·수량', () => {
    expect(itemErrors({ ...ITEM, code: 0 })).toHaveLength(1)
    expect(itemErrors({ ...ITEM, count: 0 })).toHaveLength(1)
    expect(itemErrors({ ...ITEM, count: 256 })).toHaveLength(1)
  })
  it('치수는 0 이상 유한수, 처짐 계수는 음수 허용', () => {
    expect(itemErrors({ ...ITEM, height: -1 })).toEqual(['Height: 0 이상의 숫자여야 합니다'])
    expect(itemErrors({ ...ITEM, lower_bead_height: Number.NaN })).toHaveLength(1)
    expect(itemErrors({ ...ITEM, deflection_factor: -0.2 })).toEqual([])
    expect(itemErrors({ ...ITEM, deflection_factor: Number.POSITIVE_INFINITY })).toHaveLength(1)
  })
  it('내경 < 외경 — 한쪽이 0(미입력)이면 비교하지 않는다', () => {
    expect(itemErrors({ ...ITEM, inner_diameter: 800 })).toEqual(['InnerDiameter 는 OuterDiameter 보다 작아야 합니다'])
    expect(itemErrors({ ...ITEM, inner_diameter: 780.5 })).toHaveLength(1)
    expect(itemErrors({ ...ITEM, outer_diameter: 0 })).toEqual([])
  })
})

describe('itemLabel', () => {
  it('shows code, name, dimensions and note', () => {
    expect(itemLabel(ITEM)).toBe('1001 · 225/45R17 · ID 381 · OD 780.5 · H 240 · 시험')
  })
  it('skips unset dimensions and empty text', () => {
    expect(itemLabel({ ...ITEM, name: '', inner_diameter: 0, height: 0, note: ' ' })).toBe('1001 · OD 780.5')
  })
})
