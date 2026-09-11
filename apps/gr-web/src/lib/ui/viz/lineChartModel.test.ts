import { describe, expect, it } from 'vitest'
import { scaleFor } from './lineChartModel'

describe('scaleFor', () => {
  it('pads flat data by ±1 and others by 10%', () => {
    const flat = scaleFor([{ x: 1, y: 5 }, { x: 2, y: 5 }], 800, 360)
    expect(flat.ymin).toBeCloseTo(3.8)
    expect(flat.ymax).toBeCloseTo(6.2)
    const s = scaleFor([{ x: 1, y: 0 }, { x: 3, y: 10 }], 800, 360)
    expect(s.ymin).toBeCloseTo(-1)
    expect(s.ymax).toBeCloseTo(11)
    expect(s.X(1)).toBe(60)
    expect(s.X(3)).toBe(800 - 16)
    expect(s.Y(11)).toBe(24)
  })
  it('handles empty input', () => {
    const s = scaleFor([], 800, 360)
    expect(Number.isFinite(s.Y(0))).toBe(true)
  })
})
