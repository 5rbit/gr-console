import { describe, expect, it } from 'vitest'
import { budgetFor } from '../../lib/ui/viz/downsample'
import {
  axisRange,
  buildSeries,
  fmtTick,
  niceTicks,
  padRange,
  pixelAtTime,
  plotBox,
  plotWidth,
  tickDigits,
  timeAtPixel,
  type PlotSeries,
  type SeriesSpec,
} from './traceChartModel'
import { RowArray, TraceRing } from './traceModel'

describe('plotBox', () => {
  it('오른쪽 축이 있으면 오른쪽 여백을 더 준다', () => {
    const a = plotBox(800, 300, false)
    const b = plotBox(800, 300, true)
    expect(b.R).toBeLessThan(a.R)
    expect(a.L).toBeLessThan(a.R)
    expect(a.T).toBeLessThan(a.B)
    expect(a.B).toBeLessThan(300)
  })

  it('아주 작은 캔버스에서도 상자가 뒤집히지 않는다', () => {
    const s = plotBox(20, 10, true)
    expect(s.R).toBeGreaterThan(s.L)
    expect(s.B).toBeGreaterThan(s.T)
  })

  it('그리기 폭이 점 예산의 입력이다', () => {
    expect(plotWidth(800, false)).toBe(plotBox(800, 100, false).R - plotBox(800, 100, false).L)
  })
})

describe('padRange', () => {
  it('값 범위에 5% 여유를 준다', () => {
    const r = padRange(0, 100)
    expect(r.min).toBeCloseTo(-5)
    expect(r.max).toBeCloseTo(105)
  })

  it('전부 같은 값이면 ±1 로 벌린다', () => {
    expect(padRange(7, 7)).toEqual({ min: 6, max: 8 })
  })

  it('값이 없으면(무한대) 0..1 로 떨어진다', () => {
    expect(padRange(Infinity, -Infinity)).toEqual({ min: 0, max: 1 })
  })
})

describe('axisRange', () => {
  const s = (axis: 'L' | 'R', v: (number | null)[]): PlotSeries => ({
    path: 'p',
    name: 'p',
    color: '',
    dash: [],
    axis,
    t: v.map((_, i) => i),
    v,
  })

  it('같은 축에 묶인 계열만 본다', () => {
    const series = [s('L', [0, 10]), s('R', [1000, 2000])]
    const l = axisRange(series, 'L')
    const r = axisRange(series, 'R')
    expect(l?.max).toBeLessThan(20)
    expect(r?.min).toBeGreaterThan(900)
  })

  it('결측만 있는 축은 범위가 없다', () => {
    expect(axisRange([s('L', [null, null])], 'L')).toBeNull()
    expect(axisRange([s('L', [1])], 'R')).toBeNull()
  })
})

describe('niceTicks', () => {
  it('1·2·5 간격의 읽히는 눈금을 낸다', () => {
    const t = niceTicks(0, 100, 5)
    expect(t[0]).toBe(0)
    expect(t[1] - t[0]).toBe(25)
    expect(t[t.length - 1]).toBeLessThanOrEqual(100)
  })

  it('오름차순이고 범위를 벗어나지 않는다', () => {
    const t = niceTicks(-3.7, 8.2, 6)
    for (let i = 1; i < t.length; i++) expect(t[i]).toBeGreaterThan(t[i - 1])
    expect(t[0]).toBeGreaterThanOrEqual(-3.7)
    expect(t[t.length - 1]).toBeLessThanOrEqual(8.2 + 1e-6)
  })

  it('누적 오차로 0.30000000000000004 같은 라벨이 서지 않는다', () => {
    for (const v of niceTicks(0, 1, 11)) expect(Number(v.toFixed(6))).toBe(v)
  })

  it('범위가 없으면 값 하나만 낸다', () => {
    expect(niceTicks(5, 5)).toEqual([5])
    expect(niceTicks(Number.NaN, 1)).toEqual([Number.NaN])
  })

  it('간격이 작을수록 라벨 자릿수를 늘린다', () => {
    expect(tickDigits([0, 100])).toBe(0)
    expect(tickDigits([0, 2])).toBe(1)
    expect(tickDigits([0, 0.5])).toBe(2)
    expect(tickDigits([0, 0.02])).toBe(3)
  })
})

describe('시간축 좌표', () => {
  const box = plotBox(800, 300, false)

  it('시각↔픽셀이 왕복한다', () => {
    const t = timeAtPixel(pixelAtTime(2500, box, 0, 5000), box, 0, 5000)
    expect(t).toBeCloseTo(2500, 6)
  })

  it('상자 밖 픽셀은 양 끝으로 물린다', () => {
    expect(timeAtPixel(-100, box, 0, 5000)).toBe(0)
    expect(timeAtPixel(5000, box, 0, 5000)).toBe(5000)
  })

  it('폭이 0 인 창은 왼쪽 끝에 선다', () => {
    expect(pixelAtTime(3, box, 7, 7)).toBe(box.L)
  })

  it('창이 짧을수록 라벨 자리를 늘린다', () => {
    expect(fmtTick(1500, 1000)).toBe('1.500')
    expect(fmtTick(1500, 10000)).toBe('1.50')
    expect(fmtTick(1500, 60000)).toBe('1.5')
  })
})

describe('buildSeries', () => {
  const specs: SeriesSpec[] = [
    { path: 'DB.a', name: 'a', index: 0, axis: 'L' },
    { path: 'DB.b', name: 'b', index: 1, axis: 'R' },
  ]

  it('예산 안이면 원본 점을 그대로 쓴다', () => {
    const src = new RowArray([
      [0, 1, 2],
      [1, 3, 4],
    ])
    const out = buildSeries(src, specs, 100)
    expect(out).toHaveLength(2)
    expect(out[0].t).toEqual([0, 1])
    expect(out[1].v).toEqual([2, 4])
    expect(out[1].axis).toBe('R')
  })

  it('예산을 넘으면 min/max 엔벨로프로 줄이되 극값은 남긴다', () => {
    const ring = new TraceRing(20000, 0)
    const rows: number[][] = []
    for (let i = 0; i < 6000; i++) rows.push([i, i === 3000 ? 999 : Math.sin(i), 0])
    ring.push('s', rows)
    const width = 300
    const out = buildSeries(ring, specs, width)
    expect(out[0].t.length).toBeLessThanOrEqual(budgetFor(width))
    expect(out[0].t.length).toBeLessThan(6000)
    expect(Math.max(...out[0].v.map((v) => v ?? 0))).toBe(999)
  })

  it('없는 채널 순번은 건너뛴다', () => {
    const src = new RowArray([[0, 1]])
    expect(buildSeries(src, specs, 100)).toHaveLength(1)
    expect(buildSeries(src, [{ path: 'x', name: 'x', index: 9, axis: 'L' }], 100)).toHaveLength(0)
  })

  it('색과 파선은 채널 순번이 정한다 — 계열을 껐다 켜도 안 바뀐다', () => {
    const src = new RowArray([[0, 1, 2]])
    const all = buildSeries(src, specs, 100)
    const onlySecond = buildSeries(src, [specs[1]], 100)
    expect(onlySecond[0].color).toBe(all[1].color)
  })
})
