// 인라인 SVG 스파크라인 — 값 배열의 추세를 작은 라인으로 그린다(무거운 차트 dep 회피, hand-rolled).
//
// 어느 것을 쓰나: 시계열을 읽어야 하면(값·시각·구간) `TracePlots`를 쓴다. 스파크라인은 표 한 칸
// (16~36px)의 추세 기호 전용이다.
//
// stroke=currentColor라 부모 텍스트 색(상태 톤)이 라인 색을 구동한다. `baseline`을 주면 두 배열을
// 합쳐 한 번에 정규화해 같은 y 스케일로 점선을 겹쳐 그린다(before/after 비교).
import { useMemo } from 'react'

export interface SparklineProps {
  values?: number[]
  baseline?: number[]
  width?: number
  height?: number
  area?: boolean
  className?: string
}

export function Sparkline({
  values = [],
  baseline = [],
  width = 72,
  height = 20,
  area = false,
  className: cls = '',
}: SparklineProps) {
  const geom = useMemo(() => {
    const n = values.length
    if (n < 2) return null
    const hasBase = baseline.length >= 2
    const all = hasBase ? [...values, ...baseline] : values
    let min = Math.min(...all)
    let max = Math.max(...all)
    if (max === min) {
      max += 1
      min -= 1
    }
    const sy = (v: number) => height - ((v - min) / (max - min)) * height
    const path = (arr: number[]) => {
      const dx = width / (arr.length - 1)
      return 'M' + arr.map((v, i) => `${(i * dx).toFixed(1)},${sy(v).toFixed(1)}`).join(' L')
    }
    const line = path(values)
    const fill = area ? `${line} L${width},${height} L0,${height} Z` : ''
    return { line, fill, base: hasBase ? path(baseline) : '' }
  }, [values, baseline, width, height, area])

  if (!geom) return <span className="text-[10px] text-slate-400">—</span>

  return (
    <svg
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      className={cls}
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      {geom.fill ? <path d={geom.fill} fill="currentColor" opacity="0.12" /> : null}
      {geom.base ? (
        // 기준선(직전 저장분) — 점선·저채도라 현재 파형이 항상 앞선다.
        <path
          d={geom.base}
          fill="none"
          stroke="currentColor"
          strokeWidth="1"
          strokeDasharray="2 2"
          opacity="0.4"
        />
      ) : null}
      <path
        d={geom.line}
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinejoin="round"
        strokeLinecap="round"
      />
    </svg>
  )
}
