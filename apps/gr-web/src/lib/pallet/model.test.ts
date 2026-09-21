import { describe, expect, it } from 'vitest'
import {
  MACHINE_AXES,
  applyDir,
  applyTransform,
  axisLabels,
  buildSteps,
  clampLevels,
  dirFromVec,
  dirGrid,
  dirVec,
  dragArrow,
  dragDelta,
  dragTypeByte,
  hexByte,
  levelOrder,
  minDistance,
  overhang,
  patternForOd,
  planCsv,
  planQuery,
  slotTone,
  toScreen,
  viewAxes,
  type Flow,
  type PlanSlot,
  type SizeClass,
  type Transform,
} from './model'

const ALL: Transform[] = []
for (const rotation of [0, 90, 180, 270])
  for (const mirror_x of [false, true])
    for (const mirror_y of [false, true]) ALL.push({ rotation, mirror_x, mirror_y })

const t = (rotation: number, mirror_x = false, mirror_y = false): Transform => ({
  rotation,
  mirror_x,
  mirror_y,
})

describe('drag dir codes (PLC DragDelta)', () => {
  it('byte of each dir moves along its machine vector', () => {
    for (let d = 1; d <= 8; d++) {
      const [sx, sy] = dirVec(d)!
      const b = dragTypeByte(d)
      expect(b).toBe(1 << (d - 1))
      const k = sx !== 0 && sy !== 0 ? 100 / 1.414214 : 100
      expect(dragDelta('X', b, 100)).toBeCloseTo(sx * k, 3)
      expect(dragDelta('Y', b, 100)).toBeCloseTo(sy * k, 3)
      expect(dirFromVec(sx, sy)).toBe(d)
    }
    expect(dragTypeByte(0)).toBe(0)
    expect(dragTypeByte(9)).toBe(0)
    expect(dirVec(0)).toBeNull()
    expect(hexByte(4)).toBe('0x04')
    expect(hexByte(128)).toBe('0x80')
  })

  it('transforms dirs exactly like offsets (all 16 transforms × 8 dirs)', () => {
    for (const tr of ALL) {
      for (let d = 1; d <= 8; d++) {
        const v = applyTransform(tr, dirVec(d)!)
        expect(dirVec(applyDir(tr, d))).toEqual(v)
        const b = dragTypeByte(applyDir(tr, d))
        const k = v[0] !== 0 && v[1] !== 0 ? 1 / 1.414214 : 1
        expect(dragDelta('X', b, 1)).toBeCloseTo(v[0] * k, 4)
        expect(dragDelta('Y', b, 1)).toBeCloseTo(v[1] * k, 4)
      }
      expect(applyDir(tr, 0)).toBe(0)
    }
    // same spot checks as the Rust tests
    expect(applyDir(t(90), 2)).toBe(5)
    expect(applyTransform(t(90), [1, 0])).toEqual([0, 1])
    expect(applyDir(t(0, true), 3)).toBe(8)
    expect(applyDir(t(0, false, true), 3)).toBe(1)
    expect(applyDir(t(180), 1)).toBe(8)
  })
})

describe('view orientation', () => {
  it('spec view reproduces the slide legends', () => {
    // slides 2/4/5/6: X+ left, Y+ up — legend "3 5 8 / 2 · 7 / 1 4 6"
    expect(dirGrid({ right: 'X-', down: 'Y-' })).toEqual([
      [3, 5, 8],
      [2, null, 7],
      [1, 4, 6],
    ])
    // slide 3: X+ right, Y+ down — legend "6 4 1 / 7 · 2 / 8 5 3"
    expect(dirGrid({ right: 'X+', down: 'Y+' })).toEqual([
      [6, 4, 1],
      [7, null, 2],
      [8, 5, 3],
    ])
    expect(dirGrid(MACHINE_AXES)).toEqual([
      [8, 5, 3],
      [7, null, 2],
      [6, 4, 1],
    ])
  })

  it('maps machine points to the screen and labels the axes', () => {
    const flow = { screen_axes: { right: 'X-', down: 'Y-' } } as Flow
    expect(viewAxes('machine', flow)).toEqual(MACHINE_AXES)
    const spec = viewAxes('spec', flow)
    expect(toScreen(spec, [100, 200])).toEqual([-100, -200])
    expect(toScreen(MACHINE_AXES, [100, 200])).toEqual([100, -200])
    expect(axisLabels(spec)).toEqual({ right: 'X-', left: 'X+', down: 'Y-', up: 'Y+' })
    expect(viewAxes('spec', null)).toEqual(MACHINE_AXES)
  })

  it('draws drag-in toward the slot and drag-out away from it', () => {
    const a = dragArrow([0, 0], 2, 400, 150, 'in')!
    expect(a.from).toEqual([550, 0])
    expect(a.to).toEqual([400, 0])
    const o = dragArrow([0, 0], 2, 400, 150, 'out')!
    expect(o.from).toEqual([400, 0])
    expect(o.to).toEqual([550, 0])
    const diag = dragArrow([10, 10], 6, 100, 100, 'in')!
    expect(diag.to[0]).toBeCloseTo(10 - 100 / Math.SQRT2, 6)
    expect(diag.to[1]).toBeCloseTo(10 - 100 / Math.SQRT2, 6)
    expect(dragArrow([0, 0], 0, 400, 150, 'in')).toBeNull()
    expect(dragArrow([0, 0], 3, 400, 0, 'in')).toBeNull()
  })
})

