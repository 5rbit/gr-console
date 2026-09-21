// 트레이스 파형 그리기 — 캔버스 컨텍스트만 받는 순수 함수와 그 축 계산.
//
// 차트 라이브러리를 들이지 않는 이유는 데이터가 아니라 **양** 때문이다: 200 Hz × 32 채널을 DOM
// 노드로 그리는 순간 끝난다. 캔버스에 직접 그리고, 점 수는 `viz/downsample` 의 min/max 엔벨로프로
// 화면 픽셀 수준까지 줄인다(평균으로 줄이면 스파이크가 사라져 트레이스의 존재 이유가 없어진다).

import type { ChartColors } from '../../lib/ui/viz/lineChartModel'
import { budgetFor, envelope } from '../../lib/ui/viz/downsample'
import { traceColor, traceDash } from '../../lib/ui/viz/traceSeries'
import type { RowSource } from './traceModel'

export type AxisSide = 'L' | 'R'

/** 그릴 계열 하나 — 이미 다운샘플된 (t, v) 쌍. */
export interface PlotSeries {
  path: string
  name: string
  color: string
  dash: readonly number[]
  axis: AxisSide
  t: number[]
  v: (number | null)[]
}

export interface Range {
  min: number
  max: number
}

export interface PlotBox {
  L: number
  R: number
  T: number
  B: number
}

export interface PlotMark {
  t_ms: number
  text: string
}

export interface TracePlot {
  series: readonly PlotSeries[]
  t0: number
  t1: number
  left: Range
  right: Range | null
  marks: readonly PlotMark[]
  /** 커서 시각(없으면 `null`). */
  cursor: number | null
  colors: ChartColors
  /** 비었을 때 가운데 낼 글. */
  empty: string
}

/** 눈금 라벨이 설 자리를 뺀 그리기 상자. 오른쪽 축이 없으면 오른쪽 여백을 줄인다. */
export function plotBox(w: number, h: number, hasRight: boolean): PlotBox {
  const L = 56
  const R = Math.max(L + 20, w - (hasRight ? 56 : 14))
  const T = 16
  const B = Math.max(T + 20, h - 26)
  return { L, R, T, B }
}

/** 상자의 가로 픽셀 폭 — 점 예산(`budgetFor`)의 입력이다. */
export function plotWidth(w: number, hasRight: boolean): number {
  const b = plotBox(w, 100, hasRight)
  return Math.max(1, b.R - b.L)
}

/** 값 범위에 여유 5% — 전부 같은 값이면 ±1 로 벌려 선이 상자 가장자리에 붙지 않게 한다. */
export function padRange(min: number, max: number): Range {
  if (!Number.isFinite(min) || !Number.isFinite(max)) return { min: 0, max: 1 }
  if (max - min < 1e-9) return { min: min - 1, max: max + 1 }
  const pad = (max - min) * 0.05
  return { min: min - pad, max: max + pad }
}

/** 한 축에 묶인 계열들의 범위(결측은 뺀다). 계열이 없으면 `null`. */
export function axisRange(series: readonly PlotSeries[], axis: AxisSide): Range | null {
  let min = Infinity
  let max = -Infinity
  let seen = false
  for (const s of series) {
    if (s.axis !== axis) continue
    for (const x of s.v) {
      if (x === null || !Number.isFinite(x)) continue
      seen = true
      if (x < min) min = x
      if (x > max) max = x
    }
  }
  return seen ? padRange(min, max) : null
}

/**
 * 사람이 읽는 눈금 값 — 1·2·5 × 10^n 간격.
 *
 * 범위를 n 등분하면 `3.7142` 같은 라벨이 선다. 트레이스는 값을 **눈으로 읽는** 화면이라
 * 눈금이 반올림 가능한 수여야 한다.
 */
