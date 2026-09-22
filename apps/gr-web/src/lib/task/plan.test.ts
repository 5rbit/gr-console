import { describe, expect, it } from 'vitest'
import { DEFAULT_SPEC } from '../items/levelsModel'
import type { Cell, Item, Station, StockEntry } from '../types'
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
  sameStationGroup,
  stationGroup,
  toRequest,
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
      return {
        level: k,
        lower_bead: bottom + 20,
        upper_bead: bottom + 220 - 8 * above,
        stack_height: k === 5 ? 1120 : null,
        source: 'measured' as const,
      }
    })
    const bead = {
      ...item(1003, 240),
      upper_bead_height: 220,
      spec: {
        ...DEFAULT_SPEC,
        stack_max: 5,
        compression: 8,
        profiles: [
          {
            count: 5,
            rows: profRows,
            total_height: 1120,
            each_height: 224,
            sample_plc: 'GR2',
            sample_seq: 7,
            at: 't',
          },
        ],
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
    expect(
      gripOffset(
        'pick_bead',
        { ...bead, spec: { ...DEFAULT_SPEC, stack_max: 5, compression: 8 } } as Item,
        0,
      ),
    ).toBe(120)
    // 그 크기를 통째로 잰 프로파일이 있으면 절대값을 그대로 쓴다(환산·합산 없음)
    expect(planZ('PICK', 1500, bead, 'pick_bead', 5, 1)).toEqual({
      z: 1500 + 1100 - 30,
      ref: 'pick_bead',
      source: 'profile',
    })
    expect(planZ('PICK', 1500, bead, 'pick_bead', 5, 5)).toEqual({
      z: 1500 + 188 - 30,
      ref: 'pick_bead',
      source: 'profile',
    })
    // 잰 적 없는 크기는 곡선으로 환산, 잰 게 없으면 mid
    expect(planZ('PICK', 1500, bead, 'pick_bead', 3, 1)).toMatchObject({
      source: 'curve',
      ref: 'pick_bead',
    })
    expect(planZ('PICK', 1500, item(1001, 240), 'pick_bead', 3, 1)).toMatchObject({
      source: 'computed',
      ref: 'mid',
    })
    expect(planZ('MOVE', 1500, bead, 'pick_bead', 3, 1)).toEqual({
      z: 1500,
      ref: 'mid',
      source: 'computed',
    })
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

describe('multi-pick (station group)', () => {
  const st = (
    id: number,
    type: 'PICK' | 'DROP' | 'MOVE' = 'PICK',
    robot: number | null = null,
  ) => ({
    id: `s${id}${type}`,
    type,
    target: { kind: 'station' as const, id },
    item_code: 1001,
    count: 1,
    note: '',
    robot,
  })
  it('follows the PLC group rule (id / 100) % 10, 0 = none', () => {
    expect(stationGroup(2101)).toBe(1)
    expect(sameStationGroup(st(2101), st(2102))).toBe(true)
    expect(sameStationGroup(st(2101), st(2103, 'DROP'))).toBe(true)
    expect(sameStationGroup(st(2101), st(2201))).toBe(false)
    expect(sameStationGroup(st(2001), st(2002))).toBe(false)
    expect(sameStationGroup(st(2101), st(2102, 'MOVE'))).toBe(false)
    expect(sameStationGroup(st(2101, 'PICK', 2), st(2102, 'PICK', 1))).toBe(false)
    expect(sameStationGroup(st(2101), undefined)).toBe(false)
  })
  it('marks rows whose next step continues the group and sends multi_pick', () => {
    const rows = planRows([st(2101), st(2102), st(2201)], { ...ctx, stations: [] })
    expect(rows.map((r) => r.multiPick)).toEqual([true, false, false])
    expect(toRequest(rows[0], 2, rows[0].multiPick).multi_pick).toBe(true)
    expect(toRequest(rows[1], 2, rows[1].multiPick).multi_pick).toBeUndefined()
  })
})

describe('stepForClick item matching', () => {
  const stk = new Map<number, StockEntry>([
    [101, st(101, 1001, 3)],
    [102, st(102, 0, 0)],
    [2101, st(2101, 2011, 2)],
    [2102, st(2102, 0, 0)],
  ])
  const cellT = (id: number) => ({ kind: 'cell' as const, id })
  const stT = (id: number) => ({ kind: 'station' as const, id })

  it('PICK takes the item that is actually on the target (cell or station)', () => {
    expect(stepForClick([], cellT(101), stk, 'PICK', 9999).item_code).toBe(1001)
    expect(stepForClick([], stT(2101), stk, 'PICK', 9999).item_code).toBe(2011)
  })
  it('an empty or unknown target never borrows the carried item or the first item', () => {
    const holding = [stepForClick([], cellT(101), stk, 'PICK')]
    // 빈 셀 PICK: 들고 있는 1001 이 아니라 고른 품목, 없으면 null
    expect(stepForClick(holding, cellT(102), stk, 'PICK', 7777).item_code).toBe(7777)
    expect(stepForClick(holding, cellT(102), stk, 'PICK').item_code).toBeNull()
    expect(stepForClick([], cellT(999), stk, 'PICK').item_code).toBeNull()
  })
  it('DROP keeps the carried item, else the target stock item', () => {
    const holding = [stepForClick([], stT(2101), stk, 'PICK')]
    expect(stepForClick(holding, cellT(102), stk, 'DROP', 7777).item_code).toBe(2011)
    expect(stepForClick([], cellT(101), stk, 'DROP').item_code).toBe(1001)
  })
  it('the plan simulation empties a station after it is picked', () => {
    const s1 = stepForClick([], stT(2101), stk, 'PICK')
    const s2 = { ...stepForClick([s1], stT(2101), stk, 'PICK'), count: 2 }
    // 두 번째 클릭은 아직 1 개 남은 스테이션 재고(2011)
    expect(s2.item_code).toBe(2011)
    expect(simulateStock([s1, s2], stk).get(2101)?.count).toBe(0)
    expect(stepForClick([s1, s2], stT(2101), stk, 'PICK').item_code).toBeNull()
  })
})

describe('station stock stacking (2026-09-22)', () => {
  // 스테이션 위 멀티 DROP 은 이미 있는 타이어 위로 쌓여야 한다 — 전에는 스테이션 재고를 0 으로 봤다.
  const station = { id: 2101, info: { id: 2101, position: [7000, 4800, 1100] } } as unknown as Station
  const drop = (id: string): PlanStep => ({ id, type: 'DROP', target: { kind: 'station', id: 2101 }, item_code: 1001, count: 1, note: '' })
  const pick = (id: string): PlanStep => ({ id, type: 'PICK', target: { kind: 'cell', id: 101 }, item_code: 1001, count: 1, note: '' })
  it('DROP on a station uses its stock and the chain of steps before', () => {
    const stockNow = new Map<number, StockEntry>([...stock, [2101, st(2101, 1001, 2)]])
    const rows = planRows([pick('a'), drop('b'), pick('c'), drop('d')], { ...ctx, stations: [station], stockNow })
    expect(rows[1].stockBefore).toBe(2)
    expect(rows[1].stockAfter).toBe(3)
    expect(rows[3].stockBefore).toBe(3)
    expect(rows[3].stockAfter).toBe(4)
    // 두 번째 DROP 은 첫 DROP 보다 한 단(240) 위
    expect((rows[3].z ?? 0) - (rows[1].z ?? 0)).toBe(240)
    expect(rows[1].z).toBe(planZ('DROP', 1100, item(1001, 240), 'mid', 2, 1).z)
  })
})
