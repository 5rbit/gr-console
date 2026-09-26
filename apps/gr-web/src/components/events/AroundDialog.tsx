// 행 전후 ±30 s — **모든 PLC** 의 이벤트를 오래된 것부터 한 줄로. 누른 행을 가운데에 세운다.
import { useEffect, useRef, useState } from 'react'
import { evtApi, type EventRow } from '../../lib/evtlog/api'
import { buildQuery, EMPTY_FILTER } from '../../lib/evtlog/evtFilterModel'
import { aroundWindow, fmtEvtTime, fmtOffset, newestFirst } from '../../lib/evtlog/evtRowsModel'
import { Dialog } from '../../lib/ui/Dialog'
import { EmptyState } from '../../lib/ui/EmptyState'
import { Skeleton } from '../../lib/ui/Skeleton'
import { cn } from '../../lib/utils'
import { EvtLevel, EvtPlcChip } from './EvtBits'

const LIMIT = 2000

export function AroundDialog({
  row,
  now,
  onClose,
  onCtx,
}: {
  row: EventRow | null
  now: number
  onClose: () => void
  onCtx: (ctx: number) => void
}) {
  const [rows, setRows] = useState<EventRow[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const pinned = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!row) return
    let alive = true
    setRows(null)
    setError(null)
    const w = aroundWindow(row)
    const q = buildQuery(
      { ...EMPTY_FILTER, range: 'custom', from: w.from, to: w.to },
      { now, limit: LIMIT },
    )
    evtApi
      .list(q)
      .then((p) => {
        if (!alive) return
        // 누른 행이 창 끝에서 잘려도 빠지지 않게 넣고, 오래된 것부터.
        const all = p.rows.some((r) => r.id === row.id) ? p.rows : [...p.rows, row]
        setRows([...all].sort(newestFirst).reverse())
      })
      .catch((e: unknown) => {
        if (alive) setError(e instanceof Error ? e.message : String(e))
      })
    return () => {
      alive = false
    }
    // now 는 표시 기준일 뿐 — 바뀔 때마다 다시 부르지 않는다.
  }, [row])

  useEffect(() => {
    pinned.current?.scrollIntoView({ block: 'center' })
  }, [rows])

  const plcs = rows ? new Set(rows.map((r) => r.plc)).size : 0

  return (
    <Dialog
      open={row !== null}
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
      title="전후 ±30 s"
      meta={
        row ? (
          <>
            <span className="font-mono">{fmtEvtTime(row.ts, now)}</span>
            <span>{row.plc}</span>
            {rows ? (
              <span>
                {rows.length} 건 · PLC {plcs}
              </span>
            ) : null}
          </>
        ) : null
      }
      size="lg"
      testid="evt-around-dialog"
    >
      {error ? (
        <EmptyState compact title="불러오지 못했습니다" hint={error} />
      ) : !rows ? (
        <div className="space-y-2">
          <Skeleton className="h-3 w-full" />
          <Skeleton className="h-3 w-4/5" />
          <Skeleton className="h-3 w-3/5" />
        </div>
      ) : (
        <div className="-mx-4 text-xs" role="list">
          {rows.map((r) => {
            const me = r.id === row?.id
            return (
              <div
                key={r.id}
                ref={me ? pinned : undefined}
                role="listitem"
                className={cn(
                  'grid items-center gap-2 border-b border-line-subtle px-4 py-0.5',
                  me && 'bg-accent-soft',
                )}
                style={{ gridTemplateColumns: '4.5rem 7.5rem 4.5rem 4.5rem minmax(0,1fr) 3.5rem' }}
                title={r.detail ? `${r.text}\n${r.detail}` : r.text}
              >
                <span className="text-right font-mono text-2xs text-content-faint tabular-nums">
                  {row ? fmtOffset(r.ts_ms - row.ts_ms) : ''}
                </span>
                <span className="truncate font-mono text-2xs text-content-muted">
                  {fmtEvtTime(r.ts, now)}
                </span>
                <EvtPlcChip plc={r.plc} />
                <span className="truncate">
                  <EvtLevel row={r} />
                </span>
                <span className="truncate">
                  {r.name ? (
                    <span className="mr-1.5 font-mono text-2xs text-content-faint">{r.name}</span>
                  ) : null}
                  {r.text}
                </span>
                {r.ctx ? (
                  <button
                    type="button"
                    className="justify-self-end rounded px-1 font-mono text-2xs text-content-secondary tabular-nums hover:bg-surface-inset"
                    title={`WorkId ${r.ctx} — 스텝 막대`}
                    onClick={() => onCtx(r.ctx)}
                  >
                    {r.ctx}
                  </button>
                ) : (
                  <span />
                )}
              </div>
            )
          })}
        </div>
      )}
    </Dialog>
  )
}
