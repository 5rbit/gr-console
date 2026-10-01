// 스케줄러 화면 셸 · 작업 명령 화면 요약 띠의 순수 도우미.
import type { GenCandidate } from '../taskgen'
import { targetText } from './requestsModel'

export type SchedTab = 'jobs' | 'rules' | 'sets' | 'requests' | 'demand' | 'decision' | 'history'

export const SCHED_TABS: readonly { id: SchedTab; label: string }[] = [
  { id: 'jobs', label: '작업' },
  { id: 'rules', label: '규칙' },
  { id: 'sets', label: '규칙 세트' },
  { id: 'requests', label: '요청' },
  { id: 'demand', label: '스테이션' },
  { id: 'decision', label: '판단' },
  { id: 'history', label: '기록' },
]

export const SCHED_TAB_KEY = 'gr-sched-tab'

/** 저장된 값 → 탭(모르는 값이면 작업). */
export function parseSchedTab(s: string | null | undefined): SchedTab {
  // 옛 정책 탭은 규칙 탭으로(공통 정책 섹션이 그 아래에 있다).
  if (s === 'policy') return 'rules'
  return SCHED_TABS.find((t) => t.id === s)?.id ?? 'jobs'
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