export function niceTicks(min: number, max: number, count = 5): number[] {
  if (!Number.isFinite(min) || !Number.isFinite(max) || max <= min || count < 2) return [min]
  const raw = (max - min) / (count - 1)
  const mag = 10 ** Math.floor(Math.log10(raw))
  const norm = raw / mag
  const step = (norm <= 1 ? 1 : norm <= 2 ? 2 : norm <= 2.5 ? 2.5 : norm <= 5 ? 5 : 10) * mag
  // 간격의 자릿수 — `0.1` 을 열 번 더하면 `0.30000000000000004` 가 라벨에 선다. 누적 대신
  // 정수 k 로 자리를 만들고, 간격이 가진 소수 자리까지만 남긴다.
  const dec = Math.min(12, Math.max(0, -Math.floor(Math.log10(step)) + 1))
  const first = Math.ceil(min / step - 1e-9) * step
  const n = Math.floor((max - first) / step + 1e-6)
  const out: number[] = []
  for (let k = 0; k <= n; k++) out.push(Number((first + k * step).toFixed(dec)))
  return out.length ? out : [min]
}

/** 눈금 라벨 자릿수 — 간격이 작을수록 자리를 늘린다. */
export function tickDigits(ticks: readonly number[]): number {
  if (ticks.length < 2) return Math.abs(ticks[0] ?? 0) < 10 ? 2 : 0
  const step = Math.abs(ticks[1] - ticks[0])
  if (step >= 10) return 0
  if (step >= 1) return 1
  if (step >= 0.1) return 2
  return 3
}

export function pixelAtTime(t: number, box: PlotBox, t0: number, t1: number): number {
  if (t1 <= t0) return box.L
  return box.L + ((t - t0) / (t1 - t0)) * (box.R - box.L)
}

export function timeAtPixel(px: number, box: PlotBox, t0: number, t1: number): number {
  if (box.R <= box.L) return t0
  const r = (px - box.L) / (box.R - box.L)
  return t0 + Math.min(1, Math.max(0, r)) * (t1 - t0)
}

function pixelAtValue(v: number, box: PlotBox, r: Range): number {
  if (r.max <= r.min) return box.B
  return box.B - ((v - r.min) / (r.max - r.min)) * (box.B - box.T)
}

export interface SeriesSpec {
  path: string
  name: string
  /** 색·파선을 정하는 순번 — 세션의 채널 순서다(보이기를 껐다 켜도 색이 안 바뀐다). */
  index: number
  axis: AxisSide
}

/**
 * 행 출처에서 그릴 계열을 만든다 — 채널마다 (t, v) 를 뽑아 예산까지 엔벨로프로 줄인다.
 *
 * 예산은 **계열마다** 준다: 한 계열이 픽셀당 2점이면 눈으로는 원본과 같고, 그 위에 계열이 몇 개
 * 얹히든 한 계열의 파형 충실도는 변하지 않는다.
 */
export function buildSeries(src: RowSource, specs: readonly SeriesSpec[], widthPx: number): PlotSeries[] {
  const budget = budgetFor(widthPx)
  const out: PlotSeries[] = []
  for (const s of specs) {
    const ch = s.index
    if (ch < 0 || ch >= src.chan) continue
    const raw = src.series(ch)
    const e = envelope(raw.t, raw.v, budget)
    out.push({
      path: s.path,
      name: s.name,
      color: traceColor(ch),
      dash: traceDash(ch),
      axis: s.axis,
      t: e.t,
      v: e.v,
    })
  }
  return out
}

/** 시간축 라벨 — 초. 창이 짧으면 자리를 늘린다. */
export function fmtTick(ms: number, span: number): string {
  const d = span <= 2000 ? 3 : span <= 20000 ? 2 : 1
  return `${(ms / 1000).toFixed(d)}`
}

/**
 * 파형을 그린다. 반환값은 마우스 좌표를 시각으로 되돌릴 때 쓰는 상자다.
 *
 * 선은 `null` 에서 끊는다 — 엔벨로프가 버킷 전체 결측을 `null` 한 점으로 내는 것과 짝이다.
 * 없는 데이터를 이어 그리면 링크가 끊긴 구간이 멀쩡한 직선으로 보인다.
 */
