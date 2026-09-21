// 트레이스 슬라이스 API — 공유 fetch 래퍼 위에 `/api/trace/*` 를 얹는다.
//
// PLC 가 **매 스캔 주기**로 채널 32개까지를 표본하고 소켓 링크로 청크를 밀어 올린다. 콘솔은 그것을
// `data/traces/<id>` 에 남기고 SSE 로 중계한다 — 이 화면은 그 중계를 읽는 쪽이다.
import { del, getJson, postJson } from '../api'

/** 채널 값이 32비트 워드에서 어떻게 풀리는가(백엔드 `channels::Kind`). */
export type TraceKind = 'bool' | 'uint' | 'int' | 'real' | 'time_ms'

/** 카탈로그 1건 — PEEK 로 읽을 수 있는 변수 하나. */
export interface TraceChannel {
  /** `<DB>.<멤버 경로>` — PLC 이름 그대로다(번역하지 않는다). */
  path: string
  db: string
  db_no: number
  offset: number
  /** BOOL 의 비트 번호, 그 외에는 `null`. */
  bit: number | null
  width: number
  kind: TraceKind
  bits: number
  type_name: string
}

export interface TraceCatalog {
  databases: string[]
  channels: TraceChannel[]
  total: number
  limit: number
  /** 한 세션이 실을 수 있는 채널 수(32). */
  chan_max: number
  /** PLC 가 받아들이는 최소 FlushMs(20). */
  flush_min_ms: number
}

/** 세션에 기록된 채널 — 저장 파일을 계약 없이 되읽을 만큼의 정보. */
export interface TraceChannelMeta {
  path: string
  type_name: string
  kind: TraceKind
  bits: number
  db_no: number
  offset: number
  bit: number | null
}

export interface TraceMeta {
  id: string
  label: string
  note: string
  plc: string
  robot: number | null
  cfg_id: number
  divider: number
  flush_ms: number
  started_at: string
  stopped_at: string | null
  /** 첫 행의 PLC 벽시계(CPU 시각이 맞춰져 있을 때만). */
  plc_time0: string | null
  channels: TraceChannelMeta[]
  rows: number
  chunks: number
  /** 링크가 못 따라가 PLC 가 버린 표본 수 — 0 이 아니면 더는 무손실이 아니다. */
  overrun: number
  /** 앞 청크의 사이클을 잇지 못한 청크 수. */
  gaps: number
  errors: string[]
}

export interface TraceOverview {
  /** 트레이스를 받는 로봇 PLC(TRACE_LNK 가 있는 첫 로봇, 예 GR2) — 채널은 이 PLC 레이아웃이다. 구버전 응답엔 없다. */
  plc?: string
  link_ready: boolean
  current: TraceMeta | null
  sessions: TraceMeta[]
}

export interface TraceMark {
  t_ms: number
  at: string
  text: string
}

export interface TraceStart {
  plc: string
  robot?: number | null
  channels: string[]
  divider: number
  flush_ms: number
  label: string
  note: string
}

/** 저장된 세션 읽기 — `rows` 는 `[cycle, t_ms, 값…]`(라이브보다 앞 칸이 하나 많다). */
export interface TraceSession {
  meta: TraceMeta
  marks: TraceMark[]
  rows: number[][]
  returned: number
}

/** 라이브 청크 — `rows` 는 `[t_ms, 값…]`, 값은 이미 숫자로 풀려 있고 채널 순서다. */
export interface TraceChunkEvent {
  event: 'chunk'
  id: string
  cfg_id: number
  first_cycle: number
  divider: number
  overrun: number
  rows: number[][]
  total: number
}

export interface TraceMarkEvent {
  event: 'mark'
  mark: TraceMark
}

export interface TraceStoppedEvent {
  event: 'stopped'
  id: string
  reason?: string
}

export type TraceEvent = TraceChunkEvent | TraceMarkEvent | TraceStoppedEvent

/** SSE 주소 — 이벤트 이름은 `trace`(+ 혼잡 시 `lag`). */
export const TRACE_STREAM_URL = '/api/trace/stream'

const sessionPath = (id: string) => `/api/trace/${encodeURIComponent(id)}`

export const traceApi = {
  overview: () => getJson<TraceOverview>('/api/trace'),
  /** 카탈로그는 수만 건이다 — **반드시** 검색어와 상한을 걸어 부른다. */
  channels: (q: string, limit = 200) =>
    getJson<TraceCatalog>(`/api/trace/channels?q=${encodeURIComponent(q)}&limit=${limit}`),
  start: (body: TraceStart) => postJson<{ current: TraceMeta }>('/api/trace/start', body),
  stop: () => postJson<{ session: TraceMeta }>('/api/trace/stop'),
  mark: (text: string) => postJson<{ mark: TraceMark }>('/api/trace/mark', { text }),
  session: (id: string, max: number) => getJson<TraceSession>(`${sessionPath(id)}?max=${max}`),
  remove: (id: string) => del(sessionPath(id)),
  csvUrl: (id: string): string => `${sessionPath(id)}/export.csv`,
}
