// 워크스페이스 레이아웃의 **순수 모델** — 도킹 존 넷과 그 안의 패널 탭 순서, 크기, 최대화.
//
// 왜 스토어와 가르는가: 도킹은 규칙이 잘게 많다(같은 패널이 두 존에 있으면 안 된다 · 중앙은 접히지
// 않는다 · 활성 탭은 반드시 그 존에 있다 · 백엔드가 안 내는 패널은 걷어 낸다). 그 규칙을 컴포넌트
// 안에서 지키면 드래그·프리셋·복원·명령 팔레트 네 경로가 각자 지켜야 하고, 한 곳만 빠뜨려도
// "탭은 있는데 아무것도 안 그려지는" 화면이 난다. 규칙을 여기 순수 함수로 모아 `*.test.ts`로 못
// 박고, 스토어는 이 함수들을 부르고 저장만 한다.
//
// 참고 — VSCode는 Part(사이드바·패널·에디터 그룹)를 그리드가 배치하고 뷰를 존 사이로 옮긴다.
// Unity는 창을 탭으로 도킹하고 레이아웃을 이름으로 저장한다. 둘의 공통 뼈대가 이 모델이다:
// **존 × 탭 목록 × 활성 탭 × 크기**.

/** 도킹 존 — 왼쪽 목록 · 중앙 작업면 · 오른쪽 인스펙터 · 하단 로그. VSCode의 Part 배치와 같은 축이다. */
export type ZoneId = 'left' | 'center' | 'right' | 'bottom'

/** 존 표시 순서(메뉴·명령 팔레트가 이 순서를 쓴다). */
export const ZONE_IDS: readonly ZoneId[] = ['left', 'center', 'right', 'bottom']

/** 존 이름 — 메뉴·토스트에 그대로 나간다. */
export const ZONE_LABEL: Record<ZoneId, string> = {
  left: '왼쪽',
  center: '중앙',
  right: '오른쪽',
  bottom: '하단',
}

/** 크기 한계(px) — left/right는 폭, bottom은 높이. 중앙은 나머지를 먹으므로 크기가 없다. */
export const ZONE_LIMITS: Record<Exclude<ZoneId, 'center'>, { min: number; max: number }> = {
  left: { min: 180, max: 520 },
  right: { min: 200, max: 560 },
  bottom: { min: 120, max: 600 },
}

/** 존 하나의 상태. */
export interface ZoneState {
  /** 도킹된 패널 id — **탭 순서**다. */
  panes: string[]
  /** 활성 탭(그려지는 패널). `panes`에 없으면 `normalize`가 고친다. */
  active: string | null
  /** 접힘 — 탭은 기억하고 면만 내린다(VSCode의 사이드바 토글). 중앙은 접히지 않는다. */
  collapsed: boolean
  /** 크기 px — 중앙은 무시. */
  size: number
}

/** 레이아웃 1벌 — 프리셋·저장 레이아웃·현재 상태가 모두 이 모양이다. */
export interface Layout {
  /** 최대화된 패널 하나(Unity의 Maximize · VSCode의 Zen). `null`이면 정상 배치. */
  maximized: string | null
  zones: Record<ZoneId, ZoneState>
}

function zone(panes: string[], size: number, collapsed = false): ZoneState {
  return { panes: [...panes], active: panes[0] ?? null, collapsed, size }
}

/** 빈 레이아웃 — 프리셋을 짤 때의 바닥. */
export function emptyLayout(): Layout {
  return {
    maximized: null,
    zones: {
      left: zone([], 240),
      center: zone([], 0),
      right: zone([], 280, true),
      bottom: zone([], 200, true),
    },
  }
}

/** 존 목록에서 레이아웃을 짠다 — 프리셋 정의를 한 줄로 쓰기 위한 설탕. */
export function layoutOf(spec: {
  left?: string[]
  center?: string[]
  right?: string[]
  bottom?: string[]
  sizes?: Partial<Record<Exclude<ZoneId, 'center'>, number>>
}): Layout {
  const base = emptyLayout()
  for (const id of ZONE_IDS) {
    const panes = spec[id] ?? []
    base.zones[id].panes = [...panes]
    base.zones[id].active = panes[0] ?? null
    // 빈 존은 접어 둔다 — 탭 하나 없는 빈 면이 폭을 먹으면 "고장 났나"로 읽힌다.
    if (id !== 'center') base.zones[id].collapsed = panes.length === 0
  }
  for (const [id, size] of Object.entries(spec.sizes ?? {}))
    base.zones[id as ZoneId].size = size as number
  return base
}

/** 얕은 복제 — 순수 함수들이 원본을 건드리지 않게. */
function clone(l: Layout): Layout {
  return {
    maximized: l.maximized,
    zones: {
      left: { ...l.zones.left, panes: [...l.zones.left.panes] },
      center: { ...l.zones.center, panes: [...l.zones.center.panes] },
      right: { ...l.zones.right, panes: [...l.zones.right.panes] },
      bottom: { ...l.zones.bottom, panes: [...l.zones.bottom.panes] },
    },
  }
}

