// Ack 배너 — 제출 뒤 PLC 응답(수락/거부)을 코드·한글로 보이고 Task 관리로 건너가는 링크를 둔다.
//
// Ack는 **비동기**로 온다(에코 → 원장 동기가 판정). `POST /api/tasks`는 제출 직후 돌아오므로 이 배너의
// 주인(`ComposeCard`)이 `GET /api/tasks/{id}`를 0.5초마다 최대 8초 폴링해 `ack`이 앉을 때까지 기다린다.
import { CircleCheck, CircleX, ExternalLink, LoaderCircle, X } from 'lucide-react'
import { ackText } from '../../lib/gr/rejectCodes'
import { STATE_LABEL } from '../../lib/gr/const'
import { nav } from '../../lib/nav'
import type { Task } from '../../lib/types'

export type AckPhase = 'waiting' | 'done' | 'timeout'

export interface AckBannerProps {
  task: Task
  phase: AckPhase
  onDismiss: () => void
}

export function AckBanner({ task, phase, onDismiss }: AckBannerProps) {
  const ack = task.ack
  const accepted = ack?.accepted ?? null
  const tone =
    phase === 'waiting'
      ? 'border-info bg-info-soft text-info-fg'
      : accepted === true
        ? 'border-ok bg-ok-soft text-ok-fg'
        : accepted === false
          ? 'border-fault bg-fault-soft text-fault-fg'
          : 'border-warn bg-warn-soft text-warn-fg'
  const Icon =
    phase === 'waiting'
      ? LoaderCircle
      : accepted === true
        ? CircleCheck
        : accepted === false
          ? CircleX
          : LoaderCircle

  const headline =
    phase === 'waiting'
      ? 'PLC 응답 대기 중…'
      : ack
        ? ackText(ack)
        : phase === 'timeout'
          ? '8초 안에 응답이 오지 않았습니다 — Task 관리에서 상태를 확인하세요'
          : `상태 ${STATE_LABEL[task.state]}`

  return (
    <div
      className={`flex items-start gap-2 rounded-md border px-3 py-2 text-xs ${tone}`}
      data-testid="ack-banner"
      data-phase={phase}
      data-accepted={accepted === null ? undefined : String(accepted)}
    >
      <Icon
        className={`mt-0.5 h-3.5 w-3.5 shrink-0 ${phase === 'waiting' ? 'animate-spin' : ''}`}
      />
      <div className="min-w-0 flex-1">
        <div className="font-medium">{headline}</div>
        <div className="mt-0.5 flex flex-wrap gap-x-3 opacity-80">
          <span>
            #{task.seq} · Work {task.work_id} / Task {task.task_id}
          </span>
          <span>상태 {STATE_LABEL[task.state]}</span>
          {ack && !ack.accepted && ack.reason ? (
            <span className="font-mono" title="PLC 원문">
              {ack.reason}
            </span>
          ) : null}
        </div>
        <button
          type="button"
          className="mt-1 inline-flex items-center gap-1 underline decoration-dotted underline-offset-2"
          onClick={() => nav.goTask(task.id)}
          data-testid="ack-goto-task"
        >
          <ExternalLink className="h-3 w-3" />
          Task 관리에서 보기
        </button>
      </div>
      <button
        type="button"
        className="shrink-0 opacity-60 hover:opacity-100"
        aria-label="닫기"
        onClick={onDismiss}
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
  )
}
