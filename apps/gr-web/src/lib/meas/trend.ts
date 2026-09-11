// 추세 계산 — 순수 함수(GrWeb `renderTrend/drawChart` 의 계산부 이식).
import type { Metric } from './const'
import type { MeasRow } from './rows'

export interface Point {
  x: number
  y: number
  row: MeasRow
}

/** 행(최신순) → 오래된 순의 (Seq, 값) 점. 공통 편차 항목은 0(측정 안 함)을 뺀다. */
export function trendPoints(rows: readonly MeasRow[], m: Metric): Point[] {
  const pts: Point[] = []
  for (let i = rows.length - 1; i >= 0; i--) {
    const r = rows[i]
    const y = m.fn(r)
    if (y === null || y === undefined || Number.isNaN(y)) continue
    if (m.k === 0 && y === 0) continue
    pts.push({ x: r.seq, y, row: r })
  }
  return pts
}

/** 지수 이동 평균(alpha 기본 0.2). */
export function ema(ys: readonly number[], alpha = 0.2): number[] {
  const out: number[] = []
  let e = 0
  ys.forEach((y, i) => {
    e = i === 0 ? y : e + alpha * (y - e)
    out.push(e)
  })
  return out
}

export interface Stats {
  n: number
  avg: number
  sd: number
  min: number
  max: number
}

export function stats(ys: readonly number[]): Stats {
  if (ys.length === 0) return { n: 0, avg: 0, sd: 0, min: 0, max: 0 }
  const avg = ys.reduce((a, b) => a + b, 0) / ys.length
  const sd = Math.sqrt(ys.reduce((a, b) => a + (b - avg) ** 2, 0) / ys.length)
  return { n: ys.length, avg, sd, min: Math.min(...ys), max: Math.max(...ys) }
}

/** 화면 x(px)에 가장 가까운 점(거리 `tol` 안). */
export function nearest<T extends { x: number }>(pts: readonly T[], xPx: number, X: (x: number) => number, tol = 12): T | null {
  let best: T | null = null
  let bd = Infinity
  for (const p of pts) {
    const d = Math.abs(X(p.x) - xPx)
    if (d < bd) {
      bd = d
      best = p
    }
  }
  return best && bd <= tol ? best : null
}
