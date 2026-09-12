// Task 조작 버튼 — 상태가 허용하는 것만 선다(`allowedActions`). 전부 ConfirmDialog를 거친다:
// 취소·완료 처리는 로봇에 물리적 결과가 있어 `single-robot`, 나머지는 `single`.
import { useState } from 'react'
import { Ban, CheckCircle2, RotateCw, Send, Trash2, XOctagon } from 'lucide-react'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { FieldList, type FieldItem } from '../../lib/ui/FieldList'
import { Input } from '../../lib/ui/Input'
import { runAction } from '../../lib/task/actions'
import {
  ACTION_LABEL,
  STATE_LABEL,
  allowedActions,
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
  /** 아이콘만(행 액션 열) — 라벨은 `aria-label`·`title`로 남는다. 표 셀의 유일한 아이콘 예외다. */
  iconOnly?: boolean
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

// 확인은 **질문 한 줄**이다. PLC 허용 조건·AUTO 모드 같은 규칙은 여기 적지 않는다 — 막히면 서버가
// 그 이유를 토스트로 말하고, 매번 읽지 않는 안내문은 진짜 경고(되돌릴 수 없음)까지 묻어 버린다.
function describe(action: TaskAction, task: Task): string {
  const who = `Task #${task.seq}`
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
function identity(task: Task): FieldItem[] {
  const t = targetOf(task)
  const item = task.plc_task?.Item
  const req = task.request
  return [
    { label: '종류', value: typeName(task.plc_task?.TaskType) || req?.type?.toUpperCase() || null },
    { label: '대상', value: t ? `${t.kind === 'station' ? '스테이션' : '셀'} ${t.id}` : null },
    {
      label: '품목',
      value: item?.Code
        ? `${item.Code} × ${item.Count ?? 1}`
        : req?.item_code
          ? `${req.item_code} × ${req.count ?? 1}`
          : null,
    },
    { label: '치수 ID/OD/H', value: dimsLabel(task) || null, mono: true },
    { label: '로봇', value: task.plc_name ?? null },
    { label: '상태', value: STATE_LABEL[task.state] },
    {
      label: 'WorkId / TaskId',
      value: task.work_id ? `${task.work_id} / ${task.task_id}` : null,
      mono: true,
      wide: true,
    },
    ...(req?.note ? [{ label: '메모', value: req.note, wide: true } as FieldItem] : []),
  ]
}

export function TaskActions({
  task,
  onDone,
  size = 'sm',
  only,
  icons = true,
  iconOnly = false,
  testid = 'action',
}: TaskActionsProps) {
  const [pending, setPending] = useState<TaskAction | null>(null)
  const [busy, setBusy] = useState<TaskAction | null>(null)
  const [note, setNote] = useState('')
  const actions = allowedActions(task.state).filter((a) => !only || only.includes(a))
  if (actions.length === 0) return null

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
      {/* 아이콘만일 때는 줄바꿈하지 않는다 — 폭이 내용에 맞는 표 셀(`fit`)에서 둘이 세로로 쌓인다. */}
      <div
        className={
          iconOnly ? 'flex items-center justify-center gap-1' : 'flex flex-wrap items-center gap-1'
        }
        data-testid="task-actions"
      >
        {actions.map((a) => (
          <Button
            key={a}
            size={iconOnly ? 'icon-sm' : size}
            intent={
              DANGER.has(a) ? 'outline' : a === 'submit' || a === 'resubmit' ? 'primary' : 'neutral'
            }
            icon={icons || iconOnly ? ICON[a] : undefined}
            loading={busy === a}
            disabled={busy !== null}
            aria-label={iconOnly ? ACTION_LABEL[a] : undefined}
            title={iconOnly ? ACTION_LABEL[a] : undefined}
            data-testid={`${testid}-${a}`}
            onClick={() => setPending(a)}
          >
            {iconOnly ? null : ACTION_LABEL[a]}
          </Button>
        ))}
      </div>
      {pending ? (
        <ConfirmDialog
          open
          onOpenChange={(o) => {
            if (!o) setPending(null)
          }}
          scope={isRobotAction(pending) ? 'single-robot' : 'single'}
          title={`${ACTION_LABEL[pending]} — Task #${task.seq}`}
          danger={DANGER.has(pending)}
          confirmLabel={ACTION_LABEL[pending]}
          onConfirm={() => void run(pending)}
        >
          <p className="m-0 text-content-primary">{describe(pending, task)}</p>
          <FieldList className="mt-3" items={identity(task)} columns={2} labelWidth={96} dense />
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
