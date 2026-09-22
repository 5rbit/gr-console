// 순차 계획 자동 제출 — "로봇 큐가 N 개 이하이면 다음 스텝을 보낸다"의 판단만(순수, 테스트 가능).
//
// 로봇 큐 = 그 로봇(상태 PLC) 원장에서 **아직 끝나지 않고 PLC 로 간** Task 수: 제출됨·수락·대기·실행 중.
// 제출됨(submitted)을 세는 이유: PLC 가 아직 받았다고 말하기 전에도 그 한 건은 이미 큐로 가는 중이다 — 빼면
// 응답을 기다리는 사이 같은 조건으로 몇 건을 더 밀어 넣는다. 초안·유실(lost)은 세지 않는다.
import type { Task, TaskState } from '../types'

export const QUEUE_STATES: readonly TaskState[] = ['submitted', 'accepted', 'queued', 'running']

/** 로봇 큐에 둘 수 있는 한도의 범위 — PLC Task 버퍼(4) 안에서. */
export const AUTO_LIMIT_MIN = 0
export const AUTO_LIMIT_MAX = 4
export const AUTO_LIMIT_DEFAULT = 1

export function robotQueueCount(
  tasks: readonly Pick<Task, 'plc_name' | 'state'>[],
  plc: string | null,
): number {
  if (!plc) return 0
  return tasks.filter((t) => t.plc_name === plc && QUEUE_STATES.includes(t.state)).length
}

/** 자동 제출의 에코 대기 — 직전 Task 가 없거나 제출됨, 또는 그 로봇에 제출됨(에코 대기) Task 가 있다. */
export function awaitingEcho(
  tasks: readonly Pick<Task, 'id' | 'plc_name' | 'state'>[],
  lastId: string | null,
  plc: string | null,
): boolean {
  if (plc && tasks.some((t) => t.plc_name === plc && t.state === 'submitted')) return true
  if (lastId === null) return false
  const t = tasks.find((x) => x.id === lastId)
  return !t || t.state === 'submitted' || t.state === 'draft'
}

export function clampLimit(v: number): number {
  if (!Number.isFinite(v)) return AUTO_LIMIT_DEFAULT
  return Math.min(AUTO_LIMIT_MAX, Math.max(AUTO_LIMIT_MIN, Math.round(v)))
}

export interface AutoInput {
  on: boolean
  busy: boolean
  /** 남은 스텝 수. */
  remaining: number
  /** 다음 스텝이 갈 로봇의 큐. */
  queue: number
  limit: number
  /** 게이트(카드 로봇) — 닫혔으면 사유. */
  gateOk: boolean
  gateReason?: string
  /**
   * 직전에 보낸 Task 가 아직 원장(SSE)에 없거나 **제출됨(에코 대기)** 이다. 서버 게이트가 에코 대기 중 제출을
   * 막으므로(`ops::gate`) 그 사이에 보내면 409 로 멈춘다 — 에코가 올 때까지 기다린다.
   */
  awaitingEcho: boolean
}

export type AutoDecision = { go: true } | { go: false; wait: string } | { go: false; done: true }

/** 지금 한 건을 보낼지. 보내지 않으면 기다리는 이유(화면 한 줄) 또는 끝. */
export function autoDecision(i: AutoInput): AutoDecision {
  if (!i.on) return { go: false, wait: '꺼짐' }
  if (i.remaining === 0) return { go: false, done: true }
  if (i.busy) return { go: false, wait: '제출 중' }
  if (i.awaitingEcho) return { go: false, wait: '직전 제출 에코 대기' }
  if (!i.gateOk)
    return { go: false, wait: i.gateReason ? `게이트 — ${i.gateReason}` : '게이트 닫힘' }
  if (i.queue > i.limit) return { go: false, wait: `큐 ${i.queue} > ${i.limit}` }
  return { go: true }
}
