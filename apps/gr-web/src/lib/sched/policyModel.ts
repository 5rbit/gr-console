// 정책 탭의 순수 도우미 — 정책 기본값 병합 · 초안 비교 · 검증, 스테이션 프로파일 초안 · 추정 비교.
import type { CellPick, GenConfig } from '../taskgen'
import {
  DEFAULT_POLICY,
  DROP_MODE_LABEL,
  ROLE_LABEL,
  type DropMode,
  type Policy,
  type StationProfile,
  type StationProfileRow,
  type StationRole,
} from '../sched'

// ── 정책 ─────────────────────────────────────────────────────────────

/** 백엔드가 정책을 빼먹거나 일부만 보내도 기본값으로 채운다(셀 고르기도 필드 단위로). */
export function mergePolicy(p?: Partial<Policy> | null): Policy {
  const src = p ?? {}
  return {
    ...DEFAULT_POLICY,
    ...src,
    inbound_dest: { ...DEFAULT_POLICY.inbound_dest, ...(src.inbound_dest ?? {}) },
    outbound_source: { ...DEFAULT_POLICY.outbound_source, ...(src.outbound_source ?? {}) },
  }
}

/** 저장 · 시뮬레이션에 넘길 설정 — 정책이 늘 붙어 있게. */
export function withPolicy(
  cfg: GenConfig & { policy?: Partial<Policy> | null },
  policy?: Policy,
): GenConfig & { policy: Policy } {
  return { ...cfg, rules: cfg.rules ?? [], policy: policy ?? mergePolicy(cfg.policy) }
}

/** `null`·`undefined`·빈 문자열을 같은 "없음" 으로 본다 — 입력 칸을 비운 것과 필드가 없는 것은 같다. */
function norm(v: unknown): unknown {
  return v === undefined || v === null || v === '' ? null : v
}

export function samePick(a: CellPick | null | undefined, b: CellPick | null | undefined): boolean {
  const x = (a ?? {}) as Record<string, unknown>
  const y = (b ?? {}) as Record<string, unknown>
  const keys = new Set([...Object.keys(x), ...Object.keys(y)])
  for (const k of keys) {
    const vx = norm(x[k])
    const vy = norm(y[k])
    // same_item_first 는 false 와 없음이 같다.
    if (k === 'same_item_first' ? !!vx !== !!vy : vx !== vy) return false
  }
  return true
}

/** 바뀐 필드 이름(정책 키 순서). */
export function policyDiff(base: Policy, draft: Policy): (keyof Policy)[] {
  return (Object.keys(DEFAULT_POLICY) as (keyof Policy)[]).filter((k) =>
    k === 'inbound_dest' || k === 'outbound_source'
      ? !samePick(base[k], draft[k])
      : base[k] !== draft[k],
  )
}

const PRIORITY_KEYS = [
  'inbound_priority',
  'outbound_priority',
  'measure_priority',
  'request_priority',
  'consolidate_priority',
  'merge_priority',
] as const

/** 저장을 막는 사유(없으면 undefined). */
export function validatePolicy(p: Policy): string | undefined {
  for (const k of PRIORITY_KEYS)
    if (!Number.isFinite(p[k])) return `${pascal(k)} 가 숫자가 아닙니다`
  if (!Number.isFinite(p.consolidate_idle_s) || p.consolidate_idle_s < 0)
    return 'ConsolidateIdleS 는 0 이상이어야 합니다'
  if (!Number.isInteger(p.multi_pick_max) || p.multi_pick_max < 2 || p.multi_pick_max > 3)
    return 'MultiPickMax 는 2 ~ 3'
  for (const k of ['inbound_dest', 'outbound_source'] as const) {
    const c = p[k]
    if (c.row_min != null && c.row_max != null && c.row_min > c.row_max)
      return `${pascal(k)}: RowMin > RowMax`
    if (c.col_min != null && c.col_max != null && c.col_min > c.col_max)
      return `${pascal(k)}: ColMin > ColMax`
  }
  return undefined
}

/** `inbound_priority` → `InboundPriority` — 화면 라벨은 멤버 이름 그대로. */
export function pascal(k: string): string {
  return k
    .split('_')
    .map((w) => (w ? w[0].toUpperCase() + w.slice(1) : w))
    .join('')
}

/** 셀 고르기 한 줄 — `Section 2 · Row 1..3 · Col ..5 · oldest`. */
export function cellPickText(c: CellPick | null | undefined): string {
  if (!c) return '—'
  const range = (lo?: number | null, hi?: number | null) =>
    lo == null && hi == null ? '' : `${lo ?? ''}..${hi ?? ''}`
  const parts = [
    c.section != null ? `Section ${c.section}` : '',
    range(c.row_min, c.row_max) ? `Row ${range(c.row_min, c.row_max)}` : '',
    range(c.col_min, c.col_max) ? `Col ${range(c.col_min, c.col_max)}` : '',
    c.near != null ? `Near ${c.near}` : '',
    c.same_item_first ? 'SameItemFirst' : '',
    c.order ?? '',
  ].filter(Boolean)
  return parts.length ? parts.join(' · ') : 'All'
}

