// Task 조작 버튼 — 상태가 허용하는 것만 선다(`allowedActions`). 전부 ConfirmDialog를 거친다:
// 취소·완료 처리는 로봇에 물리적 결과가 있어 `single-robot`, 나머지는 `single`.
import { useState } from 'react'
import { Ban, CheckCircle2, RotateCw, Send, Trash2, XOctagon } from 'lucide-react'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import type { MenuItem } from '../../lib/ui/menu'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { FieldList, type FieldItem } from '../../lib/ui/FieldList'
import { Input } from '../../lib/ui/Input'
import { runAction } from '../../lib/task/actions'
import { tasks } from '../../lib/tasks'
import { robots } from '../../lib/robots'
import type { RobotChipModel } from '../../lib/robotContext'
import { useStore } from '../../lib/store'
import { robotField } from '../shared/RobotChip'
import {
  ACTION_LABEL,
  STATE_LABEL,
  allowedActions,
  cascadeAfter,
  isTerminal,
  dimsLabel,
  isRobotAction,
  targetOf,
  typeName,
  type TaskAction,
} from '../../lib/task/state'
import type { Task } from '../../lib/types'

export interface TaskActionsProps {
  task: Task
  /** 조작이 끝난 뒤(성공/실패 무관) — 삭제 성공이면 상세를 닫는 데 쓴다. */
  onDone?: (action: TaskAction, ok: boolean) => void
  size?: 'sm' | 'md'
  /** 이 조작만 그린다(상태가 허용하는 것과 교집합). 표의 행 액션은 취소·완료 둘만 싣는다. */
  only?: readonly TaskAction[]
  /** 아이콘 없이 글자만. */
  icons?: boolean
  /**
   * 행 액션 열 — 아이콘+짧은 글자, **고정 너비 둘**(취소·완료)이 항상 같은 자리에 선다. 상태가
   * 허용하지 않는 쪽은 사유를 달고 비활성(자리를 비우면 열이 흔들려 세로로 훑을 수 없다).
   */
  row?: boolean
  /**
   * `only`에 없는 나머지 조작을 `⋯` 하나로 모은다(제출·재제출·실패 표시·기록 삭제).
   *
   * **표의 행에는 쓰지 않는다.** 킷의 `OverflowMenu` 가 제 머리글에 적어 둔 규칙이고(`⋯` 는 도구
   * 띠와 화면 머리띠의 오른쪽 끝에만 선다), 이유는 자리다: 행 액션 열은 **고정 너비 둘**이 줄마다
   * 같은 x 에 서야 세로로 훑을 수 있는데, 손잡이가 하나 더 붙으면 상태에 따라 열 폭이 흔들린다.
   * 행에서 못 하게 된 조작은 **행을 눌러 열리는 상세 시트**의 같은 `⋯` 에 그대로 있다(클릭 하나 더).
   */
  overflow?: boolean
  /** `data-testid` 접두 — 상세(`action-*`)와 행(`row-action-*`)이 한 화면에 같이 선다. */
  testid?: string
}

const ICON: Record<TaskAction, React.ReactNode> = {
  submit: <Send size={13} />,
  cancel: <Ban size={13} />,
  complete: <CheckCircle2 size={13} />,
  resubmit: <RotateCw size={13} />,
  fail: <XOctagon size={13} />,
  delete: <Trash2 size={13} />,
}

const DANGER: ReadonlySet<TaskAction> = new Set<TaskAction>(['cancel', 'fail', 'delete'])
const ROW_DEFAULT: readonly TaskAction[] = ['cancel', 'complete']
/** 넘침 메뉴의 순서 — 보내는 것부터 지우는 것까지. */
const ALL_ACTIONS: readonly TaskAction[] = [
  'submit',
  'resubmit',
  'cancel',
  'complete',
  'fail',
  'delete',
]
/** 행 버튼의 짧은 글자 — 폭이 고정이라 `완료 처리`는 들어가지 않는다. 대화상자 제목은 긴 이름을 쓴다. */
const ROW_LABEL: Partial<Record<TaskAction, string>> = { cancel: '취소', complete: '완료' }
/** 행 모드에서 비활성인 이유 — 회색으로 침묵하는 버튼은 고장으로 읽힌다(DESIGN.md 4절 ⑥). */
function whyNot(a: TaskAction, state: Task['state']): string {
  if (isTerminal(state)) return `${STATE_LABEL[state]} — 끝난 Task`
  if (a === 'complete' && (state === 'draft' || state === 'submitted'))
    return `${STATE_LABEL[state]} — PLC에 아직 자리가 없어 완료 처리할 수 없음`
  return `${STATE_LABEL[state]}에서는 할 수 없음`
}

