// 선 그래프 그리기 — GrWeb `drawChart/drawSeries` 이식. 캔버스 컨텍스트만 받는 순수 그리기 함수.
import { ema } from '../../meas/trend'

export interface ChartPoint {
  x: number
  y: number
  /** 점 색을 바꿀 상태(예: 에러). */
  bad?: boolean
}

export interface ChartColors {
  text: string
  grid: string
  line: string
  avg: string
  ema: string
  bad: string
}

export interface ChartOptions {
  label: string
  showAvg?: boolean
  showEma?: boolean
  colors: ChartColors
  /** x 축 눈금 라벨 — 기본은 정수. */
  xLabel?: (x: number) => string
  yDigits?: number
}

export interface Scale {
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

/** 여백과 축 스케일(값 범위가 0이면 ±1, 그 외 10% 여유). */
export function scaleFor(pts: readonly ChartPoint[], w: number, h: number): Scale {
  const L = 60
  const R = 16
  const T = 24
  const B = 34
  let ymin = Infinity
  let ymax = -Infinity
  for (const p of pts) {
    if (p.y < ymin) ymin = p.y
    if (p.y > ymax) ymax = p.y
  }
  if (!Number.isFinite(ymin)) {
    ymin = 0
    ymax = 1
  }
  if (ymax - ymin < 1e-6) {
    ymin -= 1
    ymax += 1
  }
  const pad = (ymax - ymin) * 0.1
  ymin -= pad
  ymax += pad
  const xmin = pts.length ? pts[0].x : 0
  const xmax = pts.length ? pts[pts.length - 1].x : 1
  const X = (x: number) => L + (xmax === xmin ? 0.5 : (x - xmin) / (xmax - xmin)) * (w - L - R)
  const Y = (y: number) => T + (1 - (y - ymin) / (ymax - ymin)) * (h - T - B)
  return { L, R, T, B, xmin, xmax, ymin, ymax, X, Y }
}

export function drawChart(ctx: CanvasRenderingContext2D, w: number, h: number, pts: readonly ChartPoint[], o: ChartOptions): Scale {
  const c = o.colors
  ctx.clearRect(0, 0, w, h)
  ctx.fillStyle = c.text
  ctx.font = '12px sans-serif'
  ctx.fillText(`${o.label}  (n=${pts.length})`, 60, 15)
  const s = scaleFor(pts, w, h)
  if (pts.length === 0) {
    ctx.fillStyle = c.text
    ctx.fillText('데이터 없음', w / 2 - 30, h / 2)
    return s
  }
  const digits = o.yDigits ?? 2
  ctx.strokeStyle = c.grid
  ctx.fillStyle = c.text
  ctx.font = '11px sans-serif'
  for (let i = 0; i <= 5; i++) {
    const y = s.ymin + (s.ymax - s.ymin) * (i / 5)
    ctx.beginPath()
    ctx.moveTo(s.L, s.Y(y))
    ctx.lineTo(w - s.R, s.Y(y))
    ctx.stroke()
    ctx.fillText(y.toFixed(digits), 4, s.Y(y) + 4)
  }
  const xl = o.xLabel ?? ((x: number) => String(Math.round(x)))
  for (let i = 0; i <= 6; i++) {
    const x = s.xmin + (s.xmax - s.xmin) * (i / 6)
    ctx.fillText(xl(x), s.X(x) - 10, h - 12)
  }
  const ys = pts.map((p) => p.y)
  if (o.showAvg) {
    const avg = ys.reduce((a, b) => a + b, 0) / ys.length
    ctx.strokeStyle = c.avg
    ctx.setLineDash([6, 4])
    ctx.beginPath()
    ctx.moveTo(s.L, s.Y(avg))
    ctx.lineTo(w - s.R, s.Y(avg))
    ctx.stroke()
    ctx.setLineDash([])
  }
  if (o.showEma) {
    const e = ema(ys)
    ctx.strokeStyle = c.ema
    ctx.lineWidth = 1.5
    ctx.beginPath()
    pts.forEach((p, i) => (i ? ctx.lineTo(s.X(p.x), s.Y(e[i])) : ctx.moveTo(s.X(p.x), s.Y(e[i]))))
    ctx.stroke()
  }
  ctx.strokeStyle = c.line
  ctx.lineWidth = 1.2
  ctx.beginPath()
  pts.forEach((p, i) => (i ? ctx.lineTo(s.X(p.x), s.Y(p.y)) : ctx.moveTo(s.X(p.x), s.Y(p.y))))
  ctx.stroke()
  for (const p of pts) {
    ctx.fillStyle = p.bad ? c.bad : c.line
    ctx.beginPath()
    ctx.arc(s.X(p.x), s.Y(p.y), 2.5, 0, Math.PI * 2)
    ctx.fill()
  }
  return s
}

/** 시계열(시간축 없이 순번) — 축 위치 추이용. */
export function drawSeries(ctx: CanvasRenderingContext2D, w: number, h: number, ys: readonly number[], capacity: number, label: string, c: ChartColors): void {
  ctx.clearRect(0, 0, w, h)
  const L = 60
  const R = 10
  const T = 20
  const B = 16
  ctx.fillStyle = c.text
  ctx.font = '12px sans-serif'
  ctx.fillText(label, L, 14)
  if (ys.length < 2) return
  let ymin = Math.min(...ys)
  let ymax = Math.max(...ys)
  if (ymax - ymin < 1) {
    ymin -= 1
    ymax += 1
  }
  const pad = (ymax - ymin) * 0.1
  ymin -= pad
  ymax += pad
  const X = (i: number) => L + (i / Math.max(1, capacity - 1)) * (w - L - R)
  const Y = (y: number) => T + (1 - (y - ymin) / (ymax - ymin)) * (h - T - B)
  ctx.strokeStyle = c.grid
  ctx.fillStyle = c.text
  ctx.font = '11px sans-serif'
  for (let i = 0; i <= 4; i++) {
    const y = ymin + (ymax - ymin) * (i / 4)
    ctx.beginPath()
    ctx.moveTo(L, Y(y))
    ctx.lineTo(w - R, Y(y))
    ctx.stroke()
    ctx.fillText(y.toFixed(0), 4, Y(y) + 4)
  }
  ctx.strokeStyle = c.line
  ctx.lineWidth = 1.4
  ctx.beginPath()
  const off = capacity - ys.length
  ys.forEach((y, i) => (i ? ctx.lineTo(X(off + i), Y(y)) : ctx.moveTo(X(off + i), Y(y))))
  ctx.stroke()
}

/** CSS 변수에서 색을 읽는다(테마 대응). 없으면 기본색. */
export function chartColors(el: Element | null): ChartColors {
  const cs = el ? getComputedStyle(el) : null
  const v = (name: string, fallback: string) => (cs?.getPropertyValue(name).trim() || fallback)
  const dark = document.documentElement.getAttribute('data-theme') === 'dark'
  return {
    text: v('--color-content-primary', dark ? '#e2e8f0' : '#1c2430'),
    grid: v('--color-line-default', dark ? '#334155' : '#e3e6ea'),
    line: v('--color-info', '#0ea5e9'),
    avg: v('--color-ok', '#10b981'),
    ema: v('--color-warn', '#f59e0b'),
    bad: v('--color-fault', '#ef4444'),
  }
}
