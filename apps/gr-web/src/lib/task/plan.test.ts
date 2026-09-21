import { describe, expect, it } from 'vitest'
import { DEFAULT_SPEC } from '../items/levelsModel'
import type { Cell, Item, StockEntry } from '../types'
import {
  EMPTY_HISTORY,
  GRIP_REFS,
  aboveCount,
  carried,
  gripLabel,
  gripOffset,
  planZ,
  normalizeGripRef,
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

  it('그립 기준과 눌림 — 위에 얹힌 개수로 비드가 내려간다', () => {
    // 화면이 고를 수 있는 기준은 둘뿐이다 — 맨 비드(`bead`)는 없어졌다
    expect(GRIP_REFS.map((g) => g.id)).toEqual(['mid', 'pick_bead'])
    expect(GRIP_REFS.find((g) => g.id === 'pick_bead')?.label).toContain('bead+offset')
    expect(normalizeGripRef('bead')).toBe('pick_bead')
    expect(normalizeGripRef('bead+offset')).toBe('pick_bead')
    expect(normalizeGripRef(null)).toBe('mid')
    // 미리보기 한 줄은 어느 갈래로 잡았는지 말한다
    expect(gripLabel('pick_bead', 'pick_bead')).toBe('grip=bead+offset')
    expect(gripLabel('pick_bead', 'mid')).toBe('grip=mid (측정 없음)')
    expect(gripLabel('bead', 'mid')).toBe('grip=mid (측정 없음)')
    expect(gripLabel('mid', 'mid')).toBe('grip=mid')
    // 5 단 스택을 통째로 잰 절대 프로파일(눌림 8 mm/개) — 정본
    const profRows = Array.from({ length: 5 }, (_, i) => {
      const k = i + 1
      const above = 5 - k
      let bottom = 0
      for (let j = 1; j < k; j++) bottom += 240 - 8 * (5 - j)
      return { level: k, lower_bead: bottom + 20, upper_bead: bottom + 220 - 8 * above, stack_height: k === 5 ? 1120 : null, source: 'measured' as const }
    })
    const bead = {
      ...item(1003, 240),
      upper_bead_height: 220,
      spec: {
        ...DEFAULT_SPEC,
        stack_max: 5,
        compression: 8,
        profiles: [{ count: 5, rows: profRows, total_height: 1120, each_height: 224, sample_plc: 'GR2', sample_seq: 7, at: 't' }],
      },
    } as Item
    // 한 번에 c 개를 집으면 잡는 타이어 위에 c−1 개가 얹혀 있다(DROP 은 0)
    expect(aboveCount('PICK', 5, 1)).toBe(0)
    expect(aboveCount('PICK', 5, 5)).toBe(4)
    expect(aboveCount('DROP', 5, 1)).toBe(0)
    expect(gripOffset('mid', bead, 4)).toBe(120)
    expect(gripOffset('pick_bead', bead, 0)).toBe(190)
    expect(gripOffset('pick_bead', bead, 4)).toBe(158)
    // 옛 `bead` 는 pick_bead 와 같은 값으로 읽힌다
    expect(gripOffset('bead', bead, 4)).toBe(158)
    // 한 번도 안 잰 품목·하중은 mid(H/2)
    expect(gripOffset('pick_bead', item(1001, 240), 0)).toBe(120)
    expect(gripOffset('pick_bead', { ...bead, spec: { ...DEFAULT_SPEC, stack_max: 5, compression: 8 } } as Item, 0)).toBe(120)
    // 그 크기를 통째로 잰 프로파일이 있으면 절대값을 그대로 쓴다(환산·합산 없음)
    expect(planZ('PICK', 1500, bead, 'pick_bead', 5, 1)).toEqual({ z: 1500 + 1100 - 30, ref: 'pick_bead', source: 'profile' })
    expect(planZ('PICK', 1500, bead, 'pick_bead', 5, 5)).toEqual({ z: 1500 + 188 - 30, ref: 'pick_bead', source: 'profile' })
    // 잰 적 없는 크기는 곡선으로 환산, 잰 게 없으면 mid
    expect(planZ('PICK', 1500, bead, 'pick_bead', 3, 1)).toMatchObject({ source: 'curve', ref: 'pick_bead' })
    expect(planZ('PICK', 1500, item(1001, 240), 'pick_bead', 3, 1)).toMatchObject({ source: 'computed', ref: 'mid' })
    expect(planZ('MOVE', 1500, bead, 'pick_bead', 3, 1)).toEqual({ z: 1500, ref: 'mid', source: 'computed' })
    // 눌림은 아래 스택 높이에도 먹는다 — 백엔드 stack_z_with 과 같은 수
    expect(stackZ('PICK', 1500, 240, 5, 1, 190, 8)).toBe(1500 + 880 + 190)
    expect(stackZ('DROP', 1500, 240, 5, 1, 190, 8)).toBe(1500 + 1120 + 190)
    // 계획 표가 그립 기준·눌림을 실제로 태운다(셀 101 재고 3개에서 1개 집기)
    const rows = planRows(
      [
        {
          id: 'a',
          type: 'PICK',
          target: { kind: 'cell', id: 101 },
          item_code: 1003,
          count: 1,
          note: '',
        },
      ],
      { ...ctx, items: [...ctx.items, bead], gripRef: 'pick_bead' },
    )
    expect(rows[0].z).toBe(1500 + (224 + 232) + 190)
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
    expect(sc.steps[1].label).toBe('2. DROP Station #2101')
    expect(sc.steps[0].wait_for).toBe('completed')
    const o = overlay(steps)
    expect(o.badges.get('cell-101')).toEqual([
      { no: 1, type: 'PICK' },
      { no: 3, type: 'PICK' },
    ])
    expect(o.path.length).toBe(3)
  })
})
