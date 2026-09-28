// 이벤트 필터 — 쿼리스트링과 라이브 행 판정이 **같은 필터 한 벌**을 읽는다.
//
// 서버 필터와 클라이언트 판정이 갈리면 라이브로 붙은 행이 새로고침 뒤에 사라지거나(서버는 거른다)
// 반대로 나타난다. 그래서 두 함수를 한 파일에 두고 같은 테스트 표로 묶는다.
import type { EventRow, EvtCatalogEvent } from './api'
import { isErrorCode } from './evtTypeModel'

export type RangePreset = '15m' | '1h' | '24h' | '7d' | 'all' | 'custom'

export const RANGE_PRESETS: { id: RangePreset; label: string; ms: number | null }[] = [
  { id: '15m', label: '15분', ms: 15 * 60_000 },
  { id: '1h', label: '1시간', ms: 3_600_000 },
  { id: '24h', label: '24시간', ms: 86_400_000 },
  { id: '7d', label: '7일', ms: 7 * 86_400_000 },
  { id: 'all', label: '전체', ms: null },
]

export interface EvtFilter {
  /** 비면 전부. */
  plcs: string[]
  /** 카테고리 이름. 비면 전부. */
  cats: string[]
  /** ErrorList 유형(Alarm · Warn · Operator · Info · Task). 비면 전부. */
  types: string[]
  /** 최소 레벨 id, `null` = 전부. */
  minLvl: number | null
  /** 쉼표 목록 — 숫자, 이벤트 이름, ErrorList 코드(`F3119`). */
  code: string
  /** Task ctx(정수). */
  ctx: string
  range: RangePreset
  /** `custom` 일 때만 — epoch ms. */
  from: number | null
  to: number | null
  q: string
}

export const EMPTY_FILTER: EvtFilter = {
  plcs: [],
  cats: [],
  types: [],
  minLvl: null,
  code: '',
  ctx: '',
  range: 'all',
  from: null,
  to: null,
  q: '',
}

/** 걸린 조건 수 — 빈 목록이 필터 때문인지 가르는 데 쓴다(시간 `all` 은 조건이 아니다). */
export function activeCount(f: EvtFilter): number {
  let n = 0
  if (f.plcs.length) n++
  if (f.cats.length) n++
  if (f.types.length) n++
  if (f.minLvl !== null) n++
  if (codeTokens(f.code).length) n++
  if (parseCtx(f.ctx) !== null) n++
  if (f.range !== 'all') n++
  if (f.q.trim()) n++
  return n
}

/** 코드 칸 → 토큰. 숫자는 그대로, 이름은 대문자(카탈로그 이름이 대문자다). */
export function codeTokens(s: string): string[] {
  return s
    .split(/[,\s]+/)
    .map((t) => t.trim())
    .filter(Boolean)
    .map((t) => (/^\d+$/.test(t) ? String(Number(t)) : t.toUpperCase()))
}

/** ctx 칸 — 음 아닌 정수만. 아니면 `null`(조건 없음). */
export function parseCtx(s: string): number | null {
  const t = s.trim()
  if (!/^\d+$/.test(t)) return null
  const n = Number(t)
  return Number.isSafeInteger(n) ? n : null
}

/** 시간 창 — epoch ms. 프리셋은 `now` 기준, 전체는 둘 다 `null`. */
export function timeWindow(f: EvtFilter, now: number): { from: number | null; to: number | null } {
  if (f.range === 'custom') return { from: f.from, to: f.to }
  const p = RANGE_PRESETS.find((x) => x.id === f.range)
  if (!p || p.ms === null) return { from: null, to: null }
  return { from: now - p.ms, to: null }
}

export interface QueryOpts {
  now: number
  limit?: number
  before?: string | null
}

/** `GET /api/events` · `export.csv` 쿼리 — `?` 로 시작, 비면 빈 문자열. */
export function buildQuery(f: EvtFilter, o: QueryOpts): string {
  const q = new URLSearchParams()
  if (f.plcs.length) q.set('plc', f.plcs.join(','))
  if (f.cats.length) q.set('cat', f.cats.join(','))
  if (f.types.length) q.set('type', f.types.join(','))
  if (f.minLvl !== null) q.set('lvl', String(f.minLvl))
  const codes = codeTokens(f.code)
  if (codes.length) q.set('code', codes.join(','))
  const ctx = parseCtx(f.ctx)
  if (ctx !== null) q.set('ctx', String(ctx))
  const w = timeWindow(f, o.now)
  if (w.from !== null) q.set('from', String(w.from))
  if (w.to !== null) q.set('to', String(w.to))
  const text = f.q.trim()
  if (text) q.set('q', text)
  if (o.limit !== undefined) q.set('limit', String(o.limit))
  if (o.before) q.set('before', o.before)
  const s = q.toString()
  return s ? `?${s}` : ''
}

