// 결정 탭 · 시뮬레이션 대화상자의 순수 도우미 — 상태 정규화 · 출처 묶기 · 후보 대상 한 줄.
import type { Target } from '../types'
import { EMPTY_INPUTS, type GenCandidate, type GenMetrics, type GenTerm } from '../taskgen'
import type { DerivedRuleStatus, RuleOrigin, SchedState, SimResult } from '../sched'

export const ORIGIN_ORDER: readonly RuleOrigin[] = ['request', 'policy', 'user']

export const ORIGIN_LABEL: Record<RuleOrigin, string> = {
  request: '요청',
  policy: '정책',
  user: '사용자',
}

const EMPTY_METRICS: GenMetrics = {
  started_at: '',
  generated: 0,
  issued: 0,
  completed_items: 0,
  aborted: 0,
  submit_failures: 0,
  waits: {},
}

/**
 * 새 필드(`derived` · `cooldowns` · `hand_alerts` · `requests`)가 없는 백엔드에서도 화면이 서게 —
 * 배열은 빈 것으로 채운다. 설정의 `policy` 는 정책 탭이 `mergePolicy` 로 채운다.
 */
export function normalizeState(s: SchedState | null | undefined): SchedState | null {
  if (!s) return null
  return {
    ...s,
    rules: s.rules ?? [],
    inputs: s.inputs ?? EMPTY_INPUTS,
    candidates: s.candidates ?? [],
    skipped: s.skipped ?? [],
    queue: s.queue ?? [],
    metrics: { ...EMPTY_METRICS, ...(s.metrics ?? {}), waits: s.metrics?.waits ?? {} },
    derived: s.derived ?? [],
    cooldowns: s.cooldowns ?? [],
    hand_alerts: s.hand_alerts ?? [],
    requests: s.requests ?? [],
  }
}

/** 파생 규칙을 요청 → 정책 → 사용자 순으로, 같은 출처 안에서는 이름 순. */
export function sortDerived(rows: readonly DerivedRuleStatus[]): DerivedRuleStatus[] {
  const rank = (o: RuleOrigin) => {
    const i = ORIGIN_ORDER.indexOf(o)
    return i < 0 ? ORIGIN_ORDER.length : i
  }
  return [...rows].sort(
    (a, b) => rank(a.origin) - rank(b.origin) || (a.name ?? '').localeCompare(b.name ?? ''),
  )
}

export function originCounts(rows: readonly DerivedRuleStatus[]): Record<RuleOrigin, number> {
  const out: Record<RuleOrigin, number> = { request: 0, policy: 0, user: 0 }
  for (const r of rows) if (r.origin in out) out[r.origin]++
  return out
}

/** 후보의 출처 — 파생 목록에서 찾고, 없으면 사용자 규칙. */
export function originOf(ruleId: string, derived: readonly DerivedRuleStatus[]): RuleOrigin {
  return derived.find((d) => d.rule_id === ruleId)?.origin ?? 'user'
}

export function targetText(t: Target | null | undefined): string {
  if (!t) return ''
  return `${t.kind === 'station' ? 'Station' : 'Cell'} ${t.id}`
}

/** 후보 대상 한 줄 — `Station 2101 → Cell 401`(짝이 아니면 첫 대상만). */
export function candTargets(c: Pick<GenCandidate, 'first' | 'second'>): string {
  const a = targetText(c.first)
  const b = targetText(c.second)
  if (a && b) return `${a} → ${b}`
  return a || b || '—'
}

/** 조건 충족 수 `3/4` + 툴팁 줄. */
export function termsSummary(terms: readonly GenTerm[] | null | undefined): {
  label: string
  ok: boolean
  title: string
} {
  const t = terms ?? []
  const n = t.filter((x) => x.ok).length
  return {
    label: t.length ? `${n}/${t.length}` : '—',
    ok: n === t.length,
    title: t.map((x) => `${x.ok ? '충족' : '아직'} · ${x.label}: ${x.value}`).join('\n'),
  }
}

/** 시뮬레이션 요약 — 파생 · 후보 · 이번 생성 · 못 됨. */
export function simSummary(r: SimResult | null | undefined): {
  derived: number
  candidates: number
  generate: number
  skipped: number
} {
  const cands = r?.candidates ?? []
  return {
    derived: r?.derived?.length ?? 0,
    candidates: cands.length,
    generate: cands.filter((c) => !c.reason).length,
    skipped: r?.skipped?.length ?? 0,
  }
}

/** 못 된 규칙 한 줄 + 전체 툴팁. */
export function skippedLine(skipped: readonly { rule: string; reason: string }[] | null | undefined): {
  text: string
  title: string
} | null {
  const s = skipped ?? []
  if (!s.length) return null
  return {
    text: `후보 못 됨 ${s.length}건 — ${s[0].rule}: ${s[0].reason}`,
    title: s.map((x) => `${x.rule}: ${x.reason}`).join('\n'),
  }
}
