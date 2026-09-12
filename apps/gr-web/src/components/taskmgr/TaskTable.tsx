// 진행 중 Task 표 — 스토어 목록(SSE 반영)을 그린다. 행 클릭 → 상세 드로어.
//
// "PLC와 불일치" 배지는 원장 상태와 `WebMon.Stat.Task` 배열의 대조(`deriveState`) 결과다 — 백엔드
// 동기 엔진이 놓친 전이를 화면이 한 번 더 잡는다. 대조용 스냅샷은 부모가 1초마다 건넨다(20Hz 피드를
// 표가 직접 구독하면 초당 스무 번 다시 그린다).
import { useMemo } from 'react'
import { DataTable } from '../../lib/ui/DataTable'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import { StatusDot } from '../../lib/ui/StatusDot'
import type { Column } from '../../lib/ui/table'
import {
  ORIGIN_LABEL,
  STATE_LABEL,
  STATE_TONE,
  deriveState,
  elapsed,
  fmtElapsed,
  fmtTime,
  targetOf,
  typeName,
  type PlcTaskArea,
} from '../../lib/task/state'
import type { Task } from '../../lib/types'

export interface TaskTableProps {
  rows: Task[]
  /** PLC 배열 스냅샷(대조용) — 없으면 배지를 내지 않는다. */
  area: PlcTaskArea | null
  /** 경과 시간의 기준 시각(부모의 1초 틱). */
  now: number
  selected: string | null
  onPick: (task: Task) => void
  loading?: boolean
  empty?: string
  emptyHint?: string
  testid?: string
}

export function targetLabel(task: Task): string {
  const t = targetOf(task)
  if (!t) return ''
  return `${t.kind === 'station' ? 'ST' : '셀'} ${t.id}`
}

/** PLC 자리 — 실행 중이면 스텝, 대기면 큐 슬롯. */
function plcStep(task: Task): string {
  const p = task.plc
  if (!p) return ''
  if (task.state === 'running') return String(p.step)
  if (task.state === 'queued' && p.queue_index !== null) return `Q${p.queue_index}`
  return ''
}

export function StateCell({ task, area }: { task: Task; area: PlcTaskArea | null }) {
  const d = deriveState(task, area)
  return (
    <span className="inline-flex items-center gap-1">
      <StatusBadge status={STATE_TONE[task.state]} data-testid="task-state">
        {STATE_LABEL[task.state]}
      </StatusBadge>
      {d.mismatch ? (
        <span data-testid="task-mismatch">
          <StatusDot status="warn" size="sm" title={`PLC와 불일치 — ${d.reason ?? ''}`} />
        </span>
      ) : null}
    </span>
  )
}

export function AckCell({ task }: { task: Task }) {
  const a = task.ack
  if (!a) return <span className="text-content-faint">—</span>
  return (
    <span
      className={`tabular-nums ${a.accepted ? 'text-ok-fg' : 'text-fault-fg'}`}
      title={a.reason}
    >
      {a.code}
      {a.reject_bits ? (
        <span className="ml-1 text-3xs opacity-70">
          b{a.reject_bits.toString(2).padStart(4, '0')}
        </span>
      ) : null}
    </span>
  )
}

export function TaskTable({
  rows,
  area,
  now,
  selected,
  onPick,
  loading = false,
  empty = '진행 중인 Task 없음',
  emptyHint,
  testid = 'task-table',
}: TaskTableProps) {
  const columns = useMemo<Column<Task>[]>(
    () => [
      // `priority` — 좁은 존(오른쪽 인스펙터·하단)에 이 표가 들어갈 수 있다. 1은 **행을 알아보는 데
      // 필요한 것**(번호·상태·대상), 2는 맥락(어느 로봇·무슨 종류·얼마나 됐나), 3은 세부다.
      // 접힌 열은 행을 펼치면 라벨+값 짝으로 나온다(`DataTable`).
      { key: 'seq', label: '#', get: (t) => t.seq, numeric: true, class: 'w-12', priority: 1 },
      {
        key: 'state',
        label: '상태',
        get: (t) => t.state,
        cell: (t) => <StateCell task={t} area={area} />,
        priority: 1,
      },
      {
        key: 'robot',
        label: '로봇',
        get: (t) => t.plc_name ?? '',
        class: 'font-mono',
        priority: 2,
      },
      { key: 'type', label: '종류', get: (t) => typeName(t.plc_task?.TaskType), priority: 2 },
      { key: 'target', label: '대상', get: (t) => targetLabel(t), priority: 1 },
      {
        key: 'item',
        label: '품목',
        get: (t) => t.plc_task?.Item?.Code || null,
        numeric: true,
        priority: 3,
      },
      {
        key: 'count',
        label: '수량',
        get: (t) => t.plc_task?.Item?.Count ?? null,
        numeric: true,
        priority: 3,
      },
      {
        key: 'ack',
        label: 'Ack',
        get: (t) => t.ack?.code ?? null,
        cell: (t) => <AckCell task={t} />,
        numeric: true,
        priority: 3,
      },
      { key: 'step', label: 'PLC', get: (t) => plcStep(t), class: 'font-mono', priority: 3 },
      { key: 'origin', label: '출처', get: (t) => ORIGIN_LABEL[t.origin], priority: 3 },
      {
        key: 'created',
        label: '생성',
        get: (t) => t.created_at,
        cell: (t) => <span className="tabular-nums">{fmtTime(t.created_at, now)}</span>,
        priority: 3,
      },
      {
        key: 'elapsed',
        label: '경과',
        get: (t) => elapsed(t, now),
        cell: (t) => <span className="tabular-nums">{fmtElapsed(elapsed(t, now))}</span>,
        numeric: true,
        priority: 2,
      },
    ],
    [area, now],
  )
  return (
    <DataTable
      rows={rows}
      columns={columns}
      rowKey={(t) => t.id}
      onPick={onPick}
      selected={selected}
      loading={loading}
      empty={empty}
      emptyHint={emptyHint}
      testid={testid}
    />
  )
}