/** 이 패널이 도킹된 존(없으면 `null`). */
export function findZone(l: Layout, pane: string): ZoneId | null {
  return ZONE_IDS.find((z) => l.zones[z].panes.includes(pane)) ?? null
}

/** 열려 있는(어느 존에든 도킹된) 패널 전부 — 존 순서, 탭 순서. */
export function openPanes(l: Layout): string[] {
  return ZONE_IDS.flatMap((z) => l.zones[z].panes)
}

/**
 * 실제로 **그려지는** 패널 — 최대화 중이면 그 하나, 아니면 펼쳐진 존의 활성 탭들.
 *
 * 비활성 탭을 마운트하지 않는 것이 규칙이다: 화면 넷이 동시에 살면 SSE·폴이 넷 다 돌고,
 * 보이지도 않는 표가 초당 여러 번 다시 그려진다.
 */
export function renderedPanes(l: Layout): string[] {
  if (l.maximized && findZone(l, l.maximized)) return [l.maximized]
  const out: string[] = []
  for (const z of ZONE_IDS) {
    const s = l.zones[z]
    if (z !== 'center' && s.collapsed) continue
    if (s.active) out.push(s.active)
  }
  return out
}

/**
 * 불변식 복원 — 저장된 JSON·프리셋·드래그 결과 **모든 입구**가 여기를 통과한다.
 *
 * ① 모르는 패널 id를 걷어 낸다(백엔드가 안 내는 화면·구버전 레이아웃).
 * ② 같은 패널이 두 존에 있으면 앞선 존만 남긴다(중복 탭은 한쪽이 죽은 탭이 된다).
 * ③ 활성 탭은 그 존 안의 것으로 맞춘다.
 * ④ 중앙은 접히지 않고, 비었으면 `fallback`을 넣는다 — 중앙이 비면 작업면이 사라진다.
 * ⑤ 크기를 한계로 조인다(저장된 값이 창보다 클 수 있다).
 * ⑥ 최대화 대상이 없어졌으면 최대화를 푼다.
 */
export function normalize(l: Layout, known: readonly string[], fallback?: string): Layout {
  const out = clone(l)
  const seen = new Set<string>()
  for (const z of ZONE_IDS) {
    const s = out.zones[z]
    s.panes = s.panes.filter((p) => {
      if (!known.includes(p) || seen.has(p)) return false
      seen.add(p)
      return true
    })
    if (s.active === null || !s.panes.includes(s.active)) s.active = s.panes[0] ?? null
    if (z === 'center') {
      s.collapsed = false
    } else {
      const lim = ZONE_LIMITS[z]
      s.size = Math.min(lim.max, Math.max(lim.min, Math.round(s.size) || lim.min))
      if (s.panes.length === 0) s.collapsed = true
    }
  }
  const fb = fallback && known.includes(fallback) ? fallback : known[0]
  if (out.zones.center.panes.length === 0 && fb) {
    // 중앙이 빈 레이아웃은 존재할 수 없다 — 다른 존에 있으면 끌어오고, 없으면 새로 넣는다.
    const from = findZone(out, fb)
    if (from) out.zones[from].panes = out.zones[from].panes.filter((p) => p !== fb)
    out.zones.center.panes = [fb]
    for (const z of ZONE_IDS)
      if (!out.zones[z].panes.includes(out.zones[z].active ?? ''))
        out.zones[z].active = out.zones[z].panes[0] ?? null
  }
  if (out.maximized && !findZone(out, out.maximized)) out.maximized = null
  return out
}

/** 그 패널을 활성 탭으로 올린다(존이 접혀 있으면 펼친다). 열려 있지 않으면 그대로. */
export function activatePane(l: Layout, pane: string): Layout {
  const z = findZone(l, pane)
  if (!z) return l
  const out = clone(l)
  out.zones[z].active = pane
  if (z !== 'center') out.zones[z].collapsed = false
  // 다른 것을 최대화한 채 이 탭을 고르면 최대화를 푼다 — 누른 탭이 안 보이는 일은 없어야 한다.
  if (out.maximized && out.maximized !== pane) out.maximized = null
  return out
}

/** 패널을 연다 — 이미 열려 있으면 그 자리에서 활성화(중복 탭을 만들지 않는다). */
export function openPane(l: Layout, pane: string, zoneId: ZoneId): Layout {
  if (findZone(l, pane)) return activatePane(l, pane)
  const out = clone(l)
  const s = out.zones[zoneId]
  s.panes.push(pane)
  s.active = pane
  if (zoneId !== 'center') s.collapsed = false
  if (out.maximized) out.maximized = null
  return out
}

