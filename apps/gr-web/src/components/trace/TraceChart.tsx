// 트레이스 파형 캔버스 — dpr·리사이즈·커서. 그리기는 `traceChartModel`(순수)이 맡는다.
//
// 링은 제자리에서 바뀌므로 React 의 참조 비교로는 갱신을 못 본다 — 스토어가 올리는 `version` 을
// 받아 다시 그린다. 그리기 자체는 `frameThrottle` 로 프레임에 묶여 있어 청크가 몰려도 프레임당
// 한 번이고, 혼잡하면 그보다 더 드물어진다.
import { useEffect, useMemo, useRef } from 'react'
import { frameThrottle } from '../../lib/sse'
import { chartColors } from '../../lib/ui/viz/lineChartModel'
import {
  axisRange,
  buildSeries,
  drawTrace,
  plotWidth,
  timeAtPixel,
  type PlotBox,
  type PlotMark,
  type SeriesSpec,
} from './traceChartModel'
import type { RowSource } from './traceModel'

export interface TraceChartProps {
  source: RowSource
  specs: readonly SeriesSpec[]
  marks: readonly PlotMark[]
  /** 데이터가 바뀌었다는 신호(스토어 버전). */
  version: number
  height?: number
  empty?: string
  /** 마우스가 가리키는 시각(ms). 벗어나면 `null`. */
  onCursor?: (t: number | null) => void
  /** 그리기 상자의 가로 픽셀 — 재생 행 수(`budgetFor`)를 여기에 맞춘다. */
  onWidth?: (px: number) => void
}

export function TraceChart({
  source,
  specs,
  marks,
  version,
  height = 320,
  empty = '데이터 없음',
  onCursor,
  onWidth,
}: TraceChartProps) {
  const ref = useRef<HTMLCanvasElement>(null)
  const box = useRef<PlotBox | null>(null)
  const span = useRef<[number, number]>([0, 0])
  const cursor = useRef<number | null>(null)
  const lastWidth = useRef(0)

  // 그리기가 읽는 값 — 콜백을 프레임에 묶으면 클로저가 낡으므로 ref 로 넘긴다.
  const live = useRef({ source, specs, marks, height, empty, onWidth })
  live.current = { source, specs, marks, height, empty, onWidth }

  const draw = useMemo(
    () =>
      frameThrottle(() => {
        const cv = ref.current
        if (!cv) return
        const { source: src, specs: sp, marks: mk, height: h, empty: em } = live.current
        const w = cv.clientWidth || 800
        const dpr = window.devicePixelRatio || 1
        cv.width = Math.round(w * dpr)
        cv.height = Math.round(h * dpr)
        const ctx = cv.getContext('2d')
        if (!ctx) return
        ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
        const hasRight = sp.some((s) => s.axis === 'R')
        const inner = plotWidth(w, hasRight)
        if (inner !== lastWidth.current) {
          lastWidth.current = inner
          live.current.onWidth?.(inner)
        }
        const series = buildSeries(src, sp, inner)
        const [t0, t1] = src.bounds()
        span.current = [t0, t1]
        box.current = drawTrace(ctx, w, h, {
          series,
          t0,
          t1: t1 > t0 ? t1 : t0 + 1,
          left: axisRange(series, 'L') ?? { min: 0, max: 1 },
          right: axisRange(series, 'R'),
          marks: mk,
          cursor: cursor.current,
          colors: chartColors(cv),
          empty: em,
        })
      }),
    [],
  )

  // `version`(링이 제자리에서 자란다)·계열 토글·크기 변화가 모두 다시 그리기를 부른다.
  useEffect(() => {
    draw()
  }, [draw, version, source, specs, marks, height])

  useEffect(() => {
    const cv = ref.current
    if (!cv) return
    const ro = new ResizeObserver(() => draw())
    ro.observe(cv)
    return () => {
      ro.disconnect()
      draw.cancel()
    }
  }, [draw])

  return (
    <canvas
      ref={ref}
      className="w-full rounded border border-line-default bg-surface-panel"
      style={{ height }}
      data-testid="trace-chart"
      onMouseMove={(ev) => {
        const b = box.current
        if (!b) return
        const rect = ev.currentTarget.getBoundingClientRect()
        const x = ev.clientX - rect.left
        if (x < b.L || x > b.R) {
          if (cursor.current !== null) {
            cursor.current = null
            onCursor?.(null)
            draw()
          }
          return
        }
        const [t0, t1] = span.current
        const t = timeAtPixel(x, b, t0, t1)
        cursor.current = t
        onCursor?.(t)
        draw()
      }}
      onMouseLeave={() => {
        cursor.current = null
        onCursor?.(null)
        draw()
      }}
    />
  )
}