/** 라이브 행이 지금 필터에 드는가 — 서버 필터와 같은 뜻. */
export function matchesFilter(r: EventRow, f: EvtFilter, now: number): boolean {
  if (f.plcs.length && !f.plcs.includes(r.plc)) return false
  if (f.cats.length && !f.cats.some((c) => c === r.cat_name || c === String(r.cat))) return false
  if (f.types.length && !(r.etype && f.types.includes(r.etype))) return false
  if (f.minLvl !== null && r.lvl < f.minLvl) return false
  const codes = codeTokens(f.code)
  if (codes.length) {
    const name = (r.name ?? '').toUpperCase()
    const hit = codes.some((t) =>
      /^\d+$/.test(t) ? Number(t) === r.code : isErrorCode(t) ? t === r.ecode : t === name,
    )
    if (!hit) return false
  }
  const ctx = parseCtx(f.ctx)
  if (ctx !== null && r.ctx !== ctx) return false
  const w = timeWindow(f, now)
  if (w.from !== null && r.ts_ms < w.from) return false
  if (w.to !== null && r.ts_ms > w.to) return false
  const text = f.q.trim().toLowerCase()
  if (text) {
    const hay =
      `${r.text}\n${r.text_en ?? ''}\n${r.name ?? ''}\n${r.ecode ?? ''}\n${r.detail ?? ''}`.toLowerCase()
    if (!hay.includes(text)) return false
  }
  return true
}

/**
 * 코드 칸 자동 완성 — **마지막 토큰**만 바꾼 전체 문자열을 돌려준다(datalist 는 칸 전체를 갈아 끼운다).
 * 이름·코드 앞머리 어느 쪽으로도 찾는다.
 */
export function codeSuggestions(
  input: string,
  events: readonly EvtCatalogEvent[],
  max = 30,
): string[] {
  const cut = input.lastIndexOf(',')
  const head = cut >= 0 ? input.slice(0, cut + 1).replace(/\s+/g, '') : ''
  const tail = (cut >= 0 ? input.slice(cut + 1) : input).trim().toUpperCase()
  const out: string[] = []
  const seen = new Set<string>()
  for (const e of events) {
    if (!e.name) continue
    const byName = !tail || e.name.includes(tail)
    const byCode = e.code !== null && tail !== '' && String(e.code).startsWith(tail)
    if (!byName && !byCode) continue
    if (seen.has(e.name)) continue
    seen.add(e.name)
    out.push(`${head}${e.name}`)
    if (out.length >= max) break
  }
  return out
}

/** `datetime-local` 값(로컬) ↔ epoch ms. 빈 값·잘못된 값은 `null`. */
export function localInputToMs(s: string): number | null {
  if (!s) return null
  const m = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2})(?:\.(\d{1,3}))?)?$/.exec(s)
  if (!m) return null
  const [, y, mo, d, h, mi, se, ms] = m
  const t = new Date(
    Number(y),
    Number(mo) - 1,
    Number(d),
    Number(h),
    Number(mi),
    Number(se ?? 0),
    Number((ms ?? '0').padEnd(3, '0')),
  ).getTime()
  return Number.isNaN(t) ? null : t
}

const pad = (n: number, w = 2) => String(n).padStart(w, '0')

export function msToLocalInput(ms: number | null): string {
  if (ms === null) return ''
  const d = new Date(ms)
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
}

/** 범위 선택 칸의 글자 — 직접 지정이면 창을 짧게 말한다. */
export function rangeLabel(f: EvtFilter): string {
  if (f.range !== 'custom') return RANGE_PRESETS.find((p) => p.id === f.range)?.label ?? ''
  const short = (ms: number | null) =>
    ms === null ? '…' : msToLocalInput(ms).slice(5, 16).replace('T', ' ')
  return `${short(f.from)} ~ ${short(f.to)}`
}
