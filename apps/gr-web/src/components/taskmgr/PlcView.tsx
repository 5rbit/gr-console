// PLC 뷰 — `GET /api/tasks/plc-view?robot=`를 1초마다 받아 그 로봇 PLC 의 `STAT.Task` 배열을 **있는 그대로** 보인다.
// 원장이 아니라 PLC가 말하는 것이므로 이름도 PLC 것(WorkId/TaskId/Now/Queue/…)을 쓴다.
import { useEffect, useState, type ReactNode } from 'react'
import { visibleInterval } from '../../lib/poll'
import { DataTable } from '../../lib/ui/DataTable'
import { FieldList, type FieldItem } from '../../lib/ui/FieldList'
import { StatusDot } from '../../lib/ui/StatusDot'
import type { Column } from '../../lib/ui/table'
import { taskApi, type PlcView as PlcViewData } from '../../lib/task/actions'
import { typeName } from '../../lib/task/state'
import type { PlcTask } from '../../lib/types'

export interface PlcViewProps {
  /** 행 클릭 — 그 키의 원장 Task를 여는 데 쓴다. */
  onPickKey?: (workId: number, taskId: number) => void
  /** 강조할 키(선택된 Task). */
  highlight?: { work_id: number; task_id: number } | null
  /** 볼 로봇 id(없으면 기본 로봇). */
  robot?: number | null
  /** 머리줄에 쓸 로봇 이름. */
  robotName?: string
  /** 바깥 여백 없이 — 이미 여백이 있는 시트 안에 들어갈 때(여백이 두 겹이면 값이 밀린다). */
  bare?: boolean
}

const POLL_MS = 1000

function isZero(t: PlcTask | undefined): boolean {
  return !t || (t.WorkId === 0 && t.TaskId === 0 && t.TaskType === 0)
}

/** 링의 한 칸 — 배열 자리(`slot`)는 값이 없어도 그대로 선다(자리가 곧 PLC 의 인덱스다). */
interface Slot {
  slot: number
  task: PlcTask
}

// 빈 칸은 `·` 로 남긴다 — 자리를 접으면 "몇 번 칸이 비었나"를 셀 수 없다. 빈 줄은 흐리게 — 링에서
// 눈이 찾는 것은 **찬 칸**이다.
//
// 열 다섯은 전부 우선순위 1이다(기본값): 짧은 열뿐이라 좁은 시트에서도 다 서고, 접으면 줄마다
// 펼치기 손잡이가 생겨 정작 값보다 손잡이가 많아진다.
const dim = (r: Slot, v: unknown) => (
  <span className={isZero(r.task) ? 'text-content-faint' : ''}>{v as ReactNode}</span>
)
const SLOT_COLS: Column<Slot>[] = [
  { key: 'i', label: '#', get: (r) => r.slot, cell: (r) => dim(r, r.slot), numeric: true },
  {
    key: 'w',
    label: 'WorkId',
    get: (r) => (isZero(r.task) ? '·' : r.task.WorkId),
    cell: (r) => dim(r, isZero(r.task) ? '·' : r.task.WorkId),
    numeric: true,
  },
  {
    key: 't',
    label: 'TaskId',
    get: (r) => (isZero(r.task) ? '·' : r.task.TaskId),
    cell: (r) => dim(r, isZero(r.task) ? '·' : r.task.TaskId),
    numeric: true,
  },
  { key: 'ty', label: 'Type', get: (r) => (isZero(r.task) ? '' : typeName(r.task.TaskType)) },
  { key: 'c', label: 'Cell', get: (r) => (isZero(r.task) ? '' : r.task.Cell?.Id), numeric: true },
]

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
  const slots: Slot[] = rows.map((task, slot) => ({ slot, task }))
  const hit = slots.find(
    (r) =>
      !!highlight &&
      !isZero(r.task) &&
      r.task.WorkId === highlight.work_id &&
      r.task.TaskId === highlight.task_id,
  )
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
      <DataTable
        rows={slots}
        columns={SLOT_COLS}
        rowKey={(r) => String(r.slot)}
        selected={hit ? String(hit.slot) : null}
        onPick={(r) => {
          if (!isZero(r.task)) onPickKey?.(r.task.WorkId, r.task.TaskId)
        }}
        density="compact"
        testid={`plc-ring-${title.toLowerCase()}`}
        empty="자리 없음"
      />
    </div>
  )
}

export function PlcView({ onPickKey, highlight = null, robot = null, robotName, bare = false }: PlcViewProps) {
  const [data, setData] = useState<PlcViewData | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let alive = true
    // 로봇이 바뀌면 옛 로봇 배열을 들고 있지 않는다.
    setData(null)
    setError(null)
    const tick = () =>
      taskApi
        .plcView(robot)
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
  }, [robot])

  if (!data) {
    return (
      <div className={bare ? 'text-xs text-content-faint' : 'p-3 text-xs text-content-faint'} data-testid="plc-view">
        {robotName ? `${robotName} · ` : ''}
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
    // Complete·Canceled — Now 태스크가 끝났다는 PLC의 첫 신호. 링보다 먼저 서고 원장이 이것으로 끝낸다.
    {
      label: 'Complete',
      value: s.Complete ? 'TRUE' : 'FALSE',
      status: s.Complete ? 'ok' : undefined,
    },
    {
      label: 'Canceled',
      value: s.Canceled ? 'TRUE' : 'FALSE',
      status: s.Canceled ? 'warn' : undefined,
    },
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
      label: 'Validation',
      value: `${r.validation} ${r.validation_text}`,
      status: r.validation === 1 ? 'ok' : r.validation === 0 ? undefined : 'fault',
    },
    {
      label: 'RejectBits',
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
      label: 'CompleteAllowed',
      value: r.complete_allowed ? 'YES' : 'NO',
      status: r.complete_allowed ? 'ok' : 'warn',
    },
    {
      label: 'DeleteAllowed',
      value: r.delete_allowed ? 'YES' : 'NO',
      status: r.delete_allowed ? 'ok' : 'warn',
    },
  ]

  return (
    <div className={bare ? 'space-y-3' : 'space-y-3 p-3'} data-testid="plc-view">
      {error ? <p className="m-0 text-2xs text-warn-fg">마지막 읽기 실패 — {error}</p> : null}
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
