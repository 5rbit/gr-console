// XY 그래프 그리기 — 캔버스 컨텍스트만 받는 순수 함수. 여러 계열(선/점) + 기준선(축에 수직·수평) + 범례.
// 색은 chartColors() 가 테마 토큰에서 읽어 온 것을 tone 번호로 고른다.
import type { ChartColors } from './lineChartModel'

export interface XYPoint {
  x: number
  y: number
}

export interface XYSeries {
  name: string
  /** palette() 인덱스 */
  tone: number
  points: readonly XYPoint[]
  /** true 면 점만, 아니면 선 */
  dots?: boolean
}

export interface XYRule {
  axis: 'x' | 'y'
  value: number
  label: string
  tone: number
}

export interface XYOptions {
  xLabel: string
  yLabel: string
  colors: ChartColors
  rules?: readonly XYRule[]
  xDigits?: number
  yDigits?: number
}

export interface XYScale {
  L: number
  R: number
  T: number
  B: number
  xmin: number
  xmax: number
  ymin: number
  ymax: number
  X: (x: number) => number
  Y: (y: number) => number
}

/** 계열 색 순서 — 정보·정상·경고·결함·본문. */
export function palette(c: ChartColors): string[] {
  return [c.line, c.avg, c.ema, c.bad, c.text]
}

function range(values: readonly number[]): [number, number] {
  const v = values.filter(Number.isFinite)
  if (!v.length) return [0, 1]
  let lo = Math.min(...v)
  let hi = Math.max(...v)
  if (lo === hi) {
    lo -= 1
    hi += 1
  }
  const pad = (hi - lo) * 0.05
  return [lo - pad, hi + pad]
}

/** 점과 기준선을 모두 담는 스케일. */
export function xyScale(series: readonly XYSeries[], rules: readonly XYRule[], w: number, h: number): XYScale {
  const xs: number[] = []
  const ys: number[] = []
  for (const s of series) {
    for (const p of s.points) {
      xs.push(p.x)
      ys.push(p.y)
    }
  }
  for (const r of rules) (r.axis === 'x' ? xs : ys).push(r.value)
  const [xmin, xmax] = range(xs)
  const [ymin, ymax] = range(ys)
  const L = 58
  const R = Math.max(L + 10, w - 12)
  const T = 22
  const B = Math.max(T + 10, h - 30)
  return {
    L,
    R,
    T,
    B,
    xmin,
    xmax,
    ymin,
    ymax,
    X: (x) => L + ((x - xmin) / (xmax - xmin)) * (R - L),
    Y: (y) => B - ((y - ymin) / (ymax - ymin)) * (B - T),
  }
}

export function drawXY(ctx: CanvasRenderingContext2D, w: number, h: number, series: readonly XYSeries[], o: XYOptions): XYScale {
  const rules = o.rules ?? []
  const s = xyScale(series, rules, w, h)
  const pal = palette(o.colors)
  const tone = (i: number) => pal[((i % pal.length) + pal.length) % pal.length] ?? o.colors.text
  ctx.clearRect(0, 0, w, h)
  ctx.font = '11px sans-serif'
  ctx.lineWidth = 1

  // 격자·눈금
  ctx.strokeStyle = o.colors.grid
  ctx.fillStyle = o.colors.text
  for (let i = 0; i <= 5; i++) {
    const xv = s.xmin + ((s.xmax - s.xmin) * i) / 5
    const yv = s.ymin + ((s.ymax - s.ymin) * i) / 5
    const px = s.X(xv)
    const py = s.Y(yv)
    ctx.beginPath()
    ctx.moveTo(px, s.T)
    ctx.lineTo(px, s.B)
    ctx.moveTo(s.L, py)
    ctx.lineTo(s.R, py)
    ctx.stroke()
    ctx.textAlign = 'center'
    ctx.fillText(xv.toFixed(o.xDigits ?? 1), px, s.B + 14)
    ctx.textAlign = 'right'
    ctx.fillText(yv.toFixed(o.yDigits ?? 1), s.L - 4, py + 4)
  }
  ctx.textAlign = 'right'
  ctx.fillText(o.xLabel, s.R, h - 2)
  ctx.textAlign = 'left'
  ctx.fillText(o.yLabel, 4, 12)

  // 기준선
  ctx.setLineDash([4, 3])
  for (const r of rules) {
    ctx.strokeStyle = tone(r.tone)
    ctx.fillStyle = tone(r.tone)
    ctx.beginPath()
    if (r.axis === 'x') {
      const px = s.X(r.value)
      ctx.moveTo(px, s.T)
      ctx.lineTo(px, s.B)
      ctx.stroke()
      ctx.textAlign = 'left'
      ctx.fillText(r.label, px + 3, s.T + 10)
    } else {
      const py = s.Y(r.value)
      ctx.moveTo(s.L, py)
      ctx.lineTo(s.R, py)
      ctx.stroke()
      ctx.textAlign = 'right'
      ctx.fillText(r.label, s.R - 2, py - 3)
    }
  }
  ctx.setLineDash([])

  // 계열
  ctx.lineWidth = 1.5
  for (const ser of series) {
    const c = tone(ser.tone)
    ctx.strokeStyle = c
    ctx.fillStyle = c
    if (ser.dots) {
      for (const p of ser.points) ctx.fillRect(s.X(p.x) - 1.5, s.Y(p.y) - 1.5, 3, 3)
    } else {
      ctx.beginPath()
      ser.points.forEach((p, i) => (i ? ctx.lineTo(s.X(p.x), s.Y(p.y)) : ctx.moveTo(s.X(p.x), s.Y(p.y))))
      ctx.stroke()
    }
  }

  // 범례
  let lx = s.L + 4
  ctx.textAlign = 'left'
  for (const ser of series) {
    ctx.fillStyle = tone(ser.tone)
    ctx.fillRect(lx, 6, 10, 3)
    ctx.fillStyle = o.colors.text
    ctx.fillText(ser.name, lx + 13, 11)
    lx += 18 + ctx.measureText(ser.name).width + 8
  }
  return s
}