/** 409 — 다른 곳에서 먼저 저장해 버전이 어긋났다. */
export function isConflict(e: unknown): boolean {
  const m = e instanceof Error ? e.message : String(e)
  return /→ 409\b/.test(m) || /\bconflict\b/i.test(m)
}

// ── 스테이션 프로파일 ────────────────────────────────────────────────

/** 표 한 줄의 편집 초안 — `role: null` 이면 프로파일 없음(저장하면 지운다). */
export interface ProfileDraft {
  role: StationRole | null
  drop_mode: DropMode
  max_height_mm: number
  weight: number
  max_wait_s: number
  merge_into: number | null
}

export function profileDraftOf(row: StationProfileRow): ProfileDraft {
  const p = row.profile
  if (p)
    return {
      role: p.role,
      drop_mode: p.drop_mode,
      max_height_mm: p.max_height_mm,
      weight: p.weight,
      max_wait_s: p.max_wait_s,
      merge_into: p.merge_into ?? null,
    }
  return {
    role: null,
    drop_mode: 'single',
    max_height_mm: 0,
    weight: 0,
    max_wait_s: 0,
    merge_into: null,
  }
}

/** 초안이 저장된 값과 다른가. 프로파일이 없고 역할도 비운 채면 나머지 칸은 보지 않는다. */
export function profileDirty(row: StationProfileRow, d: ProfileDraft): boolean {
  const b = profileDraftOf(row)
  if (b.role === null && d.role === null) return false
  return (
    b.role !== d.role ||
    b.drop_mode !== d.drop_mode ||
    b.max_height_mm !== d.max_height_mm ||
    b.weight !== d.weight ||
    b.max_wait_s !== d.max_wait_s ||
    b.merge_into !== d.merge_into
  )
}

/** 역할을 바꾼다 — 프로파일 없던 줄에 처음 역할을 주면 나머지 칸은 추정값으로 채운다. */
export function withRole(
  row: StationProfileRow,
  d: ProfileDraft,
  role: StationRole | null,
): ProfileDraft {
  if (d.role === null && role !== null && row.guess)
    return {
      role,
      drop_mode: row.guess.drop_mode,
      max_height_mm: row.guess.max_height_mm ?? 0,
      weight: row.guess.weight ?? 0,
      max_wait_s: row.guess.max_wait_s ?? 0,
      merge_into: null,
    }
  return { ...d, role }
}

/** 역할이 DROP 을 하나(DropMode 가 뜻이 있나). */
export function dropsHere(role: StationRole | null): boolean {
  return role === 'drop' || role === 'both'
}

export function validateProfile(d: ProfileDraft): string | undefined {
  if (d.role === null) return undefined
  if (!Number.isFinite(d.max_height_mm) || d.max_height_mm < 0) return 'MaxHeight 는 0 이상'
  if (!Number.isFinite(d.weight)) return 'Weight 가 숫자가 아닙니다'
  if (!Number.isFinite(d.max_wait_s) || d.max_wait_s < 0) return 'MaxWait 는 0 이상'
  return undefined
}

/** `PUT /api/taskgen/stations/{id}` 본문. */
export function profilePayload(
  d: ProfileDraft,
  note = '',
): Omit<StationProfile, 'station_id' | 'updated_at'> | { role: null } {
  if (d.role === null) return { role: null }
  return {
    role: d.role,
    drop_mode: d.drop_mode,
    max_height_mm: d.max_height_mm,
    weight: d.weight,
    max_wait_s: d.max_wait_s,
    merge_into: d.merge_into,
    note,
  }
}

/**
 * 추정과 저장값 비교 — 프로파일이 없거나 역할 · 방식이 다르면 추정을 보인다.
 * `null` = 보일 것 없음(저장값이 추정과 같다).
 */
export function guessHint(row: StationProfileRow): { text: string; title: string } | null {
  const g = row.guess
  if (!g) return null
  const p = row.profile
  if (p && p.role === g.role && p.drop_mode === g.drop_mode) return null
  return {
    text: `추정: ${g.role}/${g.drop_mode}`,
    title: `${ROLE_LABEL[g.role] ?? g.role} · ${DROP_MODE_LABEL[g.drop_mode] ?? g.drop_mode}${row.guess_why ? ` — ${row.guess_why}` : ''}`,
  }
}

/** 이관 계획 요약 줄(확인 대화상자). */
export function migratePlanLines(plan: {
  profiles?: StationProfile[] | null
  disabled_rules?: string[] | null
}): { profiles: string[]; rules: string[] } {
  return {
    profiles: (plan.profiles ?? []).map(
      (p) =>
        `Station ${p.station_id}: ${p.role}/${p.drop_mode}${p.max_height_mm ? ` · MaxHeight ${p.max_height_mm} mm` : ''}${p.weight ? ` · Weight ${p.weight}` : ''}`,
    ),
    rules: [...(plan.disabled_rules ?? [])],
  }
}
