// 이벤트 행 — 표시 형식 · 레벨 색 · 가져온 쪽과 라이브 행의 합치기(중복 제거 + 상한).
import type { Status } from '../ui/status'
import type { EventRow, EvtSource } from './api'

/** 메모리에 드는 행 상한 — 넘치면 오래된 쪽을 버린다. */
export const ROW_CAP = 5000
/** 한 쪽 크기. */
export const PAGE_SIZE = 200

/**
 * `ts`('YYYY-MM-DD HH:MM:SS.mmm') → 오늘이면 `HH:MM:SS.mmm`, 아니면 `MM-DD HH:MM:SS.mmm`.
 * 문자열을 그대로 자른다 — PLC 시계로 찍힌 로컬 시각이라 다시 시간대를 거치면 어긋날 수 있다.
 */
export function fmtEvtTime(ts: string, now: number): string {
  const m = /^(\d{4}-\d{2}-\d{2})[ T](\d{2}:\d{2}:\d{2}(?:\.\d{1,3})?)/.exec(ts)
  if (!m) return ts
  const [, date, time] = m
  const hms = time.includes('.') ? time.padEnd(12, '0') : `${time}.000`
  return date === localDate(now) ? hms : `${date.slice(5)} ${hms}`
}

export function localDate(ms: number): string {
  const d = new Date(ms)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
}

/** epoch ms → 'YYYY-MM-DD HH:MM:SS.mmm'(로컬) — 서버 `ts` 와 같은 모양. */
export function msToStamp(ms: number): string {
  const d = new Date(ms)
  const p = (n: number, w = 2) => String(n).padStart(w, '0')
  const hms = `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`
  return `${localDate(ms)} ${hms}.${p(d.getMilliseconds(), 3)}`
}

/** 구간 길이 — 10 s 미만은 ms, 그 위는 s(한 자리). */
export function fmtSpan(ms: number): string {
  return ms < 10_000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`
}

/** 기준 행과의 차 — `+1.234` / `-0.050`(초). */
export function fmtOffset(ms: number): string {
  const s = (Math.abs(ms) / 1000).toFixed(3)
  return `${ms < 0 ? '-' : '+'}${s}`
}

/** 레벨 색 — 정상(INFO 이하)은 칠하지 않는다. */
export function lvlTone(lvlName: string): Status {
  const n = lvlName.toUpperCase()
  if (n === 'ERROR' || n === 'FATAL') return 'fault'
  if (n === 'WARN' || n === 'WARNING') return 'warn'
  return 'neutral'
}

/** 최신 먼저 — 시각이 같으면 나중에 들어온(id 큰) 것이 위. */
export function newestFirst(a: EventRow, b: EventRow): number {
  return b.ts_ms - a.ts_ms || b.id - a.id
}

export interface Merged {
  rows: EventRow[]
  /** 상한 때문에 오래된 쪽을 버렸다 — 그 뒤로는 커서로 이어 받을 수 없다(틈이 생긴다). */
  trimmed: boolean
  /** 새로 든 행 수(중복 제외). */
  added: number
}

/** 두 묶음을 id 로 합치고 최신 먼저 정렬해 상한까지 남긴다. */
export function mergeRows(
  cur: readonly EventRow[],
  incoming: readonly EventRow[],
  cap = ROW_CAP,
): Merged {
  const seen = new Set<number>()
  const all: EventRow[] = []
  for (const r of cur) {
    if (seen.has(r.id)) continue
    seen.add(r.id)
    all.push(r)
  }
  let added = 0
  for (const r of incoming) {
    if (seen.has(r.id)) continue
    seen.add(r.id)
    all.push(r)
    added++
  }
  all.sort(newestFirst)
  const trimmed = all.length > cap
  return { rows: trimmed ? all.slice(0, cap) : all, trimmed, added }
}

export interface LevelCounts {
  error: number
  warn: number
}

export function countLevels(rows: readonly EventRow[]): LevelCounts {
  let error = 0
  let warn = 0
  for (const r of rows) {
    const t = lvlTone(r.lvl_name)
    if (t === 'fault') error++
    else if (t === 'warn') warn++
  }
  return { error, warn }
}

/** 기준 행 둘레 창(±`half` ms) — 타임라인 대화상자의 조회 범위. */
export function aroundWindow(r: EventRow, half = 30_000): { from: number; to: number } {
  return { from: r.ts_ms - half, to: r.ts_ms + half }
}

/** 소스 한 줄 요약 — 머리 숫자 띠의 PLC 칸. */
export function sourceBrief(s: EvtSource): {
  seq: string
  drops: number
  gaps: number
  bad: boolean
} {
  const drops = s.header?.dropped ?? 0
  const gaps = s.collector.gaps
  return {
    seq: s.available ? `#${s.collector.last_seq}` : '-',
    drops,
    gaps,
    bad: !!s.collector.error || s.layout_ok === false,
  }
}

/** 로거 설정을 고칠 수 없는 사유 — 고칠 수 있으면 `undefined`. */
export function cfgBlockedReason(s: EvtSource | null | undefined): string | undefined {
  if (!s) return 'PLC 를 고르세요'
  if (!s.available) return `${s.plc} 계약에 EVTLOG 가 없습니다`
  if (s.layout_ok === false)
    return `${s.plc} 레이아웃 불일치 — ${s.detail ?? 'EVTLOG 레이아웃이 다릅니다'}`
  if (!s.cfg)
    return `${s.plc} 설정을 아직 읽지 못했습니다${s.collector.error ? ` — ${s.collector.error}` : ''}`
  return undefined
}
