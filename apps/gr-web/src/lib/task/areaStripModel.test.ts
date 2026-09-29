import { describe, expect, it } from 'vitest'
import type { AreaView } from '../types'
import { areaStrip, areaSummary, separationToPass } from './areaStripModel'

// 2026-09-29 현장: GR1 이 X 1500 에 서 있고, GR2(지금 10301) 의 PICK 셀 421 이 X 6305 → 간격 4805 < 5000.
const site: AreaView = {
  separation_mm: 5000,
  enabled: true,
  active: true,
  plc_min: 2403,
  robots: [
    {
      id: 1,
      name: 'GR1',
      x: 1500,
      area: { lo: 1500, hi: 1500 },
      idle: true,
      holds_pair: false,
      margin: 0,
    },
    {
      id: 2,
      name: 'GR2',
      x: 10301,
      area: { lo: 10301, hi: 10301 },
      idle: true,
      holds_pair: false,
      margin: 0,
    },
  ],
  check: {
    robot: 2,
    x: 6305,
    mine: { lo: 6305, hi: 10301 },
    nearest: 1,
    gap: 4805,
    need: 5000,
    blocked: true,
    reason: '영역 대기: GR1 X 1500..1500 사용 중 — 이 Task X 6305..10301 (간격 5000 mm)',
  },
}

describe('areaStrip', () => {
  it('places the other robot keep-out over my task when blocked', () => {
    const s = areaStrip(site, [1000, 12000])
    expect(s.blocked).toBe(true)
    expect(s.shortBy).toBe(195)
    const gr1 = s.robots.find((r) => r.id === 1)!
    expect(gr1.keepoutSpan).toEqual({ lo: -3500, hi: 6500 })
    expect(gr1.keepout!.left + gr1.keepout!.width).toBeGreaterThan(s.mine!.left)
    expect(s.robots.find((r) => r.id === 2)!.keepout).toBeNull()
    expect(s.gapBar!.width).toBeGreaterThan(0)
  })
  it('widens keep-out by both robot margins', () => {
    const v: AreaView = {
      ...site,
      robots: site.robots.map((r) => ({ ...r, margin: r.id === 1 ? 200 : 100 })),
    }
    expect(areaStrip(v, []).robots[0].keepoutSpan).toEqual({ lo: 1500 - 5300, hi: 1500 + 5300 })
  })
  it('keeps bars inside the axis', () => {
    const s = areaStrip(site, [])
    for (const r of s.robots) {
      if (r.keepout) expect(r.keepout.left).toBeGreaterThanOrEqual(0)
      if (r.keepout) expect(r.keepout.left + r.keepout.width).toBeLessThanOrEqual(100)
    }
  })
  it('shows no keep-out when the check is off', () => {
    expect(areaStrip({ ...site, active: false }, []).robots[0].keepout).toBeNull()
  })
})

describe('separationToPass', () => {
  it('tells the separation that lets this task go now', () => {
    expect(separationToPass(site)).toBe(4805)
    const m: AreaView = { ...site, check: { ...site.check!, need: 5300 } }
    expect(separationToPass(m)).toBe(4505)
  })
  it('gives up below the PLC floor — move the robot instead', () => {
    expect(separationToPass({ ...site, check: { ...site.check!, gap: 1000 } })).toBeNull()
  })
})

describe('areaSummary', () => {
  it('says who, how far and how short', () => {
    expect(areaSummary(site)).toBe('GR1 X 1500 · 거리 4805 / 필요 5000 mm — 195 mm 부족, 영역 대기')
    expect(areaSummary({ ...site, enabled: false, active: false })).toBe('영역 검사 꺼짐')
  })
})
