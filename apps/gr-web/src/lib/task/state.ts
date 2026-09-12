// Task 상태 규칙 — **순수 모듈**(DOM 무관, 테스트 대상). Task 관리 화면(`components/taskmgr/**`)이 읽는다.
//
// 라벨·톤은 `lib/gr/const.ts`가 소유하므로 여기서는 다시 정의하지 않고 내보내기만 한다. 이 파일이 새로
// 더하는 것은 판단이다: 어느 상태에서 어느 조작이 서는가(`allowedActions`), 원장의 상태가 PLC 배열과
// 맞는가(`deriveState`), 얼마나 걸렸는가(`elapsed`).
import { TASK_TYPE_OF_CODE } from '../gr/const'
import type { PlcTask, Task, TaskState, WebMon } from '../types'

export { STATE_LABEL, STATE_TONE } from '../gr/const'

/** PLC TaskType 코드 → 이름. 표에 없는 코드는 16진수 그대로(모르는 값을 이름으로 꾸미지 않는다). */
export function typeName(code: number | undefined | null): string {
  if (!code) return '-'
  return TASK_TYPE_OF_CODE[code] ?? `0x${code.toString(16).toUpperCase()}`
}

export const ORIGIN_LABEL: Record<Task['origin'], string> = {
  console: '콘솔',
  scenario: '시나리오',
  external: '외부',
}

/** 끝난 상태 — 백엔드 `TaskState::is_terminal`과 같다(`lost`는 되살아날 수 있어 종결이 아니다). */
export const TERMINAL: readonly TaskState[] = ['completed', 'canceled', 'rejected', 'failed']

/** PLC가 아직 들고 있을 수 있는 상태(초안 제외). */
export const ACTIVE: readonly TaskState[] = ['submitted', 'accepted', 'queued', 'running', 'lost']

export const ALL_STATES: readonly TaskState[] = [
  'draft',
  'submitted',
  'accepted',
  'queued',
  'running',
  'completed',
  'canceled',
  'rejected',
  'failed',
  'lost',
]

export function isTerminal(state: TaskState): boolean {
  return TERMINAL.includes(state)
}

// ── 백엔드가 `Task`에 더 싣지만 공유 타입에는 아직 없는 필드 ───────────────────

/** `LGR_Command_Header` — 제출 때 쓴 헤더(에코 대조 키). */
export interface TaskHeader {
  protocol: number
  cmd_id: number
  cmd: number
  src: number
  dst: number
  seq: number
}

/** 백엔드 `LedgerEntry` 전체. `types.ts`의 `Task`에 `header`·`plc_name`이 없어 슬라이스 안에서만 넓힌다. */
export type TaskEx = Task & {
  header?: TaskHeader | null
  plc_name?: string
}

// ── 조작 ─────────────────────────────────────────────────────────────────────

export type TaskAction = 'submit' | 'cancel' | 'complete' | 'resubmit' | 'fail' | 'delete'

export const ACTION_LABEL: Record<TaskAction, string> = {
  submit: '제출',
  cancel: '취소',
  complete: '완료 처리',
  resubmit: '재제출',
  fail: '실패로 표시',
  delete: '기록 삭제',
}

/**
 * 상태별로 서는 조작. 백엔드 `ops.rs`의 허용 표와 같다:
 *  - 초안: 제출 · 취소(폐기) · 삭제
 *  - 제출됨/수락됨/대기/실행 중/유실: 취소 · 완료 처리 · 실패로 표시 (유실은 재제출도)
 *  - 종결: 재제출 · 삭제
 * 제출됨은 아직 PLC에 자리가 없어 완료 처리가 서지 않는다.
 */
export function allowedActions(state: TaskState): TaskAction[] {
  switch (state) {
    case 'draft':
      return ['submit', 'cancel', 'delete']
    case 'submitted':
      return ['cancel', 'fail']
    case 'accepted':
    case 'queued':
    case 'running':
      return ['cancel', 'complete', 'fail']
    case 'lost':
      return ['cancel', 'complete', 'resubmit', 'fail']
    case 'completed':
    case 'canceled':
    case 'rejected':
    case 'failed':
      return ['resubmit', 'delete']
  }
}

/** 실장비에 물리적 결과가 있는 조작 — ConfirmDialog scope `single-robot`. */
export function isRobotAction(a: TaskAction): boolean {
  return a === 'cancel' || a === 'complete'
}