export function drawTrace(ctx: CanvasRenderingContext2D, w: number, h: number, p: TracePlot): PlotBox {
  const c = p.colors
  const box = plotBox(w, h, p.right !== null)
  ctx.clearRect(0, 0, w, h)
  ctx.font = '11px sans-serif'
  ctx.lineWidth = 1
  ctx.setLineDash([])

  if (p.series.length === 0 || p.t1 <= p.t0) {
    ctx.fillStyle = c.text
    ctx.textAlign = 'center'
    ctx.fillText(p.empty, w / 2, h / 2)
    ctx.textAlign = 'left'
    return box
  }

  // 왼쪽 축 격자 + 라벨.
  const lt = niceTicks(p.left.min, p.left.max)
  const ld = tickDigits(lt)
  ctx.strokeStyle = c.grid
  ctx.fillStyle = c.text
  for (const v of lt) {
    const y = pixelAtValue(v, box, p.left)
    if (y < box.T - 1 || y > box.B + 1) continue
    ctx.beginPath()
    ctx.moveTo(box.L, y)
    ctx.lineTo(box.R, y)
    ctx.stroke()
    ctx.textAlign = 'right'
    ctx.fillText(v.toFixed(ld), box.L - 4, y + 4)
  }

  // 오른쪽 축은 라벨만 — 격자를 두 벌 그리면 어느 선이 어느 축인지 못 읽는다.
  if (p.right) {
    const rt = niceTicks(p.right.min, p.right.max)
    const rd = tickDigits(rt)
    ctx.fillStyle = c.text
    ctx.textAlign = 'left'
    for (const v of rt) {
      const y = pixelAtValue(v, box, p.right)
      if (y < box.T - 1 || y > box.B + 1) continue
      ctx.fillText(v.toFixed(rd), box.R + 4, y + 4)
    }
  }

  // 시간축.
  const span = p.t1 - p.t0
  const xt = niceTicks(p.t0, p.t1, 7)
  ctx.textAlign = 'center'
  ctx.fillStyle = c.text
  ctx.strokeStyle = c.grid
  for (const v of xt) {
    const x = pixelAtTime(v, box, p.t0, p.t1)
    if (x < box.L - 1 || x > box.R + 1) continue
    ctx.beginPath()
    ctx.moveTo(x, box.T)
    ctx.lineTo(x, box.B)
    ctx.stroke()
    ctx.fillText(fmtTick(v, span), x, box.B + 14)
  }
  ctx.textAlign = 'left'
  ctx.fillText('s', box.R - 8, box.B + 14)

  // 마크 — 세로 실선 하나와 글. 값과 경쟁하지 않게 본문색을 흐리게 쓴다.
  ctx.strokeStyle = c.text
  ctx.fillStyle = c.text
  for (const m of p.marks) {
    if (m.t_ms < p.t0 || m.t_ms > p.t1) continue
    const x = pixelAtTime(m.t_ms, box, p.t0, p.t1)
    ctx.setLineDash([2, 2])
    ctx.beginPath()
    ctx.moveTo(x, box.T)
    ctx.lineTo(x, box.B)
    ctx.stroke()
    ctx.setLineDash([])
    ctx.fillText(m.text, x + 3, box.T + 10)
  }

  // 계열.
  ctx.lineWidth = 1.3
  for (const s of p.series) {
    const r = s.axis === 'R' ? (p.right ?? p.left) : p.left
    ctx.strokeStyle = s.color
    ctx.setLineDash([...s.dash])
    ctx.beginPath()
    let pen = false
    for (let i = 0; i < s.t.length; i++) {
      const y = s.v[i]
      if (y === null || !Number.isFinite(y)) {
        pen = false
        continue
      }
      const px = pixelAtTime(s.t[i], box, p.t0, p.t1)
      const py = pixelAtValue(y, box, r)
      if (pen) ctx.lineTo(px, py)
      else ctx.moveTo(px, py)
      pen = true
    }
    ctx.stroke()
  }
  ctx.setLineDash([])

  // 커서 — 읽는 값은 DOM 표가 내고, 여기서는 어디를 읽고 있는지만 표시한다.
  if (p.cursor !== null && p.cursor >= p.t0 && p.cursor <= p.t1) {
    const x = pixelAtTime(p.cursor, box, p.t0, p.t1)
    ctx.strokeStyle = c.bad
    ctx.beginPath()
    ctx.moveTo(x, box.T)
    ctx.lineTo(x, box.B)
    ctx.stroke()
  }
  return box
}
