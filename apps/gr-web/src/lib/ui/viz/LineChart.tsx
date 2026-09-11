// 캔버스 선 그래프 — dpr·리사이즈 대응, 호버/클릭 콜백. 그리기는 `lineChart.ts`(순수)가 맡는다.
import { useEffect, useRef } from 'react'
import { nearest } from '../../meas/trend'
import { chartColors, drawChart, drawSeries, type ChartPoint, type Scale } from './lineChartModel'

export interface LineChartProps<P extends ChartPoint> {
  points: readonly P[]
  label: string
  showAvg?: boolean
  showEma?: boolean
  height?: number
  yDigits?: number
  onHover?: (p: P | null) => void
  onPick?: (p: P) => void
  className?: string
}

export function LineChart<P extends ChartPoint>({ points, label, showAvg = true, showEma = true, height = 360, yDigits, onHover, onPick, className = '' }: LineChartProps<P>) {
  const ref = useRef<HTMLCanvasElement>(null)
  const scale = useRef<Scale | null>(null)
  const hover = useRef<P | null>(null)

  useEffect(() => {
    const cv = ref.current
    if (!cv) return
    const draw = () => {
      const w = cv.clientWidth || 800
      const dpr = window.devicePixelRatio || 1
      cv.width = Math.round(w * dpr)
      cv.height = Math.round(height * dpr)
      const ctx = cv.getContext('2d')
      if (!ctx) return
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
      scale.current = drawChart(ctx, w, height, points, { label, showAvg, showEma, colors: chartColors(cv), yDigits })
    }
    draw()
    const ro = new ResizeObserver(draw)
    ro.observe(cv)
    return () => ro.disconnect()
  }, [points, label, showAvg, showEma, height, yDigits])

  return (
    <canvas
      ref={ref}
      className={`w-full rounded border border-line-default bg-surface-panel ${className}`}
      style={{ height }}
      onMouseMove={(ev) => {
        const s = scale.current
        if (!s) return
        const rect = ev.currentTarget.getBoundingClientRect()
        const p = nearest(points, ev.clientX - rect.left, s.X)
        if (p !== hover.current) {
          hover.current = p
          onHover?.(p)
        }
      }}
      onMouseLeave={() => {
        hover.current = null
        onHover?.(null)
      }}
      onClick={() => {
        if (hover.current) onPick?.(hover.current)
      }}
    />
  )
}

export interface SeriesChartProps {
  values: readonly number[]
  capacity: number
  label: string
  height?: number
}

/** 순번 시계열(축 위치 추이). */
export function SeriesChart({ values, capacity, label, height = 220 }: SeriesChartProps) {
  const ref = useRef<HTMLCanvasElement>(null)
  useEffect(() => {
    const cv = ref.current
    if (!cv) return
    const draw = () => {
      const w = cv.clientWidth || 800
      const dpr = window.devicePixelRatio || 1
      cv.width = Math.round(w * dpr)
      cv.height = Math.round(height * dpr)
      const ctx = cv.getContext('2d')
      if (!ctx) return
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
      drawSeries(ctx, w, height, values, capacity, label, chartColors(cv))
    }
    draw()
    const ro = new ResizeObserver(draw)
    ro.observe(cv)
    return () => ro.disconnect()
  }, [values, capacity, label, height])
  return <canvas ref={ref} className="w-full rounded border border-line-default bg-surface-panel" style={{ height }} />
}
