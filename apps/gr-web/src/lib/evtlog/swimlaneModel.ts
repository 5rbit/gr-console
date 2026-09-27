// 인터록 스윔레인 — 레인의 전이 점 → 구간·SVG 경로, 시각 ↔ x, 끌어서 확대, 이벤트 행에서 들어오는 길.
//
// 서버가 레인마다 `from` 의 상태를 첫 점으로 준다(창 앞의 마지막 행). 그래서 여기서는 점을 다음 점(또는 끝)까지
// 늘이기만 한다. `v = null` 은 "이 스테이션이 아님 / 아직 행 없음" — 선을 끊는다(0 으로 그리지 않는다).
import type { EventRow, SwimLane, SwimMarkerKind, SwimPoint } from './api'
import type { Status } from '../ui/status'

export interface SwimState {
  /** 스테이션 Id(21xx) — 없으면 `slot` · `event` 로 찾는다. */
  station: number | null
  /** GRM 슬롯(Id MOD 100) — STATION 행에서 들어올 때. */
  slot: number | null
  /** ILK_PI/PO 행의 id — 서버가 로그 순서로 그 행 바로 앞의 ILK_TARGET 을 찾는다(같은 ms 에 함께 찍힌다). */
  event: number | null
  /** 창 — 둘 다 `null` 이면 최근 10 분. */
  from: number | null
  to: number | null
}

export const EMPTY_SWIM: SwimState = {
  station: null,
  slot: null,
  event: null,
  from: null,
  to: null,
}

export const SWIM_RECENT_MS = 10 * 60_000
export const SWIM_HALF_MS = 5 * 60_000

export function swimRange(s: SwimState, now: number): { from: number; to: number } {
  if (s.from !== null && s.to !== null) return { from: s.from, to: s.to }
  const to = s.to ?? now
  return { from: s.from ?? to - SWIM_RECENT_MS, to }
}

/** 한 시각을 가운데 둔 창(±5 분). */
export function aroundRange(at: number, half = SWIM_HALF_MS): { from: number; to: number } {
  return { from: at - half, to: at + half }
}

/** 조회할 대상이 없으면 `null`. */
export function swimQuery(s: SwimState, now: number): string | null {
  const q = new URLSearchParams()
  if (s.station !== null) q.set('station', String(s.station))
  else if (s.slot !== null) q.set('slot', String(s.slot))
  else if (s.event !== null) q.set('event', String(s.event))
  else return null
  const r = swimRange(s, now)
  q.set('from', String(r.from))
  q.set('to', String(r.to))
  return `?${q.toString()}`
}

/** 이벤트 행에서 인터록 보기로 — ILOCK(로봇)·STATION(GRM) 행만. 창은 그 행 ±5 분. */
export function swimEntry(r: EventRow): SwimState | null {
  const w = aroundRange(r.ts_ms)
  if (r.cat_name === 'STATION' && r.src > 0) return { ...EMPTY_SWIM, slot: r.src, ...w }
  if (r.cat_name !== 'ILOCK') return null
  if (r.name === 'ILK_TARGET' && isStationId(r.a)) return { ...EMPTY_SWIM, station: r.a, ...w }
  return { ...EMPTY_SWIM, event: r.id, ...w }
}

/** 스테이션 Id — 2001..2932 중 끝 두 자리가 1..32(gr-proto `is_station_id`). */
export function isStationId(id: number): boolean {
  return Number.isInteger(id) && id >= 2001 && id <= 2932 && id % 100 >= 1 && id % 100 <= 32
}

export interface Seg {
  t0: number
  t1: number
  v: 0 | 1 | null
}

/** 점 → 구간(다음 점 또는 `to` 까지), [from, to] 로 자른다. 길이 0 인 구간은 버린다. */
export function segments(points: readonly SwimPoint[], from: number, to: number): Seg[] {
  const out: Seg[] = []
  for (let i = 0; i < points.length; i++) {
    const t0 = Math.max(points[i].t, from)
    const t1 = Math.min(i + 1 < points.length ? points[i + 1].t : to, to)
    if (t1 > t0) out.push({ t0, t1, v: points[i].v })
  }
  return out
}

