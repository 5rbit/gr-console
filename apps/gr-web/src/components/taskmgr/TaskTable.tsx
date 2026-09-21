// Task 표 — 이 화면의 본문이다. 스토어 목록(SSE 반영)을 그리고, 행을 누르면 상세 시트가 열린다.
//
// 행이 곧 상태다: 로봇 색 점 · 종류 · 대상 · 품목 · 상태 · 경과가 한 줄에 있어 상세를 열지 않고도
// "지금 무엇이 어디까지 갔나"를 읽는다(예전에는 경과를 보려면 상세를 열어야 했다).
//
// "PLC와 불일치" 배지는 원장 상태와 `WebMon.Stat.Task` 배열의 대조(`deriveState`) 결과다 — 백엔드
// 동기 엔진이 놓친 전이를 화면이 한 번 더 잡는다. 대조용 스냅샷은 부모가 1초마다 건넨다(20Hz 피드를
// 표가 직접 구독하면 초당 스무 번 다시 그린다).
import { useMemo } from 'react'
import { DataTable } from '../../lib/ui/DataTable'
import { TaskActions } from './TaskActions'
import { robotColor, robots } from '../../lib/robots'
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
  /** Task 마다 **그 Task 의 로봇** PLC 배열(여러 로봇) — 있으면 `area` 대신 쓴다. */
  areaFor?: (task: Task) => PlcTaskArea | null
  /** 경과 시간의 기준 시각(부모의 1초 틱). */
  now: number
  selected: string | null
  onPick: (task: Task) => void
  loading?: boolean
  empty?: string
  emptyHint?: string
  testid?: string
  /** 행 끝에 취소·완료 **둘** — 나머지 조작은 행을 눌러 열리는 상세 시트의 `⋯` 에 있다. */
  rowActions?: boolean
}

/**
 * 행에 서는 조작은 PLC 에 가는 둘뿐이다(취소·완료). 나머지(제출·재제출·실패 표시·삭제)는 행을 눌러
 * 열리는 **상세 시트**의 `⋯` 에 있다 — 표의 행에는 넘침 손잡이를 두지 않는다(`docs/DESIGN.md` 5절
 * 자리 표: 행 액션은 고정 너비 둘이 항상 같은 자리).
 */
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

/**
 * 로봇 칸 — 맵·사이드바와 **같은 색**의 점을 이름 앞에 둔다. 두 대가 섞인 목록에서 색이 먼저 읽히면
 * 이름을 글자로 읽지 않고도 행을 가른다. 점은 폭이 고정이라 열이 흔들리지 않는다.
 */
function RobotCell({ task }: { task: Task }) {
  const name = task.plc_name ?? ''
  if (!name) return <span className="text-content-faint">—</span>
  const r = robots.list.find((x) => x.plc === name)
  return (
    <span className="inline-flex items-center gap-1.5 whitespace-nowrap">
      <span
        className="h-1.5 w-1.5 shrink-0 rounded-full"
        // design-lint-allow: no-raw-color — 로봇 색은 `lib/robots.ts`가 소유하는 데이터(맵·사이드바와 같은 값)다
        style={{ background: robotColor(r?.id) }}
      />
      <span className="font-mono">{r?.name ?? name}</span>
    </span>
  )
}

/**
 * Task 표의 열 — **목록과 종결 이력이 같은 열**을 쓴다. 두 표가 열이 다르면 같은 Task가 위아래에서
 * 다른 모양으로 보이고, 종결 뒤에 "어디 갔지"를 찾게 된다. `area`는 대조용 PLC 스냅샷(이력은 null).
 */
export function taskColumns(
  area: PlcTaskArea | null,
  now: number,
  areaFor?: (task: Task) => PlcTaskArea | null,
): Column<Task>[] {
  return [
    // `priority` — 좁은 존에서는 행을 알아보는 것(로봇·대상·종류·상태)만 남고, 품목·경과(2),
    // Id·시각(3) 순으로 접힌다(접힌 값은 행 펼치기로).
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
      label: 'Robot',
      get: (t) => t.plc_name ?? '',
      cell: (t) => <RobotCell task={t} />,
      priority: 1,
    },
    { key: 'target', label: 'Target', get: (t) => targetLabel(t), priority: 1 },
    { key: 'type', label: 'TaskType', get: (t) => typeName(t.plc_task?.TaskType), priority: 1 },
    {
      key: 'item',
      label: 'Item.Code',
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
      key: 'state',
      label: 'State',
      get: (t) => t.state,
      cell: (t) => <StateCell task={t} area={areaFor ? areaFor(t) : area} />,
      priority: 1,
    },
    {
      // 진행 중인 행에서는 1초마다 는다 — "멈춰 있나"를 상세를 열지 않고 읽는 자리다.
      key: 'elapsed',
      label: 'Elapsed',
      get: (t) => elapsed(t, now),
      cell: (t) => <span className="font-mono tabular-nums">{fmtElapsed(elapsed(t, now))}</span>,
      numeric: true,
      priority: 2,
    },
    {
      key: 'issued',
      label: 'SubmittedAt',
      get: (t) => t.submitted_at ?? t.created_at,
      cell: (t) => (
        <span className="tabular-nums">{fmtTime(t.submitted_at ?? t.created_at, now)}</span>
      ),
      priority: 3,
    },
    {
      key: 'ended',
      label: 'EndedAt',
      get: (t) => t.ended_at ?? '',
      cell: (t) =>
        t.ended_at ? (
          <span className="tabular-nums">{fmtTime(t.ended_at, now)}</span>
        ) : (
          <span className="text-content-faint">—</span>
        ),
      priority: 3,
    },
  ]
}

export function TaskTable({
  rows,
  area,
  areaFor,
  now,
  selected,
  onPick,
  loading = false,
  empty = '진행 중인 Task 없음',
  emptyHint,
  testid = 'task-table',
  rowActions = false,
}: TaskTableProps) {
  const columns = useMemo(() => taskColumns(area, now, areaFor), [area, now, areaFor])
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
      fit
      actions={
        rowActions
          ? (t) => <TaskActions task={t} only={ROW_ACTIONS} row testid="row-action" />
          : undefined
      }
    />
  )
}
