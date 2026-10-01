// Task 스케줄러 — 백엔드 `taskgen`(정책 · 요청 · 준비 상태 · 기록)의 타입과 API. 규칙·후보 타입은 `taskgen.ts`.
// 계약 문서: docs/scheduler.md
import { getJson, postJson, putJson, patchJson, del } from './api'
import type { Target } from './types'
import type { CellPick, GenConfig, GenRuleStatus, GenState } from './taskgen'

// ── 스테이션 프로파일 ────────────────────────────────────────────────

/** 로봇이 이 스테이션에서 하는 일 — PICK(입고) · DROP(출고) · 둘 다 · 끔. */
export type StationRole = 'pick' | 'drop' | 'both' | 'off'
/** DROP 방식 — 하나씩(비어 있어야) · 스택(스테이션 최대 높이 + 품목 StackMax) · 팔렛(다음 슬롯). */
export type DropMode = 'single' | 'stack' | 'pallet'

export interface StationProfile {
  station_id: number
  role: StationRole
  drop_mode: DropMode
  /** 스택 스테이션 최대 높이(mm, 0 = 제한 없음). 단수 한도는 품목 규격 StackMax 가 따로 건다. */
  max_height_mm: number
  /** 이 스테이션 수요에 더하는 점수. */
  weight: number
  /** 준비된 뒤 이만큼(초) 못 만들면 경고(0 = 없음). */
  max_wait_s: number
  /** 멀티 피킹: 이 PICK 스테이션의 타이어를 이 스테이션 타이어 위 2단으로 합친다(예: 2102 → 2101). */
  merge_into?: number | null
  note: string
  updated_at: string
}

export interface StationProfileRow {
  id: number
  /** 등록된 스테이션 이름 · 컨베이어 번호(레지스트리). */
  conv_no: number
  /** 저장된 프로파일(없으면 정책이 이 스테이션을 보지 않는다). */
  profile: StationProfile | null
  /** GRM 연결로 추정한 값(저장 안 됨) + 근거. */
  guess: StationProfile
  guess_why: string
  pallet: boolean
}

export const ROLE_LABEL: Record<StationRole, string> = {
  pick: 'PICK (입고)',
  drop: 'DROP (출고)',
  both: 'PICK · DROP',
  off: '끔',
}

export const DROP_MODE_LABEL: Record<DropMode, string> = {
  single: '하나씩',
  stack: '스택',
  pallet: '팔렛',
}

// ── 준비 상태 ────────────────────────────────────────────────────────

export interface Ready {
  ok: boolean
  /** 안 되는 이유(또는 참고) — 한 줄. */
  why: string | null
  /** 스테이션 신호 조건식(`CVOK & Req & ItemExist & CVNO 0 & Comp 0`) — 항목마다 지금 충족하나. GRM 을 못 읽으면 없음. */
  expr?: Cond[]
}

/** 조건식 한 항목 — `neg` 면 꺼져 있어야 충족(`CVNO 0`). */
export interface Cond {
  name: string
  neg: boolean
  ok: boolean
}

/** 이 대상을 쓰는 작업(예약) — 모든 경로(단일 · 순차 · 스케줄러)의 살아 있는 Task · 예정. */
export interface Hold {
  robot: number
  robot_name: string
  op: 'PICK' | 'DROP' | 'MEASURE' | 'MOVE'
  /** 원장 Task 상태(draft · submitted · queued …) 또는 `planned` = 스케줄러 예정 큐(아직 원장에 없음, `task` 없음). */
  state: string
  order: string | null
  task: string | null
}

export interface TargetReady {
  kind: 'cell' | 'station'
  id: number
  pick: Ready
  drop: Ready
  measure: Ready
  /** 미래 재고(진행 중 · 초안 · 예정까지 접은 값). */
  item_code: number
  count: number
  /** 남은 칸(StackMax − 미래 재고, 모르면 null = 제한 없음). */
  room: number | null
  holds: Hold[]
  /** 스테이션: 콘솔이 화물 품목을 모른다 — 요청·규칙이 품목을 주거나 사람이 지정해야 입고한다. */
  need_item: boolean
  /** 스테이션: 트래킹 OD 에 맞는 품목 후보. */
  hints: number[]
  /** 스테이션: PICK 또는 DROP 이 준비된 시각(대기 시간). */
  since: string | null
  /** 스테이션 프로파일 역할(없으면 null). */
  role: StationRole | null
  drop_mode: DropMode | null
}

export interface ReadySnapshot {
  at: string
  targets: TargetReady[]
}

export const EMPTY_READY: ReadySnapshot = { at: '', targets: [] }

// ── 요청 목록 ────────────────────────────────────────────────────────

export type RequestKind = 'inbound' | 'outbound' | 'move'
export type RequestSource = 'host' | 'user'
export type RequestState = 'open' | 'active' | 'done' | 'canceled' | 'failed'