describe('pattern and fit', () => {
  const classes: SizeClass[] = [
    { pattern: 2, od_min: 814, od_max: 937 },
    { pattern: 3, od_min: 756, od_max: 813 },
    { pattern: 4, od_min: 663, od_max: 755 },
    { pattern: 5, od_min: 601, od_max: 662 },
    { pattern: 6, od_min: 546, od_max: 600 },
    { pattern: 8, od_min: 521, od_max: 545 },
    { pattern: 9, od_min: 0, od_max: 520 },
  ]
  it('picks the pattern by outer diameter like the backend', () => {
    const cases: [number, number | null][] = [
      [937, 2],
      [813.5, 3],
      [780, 3],
      [700, 4],
      [662, 5],
      [600, 6],
      [545, 8],
      [500, 9],
      [937.1, null],
      [0, null],
    ]
    for (const [od, p] of cases) expect(patternForOd(classes, od)).toBe(p)
  })

  const slot = (seq: number, ox: number, oy: number): PlanSlot => ({
    seq,
    slot: `C#${seq}`,
    u: [0, 0],
    offset_x: ox,
    offset_y: oy,
    x: ox,
    y: oy,
    spec_drag_dir: 0,
    drag_dir: 0,
    drag_type: 0,
    overhang: overhang([ox, oy], 900, 1600),
  })

  it('flags overhang, overlap and GR2 area rejects', () => {
    expect(overhang([350, -100], 900, 1600)).toEqual([0, -250])
    expect(overhang([-360, 0], 900, 1600)[0]).toBe(10)
    expect(minDistance([[0, 0], [3, 4], [10, 0]])).toBe(5)
    expect(minDistance([[0, 0]])).toBe(0)
    const ok = slot(1, 300, 0)
    const out = slot(2, 360, 0)
    expect(slotTone({ errors: [], area: null }, ok)).toBe('ok')
    expect(slotTone({ errors: [], area: null }, out)).toBe('warn')
    expect(slotTone({ errors: ['겹침'], area: null }, ok)).toBe('error')
    const area = {
      station_id: 1,
      task_type: 0,
      rotate_type: 0,
      margin: 300,
      expected_xy: [0, 0] as [number, number],
      from_registry: true,
      rejects: [{ seq: 1, slot: 'C#1', axis: 'X', code: 411, delta: 300 }],
    }
    expect(slotTone({ errors: [], area }, ok)).toBe('warn')
  })
})

describe('plan steps and export', () => {
  const plan = {
    flow: 'HP_IN',
    pattern: 3,
    drag_kind: 'in' as const,
    type: 'PICK' as const,
    slots: [
      { seq: 2, slot: 'C#2', u: [0, 0], offset_x: -1, offset_y: 2, x: 9, y: 8, spec_drag_dir: 3, drag_dir: 3, drag_type: 4, overhang: [0, 0] },
      { seq: 1, slot: 'C#1', u: [0, 0], offset_x: 0, offset_y: 0, x: 10, y: 6, spec_drag_dir: 0, drag_dir: 0, drag_type: 0, overhang: [0, 0] },
    ] as PlanSlot[],
    z_levels: [
      { level: 1, n: 1, z: 220, z_source: 'computed' },
      { level: 2, n: 2, z: null, z_source: null },
    ],
  }

  it('orders levels by flow direction', () => {
    expect(levelOrder('out', 3)).toEqual([1, 2, 3])
    expect(levelOrder('in', 3)).toEqual([3, 2, 1])
    expect(clampLevels(undefined)).toBe(1)
    expect(clampLevels(0)).toBe(1)
    expect(clampLevels(99)).toBe(20)
  })

  it('builds station steps carrying pallet refs in seq order', () => {
    let n = 0
    const steps = buildSteps(plan, { station: 2021, item_code: 1001, levels: 2, robot: null }, () => `s${++n}`)
    expect(steps.map((s) => [s.pallet?.level, s.pallet?.seq])).toEqual([
      [2, 1],
      [2, 2],
      [1, 1],
      [1, 2],
    ])
    expect(steps[0]).toMatchObject({ id: 's1', type: 'PICK', target: { kind: 'station', id: 2021 }, item_code: 1001, count: 1 })
    const drops = buildSteps({ ...plan, drag_kind: 'out', type: 'DROP' }, { station: 7, item_code: null, levels: 2, robot: 2 })
    expect(drops.map((s) => s.pallet?.level)).toEqual([1, 1, 2, 2])
    expect(drops.every((s) => s.type === 'DROP' && s.robot === 2)).toBe(true)
  })

  it('copies a CSV with English headers and hex DragType', () => {
    const csv = planCsv(plan).split('\n')
    expect(csv[0]).toBe('Seq,Slot,OffsetX,OffsetY,X,Y,DragDir,DragType,Z L1,Z L2')
    expect(csv[1]).toBe('1,C#1,0,0,10,6,0,0x00,220,')
    expect(csv[2]).toBe('2,C#2,-1,2,9,8,3,0x04,220,')
  })

  it('builds the plan query without empty values', () => {
    expect(planQuery({ station: 2021, od: 780, gap: 50, mirror_x: true, mirror_y: false, pattern: 'auto', center_x: NaN })).toBe(
      '?station=2021&od=780&gap=50&mirror_x=1&mirror_y=0&pattern=auto',
    )
    expect(planQuery({})).toBe('')
  })
})
