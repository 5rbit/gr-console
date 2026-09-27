// 스텝 통계 — (PLC, proc, step[, TaskType]) 마다 체류 시간 분포, 그 아래 이상치(WorkId 를 누르면 스텝 막대).
import type { StepOutlier, StepStatRow, StepStats } from '../../lib/evtlog/api'
import { stepRowKey } from '../../lib/evtlog/evtStatsModel'
import { fmtEvtTime } from '../../lib/evtlog/evtRowsModel'
import type { Loaded } from '../../lib/evtlog/store'
import { Button } from '../../lib/ui/Button'
import { DataTable } from '../../lib/ui/DataTable'
import { EmptyState } from '../../lib/ui/EmptyState'
import { Section } from '../../lib/ui/Section'
import type { Column } from '../../lib/ui/table'
import { EvtPlcChip } from './EvtBits'

export function StepStatsView({
  data,
  split,
  now,
  onCtx,
  onReload,
}: {
  data: Loaded<StepStats>
  split: boolean
  now: number
  onCtx: (ctx: number) => void
  onReload: () => void
}) {
  const s = data.data
  if (!s) {
    if (data.error)
      return (
        <EmptyState
          title="스텝 통계를 불러오지 못했습니다"
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
  if (s.rows.length === 0)
    return (
      <EmptyState
        title="이 기간에 STEP 이벤트 없음"
        hint="기간을 넓히거나 PLC 조건을 풀어 보세요."
        testid="evt-steps-empty"
      />
    )

  const cols: Column<StepStatRow>[] = [
    {
      key: 'plc',
      label: 'PLC',
      get: (r) => r.plc,
      cell: (r) => <EvtPlcChip plc={r.plc} />,
      priority: 2,
    },
    { key: 'proc', label: 'Proc', get: (r) => r.proc_name, priority: 2 },
    {
      key: 'step',
      label: 'Step',
      get: (r) => r.step,
      cell: (r) => (
        <span className="font-mono">
          {r.step}
          {r.step_name !== String(r.step) ? (
            <span className="ml-1.5 text-content-muted">{r.step_name}</span>
          ) : null}
        </span>
      ),
    },
    ...(split
      ? [{ key: 'type', label: 'TaskType', get: (r: StepStatRow) => r.task_type_name ?? '-' }]
      : []),
    { key: 'count', label: 'Count', get: (r) => r.count, numeric: true },
    { key: 'mean', label: 'Mean (ms)', get: (r) => r.mean_ms, numeric: true, priority: 3 },
    { key: 'p50', label: 'p50 (ms)', get: (r) => r.p50_ms, numeric: true },
    { key: 'p95', label: 'p95 (ms)', get: (r) => r.p95_ms, numeric: true },
    { key: 'max', label: 'Max (ms)', get: (r) => r.max_ms, numeric: true },
    {
      key: 'out',
      label: 'Outliers',
      get: (r) => r.outliers,
      cell: (r) => (r.outliers ? <span className="text-warn-fg">{r.outliers}</span> : ''),
      numeric: true,
      help: '체류가 max(p95 × 1.5, p95 + 2 s) 를 넘은 건수.',
    },
  ]

  const ocols: Column<StepOutlier>[] = [
    {
      key: 'ts',
      label: 'Time',
      get: (r) => r.ts_ms,
      cell: (r) => <span className="font-mono text-2xs">{fmtEvtTime(r.ts, now)}</span>,
    },
    {
      key: 'plc',
      label: 'PLC',
      get: (r) => r.plc,
      cell: (r) => <EvtPlcChip plc={r.plc} />,
      priority: 2,
    },
    {
      key: 'step',
      label: 'Step',
      get: (r) => r.step,
      cell: (r) => (
        <span className="font-mono">
          {r.step}
          <span className="ml-1.5 text-content-muted">{r.step_name}</span>
        </span>
      ),
    },
    { key: 'dwell', label: 'Dwell (ms)', get: (r) => r.dwell_ms, numeric: true },
    { key: 'p95', label: 'p95 (ms)', get: (r) => r.p95_ms, numeric: true, priority: 3 },
    {
      key: 'x',
      label: '× p95',
      get: (r) => r.dwell_ms / Math.max(1, r.p95_ms),
      cell: (r) => (r.dwell_ms / Math.max(1, r.p95_ms)).toFixed(1),
      numeric: true,
    },
    {
      key: 'ctx',
      label: 'WorkId',
      get: (r) => r.ctx,
      cell: (r) =>
        r.ctx ? (
          <button
            type="button"
            className="rounded px-1 font-mono text-content-secondary tabular-nums hover:bg-surface-inset"
            title={`WorkId ${r.ctx} — 스텝 막대`}
            onClick={(e) => {
              e.stopPropagation()
              onCtx(r.ctx)
            }}
          >
            {r.ctx}
          </button>
        ) : (
          ''
        ),
      numeric: true,
    },
  ]

  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-3 pb-3" data-testid="evt-step-stats">
      <Section title="STEP" first right={`${s.samples} 건 · ${s.rows.length} 줄`}>
        <DataTable
          rows={s.rows}
          columns={cols}
          rowKey={stepRowKey}
          density="compact"
          stickyHeader
        />
      </Section>
      <Section
        title="Outliers"
        right={
          s.outliers_total > s.outliers.length
            ? `${s.outliers.length} / ${s.outliers_total}`
            : `${s.outliers_total}`
        }
        help="체류가 그 스텝의 max(p95 × 1.5, p95 + 2 s) 를 넘은 STEP 행 — 심한 것부터. WorkId 를 누르면 그 Task 의 스텝 막대."
      >
        <DataTable
          rows={s.outliers}
          columns={ocols}
          rowKey={(r) => String(r.id)}
          density="compact"
          empty="이상치 없음"
          emptyDense
          testid="evt-step-outliers"
        />
      </Section>
    </div>
  )
}
