// Task 상세 이력 — 원장 상태 전이와 PLC 이벤트를 시각 순 한 줄로.
import type { TaskTransition } from '../types'
import type { EventRow } from './api'

export type TimelineEntry =
  { kind: 'state'; at: number; h: TaskTransition } | { kind: 'evt'; at: number; e: EventRow }

/** 시각 오름차순. 같은 시각이면 상태 전이가 먼저(원장이 원인, 이벤트가 결과로 읽힌다). 시각을 못 읽는 전이는 제 자리(앞 항목 시각)를 지킨다. */
export function mergeTimeline(
  history: readonly TaskTransition[],
  events: readonly EventRow[],
): TimelineEntry[] {
  let last = -Infinity
  const states: TimelineEntry[] = history.map((h) => {
    const t = Date.parse(h.at)
    if (!Number.isNaN(t)) last = t
    return { kind: 'state', at: Number.isNaN(t) ? last : t, h }
  })
  const seen = new Set<number>()
  const evts: TimelineEntry[] = []
  for (const e of events) {
    if (seen.has(e.id)) continue
    seen.add(e.id)
    evts.push({ kind: 'evt', at: e.ts_ms, e })
  }
  const all = [...states, ...evts]
  const order = new Map(all.map((x, i) => [x, i]))
  return all.sort(
    (x, y) =>
      x.at - y.at ||
      (x.kind === y.kind ? (order.get(x) ?? 0) - (order.get(y) ?? 0) : x.kind === 'state' ? -1 : 1),
  )
}
