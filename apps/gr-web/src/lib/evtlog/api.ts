// 이벤트 로그 슬라이스 API — `/api/events/*` · `/api/plc/{plc}/evtlog/cfg`.
//
// PLC 의 EVTLOG 링을 콘솔이 긁어 SQLite 에 쌓고, 콘솔 자신의 사건도 같은 표에 `origin: 'console'` 로 남긴다.
// 문구(`text`)는 서버가 카탈로그로 렌더한다 — 화면은 다시 조립하지 않는다.
import { del, getJson, postJson, putJson } from '../api'

export interface EventRow {
  id: number
  /** 설정 이름('GR2' · 'GRM' …) 또는 'console'. */
  plc: string
  origin: 'plc' | 'console'
  epoch: number
  seq: number | null
  /** 'YYYY-MM-DD HH:MM:SS.mmm' 로컬 — PLC 행은 PLC 시계다. */
  ts: string
  ts_ms: number
  /** 콘솔 수신 시각(같은 형식). */
  rx_ts: string
  cat: number
  cat_name: string
  lvl: number
  /** TRACE DEBUG INFO WARN ERROR */
  lvl_name: string
  src: number
  code: number
  name: string | null
  a: number
  b: number
  ctx: number
  detail: string | null
  text: string
}

export interface EventPage {
  /** 최신 먼저. */
  rows: EventRow[]
  /** 다음 쪽 커서(불투명) — 없으면 끝. */
  next_before: string | null
  scanned: number
}

export interface EvtLevel {
  id: number
  name: string
}

export interface EvtCat {
  id: number
  name: string
}

export interface EvtCatalogEvent {
  code: number | null
  name: string
  cat: number
  cat_name: string
  lvl: number
  text: string
  plc?: string[]
  console: boolean
  a?: string
  b?: string
}

export interface EvtCatalog {
  version: number | string
  capacity: number
  min_level: number
  max_per_scan: number
  cat_mask_all: number
  levels: EvtLevel[]
  cats: EvtCat[]
  events: EvtCatalogEvent[]
  /** 풀린 라벨 — `enums.proc['20'] = 'Task'`. */
  enums: Record<string, Record<string, string>>
}

export interface EvtHeader {
  layout_sig: number
  total: number
  head: number
  boot_id: number
  dropped: number
}

export interface EvtStat {
  run_us: number
  max_run_us: number
  max_per_scan_hit: number
}

export interface EvtCfg {
  min_level: number
  cat_mask: number
  max_per_scan: number
}

export interface EvtCollector {
  epoch: number
  last_seq: number
  stored: number
  gaps: number
  last_read_at: string | null
  error: string | null
}

export interface EvtSource {
  plc: string
  /** 계약에 EVTLOG 가 있다. */
  available: boolean
  layout_ok: boolean | null
  detail: string | null
  header: EvtHeader | null
  stat: EvtStat | null
  cfg: EvtCfg | null
  collector: EvtCollector
}

export type EvtCfgPatch = Partial<EvtCfg>

export interface TaskEvents {
  task_id: string
  plc: string
  ctx: number[]
  from: string
  to: string
  /** 오래된 것 먼저. */
  rows: EventRow[]
}

export interface AlarmStatRow {
  plc: string
  area: number
  area_name: string
  bit: number
  code: number
  /** `F0501` · 코드 없는 비트는 `FAULT bit 7`. */
  label: string
  text: string
  count: number
  total_ms: number
  mean_ms: number
  max_ms: number
  /** 범위 끝에 아직 켜져 있던 발생 수. */
  open: number
  last_ts: number
  last: string
  /** 이벤트 목록 검색(`q`)에 넣으면 이 알람의 행만 남는다. */
  search: string
}

export interface AlarmStats {
  from: number
  to: number
  count: number
  total_ms: number
  by_plc: { plc: string; alarms: number; count: number; total_ms: number }[]
  rows: AlarmStatRow[]
}

export interface StepStatRow {
  plc: string
  proc: number
  proc_name: string
  step: number
  step_name: string
  task_type: number | null
  task_type_name: string | null
  count: number
  mean_ms: number
  p50_ms: number
  p95_ms: number
  max_ms: number
  outliers: number
}

export interface StepOutlier {
  id: number
  ts: string
  ts_ms: number
  plc: string
  proc: number
  proc_name: string
  step: number
  step_name: string
  task_type: number | null
  dwell_ms: number
  p95_ms: number
  limit_ms: number
  /** WorkId. */
  ctx: number
}

export interface StepStats {
  from: number
  to: number
  samples: number
  rows: StepStatRow[]
  outliers: StepOutlier[]
  outliers_total: number
}

