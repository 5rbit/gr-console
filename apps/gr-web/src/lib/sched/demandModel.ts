// 스케줄러 수요 탭의 순수 도우미 — 엔진이 지금 보는 스테이션 준비 · 수요 · Hand 경고를 표 행으로.
import type { Status } from '../ui/status'
import { ruleState } from '../taskgen'
import {
  readyMark,
  waitSeconds,
  type Cond,
  type DerivedRuleStatus,
  type HandAlert,
  type Hold,
  type ReadyMark,
  type TargetReady,
} from '../sched'

export const MARK_VIEW: Record<ReadyMark, { label: string; tone: Status }> = {
  ready: { label: '준비', tone: 'ok' },
  need_item: { label: '품목?', tone: 'warn' },
  held: { label: '예약', tone: 'info' },
  blocked: { label: '막힘', tone: 'neutral' },
  none: { label: '—', tone: 'neutral' },
}

/** 예약 한 줄: `GR1 PICK TO-12 · GR2 DROP`. */
export function holdsText(holds: readonly Hold[] | undefined): string {
  return (holds ?? [])
    .map((h) => [h.robot_name || `R${h.robot}`, h.op, h.order ?? ''].filter(Boolean).join(' '))
    .join(' · ')
}

/** 수요 칩 — 이 스테이션을 겨냥한 파생 규칙 중 사람이 먼저 볼 것 하나(+ 나머지 수). */
export interface DemandView {
  label: string
  tone: 'ok' | 'warn' | 'muted'
  title: string
}

const STATE_RANK: Record<DerivedRuleStatus['state'], number> = {
  ready: 0,
  queued: 1,
  busy: 2,
  waiting: 3,
  skipped: 4,
  idle: 5,
  off: 6,
}

export function demandFor(
  station: number,
  derived: readonly DerivedRuleStatus[] | undefined,
  auto: boolean,
): DemandView | null {
  const mine = (derived ?? [])
    .filter((d) => d.station === station)
    .sort((a, b) => (STATE_RANK[a.state] ?? 9) - (STATE_RANK[b.state] ?? 9))
  if (!mine.length) return null
  const top = mine[0]
  const s = ruleState(top, auto)
  const more = mine.length > 1 ? ` +${mine.length - 1}` : ''
  const lines = mine.map((d) => {
    const v = ruleState(d, auto)
    return `${d.name || d.rule_id}: ${v.label}${d.reason ? ` — ${d.reason}` : ''}`
  })
  return { label: `${s.label}${more}`, tone: s.tone, title: lines.join('\n') }
}

const NO: { ok: boolean; why: string | null } = { ok: false, why: null }

/** 백엔드가 아직 안 채운 필드가 있어도 readyMark 가 넘어지지 않게. */
export function normTarget(t: TargetReady): TargetReady {
  return {
    ...t,
    pick: t.pick ?? NO,
    drop: t.drop ?? NO,
    measure: t.measure ?? NO,
    holds: t.holds ?? [],
    hints: t.hints ?? [],
    role: t.role ?? null,
    drop_mode: t.drop_mode ?? null,
  }
}

export interface StationDemandRow {
  id: number
  role: TargetReady['role']
  dropMode: TargetReady['drop_mode']
  pick: ReadyMark
  pickWhy: string
  /** 신호 조건식 — 이 스테이션이 PICK 을 하지 않으면 비움. */
  pickExpr: Cond[]
  drop: ReadyMark
  dropWhy: string
  dropExpr: Cond[]
  itemCode: number
  count: number
  wait: number | null
  holds: string
  demand: DemandView | null
  needItem: boolean
  hints: number[]
}

export function stationRows(
  targets: readonly TargetReady[] | undefined,
  derived: readonly DerivedRuleStatus[] | undefined,
  auto: boolean,
  now = Date.now(),
): StationDemandRow[] {
  return (targets ?? [])
    .filter((t) => t.kind === 'station')
    .map(normTarget)
    .map((t) => ({
      id: t.id,
      role: t.role,
      dropMode: t.drop_mode,
      pick: readyMark(t, 'pick'),
      pickWhy: t.pick?.why ?? '',
      pickExpr: readyMark(t, 'pick') === 'none' ? [] : (t.pick?.expr ?? []),
      drop: readyMark(t, 'drop'),
      dropWhy: t.drop?.why ?? '',
      dropExpr: readyMark(t, 'drop') === 'none' ? [] : (t.drop?.expr ?? []),
      itemCode: t.item_code ?? 0,
      count: t.count ?? 0,
      wait: waitSeconds(t.since ?? null, now),
      holds: holdsText(t.holds),
      demand: demandFor(t.id, derived, auto),
      needItem: !!t.need_item,
      hints: t.hints,
    }))
    .sort((a, b) => a.id - b.id)
}

/** 셀 요약 한 줄의 수 — PICK 준비 · DROP 준비 · 예약. */
export function cellSummary(targets: readonly TargetReady[] | undefined): {
  total: number
  pick: number
  drop: number
  held: number
} {
  const cells = (targets ?? []).filter((t) => t.kind === 'cell').map(normTarget)
  return {
    total: cells.length,
    pick: cells.filter((t) => readyMark(t, 'pick') === 'ready').length,
    drop: cells.filter((t) => readyMark(t, 'drop') === 'ready').length,
    held: cells.filter((t) => t.holds.length > 0).length,
  }
}

/** Hand 경고 한 줄: `GR2 Hand 1203 × 1 — 짝 DROP 없음`. */
export function handAlertText(a: HandAlert): string {
  return `${a.robot_name || `R${a.robot}`} Hand ${a.item_code} × ${a.count}${a.why ? ` — ${a.why}` : ''}`
}

/** 대기 초 → `42` · `3:05` · `1:02:03`. */
export function waitText(s: number | null): string {
  if (s === null) return ''
  if (s < 60) return String(s)
  const p = (n: number) => String(n).padStart(2, '0')
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  return h ? `${h}:${p(m)}:${p(s % 60)}` : `${m}:${p(s % 60)}`
}
