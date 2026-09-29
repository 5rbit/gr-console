// 두 로봇 영역 띠 — 공유 X 축 위에 로봇 위치 · 잡은 구간 · 금지 구간(간격) · 이 Task 구간을 한 줄로 놓는 계산(순수).
//
// 판정(막힘·간격·필요 간격)은 백엔드 `area::view` 가 제출 게이트와 같은 함수로 낸다 — 여기서는 자리만 잡는다.
import type { AreaView, XSpan } from '../types'

/** 띠 위 한 구간 — 축 너비에 대한 % (0..100). */
export interface Bar {
  left: number
  width: number
}

export interface StripRobot {
  id: number
  name: string
  x: number | null
  /** 지금 X 의 % 자리. */
  pos: number | null
  isMe: boolean
  /** 다른 로봇이 보는 이 로봇의 영역. */
  band: Bar | null
  /** 이 로봇 영역 ± 필요 간격 — 내 Task 구간이 여기에 닿으면 영역 대기. 나 자신은 없음. */
  keepout: Bar | null
  keepoutSpan: XSpan | null
}

export interface AreaStrip {
  domain: XSpan
  robots: StripRobot[]
  mine: Bar | null
  mineSpan: XSpan | null
  /** 이 Task 와 가장 빠듯한 상대 사이(겹치면 없음). */
  gapBar: Bar | null
  gap: number | null
  need: number | null
  blocked: boolean
  /** 막혔을 때 모자란 mm. */
  shortBy: number | null
}

const clamp = (v: number) => Math.min(100, Math.max(0, v))

/** `xs` = 레지스트리 셀·스테이션 X(축 범위를 설비 전체로 잡는다). */
export function areaStrip(view: AreaView, xs: readonly number[]): AreaStrip {
  const c = view.check
  const me = c?.robot ?? null
  const marginOf = (id: number | null) => view.robots.find((r) => r.id === id)?.margin ?? 0
  const needFor = (id: number) => view.separation_mm + marginOf(me) + marginOf(id)
  const pts: number[] = [...xs]
  for (const r of view.robots) {
    if (r.x !== null) pts.push(r.x)
    if (r.area) pts.push(r.area.lo, r.area.hi)
  }
  if (c) pts.push(c.mine.lo, c.mine.hi)
  const finite = pts.filter(Number.isFinite)
  const lo0 = finite.length ? Math.min(...finite) : 0
  const hi0 = finite.length ? Math.max(...finite) : 1
  const pad = Math.max((hi0 - lo0) * 0.04, 200)
  const domain = { lo: lo0 - pad, hi: hi0 + pad }
  const pct = (x: number) => ((x - domain.lo) / (domain.hi - domain.lo)) * 100
  const bar = (s: XSpan): Bar => {
    const left = clamp(pct(s.lo))
    return { left, width: Math.max(clamp(pct(s.hi)) - left, 0) }
  }
  const robots: StripRobot[] = view.robots.map((r) => {
    const isMe = r.id === me
    const keepoutSpan =
      !isMe && me !== null && view.active && r.area
        ? { lo: r.area.lo - needFor(r.id), hi: r.area.hi + needFor(r.id) }
        : null
    return {
      id: r.id,
      name: r.name,
      x: r.x,
      pos: r.x === null ? null : clamp(pct(r.x)),
      isMe,
      band: r.area ? bar(r.area) : null,
      keepout: keepoutSpan ? bar(keepoutSpan) : null,
      keepoutSpan,
    }
  })
  let gapBar: Bar | null = null
  const other = c?.nearest != null ? view.robots.find((r) => r.id === c.nearest)?.area : null
  if (c && other && c.gap !== null && c.gap > 0) {
    gapBar = bar(
      c.mine.hi <= other.lo ? { lo: c.mine.hi, hi: other.lo } : { lo: other.hi, hi: c.mine.lo },
    )
  }
  const blocked = !!c?.blocked
  return {
    domain,
    robots,
    mine: c ? bar(c.mine) : null,
    mineSpan: c ? c.mine : null,
    gapBar,
    gap: c?.gap ?? null,
    need: c?.need ?? null,
    blocked,
    shortBy: blocked && c?.gap !== null && c?.need != null ? c.need - (c.gap ?? 0) : null,
  }
}

/**
 * 간격 파라미터를 얼마로 두면 **지금** 이 Task 가 나가나 — 간격 − 양쪽 여유. PLC 하한보다 작아야 하면 간격으로는
 * 풀 수 없다(로봇을 옮겨야 한다) → `null`.
 */
export function separationToPass(view: AreaView): number | null {
  const c = view.check
  if (!c || c.gap === null || c.need === null) return null
  const margins = c.need - view.separation_mm
  const v = Math.floor(c.gap - margins)
  if (view.plc_min !== null && v < Math.ceil(view.plc_min)) return null
  return v > 0 ? v : null
}

const mm = (v: number) => v.toFixed(0)

/** 한 줄 요약 — 띠 옆 글자와 버튼 제목. */
export function areaSummary(view: AreaView): string {
  const c = view.check
  if (!view.active) return view.enabled ? '로봇 하나 — 영역 검사 없음' : '영역 검사 꺼짐'
  if (!c) return `간격 ${mm(view.separation_mm)} mm`
  const other = view.robots.find((r) => r.id === c.nearest)
  if (!other || c.gap === null || c.need === null) return `간격 ${mm(view.separation_mm)} mm`
  const head = `${other.name} X ${other.area ? `${mm(other.area.lo)}${other.area.hi !== other.area.lo ? `..${mm(other.area.hi)}` : ''}` : '?'} · 거리 ${mm(c.gap)} / 필요 ${mm(c.need)} mm`
  return c.blocked ? `${head} — ${mm(c.need - c.gap)} mm 부족, 영역 대기` : head
}
