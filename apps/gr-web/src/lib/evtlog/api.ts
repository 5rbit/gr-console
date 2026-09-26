// 이벤트 로그 슬라이스 API — `/api/events/*` · `/api/plc/{plc}/evtlog/cfg`.
//
// PLC 의 EVTLOG 링을 콘솔이 긁어 SQLite 에 쌓고, 콘솔 자신의 사건도 같은 표에 `origin: 'console'` 로 남긴다.
// 문구(`text`)는 서버가 카탈로그로 렌더한다 — 화면은 다시 조립하지 않는다.
import { getJson, putJson } from '../api'

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

/** SSE — 이름 붙은 이벤트 `evt`(EventRow, 필터 없음) + 혼잡 시 `lag`(건너뛴 수). */
export const EVT_STREAM_URL = '/api/events/stream'

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
}
