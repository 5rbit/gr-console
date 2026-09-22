import { describe, expect, it } from 'vitest'
import type { Cell, Station } from '../types'
import {
  bodyCentre,
  boundsOf,
  fitView,
  gridStep,
  panBy,
  resolveView,
  shapesFrom,
  stationAlign,
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

  it('station align follows GRM StationCenterAdjust (wall at Position, tire toward ±OD/2)', () => {
    expect([1, 5].map(stationAlign)).toEqual([[0, -1], [0, -1]])
    expect([2, 6].map(stationAlign)).toEqual([[0, 1], [0, 1]])
    expect([3, 7].map(stationAlign)).toEqual([[1, 0], [1, 0]])
    expect([4, 8].map(stationAlign)).toEqual([[-1, 0], [-1, 0]])
    expect([0, 9, -1].map(stationAlign)).toEqual([null, null, null])
  })

  it('aligned station body is pushed off the wall; cells and type 0 stay centred', () => {
    const [c, s0] = shapesFrom([cell(1, 100, 200)], [station(2101, 5000, 300)])
    expect(c.align).toBeNull()
    expect(bodyCentre(c, 300)).toEqual([100, 200])
    expect(bodyCentre(s0, 300)).toEqual([5000, 300])
    const [s6] = shapesFrom([], [{ ...station(2101, 5000, 300), rotate_type: 6 }])
    expect(bodyCentre(s6, 300)).toEqual([5000, 600])
    // 경계도 민 몸체 기준.
    expect(boundsOf([s6], 300)).toEqual({ minX: 4700, minY: 300, maxX: 5300, maxY: 900 })
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

describe('layoutModel flipX', () => {
  it('flipX mirrors X on screen and round-trips', () => {
    const v = fitView({ minX: 0, minY: 0, maxX: 2000, maxY: 1000 }, 800, 600, 24, true, true)
    expect(toScreen(v, 2000, 0)[0]).toBeLessThan(toScreen(v, 0, 0)[0])
    const [sx, sy] = toScreen(v, 1500, 250)
    const [wx, wy] = toWorld(v, sx, sy)
    expect(wx).toBeCloseTo(1500)
    expect(wy).toBeCloseTo(250)
  })
})

describe('layoutModel rotation', () => {
  it('round-trips and centres the bounds for every rotation and mirror', () => {
    for (const rot of [0, 90, 180, 270] as const) {
      for (const flipX of [false, true]) {
        for (const flipY of [false, true]) {
          const v = fitView({ minX: 0, minY: 0, maxX: 2000, maxY: 1000 }, 800, 600, 24, flipY, flipX, rot)
          const [sx, sy] = toScreen(v, 1500, 250)
          const [wx, wy] = toWorld(v, sx, sy)
          expect(wx).toBeCloseTo(1500)
          expect(wy).toBeCloseTo(250)
          const [cx, cy] = toScreen(v, 1000, 500)
          expect(cx).toBeCloseTo(400)
          expect(cy).toBeCloseTo(300)
        }
      }
    }
  })

  it('90 degrees clockwise: +X points down, +Y points right', () => {
    const v = fitView({ minX: 0, minY: 0, maxX: 2000, maxY: 1000 }, 800, 600, 24, true, false, 90)
    const [ox, oy] = toScreen(v, 0, 0)
    const [xx, xy] = toScreen(v, 100, 0)
    const [yx, yy] = toScreen(v, 0, 100)
    expect(xy).toBeGreaterThan(oy)
    expect(xx).toBeCloseTo(ox)
    expect(yx).toBeGreaterThan(ox)
    expect(yy).toBeCloseTo(oy)
  })

  it('fits the rotated extent (width and height swap at 90 degrees)', () => {
    const b = { minX: 0, minY: 0, maxX: 4000, maxY: 1000 }
    expect(fitView(b, 800, 800, 0, true, false, 0).k).toBeCloseTo(0.2)
    expect(fitView(b, 800, 800, 0, true, false, 90).k).toBeCloseTo(0.2)
    // 4000 mm now spans the 400 px height
    expect(fitView(b, 800, 400, 0, true, false, 90).k).toBeCloseTo(0.1)
    expect(fitView(b, 800, 400, 0, true, false, 0).k).toBeCloseTo(0.2)
  })

  describe('resolveView (2026-09-21 레이아웃이 화면에서 안 보임)', () => {
    const b = { minX: 1000, minY: 1000, maxX: 12000, maxY: 9500 }
    const allInside = (v: ReturnType<typeof fitView>, w: number, h: number) =>
      [
        [b.minX, b.minY],
        [b.maxX, b.minY],
        [b.minX, b.maxY],
        [b.maxX, b.maxY],
      ].every(([x, y]) => {
        const [sx, sy] = toScreen(v, x, y)
        return sx >= 0 && sx <= w && sy >= 0 && sy <= h
      })

    it('fit mode follows the current container size (no stale first fit)', () => {
      // first render measured the default 800x500, then the pane became 982x488 / 1920x1080
      for (const [w, h] of [
        [800, 500],
        [982, 488],
        [1678, 900],
        [444, 317],
      ]) {
        const v = resolveView(null, b, w, h, 48, true, false, 0)
        expect(allInside(v, w, h)).toBe(true)
        expect(v).toEqual(fitView(b, w, h, 48, true, false, 0))
      }
    })

    it('a fit frozen at another size clips the pane — the bug this guards', () => {
      // taller pane at first fit (e.g. before the split/table took its share) → later 982x488
      const frozen = fitView(b, 982, 900, 48, true, false, 0)
      expect(allInside(frozen, 982, 488)).toBe(false)
      expect(allInside(resolveView(null, b, 982, 488, 48, true, false, 0), 982, 488)).toBe(true)
    })

    it('keeps a manual (panned/zoomed) view while rotation/flip match', () => {
      const manual = panBy(fitView(b, 800, 500, 48, true, false, 0), 100, 50)
      expect(resolveView(manual, b, 1920, 1080, 48, true, false, 0)).toBe(manual)
    })

    it('drops a manual view made under another rotation/flip', () => {
      const manual = fitView(b, 800, 500, 48, true, false, 0)
      expect(resolveView(manual, b, 800, 500, 48, true, false, 90)).toEqual(
        fitView(b, 800, 500, 48, true, false, 90),
      )
      expect(resolveView(manual, b, 800, 500, 48, false, false, 0).flipY).toBe(false)
    })

    it('without shapes centres a placeholder view', () => {
      const v = resolveView(null, null, 600, 400, 48, true, true, 180)
      expect(v).toEqual({ k: 0.05, ox: 300, oy: 200, flipY: true, flipX: true, rot: 180 })
    })
  })
})
