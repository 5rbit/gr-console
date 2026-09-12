import { describe, expect, it } from 'vitest'
import type { Cell, Station } from '../types'
import {
  boundsOf,
  fitView,
  gridStep,
  panBy,
  shapesFrom,
  toScreen,
  toWorld,
  zoomAt,
} from './layoutModel'

const cell = (id: number, x: number, y: number): Cell => ({
  id,
  use: true,
  blend_use: false,
  section: 1,
  row: 1,
  col: id,
  length: 1200,
  width: 1100,
  position: [x, y, 1500],
  source: 'plc',
  dirty: false,
  updated_at: '',
})
const station = (id: number, x: number, y: number): Station => ({
  id,
  conv_no: 1,
  task_type: 0,
  rotate_type: 0,
  group: 1,
  group_index: 1,
  connection_prev: 0,
  connection_next: 0,
  info: {
    id,
    use: true,
    blend_use: false,
    section: 3,
    row: 1,
    col: 1,
    length: 0,
    width: 0,
    position: [x, y, 0],
  },
  sensor: {} as Station['sensor'],
  io_block_no: 0,
  source: 'plc',
  dirty: false,
  updated_at: '',
})

describe('layoutModel', () => {
  it('shapes keep PLC centre coordinates and kinds', () => {
    const s = shapesFrom([cell(101, 1000, 2000)], [station(2101, 5000, 300)])
    expect(s.map((x) => [x.kind, x.id, x.x, x.y])).toEqual([
      ['cell', 101, 1000, 2000],
      ['station', 2101, 5000, 300],
    ])
  })

  it('bounds pad by radius and ignore non-finite', () => {
    const b = boundsOf(
      shapesFrom(
        [cell(1, 0, 0), cell(2, 1000, 500), { ...cell(3, NaN, 1), position: [NaN, 1, 0] }],
        [],
      ),
      100,
    )
    expect(b).toEqual({ minX: -100, minY: -100, maxX: 1100, maxY: 600 })
    expect(boundsOf([], 10)).toBeNull()
  })

  it('fitView centres bounds and round-trips screen <-> world (flipped Y)', () => {
    const v = fitView({ minX: 0, minY: 0, maxX: 2000, maxY: 1000 }, 800, 600, 24, true)
    const [sx, sy] = toScreen(v, 1000, 500)
    expect(sx).toBeCloseTo(400)
    expect(sy).toBeCloseTo(300)
    // Y up: higher PLC Y → smaller screen y
    expect(toScreen(v, 1000, 1000)[1]).toBeLessThan(sy)
    const [wx, wy] = toWorld(v, sx, sy)
    expect(wx).toBeCloseTo(1000)
    expect(wy).toBeCloseTo(500)
  })

  it('fitView without flip puts higher Y lower on screen', () => {
    const v = fitView({ minX: 0, minY: 0, maxX: 2000, maxY: 1000 }, 800, 600, 24, false)
    expect(toScreen(v, 0, 1000)[1]).toBeGreaterThan(toScreen(v, 0, 0)[1])
  })

  it('zoomAt keeps the anchor point fixed; panBy shifts', () => {
    const v = fitView({ minX: 0, minY: 0, maxX: 2000, maxY: 1000 }, 800, 600)
    const [ax, ay] = toScreen(v, 1500, 250)
    const z = zoomAt(v, ax, ay, 2)
    expect(z.k).toBeCloseTo(v.k * 2)
    const [bx, by] = toScreen(z, 1500, 250)
    expect(bx).toBeCloseTo(ax)
    expect(by).toBeCloseTo(ay)
    const p = panBy(z, 10, -5)
    expect(toScreen(p, 1500, 250)).toEqual([ax + 10, ay - 5])
  })

  it('gridStep picks a 1-2-5 step at least minPx apart', () => {
    expect(gridStep(0.1, 60)).toBe(1000) // 60px / 0.1 = 600mm → 1000
    expect(gridStep(1, 60)).toBe(100)
    expect(gridStep(0.05, 60)).toBe(2000)
  })
})