export interface TransferRequest {
  id: string
  seq: number
  source: RequestSource
  ext_ref: string | null
  kind: RequestKind
  item_code: number | null
  qty: number
  /** 이송 지시에서 센 값. */
  done: number
  active: number
  failed: number
  from: Target | null
  to: Target | null
  priority: number
  state: RequestState
  note: string
  created_at: string
  updated_at: string
  ended_at: string | null
  /** 지금 못 도는 이유(엔진이 판정마다 적음, 저장 안 됨). */
  reason: string | null
}

export interface NewRequest {
  kind: RequestKind
  source?: RequestSource
  ext_ref?: string | null
  item_code?: number | null
  qty: number
  from?: Target | null
  to?: Target | null
  priority?: number
  note?: string
}

export const REQUEST_KIND_LABEL: Record<RequestKind, string> = {
  inbound: '입고',
  outbound: '출고',
  move: '이동',
}

export const REQUEST_STATE_LABEL: Record<RequestState, string> = {
  open: '대기',
  active: '진행 중',
  done: '완료',
  canceled: '취소',
  failed: '실패',
}

// ── 정책 ─────────────────────────────────────────────────────────────

export interface Policy {
  inbound_auto: boolean
  inbound_dest: CellPick
  /** 적재 셀을 가장 가까운 DROP 스테이션 기준으로 고른다. */
  inbound_near_drop: boolean
  inbound_priority: number
  outbound_auto: boolean
  outbound_source: CellPick
  outbound_priority: number
  measure_first: boolean
  measure_priority: number
  request_priority: number
  consolidate: boolean
  consolidate_idle_s: number
  consolidate_priority: number
  /** 멀티 피킹 — merge_into 가 있는 PICK 스테이션을 합친 뒤 2개를 한 번에 입고(빈 셀에). */
  multi_pick: boolean
  merge_priority: number
  /** 한 번에 집을 최대 개수(2..3). */
  multi_pick_max: number
}

export const DEFAULT_POLICY: Policy = {
  inbound_auto: true,
  inbound_dest: { order: 'nearest' },
  inbound_near_drop: true,
  inbound_priority: 40,
  outbound_auto: false,
  outbound_source: { order: 'oldest' },
  outbound_priority: 60,
  measure_first: true,
  measure_priority: 80,
  request_priority: 1000,
  consolidate: false,
  consolidate_idle_s: 60,
  consolidate_priority: 5,
  multi_pick: true,
  merge_priority: 1100,
  multi_pick_max: 3,
}

// ── 엔진 상태 확장(GET /api/taskgen 에 실리는 추가 필드) ─────────────

/** 파생 규칙의 출처 — 사용자 규칙 · 정책 수요 · 요청. */
export type RuleOrigin = 'user' | 'policy' | 'request'

export interface DerivedRuleStatus extends GenRuleStatus {
  origin: RuleOrigin
  name: string
  /** 요청에서 나온 규칙이면 요청 id. */
  request_id: string | null
  /** 이 수요가 겨냥하는 스테이션(있으면). */
  station: number | null
}

export interface Cooldown {
  key: string
  fails: number
  until: string
  last_reason: string
}

/** 로봇 Hand 에 화물이 있는데 짝 DROP 이 없다 — 사람이 정리해야 한다. */
export interface HandAlert {
  robot: number
  robot_name: string
  item_code: number
  count: number
  order: string | null
  why: string
}

export interface SchedState extends GenState {
  config: GenConfig & { policy: Policy }
  /** 사용자 규칙 + 파생 규칙(정책 · 요청) 상태 — `origin` 으로 나눈다. */
  derived: DerivedRuleStatus[]
  cooldowns: Cooldown[]
  hand_alerts: HandAlert[]
  /** 열린 요청(open · active) — 요청 탭은 `/api/requests` 를 따로 읽는다. */
  requests: TransferRequest[]
}

export interface SimResult {
  derived: DerivedRuleStatus[]
  candidates: GenState['candidates']
  skipped: GenState['skipped']
}

// ── 기록 · KPI ───────────────────────────────────────────────────────

export interface LogRow {
  id: number
  at: string
  /** generated · issued · completed · aborted · cooldown · request · hand_remove · wait_alert */
  kind: string
  key: string
  robot: number | null
  detail: string
}

export interface Kpi {
  hours: number
  generated: number
  completed: number
  aborted: number
  /** 이송 지시 열림 → 끝남(초) 평균 · p90. */
  cycle_avg_s: number | null
  cycle_p90_s: number | null
  /** 스테이션마다 준비 → 생성까지 기다린 시간(초). */
  stations: {
    id: number
    generated: number
    wait_avg_s: number | null
    wait_max_s: number | null
  }[]
  robots: { id: number; name: string; generated: number; completed: number; aborted: number }[]
  requests: { done: number; open: number; failed: number }
}

// ── API ──────────────────────────────────────────────────────────────

const enc = encodeURIComponent

