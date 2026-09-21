// 추세 — 고른 항목의 값이 기록 순번을 따라 어떻게 움직였나. 점을 누르면 그 기록의 이력 행으로 간다.
//
// 조작 줄은 한 줄이다: 항목 · EMA · Avg · 값. 설명문 두 줄은 `?` 도움말로 접었다 — 처음 한 번 읽고
// 다시는 안 읽는 글이 값과 같은 자리를 상시로 먹고 있었다.
import { useMemo, useState } from 'react'
import { METRICS, metricsFor } from '../../lib/meas/const'
import { delta, fine } from '../../lib/meas/format'
import type { MeasRow } from '../../lib/meas/rows'
import { stats, trendPoints, type Point } from '../../lib/meas/trend'
import { Card } from '../../lib/ui/Card'
import { HelpTip } from '../../lib/ui/HelpTip'
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
    <Card>
      <div className="mb-2 flex flex-wrap items-center gap-3">
        <Select dense value={metric.id} onValueChange={setMetricId} aria-label="추세 항목">
          {list.map((m) => (
            <option key={m.id} value={m.id}>
              {m.label}
            </option>
          ))}
        </Select>
        <Switch inline label="EMA(0.2)" checked={showEma} onCheckedChange={setShowEma} />
        <Switch inline label="Avg" checked={showAvg} onCheckedChange={setShowAvg} />
        <HelpTip
          title="추세"
          text="X축은 기록 순번(Seq)이고 위의 Kind·Code 필터가 그대로 걸립니다. 점 위에 마우스를 올리면 값, 클릭하면 그 기록의 이력 행으로 갑니다."
        />
        {/* 값은 **편차 규칙(2자리)**, 산포(σ)만 3자리다 — σ 는 0.01 아래가 실제 값이다. */}
        <span className="ml-auto font-mono text-2xs tabular-nums text-content-muted">
          {hover
            ? `Seq ${hover.x}: ${delta(hover.y)} mm  (${hover.row.kindName} ${hover.row.statusName}, Code ${hover.row.code}, ${hover.row.time})`
            : pts.length
              ? `Avg ${delta(s.avg)}  σ ${fine(s.sd)}  Min ${delta(s.min)}  Max ${delta(s.max)} mm`
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
    </Card>
  )
}
