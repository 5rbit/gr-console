// 알람 통계 — 알람마다 발생 수 · 켜져 있던 시간, 파레토 막대. 행을 누르면 그 알람의 이벤트 목록으로.
import { useMemo } from 'react'
import type { AlarmStats } from '../../lib/evtlog/api'
import { pareto, secs, type ParetoMetric, type ParetoRow } from '../../lib/evtlog/evtStatsModel'
import { fmtEvtTime } from '../../lib/evtlog/evtRowsModel'
import type { Loaded } from '../../lib/evtlog/store'
import { Button } from '../../lib/ui/Button'
import { DataTable } from '../../lib/ui/DataTable'
import { EmptyState } from '../../lib/ui/EmptyState'
import type { Column } from '../../lib/ui/table'
import { EvtPlcChip, EvtType } from './EvtBits'

export function AlarmStatsView({
  data,
  metric,
  now,
  onPick,
  onReload,
}: {
  data: Loaded<AlarmStats>
  metric: ParetoMetric
  now: number
  onPick: (r: ParetoRow) => void
  onReload: () => void
}) {
  const rows = useMemo(() => pareto(data.data?.rows ?? [], metric), [data.data, metric])

  if (!data.data) {
    if (data.error)
      return (
        <EmptyState
          title="알람 통계를 불러오지 못했습니다"
          hint={data.error}
          action={
            <Button size="sm" onClick={onReload}>
              다시 시도
            </Button>
          }
        />
      )
    return <EmptyState title="불러오는 중" />
  }
  if (rows.length === 0)
    return (
      <EmptyState
        title="이 기간에 알람 없음"
        hint="기간을 넓히거나 PLC · Type 조건을 풀어 보세요."
        testid="evt-alarms-empty"
      />
    )

  const cols: Column<ParetoRow>[] = [
    {
      key: 'plc',
      label: 'PLC',
      get: (r) => r.plc,
      cell: (r) => <EvtPlcChip plc={r.plc} />,
      priority: 2,
    },
    {
      key: 'type',
      label: 'Type',
      get: (r) => r.type,
      cell: (r) => <EvtType row={{ etype: r.type }} />,
      priority: 2,
    },
    {
      key: 'label',
      label: 'Code',
      get: (r) => r.label,
      cell: (r) => <span className="font-mono">{r.label}</span>,
    },
    { key: 'text', label: 'Text', get: (r) => r.text },
    { key: 'count', label: 'Count', get: (r) => r.count, numeric: true },
    {
      key: 'total',
      label: 'Total (s)',
      get: (r) => r.total_ms,
      cell: (r) => secs(r.total_ms),
      numeric: true,
    },
    {
      key: 'mean',
      label: 'Mean (s)',
      get: (r) => r.mean_ms,
      cell: (r) => secs(r.mean_ms),
      numeric: true,
      priority: 3,
    },
    {
      key: 'max',
      label: 'Max (s)',
      get: (r) => r.max_ms,
      cell: (r) => secs(r.max_ms),
      numeric: true,
      priority: 3,
    },
    {
      key: 'open',
      label: 'Open',
      get: (r) => r.open,
      cell: (r) => (r.open ? r.open : ''),
      numeric: true,
      priority: 3,
      help: '기간 끝에 아직 켜져 있던 발생 — 길이는 기간 끝(또는 지금)까지로 셉니다.',
    },
    {
      key: 'flicker',
      label: 'Flicker',
      get: (r) => r.flicker,
      cell: (r) => (r.flicker ? r.flicker : ''),
      numeric: true,
      priority: 3,
      help: 'PLC 해제 지연(2 s)이 삼킨 깜빡임 수 — 해제 행의 A 를 더한 값(v2 행만).',
    },
    {
      key: 'last',
      label: 'Last',
      get: (r) => r.last_ts,
      cell: (r) => <span className="font-mono text-2xs">{fmtEvtTime(r.last, now)}</span>,
      priority: 2,
    },
    {
      key: 'pareto',
      label: metric === 'count' ? 'Count Σ%' : 'Duration Σ%',
      name: 'Pareto',
      get: (r) => r.value,
      help: '막대 = 가장 큰 값 대비, 숫자 = 위에서부터 누적 몫(%). 기준은 도구 띠의 Count / Duration.',
      cell: (r) => (
        <span className="flex items-center gap-2">
          <span className="relative h-2 w-24 shrink-0 rounded-sm bg-surface-inset">
            <span
              className="absolute inset-y-0 left-0 rounded-sm bg-info"
              style={{ width: `${Math.max(1, r.bar)}%` }}
            />
          </span>
          <span className="w-8 text-right text-2xs text-content-muted tabular-nums">
            {Math.round(r.cum)}
          </span>
        </span>
      ),
    },
  ]

  return (
    <DataTable
      rows={rows}
      columns={cols}
      rowKey={(r) => `${r.plc}/${r.area}/${r.bit ?? `c${r.code}`}`}
      onPick={onPick}
      density="compact"
      stickyHeader
      className="min-h-0 flex-1 overflow-y-auto"
      testid="evt-alarm-stats"
    />
  )
}
