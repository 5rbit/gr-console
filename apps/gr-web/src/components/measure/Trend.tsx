// 추세 탭 — 항목 선택, 평균/EMA, 호버 값, 점 클릭 → 이력.
import { useMemo, useState } from 'react'
import { METRICS, metricsFor } from '../../lib/meas/const'
import type { MeasRow } from '../../lib/meas/rows'
import { stats, trendPoints, type Point } from '../../lib/meas/trend'
import { Select } from '../../lib/ui/Select'
import { Switch } from '../../lib/ui/Switch'
import { LineChart } from '../../lib/ui/viz/LineChart'

type P = Point & { bad?: boolean }

export function Trend({
  rows,
  kind,
  onPick,
}: {
  rows: MeasRow[]
  kind: string
  onPick: (seq: number) => void
}) {
  const list = metricsFor(Number(kind || 0))
  const [metricId, setMetricId] = useState(list[0]?.id ?? 'dInnerDia')
  const [showAvg, setShowAvg] = useState(true)
  const [showEma, setShowEma] = useState(true)
  const [hover, setHover] = useState<P | null>(null)
  const metric = list.find((m) => m.id === metricId) ?? list[0] ?? METRICS[0]
  const pts = useMemo<P[]>(
    () => trendPoints(rows, metric).map((p) => ({ ...p, bad: p.row.status >= 3 })),
    [rows, metric],
  )
  const s = useMemo(() => stats(pts.map((p) => p.y)), [pts])
  return (
    <div>
      <div className="mb-2 flex flex-wrap items-center gap-3">
        <Select dense label="항목" value={metric.id} onValueChange={setMetricId}>
          {list.map((m) => (
            <option key={m.id} value={m.id}>
              {m.label}
            </option>
          ))}
        </Select>
        <Switch inline label="EMA(0.2)" checked={showEma} onCheckedChange={setShowEma} />
        <Switch inline label="평균" checked={showAvg} onCheckedChange={setShowAvg} />
        <span className="font-mono text-2xs tabular-nums text-content-muted">
          {hover
            ? `Seq ${hover.x}: ${hover.y.toFixed(3)}  (${hover.row.kindName} ${hover.row.statusName}, Code ${hover.row.code}, ${hover.row.time})`
            : pts.length
              ? `평균 ${s.avg.toFixed(3)}  σ ${s.sd.toFixed(3)}  최소 ${s.min.toFixed(3)}  최대 ${s.max.toFixed(3)}`
              : ''}
        </span>
      </div>
      <LineChart
        points={pts}
        label={metric.label}
        showAvg={showAvg}
        showEma={showEma}
        onHover={setHover}
        onPick={(p: P) => onPick(p.x)}
      />
      <div className="mt-1 text-2xs text-content-muted">
        X축 = 기록 순번(Seq). 종류/Code 필터가 적용됩니다. 점 위에 마우스를 올리면 값, 클릭하면
        이력으로 이동합니다.
      </div>
    </div>
  )
}
