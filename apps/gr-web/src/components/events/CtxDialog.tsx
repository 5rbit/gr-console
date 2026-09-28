// Task ctx(= 로봇의 현재 Task WorkId) — STEP 이벤트로 그린 스텝 머문 시간 막대 + 그 WorkId 의 다른 이벤트.
import { useEffect, useMemo, useState } from 'react'
import { evtApi, type EventRow, type EvtCatalog } from '../../lib/evtlog/api'
import { EMPTY_FILTER, buildQuery } from '../../lib/evtlog/evtFilterModel'
import { fmtEvtTime, fmtSpan, msToStamp, newestFirst } from '../../lib/evtlog/evtRowsModel'
import { barGeometry, stepBars } from '../../lib/evtlog/evtStepModel'
import { Dialog } from '../../lib/ui/Dialog'
import { DataTable } from '../../lib/ui/DataTable'
import { EmptyState } from '../../lib/ui/EmptyState'
import { Section } from '../../lib/ui/Section'
import { Skeleton } from '../../lib/ui/Skeleton'
import type { Column } from '../../lib/ui/table'
import { nav } from '../../lib/nav'
import { tasks } from '../../lib/tasks'
import { Button } from '../../lib/ui/Button'
import { EvtLevel, EvtPlcChip, EvtType } from './EvtBits'
import { rowText } from '../../lib/evtlog/evtTypeModel'
import { evtView } from '../../lib/evtlog/store'

const LIMIT = 2000

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

export function CtxDialog({
  ctx,
  catalog,
  now,
  onClose,
}: {
  ctx: number | null
  catalog: EvtCatalog | null
  now: number
  onClose: () => void
}) {
  const [steps, setSteps] = useState<EventRow[] | null>(null)
  const [others, setOthers] = useState<EventRow[] | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (ctx === null) return
    let alive = true
    setSteps(null)
    setOthers(null)
    setError(null)
    const base = { ...EMPTY_FILTER, ctx: String(ctx) }
    Promise.all([
      evtApi.list(buildQuery({ ...base, cats: ['STEP'] }, { now: 0, limit: LIMIT })),
      evtApi.list(buildQuery(base, { now: 0, limit: LIMIT })),
    ])
      .then(([s, all]) => {
        if (!alive) return
        setSteps(s.rows)
        setOthers(
          all.rows
            .filter((r) => r.cat_name !== 'STEP')
            .sort(newestFirst)
            .reverse(),
        )
      })
      .catch((e: unknown) => {
        if (alive) setError(errText(e))
      })
    return () => {
      alive = false
    }
  }, [ctx])

  const chart = useMemo(() => stepBars(steps ?? [], catalog?.enums), [steps, catalog])
  const bars = chart.lanes.flatMap((l) => l.bars).sort((a, b) => a.start - b.start || a.id - b.id)
  const plc = steps?.[0]?.plc ?? others?.[0]?.plc ?? null
  // 원장은 WorkId 로 잇는다 — 같은 WorkId 가 두 로봇에 있으면 이 이벤트의 PLC 쪽.
  const ledgerId =
    ctx === null
      ? null
      : (tasks.list.find((t) => t.work_id === ctx && (!plc || t.plc_name === plc))?.id ?? null)

  const cols: Column<EventRow>[] = [
    {
      key: 'ts',
      label: 'Time',
      cell: (r) => <span className="font-mono text-2xs">{fmtEvtTime(r.ts, now)}</span>,
    },
    { key: 'plc', label: 'PLC', cell: (r) => <EvtPlcChip plc={r.plc} />, priority: 2 },
    { key: 'lvl', label: 'Level', cell: (r) => <EvtLevel row={r} /> },
    { key: 'cat', label: 'Cat', get: (r) => r.cat_name, priority: 3 },
    { key: 'type', label: 'Type', cell: (r) => <EvtType row={r} />, priority: 2 },
    { key: 'text', label: 'Text', get: (r) => rowText(r, evtView.lang) },
  ]

  return (
    <Dialog
      open={ctx !== null}
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
      title={`WorkId ${ctx ?? ''}`}
      footer={
        ledgerId ? (
          <Button
            size="sm"
            onClick={() => {
              onClose()
              nav.goTask(ledgerId)
            }}
            data-testid="evt-ctx-task"
          >
            Task 관리에서 보기
          </Button>
        ) : undefined
      }
      meta={
        steps ? (
          <>
            {plc ? <span>{plc}</span> : null}
            <span>STEP {bars.length}</span>
            <span>{`Σ ${fmtSpan(chart.total)}`}</span>
          </>
        ) : null
      }
      size="xl"
      testid="evt-ctx-dialog"
    >
      {error ? (
        <EmptyState compact title="불러오지 못했습니다" hint={error} />
      ) : !steps || !others ? (
        <div className="space-y-2">
          <Skeleton className="h-3 w-full" />
          <Skeleton className="h-3 w-4/5" />
        </div>
      ) : (
        <>
          <Section
            title="스텝 (STEP)"
            first
            help="STEP 이벤트 하나 = 떠난 스텝 b 에 a ms 머묾. 막대 끝이 그 이벤트 시각이다. proc 은 src."
            right={
              chart.t1 > chart.t0
                ? `${fmtEvtTime(msToStamp(chart.t0), now)} → ${fmtSpan(chart.t1 - chart.t0)}`
                : undefined
            }
          >
            {bars.length === 0 ? (
              <EmptyState
                compact
                title="STEP 이벤트 없음"
                hint="이 WorkId 로 기록된 스텝 전이가 없습니다."
              />
            ) : (
              <div className="space-y-px" data-testid="evt-steps">
                {bars.map((b) => {
                  const g = barGeometry(b, chart.t0, chart.t1)
                  return (
                    <div
                      key={b.id}
                      className="grid items-center gap-2 text-2xs"
                      style={{ gridTemplateColumns: '5rem 7rem minmax(0,1fr) 4.5rem' }}
                      title={`${b.procName} · ${b.stepName} → ${b.next} · ${b.dur} ms`}
                    >
                      <span className="truncate text-content-muted">{b.procName}</span>
                      <span className="truncate font-mono text-content-primary">{b.stepName}</span>
                      <span className="relative h-3.5 rounded-sm bg-surface-inset">
                        <span
                          className="absolute inset-y-0 rounded-sm border border-info bg-info-soft"
                          style={{ left: `${g.left}%`, width: `${g.width}%` }}
                        />
                      </span>
                      <span className="text-right font-mono text-content-secondary tabular-nums">
                        {b.dur}
                      </span>
                    </div>
                  )
                })}
                <div
                  className="grid gap-2 pt-0.5 text-3xs text-content-faint"
                  style={{ gridTemplateColumns: '5rem 7rem minmax(0,1fr) 4.5rem' }}
                >
                  <span>proc</span>
                  <span>step</span>
                  <span />
                  <span className="text-right">a (ms)</span>
                </div>
              </div>
            )}
          </Section>
          <Section title="다른 이벤트" right={`${others.length} 건`}>
            <DataTable
              rows={others}
              columns={cols}
              rowKey={(r) => String(r.id)}
              density="compact"
              stickyHeader
              className="max-h-72 overflow-y-auto"
              empty="STEP 밖의 이벤트 없음"
              emptyDense
            />
          </Section>
        </>
      )}
    </Dialog>
  )
}
