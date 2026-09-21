import { describe, expect, it } from 'vitest'
import type { Cell, CellUpsert } from '../types'
import { DEFAULT_RULE, HEX_ROW, MODE_LABEL, annotate, applyButtonLabel, conflicts, extent, fitCount, generate, inferRule, previewApply, previewLabel, ruleWarnings, validateRule } from './layoutGen'

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

  it('validation: origin 0 is allowed, floor Z ≤ 0 only warns, negative coordinates are flagged', () => {
    expect(validateRule({ ...rule, section: 4 })).toContain('구간은 1..3')
    expect(validateRule({ ...rule, startId: 999 })[0]).toMatch(/> 1000/)
    expect(validateRule({ ...rule, originX: 0, originY: 0 })).toEqual([])
    expect(generate({ ...rule, originX: 0, originY: 0 })[0].position).toEqual([0, 0, 100])
    // 바닥 Z ≤ 0 은 바닥 평탄도 보정이라 생성도 되고 경고도 없다(PLC 는 셀 Z 를 검사하지 않는다)
    for (const z of [0, -8.8]) {
      expect(validateRule({ ...rule, z })).toEqual([])
      expect(ruleWarnings({ ...rule, z })).toEqual([])
      expect(generate({ ...rule, z })[0].position[2]).toBe(z)
    }
    expect(ruleWarnings(rule)).toEqual([])
    expect(validateRule({ ...rule, z: Number.NaN }).some((p) => p.includes('숫자'))).toBe(true)
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

// 적용 미리보기 — 서버 `registry/bulk.rs` 와 같은 규칙을 화면에서도 센다.
describe('previewApply', () => {
  const base = { use: true, blend_use: false, section: 1, row: 1, col: 1, length: 800, width: 800, source: 'local' as const, dirty: true, updated_at: '' }
  const cell = (id: number, x: number, y: number, section = 1): Cell => ({ ...base, id, section, position: [x, y, 100] })
  const gen = (id: number, x: number, y: number, section = 1): CellUpsert => ({ id, use: true, blend_use: false, section, row: 1, col: 1, length: 800, width: 800, position: [x, y, 100] })

  it('append skips id clashes and overlaps with a reason, and deletes nothing', () => {
    const existing = [cell(101, 1000, 1000)]
    const p = previewApply([gen(101, 5000, 5000), gen(102, 1100, 1000), gen(103, 9000, 9000)], existing, { mode: 'append', section: 1 })
    expect(p.rows.map((r) => r.outcome)).toEqual(['skipped', 'skipped', 'added'])
    expect(p.rows[0].reason).toMatch(/id 중복/)
    expect(p.rows[1].reason).toMatch(/#101.*겹침|겹침/)
    expect(p.removed).toBe(0)
    expect(p.counts).toMatchObject({ added: 1, skipped: 2 })
    expect(previewLabel({ ...p.counts, removed: p.removed })).toBe('추가 1 · 건너뜀 2')
    expect(applyButtonLabel('append', p)).toBe('추가 1칸 (건너뜀 2)')
  })

  it('identical row is unchanged, autoId remaps to the next free id of the section', () => {
    const existing = [cell(101, 1000, 1000), cell(102, 2000, 1000)]
    expect(previewApply([gen(101, 1000, 1000)], existing, { mode: 'append', section: 1 }).rows[0].outcome).toBe('unchanged')
    const p = previewApply([gen(101, 9000, 9000)], existing, { mode: 'append', section: 1, autoId: true })
    expect(p.rows[0]).toMatchObject({ id: 101, newId: 103, outcome: 'remapped' })
    expect(p.byId.get(101)?.newId).toBe(103)
    // 옮겨도 겹치면 건너뛴다
    expect(previewApply([gen(101, 2050, 1000)], existing, { mode: 'append', section: 1, autoId: true }).rows[0].outcome).toBe('skipped')
  })

  it('replace_section counts the wipe, overwrite keeps everything', () => {
    const existing = [cell(101, 1000, 1000), cell(102, 2000, 1000), cell(201, 1000, 9000, 2)]
    const p = previewApply([gen(101, 1500, 1500)], existing, { mode: 'replace_section', section: 1 })
    expect(p.removed).toBe(1) // #102 만 (101 은 생성분이 이어받는다)
    expect(p.rows[0].outcome).toBe('updated')
    expect(applyButtonLabel('replace_section', p)).toBe('구역 교체 — 삭제 1 · 생성 1')
    const o = previewApply([gen(101, 1100, 1000), gen(103, 2050, 1000)], existing, { mode: 'overwrite', section: 1 })
    expect(o.removed).toBe(0)
    expect(o.rows.map((r) => r.outcome)).toEqual(['updated', 'added'])
    expect(o.rows[1].reason).toMatch(/덮어씁니다/)
  })

  it('overlap uses the tolerance and marks the map cells', () => {
    const existing = [cell(101, 1000, 1000)]
    // 중심 거리 799.6 < 800 이지만 오차 0.5 안이라 통과
    expect(previewApply([gen(102, 1799.6, 1000)], existing, { mode: 'append', section: 1 }).rows[0].outcome).toBe('added')
    const p = previewApply([gen(102, 1799.6, 1000)], existing, { mode: 'append', section: 1, diameter: 1000 })
    expect(p.rows[0].outcome).toBe('skipped')
    const marks = annotate([gen(102, 1799.6, 1000)], p)
    expect(marks[0]).toMatchObject({ id: 102, outcome: 'skipped' })
    expect(marks[0].reason).toMatch(/겹침/)
  })

  it('previewLabel reads the server counts too (created 대신 added)', () => {
    expect(previewLabel({ created: 2, updated: 1, skipped: 0, unchanged: 0, remapped: 3, removed: 4 })).toBe('추가 2 · 갱신 1 · 번호 변경 3 · 삭제 4')
    expect(previewLabel({})).toBe('바뀌는 것 없음')
    expect(MODE_LABEL.append).toBe('추가')
  })
})
