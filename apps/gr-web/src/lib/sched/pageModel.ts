// 스케줄러 화면 셸 · 작업 명령 화면 요약 띠의 순수 도우미.
import type { GenCandidate } from '../taskgen'
import { targetText } from './requestsModel'

export type SchedTab = 'requests' | 'demand' | 'decision' | 'policy' | 'history'

export const SCHED_TABS: readonly { id: SchedTab; label: string }[] = [
  { id: 'requests', label: '요청' },
  { id: 'demand', label: '수요' },
  { id: 'decision', label: '결정' },
  { id: 'policy', label: '정책' },
  { id: 'history', label: '기록' },
]

export const SCHED_TAB_KEY = 'gr-sched-tab'

/** 저장된 값 → 탭(모르는 값이면 요청). */
export function parseSchedTab(s: string | null | undefined): SchedTab {
  return SCHED_TABS.find((t) => t.id === s)?.id ?? 'requests'
}

/** 다음 후보 — 이번에 만드는 것 먼저, 그다음 점수 큰 순. */
export function nextCandidates(cands: readonly GenCandidate[] | undefined, n = 3): GenCandidate[] {
  return [...(cands ?? [])]
    .sort((a, b) => Number(!!a.reason) - Number(!!b.reason) || b.score - a.score)
    .slice(0, n)
}

/** 후보 경로 한 줄: `PICK Station 2101 → DROP Cell 401`(대상이 없으면 규칙 이름). */
export function candidateRoute(c: Pick<GenCandidate, 'first' | 'second' | 'rule_name'>): string {
  if (!c.first) return c.rule_name
  if (!c.second) return `${targetText(c.first)} · ${c.rule_name}`
  return `PICK ${targetText(c.first)} → DROP ${targetText(c.second)}`
}