// ── PLC 배열과의 대조 ─────────────────────────────────────────────────────────

export type PlcLocation = 'now' | 'queue' | 'completed' | 'canceled' | 'rejected' | 'absent'

export interface Derived {
  /** PLC 배열에서 찾은 자리. */
  location: PlcLocation
  /** 배열 안 인덱스(Now는 0, 없으면 null). */
  index: number | null
  /** 원장 상태와 PLC 자리가 어긋난다 — 배지 "PLC와 불일치". */
  mismatch: boolean
  /** 어긋남의 이유(툴팁). */
  reason: string | null
}

/** `WebMon.Stat.Task` 모양의 부분집합 — 테스트에서 통째로 만들지 않아도 되게. */
export type PlcTaskArea = Pick<
  WebMon['Stat']['Task'],
  'Now' | 'Queue' | 'Completed' | 'Canceled' | 'Rejected'
>

function isZero(t: PlcTask | undefined | null): boolean {
  return !t || (t.WorkId === 0 && t.TaskId === 0 && t.TaskType === 0)
}

function find(arr: PlcTask[] | undefined, work: number, task: number): number | null {
  if (!arr) return null
  const i = arr.findIndex((t) => !isZero(t) && t.WorkId === work && t.TaskId === task)
  return i < 0 ? null : i
}

/** Task 키를 PLC 배열에서 찾는다. */
export function locate(
  task: Pick<Task, 'work_id' | 'task_id'>,
  area: PlcTaskArea,
): { location: PlcLocation; index: number | null } {
  const { work_id: w, task_id: t } = task
  if (!isZero(area.Now) && area.Now.WorkId === w && area.Now.TaskId === t)
    return { location: 'now', index: 0 }
  const q = find(area.Queue, w, t)
  if (q !== null) return { location: 'queue', index: q }
  const c = find(area.Completed, w, t)
  if (c !== null) return { location: 'completed', index: c }
  const n = find(area.Canceled, w, t)
  if (n !== null) return { location: 'canceled', index: n }
  const r = find(area.Rejected, w, t)
  if (r !== null) return { location: 'rejected', index: r }
  return { location: 'absent', index: null }
}

const LOC_LABEL: Record<PlcLocation, string> = {
  now: 'Now',
  queue: 'Queue',
  completed: 'Completed',
  canceled: 'Canceled',
  rejected: 'Rejected',
  absent: '어디에도 없음',
}

/**
 * 원장 상태 ↔ PLC 배열 일관성. 링은 10건이라 종결 상태가 링에서 밀려난 것(`absent`)은 정상이고,
 * 제출됨/수락됨은 PLC에 아직 없거나 막 들어간 과도기라 어느 자리든 어긋남이 아니다.
 */
export function deriveState(
  task: Pick<Task, 'work_id' | 'task_id' | 'state'>,
  area: PlcTaskArea | null | undefined,
): Derived {
  if (!area) return { location: 'absent', index: null, mismatch: false, reason: null }
  const { location, index } = locate(task, area)
  const bad = (reason: string): Derived => ({ location, index, mismatch: true, reason })
  const ok: Derived = { location, index, mismatch: false, reason: null }
  const at = LOC_LABEL[location]
  switch (task.state) {
    case 'running':
      return location === 'now' ? ok : bad(`원장은 실행 중인데 PLC에서는 ${at}`)
    case 'queued':
      return location === 'queue' ? ok : bad(`원장은 대기인데 PLC에서는 ${at}`)
    case 'completed':
      return location === 'completed' || location === 'absent'
        ? ok
        : bad(`원장은 완료인데 PLC에서는 ${at}`)
    case 'canceled':
      return location === 'canceled' || location === 'absent'
        ? ok
        : bad(`원장은 취소인데 PLC에서는 ${at}`)
    case 'rejected':
      return location === 'rejected' || location === 'absent'
        ? ok
        : bad(`원장은 거부인데 PLC에서는 ${at}`)
    case 'lost':
      return location === 'absent' ? ok : bad(`원장은 유실인데 PLC에서는 ${at}`)
    case 'failed':
    case 'draft':
      return location === 'absent'
        ? ok
        : bad(`원장은 ${task.state === 'draft' ? '초안' : '실패'}인데 PLC에서는 ${at}`)
    case 'submitted':
    case 'accepted':
      return ok
  }
}