export const schedApi = {
  get: () => getJson<SchedState>('/api/taskgen'),
  save: (c: GenConfig & { policy: Policy }) => putJson<GenConfig>('/api/taskgen/config', c),
  simulate: (c: GenConfig & { policy: Policy }) =>
    postJson<SimResult>('/api/taskgen/simulate', { config: c }),
  ready: () => getJson<ReadySnapshot>('/api/taskgen/ready'),
  stations: () => getJson<StationProfileRow[]>('/api/taskgen/stations'),
  saveStation: (
    id: number,
    p: Omit<StationProfile, 'station_id' | 'updated_at'> | { role: null },
  ) => putJson<StationProfileRow>(`/api/taskgen/stations/${id}`, p),
  applyGuess: (ids?: number[]) =>
    postJson<{ applied: number[] }>('/api/taskgen/stations/apply-guess', { ids }),
  migrateRules: (dry_run: boolean) =>
    postJson<{ profiles: StationProfile[]; disabled_rules: string[]; policy: Policy }>(
      '/api/taskgen/migrate-rules',
      { dry_run },
    ),
  clearCooldown: (key: string) => del(`/api/taskgen/cooldown/${enc(key)}`),
  /** 규칙 세트 적용 — 세트의 규칙만 켠다 */
  applySet: (id: string) => postJson<unknown>(`/api/taskgen/rule-sets/${enc(id)}/apply`),
  handRemove: (robot: number) =>
    postJson<{ canceled: string[]; order: string | null }>(`/api/taskgen/hand/${robot}/remove`),
  log: (limit = 200, kind?: string) =>
    getJson<LogRow[]>(`/api/taskgen/log?limit=${limit}${kind ? `&kind=${enc(kind)}` : ''}`),
  kpi: (hours = 24) => getJson<Kpi>(`/api/taskgen/kpi?hours=${hours}`),
  requests: (state: 'open' | 'all' = 'open', limit = 200) =>
    getJson<TransferRequest[]>(`/api/requests?state=${state}&limit=${limit}`),
  addRequest: (r: NewRequest) => postJson<TransferRequest>('/api/requests', r),
  patchRequest: (id: string, p: { priority?: number; qty?: number; note?: string }) =>
    patchJson<TransferRequest>(`/api/requests/${enc(id)}`, p),
  cancelRequest: (id: string) => postJson<TransferRequest>(`/api/requests/${enc(id)}/cancel`),
}

// ── 순수 도우미 ──────────────────────────────────────────────────────

/** 요청 진행 한 줄: `3/10 (진행 1)`. */
export function requestProgress(
  r: Pick<TransferRequest, 'qty' | 'done' | 'active' | 'failed'>,
): string {
  const extra = [r.active ? `진행 ${r.active}` : '', r.failed ? `실패 ${r.failed}` : '']
    .filter(Boolean)
    .join(' · ')
  return `${r.done}/${r.qty}${extra ? ` (${extra})` : ''}`
}

/** 요청의 남은 수. */
export function requestRemaining(r: Pick<TransferRequest, 'qty' | 'done' | 'active'>): number {
  return Math.max(0, r.qty - r.done - r.active)
}

/** 대상의 PICK/DROP 표시 — 지도 ▲▼ 와 수요 표가 같이 쓴다. */
export type ReadyMark = 'ready' | 'need_item' | 'held' | 'blocked' | 'none'

export function readyMark(t: TargetReady | undefined, op: 'pick' | 'drop'): ReadyMark {
  if (!t) return 'none'
  const r = t[op]
  if (
    t.holds.some((h) => (op === 'pick' ? h.op === 'PICK' || h.op === 'MEASURE' : h.op === 'DROP'))
  )
    return 'held'
  if (!r.ok) {
    // 셀은 비었거나 가득이면 표시하지 않는다. 스테이션은 역할이 이 일을 할 때만 "막힘" 을 보인다.
    if (t.kind !== 'station' || t.role === null) return 'none'
    const does =
      op === 'pick'
        ? t.role === 'pick' || t.role === 'both'
        : t.role === 'drop' || t.role === 'both'
    return does ? 'blocked' : 'none'
  }
  if (op === 'pick' && t.kind === 'station' && t.need_item) return 'need_item'
  return 'ready'
}

/** 대상 키 — `station:2101` · `cell:401`. */
export function targetKey(t: { kind: string; id: number }): string {
  return `${t.kind}:${t.id}`
}

export function readyIndex(s: ReadySnapshot): Map<string, TargetReady> {
  return new Map(s.targets.map((t) => [targetKey(t), t]))
}

/** 대기 시간 `since` → 초(없으면 null). */
export function waitSeconds(since: string | null, now = Date.now()): number | null {
  if (!since) return null
  const t = Date.parse(since)
  return Number.isFinite(t) ? Math.max(0, Math.round((now - t) / 1000)) : null
}

export function isStationTarget(t: Target | null | undefined): boolean {
  return t?.kind === 'station'
}

export type { GenConfig, GenState }
