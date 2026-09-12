// PLC 뷰 — `GET /api/tasks/plc-view`를 1초마다 받아 GR2 `STAT.Task` 배열을 **있는 그대로** 보인다.
// 원장이 아니라 PLC가 말하는 것이므로 이름도 PLC 것(WorkId/TaskId/Now/Queue/…)을 쓴다.
import { useEffect, useState } from 'react'
import { visibleInterval } from '../../lib/poll'
import { FieldList, type FieldItem } from '../../lib/ui/FieldList'
import { StatusDot } from '../../lib/ui/StatusDot'
import { cn } from '../../lib/utils'
import { taskApi, type PlcView as PlcViewData } from '../../lib/task/actions'
import { typeName } from '../../lib/task/state'
import type { PlcTask } from '../../lib/types'

export interface PlcViewProps {
  /** 행 클릭 — 그 키의 원장 Task를 여는 데 쓴다. */
  onPickKey?: (workId: number, taskId: number) => void
  /** 강조할 키(선택된 Task). */
  highlight?: { work_id: number; task_id: number } | null
}

const POLL_MS = 1000

function isZero(t: PlcTask | undefined): boolean {
  return !t || (t.WorkId === 0 && t.TaskId === 0 && t.TaskType === 0)
}

function Ring({
  title,
  rows,
  onPickKey,
  highlight,
  tone,
}: {
  title: string
  rows: PlcTask[]
  onPickKey?: PlcViewProps['onPickKey']
  highlight: PlcViewProps['highlight']
  tone: string
}) {
  const filled = rows.filter((t) => !isZero(t)).length
  return (
    <div>
      <div className="mb-1 flex items-center gap-1.5">
        <StatusDot status={tone} size="sm" />
        <span className="text-2xs font-semibold tracking-wide text-content-muted uppercase">
          {title}
        </span>
        <span className="text-2xs text-content-faint tabular-nums">
          {filled}/{rows.length}
        </span>
      </div>
      <table className="w-full text-[11px]" data-testid={`plc-ring-${title.toLowerCase()}`}>
        <thead>
          <tr className="text-left text-content-faint">
            <th className="w-6 px-1 py-0.5 font-medium">#</th>
            <th className="px-1 py-0.5 font-medium">WorkId</th>
            <th className="px-1 py-0.5 font-medium">TaskId</th>
            <th className="px-1 py-0.5 font-medium">Type</th>
            <th className="px-1 py-0.5 font-medium">Cell</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((t, i) => {
            const zero = isZero(t)
            const hit =
              !!highlight &&
              !zero &&
              t.WorkId === highlight.work_id &&
              t.TaskId === highlight.task_id
            return (
              <tr
                key={i}
                className={cn(
                  'border-t border-line-subtle font-mono tabular-nums',
                  zero ? 'text-content-faint' : 'cursor-pointer hover:bg-surface-hover',
                  hit && 'bg-accent-soft',
                )}
                onClick={() => {
                  if (!zero) onPickKey?.(t.WorkId, t.TaskId)
                }}
              >
                <td className="px-1 py-0.5">{i}</td>
                <td className="px-1 py-0.5">{zero ? '·' : t.WorkId}</td>
                <td className="px-1 py-0.5">{zero ? '·' : t.TaskId}</td>
                <td className="px-1 py-0.5 font-sans">{zero ? '' : typeName(t.TaskType)}</td>
                <td className="px-1 py-0.5">{zero ? '' : t.Cell?.Id}</td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}

export function PlcView({ onPickKey, highlight = null }: PlcViewProps) {
  const [data, setData] = useState<PlcViewData | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let alive = true
    const tick = () =>
      taskApi
        .plcView()
        .then((d) => {
          if (!alive) return
          setData(d)
          setError(null)
        })
        .catch((e: unknown) => {
          if (alive) setError(e instanceof Error ? e.message : String(e))
        })
    void tick()
    const t = visibleInterval(() => void tick(), POLL_MS)
    return () => {
      alive = false
      clearInterval(t)
    }
  }, [])

  if (!data) {
    return (
      <div className="p-3 text-xs text-slate-400" data-testid="plc-view">
        {error ?? 'PLC 뷰 읽는 중…'}
      </div>
    )
  }

  const s = data.status
  const bits: FieldItem[] = [
    { label: 'Accept', value: s.Accept ? 'TRUE' : 'FALSE', status: s.Accept ? 'ok' : 'warn' },
    { label: 'Idle', value: s.Idle ? 'TRUE' : 'FALSE' },
    { label: 'Assigned', value: s.Assigned ? 'TRUE' : 'FALSE' },
    {
      label: 'Inprogress',
      value: s.Inprogress ? 'TRUE' : 'FALSE',
      status: s.Inprogress ? 'ok' : undefined,
    },
    { label: 'HoldItem', value: s.HoldItem ? 'TRUE' : 'FALSE' },
    { label: 'Step', value: Number(s.Step ?? 0), mono: true },
  ]
  const h = data.res.Header
  const r = data.reject
  const res: FieldItem[] = [
    {
      label: 'RES Header',
      value:
        h.CMD_ID || h.SEQ
          ? `CMD_ID ${h.CMD_ID} · SEQ ${h.SEQ} · CMD 0x${h.CMD.toString(16).toUpperCase()}`
          : null,
      mono: true,
      wide: true,
      missing: '에코 없음',
    },
    {
      label: '검증',
      value: `${r.validation} ${r.validation_text}`,
      status: r.validation === 1 ? 'ok' : r.validation === 0 ? undefined : 'fault',
    },
    {
      label: '거부 비트',
      value:
        [
          r.wrong_robot && 'WRONG_ROBOT',
          r.duplicate && 'DUPLICATE',
          r.invalid_task && 'INVALID',
          r.buffer_full && 'BUFFER_FULL',
          r.offline && 'OFFLINE',
        ]
          .filter(Boolean)
          .join(', ') || null,
      status: 'fault',
      missing: '거부 비트 없음',
    },
    {
      label: 'Complete 허용',
      value: r.complete_allowed ? 'YES' : 'NO',
      status: r.complete_allowed ? 'ok' : 'warn',
    },
    {
      label: 'Delete 허용',
      value: r.delete_allowed ? 'YES' : 'NO',
      status: r.delete_allowed ? 'ok' : 'warn',
    },
  ]

  return (
    <div className="space-y-3 p-3" data-testid="plc-view">
      {error ? (
        <p className="m-0 text-[11px] text-amber-600 dark:text-amber-400">
          마지막 읽기 실패 — {error}
        </p>
      ) : null}
      <div>
        <div className="mb-1 text-2xs font-semibold tracking-wide text-content-muted uppercase">
          Task.Status
        </div>
        <FieldList items={bits} columns={2} dense labelWidth={72} />
      </div>
      <div>
        <div className="mb-1 text-2xs font-semibold tracking-wide text-content-muted uppercase">
          RES
        </div>
        <FieldList items={res} columns={1} dense labelWidth={96} />
      </div>
      <Ring
        title="Now"
        rows={[data.now]}
        onPickKey={onPickKey}
        highlight={highlight}
        tone={isZero(data.now) ? 'neutral' : 'ok'}
      />
      <Ring
        title="Queue"
        rows={data.queue}
        onPickKey={onPickKey}
        highlight={highlight}
        tone="info"
      />
      <Ring
        title="Completed"
        rows={data.completed}
        onPickKey={onPickKey}
        highlight={highlight}
        tone="ok"
      />
      <Ring
        title="Canceled"
        rows={data.canceled}
        onPickKey={onPickKey}
        highlight={highlight}
        tone="neutral"
      />
      <Ring
        title="Rejected"
        rows={data.rejected}
        onPickKey={onPickKey}
        highlight={highlight}
        tone="fault"
      />
    </div>
  )
}