// ── 시간 ─────────────────────────────────────────────────────────────────────

/** 시작(제출, 없으면 생성)부터 끝(종결, 진행 중이면 `now`)까지 초. 시각을 못 읽으면 `null`. */
export function elapsed(
  task: Pick<Task, 'created_at' | 'submitted_at' | 'ended_at' | 'state'>,
  now: number = Date.now(),
): number | null {
  const start = Date.parse(task.submitted_at ?? task.created_at)
  if (Number.isNaN(start)) return null
  const endStr = task.ended_at
  const end = endStr ? Date.parse(endStr) : isTerminal(task.state) ? NaN : now
  if (Number.isNaN(end)) return null
  return Math.max(0, (end - start) / 1000)
}

/** 초 → `12.3s` / `1m 05s` / `2h 03m`. */
export function fmtElapsed(sec: number | null): string {
  if (sec === null) return ''
  if (sec < 60) return `${sec.toFixed(1)}s`
  const m = Math.floor(sec / 60)
  const s = Math.floor(sec % 60)
  if (m < 60) return `${m}m ${String(s).padStart(2, '0')}s`
  const h = Math.floor(m / 60)
  return `${h}h ${String(m % 60).padStart(2, '0')}m`
}

/** `HH:MM:SS`(오늘) 또는 `MM-DD HH:MM`(다른 날). 못 읽으면 원문. */
export function fmtTime(iso: string | null | undefined, now: number = Date.now()): string {
  if (!iso) return ''
  const t = Date.parse(iso)
  if (Number.isNaN(t)) return iso
  const d = new Date(t)
  const n = new Date(now)
  const pad = (x: number) => String(x).padStart(2, '0')
  const hms = `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
  if (
    d.getFullYear() === n.getFullYear() &&
    d.getMonth() === n.getMonth() &&
    d.getDate() === n.getDate()
  )
    return hms
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${hms.slice(0, 5)}`
}

/** 대상 표시 — `request.target`이 있으면 그것, 없으면(외부 Task) PLC `Cell.Id`로 셀/스테이션을 가른다. */
export function targetOf(
  task: Pick<Task, 'request' | 'plc_task'>,
): { kind: 'cell' | 'station'; id: number } | null {
  const t = task.request?.target
  if (t) return { kind: t.kind, id: t.id }
  const id = task.plc_task?.Cell?.Id ?? 0
  if (!id) return null
  return { kind: isStationId(id) ? 'station' : 'cell', id }
}

/** 스테이션 Id 규칙 — 백엔드 `gr_proto::is_station_id`: 2001..2999 이고 `id MOD 100 ∈ 1..32`. */
export function isStationId(id: number): boolean {
  return id >= 2001 && id <= 2999 && id % 100 >= 1 && id % 100 <= 32
}

/** 오늘 종결된 Task인가(로컬 날짜 기준). */
export function endedToday(task: Pick<Task, 'ended_at'>, now: number = Date.now()): boolean {
  if (!task.ended_at) return false
  const t = Date.parse(task.ended_at)
  if (Number.isNaN(t)) return false
  const a = new Date(t)
  const b = new Date(now)
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  )
}

/** 필터 — 화면의 검색어와 상태·종류 선택을 한 번에 건다(스토어 목록에 클라이언트 측으로). */
export interface TaskFilter {
  states: readonly TaskState[]
  type: string
  q: string
  includeTerminal: boolean
}

export const EMPTY_FILTER: TaskFilter = { states: [], type: '', q: '', includeTerminal: false }

export function matchesFilter(
  task: Task,
  f: TaskFilter,
  typeName: (code: number) => string,
): boolean {
  if (f.states.length > 0) {
    if (!f.states.includes(task.state)) return false
  } else if (!f.includeTerminal && isTerminal(task.state)) return false
  if (f.type && typeName(task.plc_task?.TaskType ?? 0) !== f.type) return false
  const q = f.q.trim().toLowerCase()
  if (q) {
    const hay = [
      String(task.seq),
      String(task.work_id),
      String(task.task_id),
      String(task.plc_task?.Cell?.Id ?? ''),
      String(task.plc_task?.Item?.Code ?? ''),
      task.request?.note ?? '',
      task.error ?? '',
      task.ack?.reason ?? '',
    ]
      .join(' ')
      .toLowerCase()
    if (!hay.includes(q)) return false
  }
  return true
}
