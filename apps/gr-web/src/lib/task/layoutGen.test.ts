import { describe, expect, it } from 'vitest'
import type { Cell } from '../types'
import { DEFAULT_RULE, HEX_ROW, conflicts, extent, fitCount, generate, inferRule, validateRule } from './layoutGen'

// 축 규약: 행(Row) = X 방향, 열(Col) = Y 방향
const rule = { ...DEFAULT_RULE, originX: 1000, originY: 2000, z: 100, diameter: 800, gap: 200, cols: 3, rows: 3 }

describe('layoutGen', () => {
  it('grid: rows advance along X, columns along Y, row-major ids', () => {
    const cells = generate({ ...rule, pattern: 'grid' })
    expect(cells.length).toBe(9)
    expect(cells[0]).toMatchObject({ id: 101, row: 1, col: 1, section: 1, length: 800, width: 800, position: [1000, 2000, 100] })
    expect(cells[1]).toMatchObject({ row: 1, col: 2, position: [1000, 3000, 100] }) // same row → same X, next column → Y + pitch
    expect(cells[3]).toMatchObject({ row: 2, col: 1, position: [2000, 2000, 100] }) // next row → X + pitch
    expect(cells[8].id).toBe(109)
  })

  it('honeycomb: odd rows shifted half a pitch in Y, rows √3/2 apart in X, one shorter when asked', () => {
    const cells = generate({ ...rule, pattern: 'honeycomb', shortOddRows: true })
    expect(cells.length).toBe(3 + 2 + 3)
    const row2 = cells.filter((c) => c.row === 2)
    expect(row2.length).toBe(2)
    expect(row2[0].position[1]).toBe(2500)
    expect(row2[0].position[0]).toBeCloseTo(1000 + 1000 * HEX_ROW, 0)
    const full = generate({ ...rule, pattern: 'honeycomb', shortOddRows: false })
    expect(full.length).toBe(9)
    // honeycomb packs rows closer in X, the half-pitch shift widens Y by 500
    const eg = extent(generate({ ...rule, pattern: 'grid' }), 800)
    const eh = extent(full, 800)
    expect(eh.w).toBeLessThan(eg.w)
    expect(eh.h).toBe(eg.h + 500)
  })

  it('direction and serpentine numbering', () => {
    const cells = generate({ ...rule, originX: 5000, originY: 5000, pattern: 'grid', dirX: -1, dirY: -1, serpentine: true })
    expect(cells[0].position).toEqual([5000, 5000, 100])
    expect(cells[1].position[1]).toBe(4000) // columns go toward -Y
    expect(cells[3].position[0]).toBe(4000) // rows go toward -X
    // row 2 numbered in reverse column order
    expect(cells[3].col).toBe(3)
    expect(cells[5].col).toBe(1)
    const colFirst = generate({ ...rule, pattern: 'grid', order: 'col' })
    expect(colFirst[1]).toMatchObject({ row: 2, col: 1 })
  })

  it('validation: origin 0 is allowed, floor Z must be > 0, negative coordinates are flagged', () => {
    expect(validateRule({ ...rule, section: 4 })).toContain('구간은 1..3')
    expect(validateRule({ ...rule, startId: 999 })[0]).toMatch(/> 1000/)
    expect(validateRule({ ...rule, originX: 0, originY: 0 })).toEqual([])
    expect(generate({ ...rule, originX: 0, originY: 0 })[0].position).toEqual([0, 0, 100])
    expect(validateRule({ ...rule, z: 0 }).some((p) => p.includes('Z > 0'))).toBe(true)
    expect(validateRule({ ...rule, originX: 500, dirX: -1 }).some((p) => p.includes('음수'))).toBe(true)
    expect(validateRule({ ...rule, originY: 500, dirY: -1 }).some((p) => p.includes('음수'))).toBe(true)
    expect(generate({ ...rule, section: 0 })).toEqual([])
  })

  it('fitCount: rows from the X length, columns from the Y length', () => {
    expect(fitCount(4000, 10000, 800, 200, 'grid')).toEqual({ rows: 4, cols: 10 })
    expect(fitCount(4000, 10000, 800, 200, 'honeycomb').rows).toBe(4) // (4000-800)/866 = 3.69 → 3 + 1
    expect(fitCount(500, 500, 800, 0, 'grid')).toEqual({ cols: 0, rows: 0 })
  })

  it('conflicts: duplicate ids and overlaps, except the section being replaced', () => {
    const base = { use: true, blend_use: false, row: 1, col: 1, length: 800, width: 800, source: 'plc' as const, dirty: false, updated_at: '' }
    const existing: Cell[] = [
      { ...base, id: 101, section: 2, position: [9000, 9000, 100] },
      { ...base, id: 300, section: 2, position: [1100, 2000, 100] },
      { ...base, id: 400, section: 1, position: [1000, 2000, 100] },
    ]
    const gen = generate({ ...rule, pattern: 'grid' })
    const c = conflicts(gen, existing, 800, 1)
    expect(c.some((x) => x.id === 101 && /id 중복/.test(x.reason))).toBe(true)
    expect(c.some((x) => x.id === 101 && /겹침/.test(x.reason))).toBe(true) // #101 at (1000,2000) vs #300 at (1100,2000)
    expect(c.some((x) => x.reason.includes('#400'))).toBe(false) // section 1 is being replaced
  })

  it('inferRule recovers pitch, pattern and direction from existing cells', () => {
    const gen = generate({ ...rule, pattern: 'honeycomb', shortOddRows: false })
    const cells: Cell[] = gen.map((c) => ({ ...c, source: 'plc', dirty: false, updated_at: '' }))
    const r = inferRule(cells, 1)!
    expect(r.pattern).toBe('honeycomb')
    expect(r.gap).toBe(200)
    expect(r.startId).toBe(101)
    expect(r.cols).toBe(3)
    expect(r.rows).toBe(3)
    expect(r.dirX).toBe(1)
    expect(r.dirY).toBe(1)
    const back = generate({ ...rule, originX: 6000, originY: 6000, pattern: 'grid', dirX: -1, dirY: -1 }).map((c) => ({ ...c, source: 'plc' as const, dirty: false, updated_at: '' }))
    const rb = inferRule(back, 1)!
    expect(rb.dirX).toBe(-1)
    expect(rb.dirY).toBe(-1)
    expect(rb.pattern).toBe('grid')
  })
})
