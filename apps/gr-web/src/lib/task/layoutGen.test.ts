import { describe, expect, it } from 'vitest'
import type { Cell } from '../types'
import {
  DEFAULT_RULE,
  HEX_ROW,
  conflicts,
  extent,
  fitCount,
  generate,
  inferRule,
  validateRule,
} from './layoutGen'

const rule = {
  ...DEFAULT_RULE,
  originX: 1000,
  originY: 2000,
  z: 100,
  diameter: 800,
  gap: 200,
  cols: 3,
  rows: 3,
}

describe('layoutGen', () => {
  it('grid: pitch = diameter + gap, row-major ids', () => {
    const cells = generate({ ...rule, pattern: 'grid' })
    expect(cells.length).toBe(9)
    expect(cells[0]).toMatchObject({
      id: 101,
      row: 1,
      col: 1,
      section: 1,
      length: 800,
      width: 800,
      position: [1000, 2000, 100],
    })
    expect(cells[1].position).toEqual([2000, 2000, 100])
    expect(cells[3].position).toEqual([1000, 3000, 100])
    expect(cells[8].id).toBe(109)
  })

  it('honeycomb: odd rows shifted half a pitch, rows √3/2 apart, one shorter when asked', () => {
    const cells = generate({ ...rule, pattern: 'honeycomb', shortOddRows: true })
    expect(cells.length).toBe(3 + 2 + 3)
    const row2 = cells.filter((c) => c.row === 2)
    expect(row2.length).toBe(2)
    expect(row2[0].position[0]).toBe(1500)
    expect(row2[0].position[1]).toBeCloseTo(2000 + 1000 * HEX_ROW, 0)
    const full = generate({ ...rule, pattern: 'honeycomb', shortOddRows: false })
    expect(full.length).toBe(9)
    // honeycomb is shorter than grid for the same rows
    const eg = extent(generate({ ...rule, pattern: 'grid' }), 800)
    const eh = extent(full, 800)
    expect(eh.h).toBeLessThan(eg.h)
    expect(eh.w).toBeGreaterThan(eg.w) // the half-pitch shift widens by 500
  })

  it('direction and serpentine numbering', () => {
    const cells = generate({ ...rule, pattern: 'grid', dirX: -1, dirY: -1, serpentine: true })
    expect(cells[1].position[0]).toBe(0) // x decreases
    expect(cells[3].position[1]).toBe(1000) // y decreases
    // row 2 numbered right-to-left: id 104 sits at the far column
    expect(cells[3].col).toBe(3)
    expect(cells[5].col).toBe(1)
    const colFirst = generate({ ...rule, pattern: 'grid', order: 'col' })
    expect(colFirst[1]).toMatchObject({ row: 2, col: 1 })
  })

  it('validation and fitCount', () => {
    expect(validateRule({ ...rule, section: 4 })).toContain('구간은 1..3')
    expect(validateRule({ ...rule, startId: 999, cols: 3, rows: 3 })[0]).toMatch(/> 1000/)
    expect(generate({ ...rule, section: 0 })).toEqual([])
    expect(fitCount(10000, 4000, 800, 200, 'grid')).toEqual({ cols: 10, rows: 4 })
    expect(fitCount(10000, 4000, 800, 200, 'honeycomb').rows).toBe(4) // (4000-800)/866 = 3.69 → 3 + 1
    expect(fitCount(500, 500, 800, 0, 'grid')).toEqual({ cols: 0, rows: 0 })
  })

  it('conflicts: duplicate ids and overlaps, except the section being replaced', () => {
    const existing: Cell[] = [
      {
        id: 101,
        use: true,
        blend_use: false,
        section: 2,
        row: 1,
        col: 1,
        length: 800,
        width: 800,
        position: [9000, 9000, 0],
        source: 'plc',
        dirty: false,
        updated_at: '',
      },
      {
        id: 300,
        use: true,
        blend_use: false,
        section: 2,
        row: 1,
        col: 2,
        length: 800,
        width: 800,
        position: [1100, 2000, 0],
        source: 'plc',
        dirty: false,
        updated_at: '',
      },
      {
        id: 400,
        use: true,
        blend_use: false,
        section: 1,
        row: 1,
        col: 1,
        length: 800,
        width: 800,
        position: [1000, 2000, 0],
        source: 'plc',
        dirty: false,
        updated_at: '',
      },
    ]
    const gen = generate({ ...rule, pattern: 'grid' })
    const c = conflicts(gen, existing, 800, 1)
    expect(c.some((x) => x.id === 101 && /id 중복/.test(x.reason))).toBe(true)
    expect(c.some((x) => x.id === 101 && /겹침/.test(x.reason))).toBe(true) // #101 at (1000,2000) vs #300 at (1100,2000)
    expect(c.some((x) => x.reason.includes('#400'))).toBe(false) // section 1 is being replaced
  })

  it('inferRule recovers pitch and pattern from existing cells', () => {
    const gen = generate({ ...rule, pattern: 'honeycomb', shortOddRows: false })
    const cells: Cell[] = gen.map((c) => ({ ...c, source: 'plc', dirty: false, updated_at: '' }))
    const r = inferRule(cells, 1)!
    expect(r.pattern).toBe('honeycomb')
    expect(r.gap).toBe(200)
    expect(r.startId).toBe(101)
    expect(r.cols).toBe(3)
    expect(r.rows).toBe(3)
  })
})
