// Task 조작 버튼 — 상태가 허용하는 것만 선다(`allowedActions`). 전부 ConfirmDialog를 거친다:
// 취소·완료 처리는 로봇에 물리적 결과가 있어 `single-robot`, 나머지는 `single`.
import { useState } from 'react'
import { Ban, CheckCircle2, RotateCw, Send, Trash2, XOctagon } from 'lucide-react'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { Input } from '../../lib/ui/Input'
import { runAction } from '../../lib/task/actions'
import {
  ACTION_LABEL,
  STATE_LABEL,
  allowedActions,
  isRobotAction,
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
  /** 아이콘 없이 글자만 — 표 셀 안에는 아이콘을 두지 않는다(DESIGN.md 5절 예산). */
  icons?: boolean
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

function describe(action: TaskAction, task: Task): string {
  const who = `#${task.seq} (WorkId ${task.work_id} · TaskId ${task.task_id}, ${STATE_LABEL[task.state]})`
  switch (action) {
    case 'submit':
      return `${who} 을(를) PLC에 제출합니다.`
    case 'cancel':
      return task.state === 'draft'
        ? `${who} 초안을 폐기합니다.`
        : `${who} 에 Delete 명령을 보냅니다. PLC가 허용(STAT.RES.Data[6])할 때만 통하고, 실행 중인 Task는 AUTO 모드가 아닐 때만 처리됩니다. 결과는 PLC의 Task.Status.Canceled 또는 Canceled 링으로 돌아옵니다.`
    case 'complete':
      return `${who} 에 Complete 명령을 보내 강제로 완료 처리합니다. PLC가 허용(STAT.RES.Data[5])할 때만 통합니다.`
    case 'resubmit':
      return `${who} 과 같은 내용으로 새 Task를 만들어 제출합니다(WorkId/TaskId는 새로 받습니다).`
    case 'fail':
      return `${who} 을(를) 콘솔 원장에서 실패로 표시합니다. PLC에는 아무것도 보내지 않습니다.`
    case 'delete':
      return `${who} 기록을 원장에서 지웁니다. 되돌릴 수 없습니다.`
  }
}

export function TaskActions({
  task,
  onDone,
  size = 'sm',
  only,
  icons = true,
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
      <div className="flex flex-wrap items-center gap-1" data-testid="task-actions">
        {actions.map((a) => (
          <Button
            key={a}
            size={size}
            intent={
              DANGER.has(a) ? 'outline' : a === 'submit' || a === 'resubmit' ? 'primary' : 'neutral'
            }
            icon={icons ? ICON[a] : undefined}
            loading={busy === a}
            disabled={busy !== null}
            data-testid={`${testid}-${a}`}
            onClick={() => setPending(a)}
          >
            {ACTION_LABEL[a]}
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
          <p className="m-0">{describe(pending, task)}</p>
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
