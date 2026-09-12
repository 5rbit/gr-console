// 수평 바 게이지 — 0..max 비율을 상태색 바로. 토크%·부하·진행 등에 공용(hand-rolled). 선택 라벨.
import { statusTone } from '../status'
import type { Status } from '../status'

export interface GaugeProps {
  value?: number
  max?: number
  label?: string
  status?: Status | string
  className?: string
}

export function Gauge({
  value = 0,
  max = 1,
  label = '',
  status = 'info',
  className: cls = '',
}: GaugeProps) {
  const pct = Math.max(0, Math.min(1, max ? value / max : 0)) * 100
  const t = statusTone(status)

  return (
    <div className={`flex items-center gap-2 ${cls}`}>
      <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-slate-200 dark:bg-slate-700">
        <div
          className={`h-full rounded-full transition-all ${t.dot}`}
          style={{ width: `${pct}%` }}
        />
      </div>
      {label ? (
        <span className="w-12 text-right text-3xs text-slate-500 tabular-nums dark:text-slate-400">
          {label}
        </span>
      ) : null}
    </div>
  )
}
