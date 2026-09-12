// 진행 중 Task 표 — 스토어 목록(SSE 반영)을 그린다. 행 클릭 → 상세 드로어.
//
// "PLC와 불일치" 배지는 원장 상태와 `WebMon.Stat.Task` 배열의 대조(`deriveState`) 결과다 — 백엔드
// 동기 엔진이 놓친 전이를 화면이 한 번 더 잡는다. 대조용 스냅샷은 부모가 1초마다 건넨다(20Hz 피드를
// 표가 직접 구독하면 초당 스무 번 다시 그린다).
import { useMemo } from 'react'
import { DataTable } from '../../lib/ui/DataTable'
import { TaskActions } from './TaskActions'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import { StatusDot } from '../../lib/ui/StatusDot'
import type { Column } from '../../lib/ui/table'
import {
  STATE_LABEL,
  STATE_TONE,
  deriveState,
  elapsed,
  fmtElapsed,
  fmtTime,
  dimsLabel,
  targetLabel,
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
  /** 행 끝에 취소·완료 버튼 — 상세를 열지 않고 목록에서 바로 보낸다. 확인 대화상자는 그대로 거친다. */
  rowActions?: boolean
}

/** 행에 싣는 조작은 PLC에 가는 둘뿐이다 — 실패 표시·삭제·재제출은 상세에서(행이 조작 띠가 되지 않게). */
const ROW_ACTIONS = ['cancel', 'complete'] as const

// 라벨 도우미는 `lib/task/state.ts`에 있다(순수 · 테스트 가능). 여기서는 이력 표가 쓰던 이름을 그대로 낸다.
export { targetLabel }

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
  rowActions = false,
}: TaskTableProps) {
  const columns = useMemo<Column<Task>[]>(
    () => [
      // 열의 순서와 구성은 현장 요구 그대로다: WorkId · TaskId · 로봇 · 셀 · 종류 · 품목 · 명령시간 ·
      // 처리시간 · 상태 · 조작. `priority` — 좁은 존에서는 셀·종류·상태(1)가 남고, 로봇·품목·처리시간(2),
      // Id·명령시간(3) 순으로 접힌다(접힌 값은 행 펼치기로).
      {
        key: 'work',
        label: 'WorkId',
        get: (t) => (t.work_id ? String(t.work_id) : ''),
        class: 'font-mono',
        priority: 3,
      },
      {
        key: 'task',
        label: 'TaskId',
        get: (t) => (t.work_id ? t.task_id : null),
        numeric: true,
        priority: 3,
      },
      {
        key: 'robot',
        label: '로봇',
        get: (t) => t.plc_name ?? '',
        class: 'font-mono',
        priority: 2,
      },
      { key: 'target', label: '셀', get: (t) => targetLabel(t), priority: 1 },
      { key: 'type', label: '종류', get: (t) => typeName(t.plc_task?.TaskType), priority: 1 },
      {
        key: 'item',
        label: '품목',
        get: (t) => t.plc_task?.Item?.Code || null,
        cell: (t) => {
          const i = t.plc_task?.Item
          if (!i?.Code) return <span className="text-content-faint">—</span>
          return (
            <span
              className="tabular-nums"
              title={dimsLabel(t) ? `ID/OD/H ${dimsLabel(t)}` : undefined}
            >
              {i.Code}
              {i.Count > 1 ? <span className="text-content-faint"> ×{i.Count}</span> : null}
            </span>
          )
        },
        numeric: true,
        priority: 2,
      },
      {
        key: 'issued',
        label: '명령시간',
        get: (t) => t.submitted_at ?? t.created_at,
        cell: (t) => (
          <span className="tabular-nums">{fmtTime(t.submitted_at ?? t.created_at, now)}</span>
        ),
        priority: 3,
      },
      {
        key: 'elapsed',
        label: '처리시간',
        get: (t) => elapsed(t, now),
        cell: (t) => <span className="tabular-nums">{fmtElapsed(elapsed(t, now))}</span>,
        numeric: true,
        priority: 2,
      },
      {
        key: 'state',
        label: '상태',
        get: (t) => t.state,
        cell: (t) => <StateCell task={t} area={area} />,
        priority: 1,
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
      actions={
        rowActions
          ? (t) => <TaskActions task={t} only={ROW_ACTIONS} iconOnly testid="row-action" />
          : undefined
      }
    />
  )
}
