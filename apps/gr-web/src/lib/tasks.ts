// Task 스토어 — `tasksFeed`의 이벤트(스냅샷/갱신/삭제)를 Map에 적용해 화면이 읽는 목록을 만든다.
//
// 이벤트는 `onMessage`로 **메시지 단위**로 받는다(코얼레싱되면 upsert를 놓친다). 알림은 프레임에
// 묶는다 — 시나리오가 돌면 초당 여러 건이 갈린다.
import { api } from './api'
import { tasksFeed } from './feeds'
import { frameThrottle } from './sse'
import { Store } from './store'
import { toast } from './ui/toast'
import type { Task, TaskState, TasksEvent } from './types'

/** PLC가 아직 들고 있는(끝나지 않은) 상태. */
export const ACTIVE_STATES: readonly TaskState[] = ['submitted', 'accepted', 'queued', 'running']
/** 끝난 상태 — 백엔드 `is_terminal()`·`task/state.ts`와 같은 넷. `lost`는 끝이 아니다(재출현하면 되살아난다). */
export const TERMINAL_STATES: readonly TaskState[] = ['rejected', 'completed', 'canceled', 'failed']

const ALL_STATES: readonly TaskState[] = ['draft', ...ACTIVE_STATES, 'lost', ...TERMINAL_STATES]

export type TaskCounts = Record<TaskState, number> & { active: number; total: number }

class Tasks extends Store {
  #map = new Map<string, Task>()
  #refs = 0
  #release: (() => void) | null = null
  #off: (() => void) | null = null
  #flush = frameThrottle(() => this.notify())
  /** 정렬 캐시 — Map이 안 바뀌면 같은 배열을 돌려준다(렌더마다 정렬하지 않게). */
  #sorted: Task[] | null = null

  /** 전 Task — seq 내림차순(최근이 위). */
  get list(): Task[] {
    if (this.#sorted === null) this.#sorted = [...this.#map.values()].sort((a, b) => b.seq - a.seq)
    return this.#sorted
  }

  get(id: string): Task | null {
    return this.#map.get(id) ?? null
  }

  byState(state: TaskState | readonly TaskState[]): Task[] {
    const set = new Set(typeof state === 'string' ? [state] : state)
    return this.list.filter((t) => set.has(t.state))
  }

  get active(): Task[] {
    return this.byState(ACTIVE_STATES)
  }

  get counts(): TaskCounts {
    const c = Object.fromEntries(ALL_STATES.map((s) => [s, 0])) as Record<TaskState, number>
    let active = 0
    for (const t of this.#map.values()) {
      c[t.state] = (c[t.state] ?? 0) + 1
      if (ACTIVE_STATES.includes(t.state)) active++
    }
    return { ...c, active, total: this.#map.size }
  }

  /** 이벤트 1건 적용. */
  apply(ev: TasksEvent): void {
    if (ev.kind === 'snapshot') {
      this.#map = new Map(ev.tasks.map((t) => [t.id, t]))
    } else if (ev.kind === 'upsert') {
      this.#map.set(ev.task.id, ev.task)
    } else {
      if (!this.#map.delete(ev.id)) return
    }
    this.#sorted = null
    this.#flush()
  }

  /** 구독 수요 등록 — 해제 함수를 돌려준다. 첫 구독자가 스트림을 연다. */
  start(): () => void {
    this.#refs++
    if (this.#refs === 1) {
      this.#off = tasksFeed.onMessage((ev) => this.apply(ev))
      this.#release = tasksFeed.start()
    }
    let released = false
    return () => {
      if (released) return
      released = true
      this.stop()
    }
  }

  stop(): void {
    if (this.#refs === 0) return
    this.#refs--
    if (this.#refs === 0) {
      this.#off?.()
      this.#off = null
      this.#release?.()
      this.#release = null
      this.#flush.cancel()
    }
  }

  /**
   * 조작 하나 — `robot`이면 PLC에 명령을 보내는 조작이라 응답이 **원장 상태로** 온다. 돌아온 Task가
   * 아직 끝나지 않았으면 "취소" 대신 "취소 요청 보냄"이라고 말한다: PLC가 무시한 Delete는 그 뒤로도
   * 아무 일이 없고, 그때 "취소 완료" 토스트는 거짓이다(이력에 System 줄이 뜨는 것이 그 다음 신호다).
   */
  async #act(label: string, run: () => Promise<Task>, robot = false): Promise<Task | null> {
    const tid = toast.pending(`${label} 중…`)
    try {
      const t = await run()
      this.#map.set(t.id, t)
      this.#sorted = null
      this.notify()
      if (robot && !TERMINAL_STATES.includes(t.state)) {
        toast.resolve(tid, 'info', `#${t.seq} ${label} 요청 보냄 — PLC 응답 대기`)
      } else {
        toast.resolve(tid, 'ok', `#${t.seq} ${label}`)
      }
      return t
    } catch (e) {
      toast.resolve(tid, 'error', `${label} 실패 — ${e instanceof Error ? e.message : String(e)}`)
      return null
    }
  }

  cancel(id: string): Promise<Task | null> {
    return this.#act('취소', () => api.taskCancel(id), true)
  }
  complete(id: string): Promise<Task | null> {
    return this.#act('완료 처리', () => api.taskComplete(id), true)
  }
  resubmit(id: string): Promise<Task | null> {
    return this.#act('재제출', () => api.taskResubmit(id))
  }
}

export const tasks = new Tasks()
