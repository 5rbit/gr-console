// 트레이스 화면의 순수 연산 — 브라우저 링 버퍼, 행 출처, 시작 가능 판정, 값 서식.
//
// 화면(`TracePage`)에는 상태를 옮기는 일만 남기고 **셀 수 있는 것은 전부 여기 둔다**. 링은 초당
// 수천 행이 드는 자리라 버그가 나면 눈으로는 안 보이고 메모리로만 보인다 — 그래서 테스트가 있다.

import type { TraceChannel, TraceKind } from '../../lib/trace/api'

/** 브라우저에 들고 있는 시간 창 — 30초. 그 앞은 세션 파일에 있고 재생으로 본다. */
export const RING_MS = 30_000
/**
 * 행 수 상한 — 창이 30초여도 스캔이 1 ms 면 3만 행이라 시간만으로는 메모리가 안 잡힌다.
 * 12 000 행 × (1 + 32) × 8 B ≈ 3.2 MB 로 **먼저 닿는 쪽이 이긴다**(빠른 PLC 면 창이 짧아진다).
 */
export const RING_ROWS = 12_000

/** 차트가 읽는 행 출처 — 라이브 링과 재생 배열이 같은 모양을 낸다. */
export interface RowSource {
  /** 지금 든 행 수. */
  readonly length: number
  /** 채널 수(행의 값 칸 수). */
  readonly chan: number
  /** 행 i 의 시각(ms). */
  time(i: number): number
  /** 행 i, 채널 ch 의 값. */
  value(i: number, ch: number): number
  /** 채널 하나를 `envelope` 가 받는 모양으로 뽑는다(오래된 것부터). */
  series(ch: number): { t: number[]; v: (number | null)[] }
  /** 시간 범위 `[처음, 마지막]` — 비면 `[0, 0]`. */
  bounds(): [number, number]
  /** `t` 에 가장 가까운 행 번호(비면 -1). 시각은 오름차순이라 이분 탐색이다. */
  indexAtTime(t: number): number
  /** 마지막 행 `[t_ms, 값…]`(비면 `null`). */
  lastRow(): number[] | null
}

const EMPTY_SERIES = { t: [] as number[], v: [] as (number | null)[] }

/**
 * 고정 크기 링 — 라이브 청크가 쌓이는 자리.
 *
 * 값은 `Float64Array` 하나에 **행 우선**으로 평탄화한다(`v[row * chan + ch]`). 행마다 배열을 만들면
 * 초당 수천 개의 작은 배열이 GC 를 계속 깨운다 — 30초 창에서 그 비용이 그리기보다 크다.
 */
export class TraceRing implements RowSource {
  readonly cap: number
  readonly windowMs: number
  #chan = 0
  #t: Float64Array = new Float64Array(0)
  #v: Float64Array = new Float64Array(0)
  /** 가장 오래된 행의 물리 위치. */
  #tail = 0
  #len = 0
  /** 지금 담고 있는 세션 id — 바뀌면 통째로 버린다(옛 세션의 행이 섞이면 파형이 거짓말을 한다). */
  #id = ''

  constructor(cap: number = RING_ROWS, windowMs: number = RING_MS) {
    this.cap = Math.max(2, Math.floor(cap))
    this.windowMs = Math.max(0, windowMs)
  }

  get length(): number {
    return this.#len
  }
  get chan(): number {
    return this.#chan
  }
  get sessionId(): string {
    return this.#id
  }

  /** 세션·채널 수를 새로 잡고 비운다. */
  reset(id = '', chan = 0): void {
    this.#id = id
    this.#chan = Math.max(0, chan)
    this.#t = new Float64Array(this.cap)
    this.#v = new Float64Array(this.cap * this.#chan)
    this.#tail = 0
    this.#len = 0
  }