// 확인은 **질문 한 줄**이다. PLC 허용 조건·AUTO 모드 같은 규칙은 여기 적지 않는다 — 막히면 서버가
// 그 이유를 토스트로 말하고, 매번 읽지 않는 안내문은 진짜 경고(되돌릴 수 없음)까지 묻어 버린다.
function describe(action: TaskAction, task: Task, robot: string): string {
  // 질문이 **어느 호기의** Task 인지 말한다 — 표에는 여러 로봇의 행이 섞여 선다.
  const who = `${robot} Task #${task.seq}`
  switch (action) {
    case 'submit':
      return `${who}을(를) PLC에 제출하시겠습니까?`
    case 'cancel':
      return task.state === 'draft'
        ? `${who} 초안을 폐기하시겠습니까?`
        : `${who}을(를) 정말로 취소하시겠습니까?`
    case 'complete':
      return `${who}을(를) 강제로 완료 처리하시겠습니까?`
    case 'resubmit':
      return `${who}과(와) 같은 내용으로 다시 제출하시겠습니까?`
    case 'fail':
      return `${who}을(를) 실패로 표시하시겠습니까?`
    case 'delete':
      return `${who} 기록을 정말로 삭제하시겠습니까? 되돌릴 수 없습니다.`
  }
}

/**
 * 어떤 Task인지 알아보는 라벨+값 짝 — 질문 한 줄 아래에 선다. 번호만으로는 작업자가 "무슨 작업"인지
 * 모른다(같은 셀·같은 품목의 Task가 열 개 줄지어 있다). 상세와 같은 말(종류·대상·품목·로봇)을 쓴다.
 */
function identity(task: Task, robot: RobotChipModel): FieldItem[] {
  const t = targetOf(task)
  const item = task.plc_task?.Item
  const req = task.request
  return [
    // 첫 줄이 로봇이다 — 이 Task 의 주인(원장 PLC)이지 사이드바 선택이 아니다.
    robotField(robot, 'Robot'),
    { label: 'TaskType', value: typeName(task.plc_task?.TaskType) || req?.type?.toUpperCase() || null },
    { label: 'Target', value: t ? `${t.kind === 'station' ? 'Station' : 'Cell'} ${t.id}` : null },
    {
      label: 'Item',
      value: item?.Code
        ? `${item.Code} × ${item.Count ?? 1}`
        : req?.item_code
          ? `${req.item_code} × ${req.count ?? 1}`
          : null,
    },
    { label: 'InnerDiameter/OuterDiameter/Height', value: dimsLabel(task) || null, mono: true },
    { label: 'State', value: STATE_LABEL[task.state] },
    {
      label: 'WorkId / TaskId',
      value: task.work_id ? `${task.work_id} / ${task.task_id}` : null,
      mono: true,
      wide: true,
    },
    ...(req?.note ? [{ label: 'Note', value: req.note, wide: true } as FieldItem] : []),
  ]
}

