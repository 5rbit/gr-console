// Task 관리 조작 — `api.ts`(리드 소유)에 없는 엔드포인트와, 조작 한 건을 토스트와 함께 실행하는 디스패처.
//
// 취소·완료·재제출은 `lib/tasks.ts` 스토어가 이미 토스트를 붙여 제공하므로 그대로 쓰고, 나머지
// (제출·실패 표시·삭제)만 여기서 같은 모양으로 감싼다. 서버 응답은 스토어 Map에 바로 앉힌다 —
// SSE upsert가 곧 따라오지만, 그때까지 화면이 옛 상태를 들고 있지 않게.
import { api, del, getJson, postJson } from '../api'
import { robotFailure, withRobot } from '../robotContext'
import { tasks } from '../tasks'
import { toast } from '../ui/toast'
import type { PlcTask, Task } from '../types'
import type { TaskAction } from './state'

/** `GET /api/tasks/stats`. */
export interface TaskStats {
  total: number
  active: number
  by_state: Record<string, number>
  completed_today: number
  rejected_today: number
  avg_complete_secs: number | null
  completed_samples: number
}

/** `GET /api/tasks/plc-view` — PLC 배열 그대로(PascalCase). */
export interface PlcView {
  status: Record<string, boolean | number>
  now: PlcTask
  queue: PlcTask[]
  completed: PlcTask[]
  canceled: PlcTask[]
  rejected: PlcTask[]
  res: {
    Header: { Protocol: number; CMD_ID: number; CMD: number; SRC: number; DST: number; SEQ: number }
    Data: number[]
  }
  reject: {
    wrong_robot: boolean
    duplicate: boolean
    invalid_task: boolean
    buffer_full: boolean
    offline: boolean
    validation: number
    validation_text: string
    complete_allowed: boolean
    delete_allowed: boolean
  }
}

export const taskApi = {
  /** `robot` 이 없으면 모든 로봇 합계. */
  stats: (robot?: number | null) =>
    getJson<TaskStats>(`/api/tasks/stats${robot != null ? `?robot=${robot}` : ''}`),
  /** 로봇 한 대의 PLC `STAT.Task` 배열(없으면 기본 로봇). */
  plcView: (robot?: number | null) =>
    getJson<PlcView>(`/api/tasks/plc-view${robot != null ? `?robot=${robot}` : ''}`),
  markFailed: (id: string, note?: string) =>
    postJson<Task>(`/api/tasks/${id}/mark-failed`, note ? { note } : undefined),
  remove: (id: string) => del(`/api/tasks/${id}`),
}

async function withToast(
  who: string | null,
  label: string,
  run: () => Promise<Task>,
): Promise<Task | null> {
  const tid = toast.pending(withRobot(who, `${label} 중…`))
  try {
    const t = await run()
    tasks.apply({ kind: 'upsert', task: t })
    toast.resolve(tid, 'ok', withRobot(who, `#${t.seq} ${label}`))
    return t
  } catch (e) {
    toast.resolve(
      tid,
      'error',
      robotFailure(who, `${label} 실패`, e instanceof Error ? e.message : String(e)),
    )
    return null
  }
}

/** 조작 한 건 실행. 삭제는 Task를 돌려주지 않으므로 성공 시 `null`이지만 `ok`가 참이다. */
export async function runAction(
  action: TaskAction,
  task: Pick<Task, 'id' | 'seq'>,
  opts: { note?: string } = {},
): Promise<{ ok: boolean; task: Task | null }> {
  switch (action) {
    case 'cancel': {
      const t = await tasks.cancel(task.id)
      return { ok: t !== null, task: t }
    }
    case 'complete': {
      const t = await tasks.complete(task.id)
      return { ok: t !== null, task: t }
    }
    case 'resubmit': {
      const t = await tasks.resubmit(task.id)
      return { ok: t !== null, task: t }
    }
    case 'submit': {
      const t = await withToast(tasks.ownerOf(task.id), '제출', () => api.taskSubmit(task.id))
      return { ok: t !== null, task: t }
    }
    case 'fail': {
      const t = await withToast(tasks.ownerOf(task.id), '실패로 표시', () => taskApi.markFailed(task.id, opts.note))
      return { ok: t !== null, task: t }
    }
    case 'delete': {
      const tid = toast.pending('기록 삭제 중…')
      try {
        await taskApi.remove(task.id)
        tasks.apply({ kind: 'remove', id: task.id })
        toast.resolve(tid, 'ok', `#${task.seq} 기록 삭제`)
        return { ok: true, task: null }
      } catch (e) {
        toast.resolve(
          tid,
          'error',
          `기록 삭제 실패 — ${e instanceof Error ? e.message : String(e)}`,
        )
        return { ok: false, task: null }
      }
    }
  }
}