/** 패널을 닫는다(탭에서 뺀다). 중앙의 마지막 탭은 닫히지 않는다 — 작업면이 비면 안 된다. */
export function closePane(l: Layout, pane: string): Layout {
  const z = findZone(l, pane)
  if (!z) return l
  if (z === 'center' && l.zones.center.panes.length === 1) return l
  const out = clone(l)
  const s = out.zones[z]
  const i = s.panes.indexOf(pane)
  s.panes.splice(i, 1)
  // 닫은 자리의 **이웃**을 고른다(브라우저 탭 관용구) — 항상 첫 탭으로 튀면 맥락이 끊긴다.
  if (s.active === pane) s.active = s.panes[Math.min(i, s.panes.length - 1)] ?? null
  if (z !== 'center' && s.panes.length === 0) s.collapsed = true
  if (out.maximized === pane) out.maximized = null
  return out
}

/**
 * 패널을 다른 존(또는 같은 존의 다른 자리)으로 옮긴다 — 탭 드래그의 유일한 출구.
 * `index`가 없으면 맨 뒤에 붙는다. 중앙의 마지막 탭은 나갈 수 없다.
 */
export function movePane(l: Layout, pane: string, to: ZoneId, index?: number): Layout {
  const from = findZone(l, pane)
  if (!from) return openPane(l, pane, to)
  if (from === 'center' && to !== 'center' && l.zones.center.panes.length === 1) return l
  const out = clone(l)
  const src = out.zones[from]
  const i = src.panes.indexOf(pane)
  src.panes.splice(i, 1)
  if (src.active === pane) src.active = src.panes[Math.min(i, src.panes.length - 1)] ?? null
  if (from !== 'center' && src.panes.length === 0) src.collapsed = true

  const dst = out.zones[to]
  const at = index === undefined ? dst.panes.length : Math.max(0, Math.min(index, dst.panes.length))
  dst.panes.splice(at, 0, pane)
  dst.active = pane
  if (to !== 'center') dst.collapsed = false
  if (out.maximized) out.maximized = null
  return out
}

/** 존 접기/펼치기 — 중앙은 접히지 않는다. 빈 존을 펼쳐 달라는 요청은 무시한다. */
export function toggleZone(l: Layout, zoneId: ZoneId, next?: boolean): Layout {
  if (zoneId === 'center') return l
  const s = l.zones[zoneId]
  const want = next ?? !s.collapsed
  if (want === s.collapsed) return l
  if (!want && s.panes.length === 0) return l
  const out = clone(l)
  out.zones[zoneId].collapsed = want
  return out
}

/** 존 크기 변경(px) — 한계로 조인다. 스플리터 드래그가 매 프레임 부른다. */
export function resizeZone(l: Layout, zoneId: ZoneId, size: number): Layout {
  if (zoneId === 'center') return l
  const lim = ZONE_LIMITS[zoneId]
  const v = Math.min(lim.max, Math.max(lim.min, Math.round(size)))
  if (v === l.zones[zoneId].size) return l
  const out = clone(l)
  out.zones[zoneId].size = v
  return out
}

/** 최대화 토글 — 같은 패널을 다시 부르면 풀린다. 열려 있지 않은 패널은 무시. */
export function toggleMaximize(l: Layout, pane: string | null): Layout {
  if (pane !== null && !findZone(l, pane)) return l
  const next = pane === null || l.maximized === pane ? null : pane
  if (next === l.maximized) return l
  const out = clone(l)
  out.maximized = next
  return out
}

/** 저장·복원용 직렬화. 버전을 붙여 둔다 — 모양이 바뀌면 옛 값을 조용히 버릴 수 있게. */
export interface StoredLayout {
  v: 1
  layout: Layout
}

/** JSON 문자열 → 레이아웃. 깨진 값·구버전은 `null`(호출부가 프리셋으로 떨어진다). */
export function parseLayout(text: string | null): Layout | null {
  if (!text) return null
  try {
    const raw = JSON.parse(text) as Partial<StoredLayout>
    if (raw.v !== 1 || !raw.layout) return null
    const l = raw.layout as Layout
    // 존 넷이 다 있어야 한다 — 하나만 빠져도 렌더가 undefined를 만진다.
    for (const z of ZONE_IDS) if (!Array.isArray(l.zones?.[z]?.panes)) return null
    return {
      maximized: typeof l.maximized === 'string' ? l.maximized : null,
      zones: {
        left: sane(l.zones.left),
        center: sane(l.zones.center),
        right: sane(l.zones.right),
        bottom: sane(l.zones.bottom),
      },
    }
  } catch {
    return null
  }
}

function sane(s: ZoneState): ZoneState {
  return {
    panes: s.panes.filter((p): p is string => typeof p === 'string'),
    active: typeof s.active === 'string' ? s.active : null,
    collapsed: s.collapsed === true,
    size: Number.isFinite(s.size) ? s.size : 240,
  }
}

/** 레이아웃 → JSON 문자열. */
export function serializeLayout(l: Layout): string {
  return JSON.stringify({ v: 1, layout: l } satisfies StoredLayout)
}