export interface SwimPoint {
  t: number
  /** `null` = 이 스테이션이 아님(로봇) · 아직 행 없음. */
  v: 0 | 1 | null
}

export interface SwimLane {
  key: string
  /** 로봇 이름 또는 GRM PLC 이름. */
  group: string
  label: string
  points: SwimPoint[]
}

export interface SwimSpan {
  group: string
  from: number
  to: number
}

export type SwimMarkerKind = 'cvno_reason' | 'meas' | 'tracking' | 'ilk_timeout' | 'alarm'

export interface SwimMarker extends EventRow {
  kind: SwimMarkerKind
}

export interface Swimlane {
  station: number | null
  slot: number
  from: number
  to: number
  lanes: SwimLane[]
  spans: SwimSpan[]
  markers: SwimMarker[]
}

export interface RuleMatch {
  plc?: string
  cat?: string
  codes?: string[]
  min_lvl?: string
  text_contains?: string
  a_eq?: number
}

export interface AlertRule {
  id: number
  name: string
  enabled: boolean
  match: RuleMatch
  cooldown_s: number
  updated_at: string
}

export type AlertRuleBody = Omit<AlertRule, 'id' | 'updated_at'>

export interface AlertRec {
  id: number
  ts: string
  ts_ms: number
  rule_id: number
  rule_name: string
  acked: boolean
  /** 보존 기간으로 지워졌으면 `null`. */
  event: EventRow | null
}

export interface AlertList {
  rows: AlertRec[]
  unacked: number
}

export interface SavedFilter {
  id: number
  name: string
  /** 화면 URL 의 `e.*` 쿼리 — 링크 복사와 같은 모양. */
  query: string
  updated_at: string
}

/** SSE — 이름 붙은 이벤트 `evt`(EventRow, 필터 없음) · `alert`(AlertRec) · `alert_ack`(id 목록, 빈 목록 = 전부)
 *  + 혼잡 시 `lag`(건너뛴 수). */
export const EVT_STREAM_URL = '/api/events/stream'
/** 알림만(행 없이) — 상태바 배지. */
export const EVT_ALERT_STREAM_URL = '/api/events/stream?alerts=only'

const cfgPath = (plc: string) => `/api/plc/${encodeURIComponent(plc)}/evtlog/cfg`

export const evtApi = {
  /** `query` 는 `?` 로 시작하는 쿼리스트링(비면 빈 문자열) — `evtFilterModel.buildQuery`. */
  list: (query: string) => getJson<EventPage>(`/api/events${query}`),
  catalog: () => getJson<EvtCatalog>('/api/events/catalog'),
  sources: () => getJson<EvtSource[]>('/api/events/sources'),
  cfg: (plc: string) => getJson<EvtSource>(cfgPath(plc)),
  setCfg: (plc: string, body: EvtCfgPatch) => putJson<EvtSource>(cfgPath(plc), body),
  task: (taskId: string) => getJson<TaskEvents>(`/api/events/task/${encodeURIComponent(taskId)}`),
  csvUrl: (query: string): string => `/api/events/export.csv${query}`,
  /** `query` 는 `?from=…&to=…&plc=…` 모양(`evtStatsModel.statsQuery`). */
  alarmStats: (query: string) => getJson<AlarmStats>(`/api/events/stats/alarms${query}`),
  stepStats: (query: string) => getJson<StepStats>(`/api/events/stats/steps${query}`),
  swimlane: (query: string) => getJson<Swimlane>(`/api/events/swimlane${query}`),
  rules: () => getJson<AlertRule[]>('/api/events/alerts/rules'),
  createRule: (b: AlertRuleBody) => postJson<AlertRule>('/api/events/alerts/rules', b),
  updateRule: (id: number, b: AlertRuleBody) =>
    putJson<AlertRule>(`/api/events/alerts/rules/${id}`, b),
  deleteRule: (id: number) => del(`/api/events/alerts/rules/${id}`),
  alerts: (unacked: boolean, limit = 100) =>
    getJson<AlertList>(`/api/events/alerts?unacked=${unacked ? 1 : 0}&limit=${limit}`),
  ack: (id: number) => postJson<{ acked: number; unacked: number }>(`/api/events/alerts/${id}/ack`),
  ackAll: () => postJson<{ acked: number; unacked: number }>('/api/events/alerts/ack-all'),
  filters: () => getJson<SavedFilter[]>('/api/events/filters'),
  saveFilter: (name: string, query: string) =>
    postJson<SavedFilter>('/api/events/filters', { name, query }),
  deleteFilter: (id: number) => del(`/api/events/filters/${id}`),
  reportUrl: (date: string): string => `/api/events/report.xlsx?date=${encodeURIComponent(date)}`,
}