export function TaskActions({
  task,
  onDone,
  size = 'sm',
  only,
  icons = true,
  row = false,
  overflow = false,
  testid = 'action',
}: TaskActionsProps) {
  const [pending, setPending] = useState<TaskAction | null>(null)
  const [busy, setBusy] = useState<TaskAction | null>(null)
  const [note, setNote] = useState('')
  useStore(tasks, robots)
  // 이 Task 의 주인 로봇 — 원장의 상태 PLC 로 찾는다(사이드바 선택과 다를 수 있다).
  const owner = robots.chipOfPlc(task.plc_name)
  const allowed = allowedActions(task.state)
  // 행 모드는 슬롯이 고정이다(`only` 순서대로, 허용 안 되면 비활성). 그 외는 허용된 것만.
  const actions = row
    ? [...(only ?? ROW_DEFAULT)]
    : allowed.filter((a) => !only || only.includes(a))
  // 넘침 메뉴 — 버튼으로 서지 않은 조작. 상태가 허용하지 않는 것도 **사유를 달아 남긴다**
  // (회색으로 침묵하는 대신 왜 안 되는지 말한다 — DESIGN.md 4절 ⑥).
  // 행 모드에서는 `overflow` 를 무시한다 — 자리 규칙(위 `overflow` 주석)이 코드에서도 한 번 더 막는다.
  const rest: MenuItem[] = overflow && !row
    ? ALL_ACTIONS.filter((a) => !actions.includes(a)).map((a) => ({
        label: ACTION_LABEL[a],
        danger: DANGER.has(a),
        disabled: allowed.includes(a) ? undefined : whyNot(a, task.state),
        run: () => setPending(a),
      }))
    : []
  if (actions.length === 0 && rest.length === 0) return null
  const tail = pending === 'cancel' && task.state !== 'draft' ? cascadeAfter(tasks.list, task) : []

  const run = async (a: TaskAction) => {
    setBusy(a)
    try {
      const r = await runAction(a, task, a === 'fail' && note.trim() ? { note: note.trim() } : {})
      onDone?.(a, r.ok)
    } finally {
      setBusy(null)
      setNote('')
    }
  }

  return (
    <>
      {/* 행 모드는 줄바꿈하지 않는다 — 폭이 내용에 맞는 표 셀(`fit`)에서 둘이 세로로 쌓인다. */}
      <div
        className={
          row ? 'flex items-center justify-center gap-1' : 'flex flex-wrap items-center gap-1'
        }
        data-testid="task-actions"
      >
        {actions.map((a) => (
          <Button
            key={a}
            size={size}
            intent={
              DANGER.has(a) ? 'outline' : a === 'submit' || a === 'resubmit' ? 'primary' : 'neutral'
            }
            icon={icons ? ICON[a] : undefined}
            loading={busy === a}
            disabled={busy !== null || (row && !allowed.includes(a))}
            title={row && !allowed.includes(a) ? whyNot(a, task.state) : undefined}
            className={row ? 'w-18 justify-center' : undefined}
            data-testid={`${testid}-${a}`}
            onClick={() => setPending(a)}
          >
            {row ? (ROW_LABEL[a] ?? ACTION_LABEL[a]) : ACTION_LABEL[a]}
          </Button>
        ))}
        {rest.length > 0 ? (
          <OverflowMenu items={rest} title="다른 조작" testid={`${testid}-more`} />
        ) : null}
      </div>
      {pending ? (
        <ConfirmDialog
          open
          onOpenChange={(o) => {
            if (!o) setPending(null)
          }}
          scope={isRobotAction(pending) ? 'single-robot' : 'single'}
          title={`${ACTION_LABEL[pending]} — ${owner.name} Task #${task.seq}`}
          danger={DANGER.has(pending)}
          confirmLabel={ACTION_LABEL[pending]}
          onConfirm={() => void run(pending)}
        >
          <p className="m-0 text-content-primary">{describe(pending, task, owner.name)}</p>
          <FieldList
            className="mt-3"
            items={identity(task, owner)}
            columns={2}
            labelWidth={96}
            dense
          />
          {tail.length > 0 ? (
            <p className="mt-3 mb-0 text-warn-fg" data-testid="cascade-note">
              같은 WorkId의 뒤 Task {tail.length}건도 함께 취소됩니다 —{' '}
              {tail.map((t) => `#${t.seq}(TaskId ${t.task_id})`).join(' · ')}
            </p>
          ) : null}
          {pending === 'fail' ? (
            <Input
              className="mt-3"
              label="사유(선택)"
              placeholder="이력에 남길 메모"
              value={note}
              onValueChange={setNote}
            />
          ) : null}
        </ConfirmDialog>
      ) : null}
    </>
  )
}
