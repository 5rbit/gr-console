import { describe, expect, it } from 'vitest'
import type { Cell, Item, StockEntry } from '../types'
import {
  EMPTY_HISTORY,
  carried,
  commit,
  move,
  nextType,
  overlay,
  planRows,
  redo,
  simulateStock,
  stackZ,
  stepForClick,
  toScenario,
  toggleType,
  undo,
  type PlanStep,
} from './plan'

const cell = (id: number, z = 1500): Cell => ({
  id,
  use: true,
  blend_use: false,
  section: 1,
  row: 1,
  col: 1,
  length: 1200,
  width: 1100,
  position: [id * 100, 3000, z],
  source: 'plc',
  dirty: false,
  updated_at: '',
})
const item = (code: number, height: number): Item =>
  ({
    code,
    name: '',
    count: 1,
    inner_diameter: 381,
    outer_diameter: 780,
    lower_bead_height: 0,
    upper_bead_height: 0,
    height,
    deflection_factor: 0,
  }) as Item
const st = (cell_id: number, item_code: number, count: number): StockEntry => ({
  cell_id,
  item_code,
  count,
  note: '',
  updated_at: '',
})
const stock = new Map<number, StockEntry>([
  [101, st(101, 1001, 3)],
  [102, st(102, 0, 0)],
])
const ctx = {
  cells: [cell(101), cell(102), cell(103)],
  stations: [],
  items: [item(1001, 240), item(1002, 260)],
  stockNow: stock,
}

describe('plan', () => {
  it('stackZ follows the agreed formulas', () => {
    expect(stackZ('PICK', 1500, 240, 3, 1)).toBe(1500 + 240 * 2 + 120)
    expect(stackZ('DROP', 1500, 240, 3, 1)).toBe(1500 + 240 * 3 + 120)
    expect(stackZ('PICK', 0, 240, 0, 1)).toBe(120)
    expect(stackZ('DROP', 0, 240, 0, 1)).toBe(120)
  })

  it('clicks alternate PICK → DROP and carry the item code', () => {
    let steps: PlanStep[] = []
    expect(nextType(steps)).toBe('PICK')
    steps = [...steps, stepForClick(steps, { kind: 'cell', id: 101 }, stock)]
    expect(steps[0].type).toBe('PICK')
    expect(steps[0].item_code).toBe(1001) // from cell stock
    expect(nextType(steps)).toBe('DROP')
    expect(carried(steps)).toEqual({ item_code: 1001, count: 1 })
    steps = [...steps, stepForClick(steps, { kind: 'cell', id: 102 }, stock)]
    expect(steps[1].type).toBe('DROP')
    expect(steps[1].item_code).toBe(1001) // carried
    expect(carried(steps)).toBeNull()
    // third click picks again, from a cell whose stock is only known via simulation (102 now has 1)
    steps = [...steps, stepForClick(steps, { kind: 'cell', id: 102 }, stock)]
    expect(steps[2].type).toBe('PICK')
    expect(steps[2].item_code).toBe(1001)
    const sim = simulateStock(steps, stock)
    expect(sim.get(101)?.count).toBe(2)
    expect(sim.get(102)?.count).toBe(0)
  })

  it('planRows computes Z from simulated stock and flags problems', () => {
    const steps: PlanStep[] = [
      {
        id: 'a',
        type: 'PICK',
        target: { kind: 'cell', id: 101 },
        item_code: 1001,
        count: 1,
        note: '',
      },
      {
        id: 'b',
        type: 'DROP',
        target: { kind: 'cell', id: 102 },
        item_code: 1001,
        count: 1,
        note: '',
      },
      {
        id: 'c',
        type: 'DROP',
        target: { kind: 'cell', id: 102 },
        item_code: 1001,
        count: 1,
        note: '',
      },
      {
        id: 'd',
        type: 'PICK',
        target: { kind: 'cell', id: 103 },
        item_code: 1002,
        count: 1,
        note: '',
      },
    ]
    const rows = planRows(steps, ctx)
    expect(rows[0].z).toBe(1500 + 240 * 2 + 120) // 3 in stock → top tire
    expect(rows[0].stockBefore).toBe(3)
    expect(rows[0].stockAfter).toBe(2)
    expect(rows[1].z).toBe(1500 + 120) // empty cell drop
    expect(rows[2].z).toBe(1500 + 240 + 120) // one already dropped
    expect(rows[2].warnings).toContain('들고 있는 화물 없음 (앞에 PICK 없음)')
    expect(rows[3].warnings).toContain('셀 재고 없음')
    expect(rows[3].z).toBe(1500 + 130) // 0 stock → floor + mid of 260
  })

  it('edit helpers are pure; history undo/redo', () => {
    const a: PlanStep = {
      id: 'a',
      type: 'PICK',
      target: { kind: 'cell', id: 101 },
      item_code: 1001,
      count: 1,
      note: '',
    }
    const b: PlanStep = {
      id: 'b',
      type: 'DROP',
      target: { kind: 'cell', id: 102 },
      item_code: 1001,
      count: 1,
      note: '',
    }
    expect(move([a, b], 1, 0).map((s) => s.id)).toEqual(['b', 'a'])
    expect(toggleType([a, b], 'a')[0].type).toBe('DROP')
    let h = commit(EMPTY_HISTORY, [a])
    h = commit(h, [a, b])
    expect(h.present.length).toBe(2)
    h = undo(h)
    expect(h.present.length).toBe(1)
    h = undo(h)
    expect(h.present.length).toBe(0)
    expect(undo(h)).toBe(h)
    h = redo(h)
    expect(h.present.length).toBe(1)
    h = commit(h, [b])
    expect(h.future).toEqual([])
  })

  it('toScenario and overlay', () => {
    const steps: PlanStep[] = [
      {
        id: 'a',
        type: 'PICK',
        target: { kind: 'cell', id: 101 },
        item_code: 1001,
        count: 1,
        note: '',
      },
      {
        id: 'b',
        type: 'DROP',
        target: { kind: 'station', id: 2101 },
        item_code: 1001,
        count: 1,
        note: '',
      },
      {
        id: 'c',
        type: 'PICK',
        target: { kind: 'cell', id: 101 },
        item_code: 1001,
        count: 1,
        note: '',
      },
    ]
    const sc = toScenario(steps, 'plan')
    expect(sc.steps.length).toBe(3)
    expect(sc.steps[1].label).toBe('2. DROP 스테이션 #2101')
    expect(sc.steps[0].wait_for).toBe('completed')
    const o = overlay(steps)
    expect(o.badges.get('cell-101')).toEqual([
      { no: 1, type: 'PICK' },
      { no: 3, type: 'PICK' },
    ])
    expect(o.path.length).toBe(3)
  })
})
