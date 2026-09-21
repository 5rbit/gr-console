// 작업 명령 화면 오른쪽 칸의 **Task Manager 목록** — 순수 규칙(DOM 무관, 테스트 대상).
//
// 원장(`/api/tasks`)은 콘솔·시나리오가 제출한 Task 와, PLC 에서 처음 본 **외부(GCS) Task** 를 한 표로
// 들고 종결까지 따라간다. 이 모듈은 그 원장을 오른쪽 칸 두 보기로 가른다:
//   진행   = 스토어(SSE)에서 고른 로봇의 끝나지 않은 Task — 실행 → PLC 대기열 순서 → 나머지.
//   히스토리 = 서버 페이징(`state=terminal`) — 상태·종류·출처·날짜 필터를 쿼리로 옮긴다.
import type { Task, TaskQuery, TaskState, TaskType } from '../types'
import { ACTIVE, TERMINAL } from './state'

/** 진행 보기 — 고른 로봇(`plc`, null = 전부)의 끝나지 않은 Task, 실행 중이 맨 위, 그다음 대기열 슬롯 순. */
export function liveQueue(list: readonly Task[], plc: string | null): Task[] {
  const rank = (t: Task): number =>
    t.state === 'running' ? 0 : t.state === 'queued' ? 1 : t.state === 'lost' ? 3 : 2
  return list
    .filter((t) => ACTIVE.includes(t.state) && (!plc || t.plc_name === plc))
    .sort(
      (a, b) =>
        rank(a) - rank(b) ||
        (a.plc?.queue_index ?? Number.MAX_SAFE_INTEGER) -
          (b.plc?.queue_index ?? Number.MAX_SAFE_INTEGER) ||
        a.seq - b.seq,
    )
}

/** 고른 로봇의 종결 건수 — 바뀌면 히스토리 첫 쪽을 다시 받는다. */
export function terminalCount(list: readonly Task[], plc: string | null): number {
  let n = 0
  for (const t of list) if (TERMINAL.includes(t.state) && (!plc || t.plc_name === plc)) n++
  return n
}

/** PLC 가 실행을 시작한 시각(처음 `running` 으로 간 전이). 없으면 null. */
export function startedAt(task: Pick<Task, 'history'>): string | null {
  return task.history.find((h) => h.to === 'running')?.at ?? null
}

/** 히스토리 필터 — 빈 값 = 걸지 않음. `since` 는 `YYYY-MM-DD`(로컬 날짜). */
export interface HistoryFilter {
  states: readonly TaskState[]
  type: TaskType | ''
  origin: Task['origin'] | ''
  since: string
}

export const EMPTY_HISTORY_FILTER: HistoryFilter = { states: [], type: '', origin: '', since: '' }

/** 걸린 필터 수(버튼 배지). */
export function filterCount(f: HistoryFilter): number {
  return (f.states.length > 0 ? 1 : 0) + (f.type ? 1 : 0) + (f.origin ? 1 : 0) + (f.since ? 1 : 0)
}

/** `YYYY-MM-DD`(로컬 자정) → RFC 3339. 못 읽으면 undefined. */
export function sinceIso(date: string): string | undefined {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date.trim())
  if (!m) return undefined
  const d = new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]))
  return Number.isNaN(d.getTime()) ? undefined : d.toISOString()
}

/** 히스토리 한 쪽의 쿼리 — 상태를 안 고르면 종결 넷 전부. 종결이 아닌 상태는 버린다. */
export function historyQuery(
  f: HistoryFilter,
  robot: number | null,
  offset: number,
  limit: number,
): TaskQuery {
  const states = f.states.filter((s) => TERMINAL.includes(s))
  return {
    robot: robot ?? undefined,
    state: states.length > 0 ? [...states] : 'terminal',
    type: f.type || undefined,
    origin: f.origin || undefined,
    since: sinceIso(f.since),
    offset,
    limit,
  }
}
