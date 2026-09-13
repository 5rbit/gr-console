// 캔버스 XY 그래프 — dpr·리사이즈 대응. 그리기는 xyChartModel(순수)이 맡는다.
import { useEffect, useRef } from 'react'
import { chartColors } from './lineChartModel'
import { drawXY, type XYRule, type XYSeries } from './xyChartModel'

export interface XYChartProps {
  series: readonly XYSeries[]
  xLabel: string
  yLabel: string
  rules?: readonly XYRule[]
  height?: number
  xDigits?: number
  yDigits?: number
}

export function XYChart({ series, xLabel, yLabel, rules, height = 280, xDigits, yDigits }: XYChartProps) {
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
      drawXY(ctx, w, height, series, { xLabel, yLabel, rules, colors: chartColors(cv), xDigits, yDigits })
    }
    draw()
    const ro = new ResizeObserver(draw)
    ro.observe(cv)
    return () => ro.disconnect()
  }, [series, xLabel, yLabel, rules, height, xDigits, yDigits])
  return <canvas ref={ref} className="w-full rounded border border-line-default bg-surface-panel" style={{ height }} />
}