  /**
   * 청크 하나를 넣는다. 각 행은 `[t_ms, 값…]`.
   *
   * 세션 id 나 채널 수가 달라지면 **먼저 비운다** — 설정이 바뀐 뒤의 청크를 앞 파형에 이어 붙이면
   * 같은 선 위에 다른 변수가 그려진다.
   */
  push(id: string, rows: readonly number[][]): void {
    if (rows.length === 0) return
    const chan = Math.max(0, (rows[0]?.length ?? 1) - 1)
    if (id !== this.#id || chan !== this.#chan) this.reset(id, chan)
    for (const row of rows) {
      if (row.length - 1 !== this.#chan) continue
      const head = (this.#tail + this.#len) % this.cap
      this.#t[head] = row[0]
      const base = head * this.#chan
      for (let c = 0; c < this.#chan; c++) this.#v[base + c] = row[c + 1]
      if (this.#len === this.cap) this.#tail = (this.#tail + 1) % this.cap
      else this.#len++
    }
    this.#trim()
  }

  /** 창보다 오래된 행을 꼬리에서 떨군다. */
  #trim(): void {
    if (this.windowMs <= 0 || this.#len === 0) return
    const newest = this.#t[(this.#tail + this.#len - 1) % this.cap]
    while (this.#len > 1 && newest - this.#t[this.#tail] > this.windowMs) {
      this.#tail = (this.#tail + 1) % this.cap
      this.#len--
    }
  }

  time(i: number): number {
    if (i < 0 || i >= this.#len) return 0
    return this.#t[(this.#tail + i) % this.cap]
  }

  value(i: number, ch: number): number {
    if (i < 0 || i >= this.#len || ch < 0 || ch >= this.#chan) return 0
    return this.#v[((this.#tail + i) % this.cap) * this.#chan + ch]
  }

  series(ch: number): { t: number[]; v: (number | null)[] } {
    if (ch < 0 || ch >= this.#chan || this.#len === 0) return { t: [], v: [] }
    const t: number[] = new Array(this.#len)
    const v: (number | null)[] = new Array(this.#len)
    for (let i = 0; i < this.#len; i++) {
      const p = (this.#tail + i) % this.cap
      t[i] = this.#t[p]
      v[i] = this.#v[p * this.#chan + ch]
    }
    return { t, v }
  }

  bounds(): [number, number] {
    if (this.#len === 0) return [0, 0]
    return [this.time(0), this.time(this.#len - 1)]
  }

  indexAtTime(t: number): number {
    return nearestIndex((i) => this.time(i), this.#len, t)
  }

  lastRow(): number[] | null {
    if (this.#len === 0) return null
    const out = new Array<number>(this.#chan + 1)
    out[0] = this.time(this.#len - 1)
    for (let c = 0; c < this.#chan; c++) out[c + 1] = this.value(this.#len - 1, c)
    return out
  }
}

/** 재생용 행 출처 — 저장 세션에서 받은 `[t_ms, 값…]` 배열을 그대로 읽는다(복사하지 않는다). */
export class RowArray implements RowSource {
  readonly rows: readonly number[][]
  readonly chan: number

  constructor(rows: readonly number[][], chan?: number) {
    this.rows = rows
    this.chan = chan ?? Math.max(0, (rows[0]?.length ?? 1) - 1)
  }

  get length(): number {
    return this.rows.length
  }

  time(i: number): number {
    return this.rows[i]?.[0] ?? 0
  }

  value(i: number, ch: number): number {
    return this.rows[i]?.[ch + 1] ?? 0
  }

  series(ch: number): { t: number[]; v: (number | null)[] } {
    if (ch < 0 || ch >= this.chan || this.rows.length === 0) return EMPTY_SERIES
    const t: number[] = new Array(this.rows.length)
    const v: (number | null)[] = new Array(this.rows.length)
    for (let i = 0; i < this.rows.length; i++) {
      t[i] = this.rows[i][0]
      v[i] = this.rows[i][ch + 1]
    }
    return { t, v }
  }

  bounds(): [number, number] {
    if (this.rows.length === 0) return [0, 0]
    return [this.time(0), this.time(this.rows.length - 1)]
  }

  indexAtTime(t: number): number {
    return nearestIndex((i) => this.time(i), this.rows.length, t)
  }

  lastRow(): number[] | null {
    return this.rows.length ? [...this.rows[this.rows.length - 1]] : null
  }
}

/** 저장 행(`[cycle, t_ms, 값…]`)에서 앞의 cycle 칸을 떼어 라이브와 같은 모양으로 만든다. */
export function stripCycle(rows: readonly number[][]): number[][] {
  return rows.map((r) => r.slice(1))
}

/** 오름차순 값 `len` 개에서 `target` 에 가장 가까운 번호(비면 -1). */
export function nearestIndex(at: (i: number) => number, len: number, target: number): number {
  if (len <= 0) return -1
  let lo = 0
  let hi = len - 1
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (at(mid) < target) lo = mid + 1
    else hi = mid
  }
  // 이분 탐색은 `target` 이상인 첫 자리를 준다 — 한 칸 앞이 더 가까울 수 있다.
  if (lo > 0 && Math.abs(at(lo - 1) - target) <= Math.abs(at(lo) - target)) return lo - 1
  return lo
}

// ── 채널 고르기 ───────────────────────────────────────────────────────────────

/** 채널 수 상한(백엔드 `chan_max` 가 오기 전의 기본값). */
export const CHAN_MAX = 32
/** PLC 가 받는 최소 FlushMs. */
export const FLUSH_MIN_MS = 20

export interface DbGroup {
  db: string
  channels: TraceChannel[]
}

/** 카탈로그 결과를 DB 별로 묶는다 — 검색 결과가 수백 건이면 DB 이름이 유일한 이정표다. */
export function groupByDb(channels: readonly TraceChannel[]): DbGroup[] {
  const map = new Map<string, TraceChannel[]>()
  for (const c of channels) {
    const g = map.get(c.db)
    if (g) g.push(c)
    else map.set(c.db, [c])
  }
  return [...map.entries()]
    .map(([db, cs]) => ({ db, channels: cs }))
    .sort((a, b) => a.db.localeCompare(b.db))
}

/** 선택에 하나 더한다 — 중복은 무시하고 상한을 넘기지 않는다(넘으면 그대로 돌려준다). */
export function addChannel(sel: readonly string[], path: string, max = CHAN_MAX): string[] {
  if (!path || sel.includes(path) || sel.length >= max) return [...sel]
  return [...sel, path]
}

export function removeChannel(sel: readonly string[], path: string): string[] {
  return sel.filter((p) => p !== path)
}

/** localStorage 에서 읽은 문자열을 선택으로 — 깨져 있거나 넘치면 살릴 수 있는 만큼만 살린다. */
export function parseSelection(text: string | null, max = CHAN_MAX): string[] {
  if (!text) return []
  let raw: unknown
  try {
    raw = JSON.parse(text)
  } catch {
    return []
  }
  if (!Array.isArray(raw)) return []
  const out: string[] = []
  for (const v of raw) {
    if (typeof v !== 'string' || !v || out.includes(v)) continue
    out.push(v)
    if (out.length >= max) break
  }
  return out
}

// ── 시작 가능 판정 ────────────────────────────────────────────────────────────

export interface StartInput {
  linkReady: boolean
  running: boolean
  /** 명령을 받을 PLC 이름 — 로봇 목록에서 온다. */
  plc: string
  channels: readonly string[]
  divider: string
  flushMs: string
  chanMax: number
  flushMin: number
}

/**
 * 시작 버튼을 막는 **이유**(막지 않으면 `null`).
 *
 * 비활성 버튼은 이유 없이는 고장과 구분되지 않는다 — 여기서 나온 문장이 그대로 버튼 툴팁과
 * 안내줄에 선다.
 */
export function startDisabledReason(i: StartInput): string | null {
  if (!i.linkReady) return 'PLC 소켓 링크가 준비되지 않았습니다 — 링크 연결 후 다시 시도하세요'
  if (i.running) return '이미 세션이 돌고 있습니다 — 정지한 뒤 시작하세요'
  if (!i.plc) return '대상 PLC 를 찾지 못했습니다 — 사이드바에서 로봇을 고르세요'
  if (i.channels.length === 0) return '채널을 하나 이상 고르세요'
  if (i.channels.length > i.chanMax) return `채널은 ${i.chanMax}개까지입니다 (지금 ${i.channels.length}개)`
  const d = Number(i.divider)
  if (!Number.isInteger(d) || d < 1) return '분주(divider)는 1 이상의 정수입니다 (1 = 매 스캔)'
  const f = Number(i.flushMs)
  if (!Number.isInteger(f) || f < i.flushMin) return `전송 주기는 ${i.flushMin} ms 이상의 정수입니다`
  return null
}

// ── 표시 ─────────────────────────────────────────────────────────────────────

/** PLC 타입에 맞는 값 글자 — BOOL 은 0/1, REAL 은 소수 세 자리, 정수는 그대로. */
export function fmtValue(kind: TraceKind, v: number): string {
  if (!Number.isFinite(v)) return '-'
  if (kind === 'bool') return v ? '1' : '0'
  if (kind === 'real') return v.toFixed(3)
  if (kind === 'time_ms') return `${Math.round(v)} ms`
  return String(Math.round(v))
}

/** 경과 시간 — 초 단위로 세 자리. 트레이스는 ms 가 의미 있는 자리라 반올림하지 않는다. */
export function fmtT(ms: number): string {
  return `${(ms / 1000).toFixed(3)} s`
}

/** 채널 경로의 짧은 이름 — `<DB>.` 앞머리를 떼어 범례가 폭을 덜 먹게. */
export function shortPath(path: string): string {
  const i = path.indexOf('.')
  return i > 0 ? path.slice(i + 1) : path
}

export interface Health {
  rows: number
  chunks: number
  overrun: number
  gaps: number
  /** SSE 가 밀려 건너뛴 이벤트 수(브라우저 쪽 손실). */
  lag: number
}

/**
 * 진행 중 세션의 건강 — `Meta`(백엔드 집계)를 바닥에 깔고 라이브 이벤트로 덮는다.
 *
 * `overrun` 이 0 이 아니면 **더는 무손실이 아니다**: PLC 가 표본을 버렸다는 뜻이고, 분주를 키우거나
 * 채널을 줄여야 한다. 그래서 이 값만 따로 색을 받는다.
 */
export function health(meta: { rows: number; chunks: number; overrun: number; gaps: number } | null, live: Partial<Health>): Health {
  return {
    rows: live.rows ?? meta?.rows ?? 0,
    chunks: live.chunks ?? meta?.chunks ?? 0,
    overrun: Math.max(live.overrun ?? 0, meta?.overrun ?? 0),
    gaps: meta?.gaps ?? 0,
    lag: live.lag ?? 0,
  }
}
