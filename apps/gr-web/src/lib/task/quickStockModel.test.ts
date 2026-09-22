import { describe, expect, it } from 'vitest'
import { clampCount, pushRecent, quickAction } from './quickStockModel'

const cur = { cell_id: 101, item_code: 1001, count: 3, note: '', updated_at: '' }

describe('clampCount', () => {
  it('add starts at 1, edit allows 0, capped by StackMax', () => {
    expect(clampCount(0, 'add')).toBe(1)
    expect(clampCount(0, 'edit')).toBe(0)
    expect(clampCount(7, 'edit', 5)).toBe(5)
    expect(clampCount(150, 'add')).toBe(99)
    expect(clampCount(NaN, 'add')).toBe(1)
    expect(clampCount(2.6, 'edit')).toBe(3)
  })
})

it('pushRecent keeps newest first without duplicates', () => {
  expect(pushRecent([1, 2, 3, 4, 5], 3)).toEqual([3, 1, 2, 4, 5])
  expect(pushRecent([1, 2, 3, 4, 5], 9)).toEqual([9, 1, 2, 3, 4])
})

describe('quickAction', () => {
  it('empty place: needs an item', () => {
    expect(quickAction(null, null, 1)).toEqual({ kind: 'none' })
    expect(quickAction(null, 1002, 1)).toEqual({ kind: 'set', item_code: 1002, count: 1 })
    expect(quickAction({ ...cur, count: 0 }, 1002, 2)).toEqual({
      kind: 'set',
      item_code: 1002,
      count: 2,
    })
  })

  it('stocked place: change count keeps the item, 0 deletes, same is nothing', () => {
    expect(quickAction(cur, null, 5)).toEqual({ kind: 'set', item_code: 1001, count: 5 })
    expect(quickAction(cur, null, 0)).toEqual({ kind: 'delete' })
    expect(quickAction(cur, null, 3)).toEqual({ kind: 'none' })
  })
})