/** 시각 `t` 의 값 — 첫 점보다 앞이면 `null`. */
export function valueAt(points: readonly SwimPoint[], t: number): 0 | 1 | null {
  let v: 0 | 1 | null = null
  for (const p of points) {
    if (p.t > t) break
    v = p.v
  }
  return v
}

export interface View {
  t0: number
  t1: number
}

export function xOf(t: number, v: View, width: number): number {
  return ((t - v.t0) / Math.max(1, v.t1 - v.t0)) * width
}

export function tOf(x: number, v: View, width: number): number {
  return v.t0 + (x / Math.max(1, width)) * (v.t1 - v.t0)
}

/** 끌어 고른 x 구간 → 새 보기. 4 px 미만(클릭)이나 1 s 미만이면 `null`. */
export function zoomTo(x0: number, x1: number, v: View, width: number, minMs = 1000): View | null {
  const [a, b] = x0 < x1 ? [x0, x1] : [x1, x0]
  if (b - a < 4) return null
  const t0 = tOf(Math.max(0, a), v, width)
  const t1 = tOf(Math.min(width, b), v, width)
  return t1 - t0 < minMs ? null : { t0, t1 }
}

/**
 * 계단 선의 SVG 경로 — 1 은 위(`pad`), 0 은 아래(`h - pad`). `null` 구간은 선을 끊는다. 폭 밖은 잘린다.
 * `until`(보통 지금) 뒤로는 그리지 않는다 — 창이 미래까지 걸쳐도 상태를 앞질러 늘이지 않는다.
 */
export function stepPath(
  points: readonly SwimPoint[],
  v: View,
  width: number,
  h: number,
  until = v.t1,
  pad = 3,
): string {
  const y = (b: 0 | 1) => (b ? pad : h - pad)
  let d = ''
  let prev: 0 | 1 | null = null
  for (const s of segments(points, v.t0, Math.min(v.t1, until))) {
    const x0 = Math.round(xOf(s.t0, v, width) * 10) / 10
    const x1 = Math.round(xOf(s.t1, v, width) * 10) / 10
    if (s.v === null) {
      prev = null
      continue
    }
    d += prev === null ? `M${x0} ${y(s.v)}` : `V${y(s.v)}`
    d += `H${x1}`
    prev = s.v
  }
  return d
}

/** 켜진(1) 구간 — 면을 옅게 칠한다. */
export function highSegs(points: readonly SwimPoint[], v: View, until = v.t1): Seg[] {
  return segments(points, v.t0, Math.min(v.t1, until)).filter((s) => s.v === 1)
}

/** 같은 group 끼리 순서를 지켜 묶는다. */
export function groupLanes(lanes: readonly SwimLane[]): { group: string; lanes: SwimLane[] }[] {
  const out: { group: string; lanes: SwimLane[] }[] = []
  for (const l of lanes) {
    const g = out.find((x) => x.group === l.group)
    if (g) g.lanes.push(l)
    else out.push({ group: l.group, lanes: [l] })
  }
  return out
}

export const MARKER_TONE: Record<SwimMarkerKind, Status> = {
  alarm: 'fault',
  ilk_timeout: 'fault',
  cvno_reason: 'warn',
  meas: 'info',
  tracking: 'neutral',
}

export const MARKER_LABEL: Record<SwimMarkerKind, string> = {
  alarm: 'Alarm',
  ilk_timeout: 'ILK_TIMEOUT',
  cvno_reason: 'ST_CVNO_REASON',
  meas: 'ST_MEAS',
  tracking: 'ST_TRACKING',
}

/** 눈금 — 보기 폭에 맞춰 1 s … 1 h 중 6 개 안팎이 되는 간격. */
export function ticks(v: View, want = 6): number[] {
  const span = v.t1 - v.t0
  const steps = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600].map((s) => s * 1000)
  const step = steps.find((s) => span / s <= want) ?? steps[steps.length - 1]
  const out: number[] = []
  for (let t = Math.ceil(v.t0 / step) * step; t <= v.t1; t += step) out.push(t)
  return out
}
