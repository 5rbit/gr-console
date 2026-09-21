// 레이아웃 프리셋 — Unity의 `Window > Layouts`(Default · 2 by 3 · Tall · Wide)에 해당한다.
//
// 왜 프리셋이 먼저인가: 자유 도킹만 주면 사용자가 **처음 한 번** 자기 배치를 짜야 하고, 그 한 번이
// 대부분의 사람에게 일어나지 않는다(Unity가 프리셋을 다섯 개 들고 있는 이유). 일이 세 가지라면
// 배치도 세 가지면 된다 — 명령을 내는 자리 · 돌아가는 걸 보는 자리 · 값을 파는 자리.
//
// 패널 id만 적는다(라벨·컴포넌트는 `components/workspace/paneRegistry`). 여기 있는 id 중 백엔드가
// 안 내는 것은 `normalize`가 걷어 내므로, 프리셋은 "있으면 이렇게"만 말한다.
import { layoutOf, type Layout } from './model'

/** 프리셋 1건. */
export interface PresetDef {
  id: string
  label: string
  /** 무엇을 하는 배치인지 — 메뉴 항목의 부제로 그대로 나간다. */
  hint: string
  layout: Layout
}

/**
 * 기본 제공 프리셋 넷.
 *
 * - `standard` 셸 그대로 — 왼쪽 목록 + 중앙 화면 탭. 지금까지의 화면과 같은 배치라 기준점이 된다.
 * - `command` 명령을 내는 배치 — 중앙은 작업 명령 하나, 하단에 Task 목록을 깔아 **보낸 것이 어떻게
 *   되는지**가 눈을 안 떼고 보인다(명령→결과 왕복이 탭 전환 없이 한 화면).
 * - `triage` 값을 파는 배치 — 중앙 Task 관리, 오른쪽 인스펙터에 상태, 하단에 측정.
 * - `monitor` 보는 배치 — 중앙 측정 모니터, 왼쪽은 접고 오른쪽에 상태·로봇(벽 모니터용).
 */
export const PRESETS: readonly PresetDef[] = [
  {
    id: 'standard',
    label: '기본',
    hint: '왼쪽 목록 · 중앙 화면 탭',
    layout: layoutOf({
      left: ['robots', 'plcs', 'status'],
      center: ['task', 'items', 'taskmgr', 'measure', 'scenario', 'pallet'],
      sizes: { left: 240 },
    }),
  },
  {
    id: 'command',
    label: '명령 중심',
    hint: '중앙 작업 명령 · 하단 Task 목록',
    layout: layoutOf({
      left: ['robots', 'plcs'],
      center: ['task', 'scenario'],
      right: ['status'],
      bottom: ['taskmgr'],
      sizes: { left: 220, right: 260, bottom: 260 },
    }),
  },
  {
    id: 'triage',
    label: '데이터 3분할',
    hint: '중앙 Task 관리 · 오른쪽 상태 · 하단 측정',
    layout: layoutOf({
      left: ['robots', 'plcs'],
      center: ['taskmgr', 'task'],
      right: ['status'],
      bottom: ['measure'],
      sizes: { left: 220, right: 300, bottom: 300 },
    }),
  },
  {
    id: 'monitor',
    label: '모니터링',
    hint: '중앙 측정 모니터 · 왼쪽 접기',
    layout: layoutOf({
      left: ['robots'],
      center: ['measure', 'taskmgr'],
      right: ['status', 'plcs'],
      sizes: { left: 200, right: 300 },
    }),
  },
]

/** 첫 부팅·초기화가 쓰는 프리셋. */
export const DEFAULT_PRESET = 'standard'

/** 프리셋을 id로 찾는다(없으면 `null`). */
export function preset(id: string): PresetDef | null {
  return PRESETS.find((p) => p.id === id) ?? null
}

/** 프리셋의 레이아웃 사본 — 프리셋 상수를 그대로 내보내면 스토어가 그 객체를 갈아 버린다. */
export function presetLayout(id: string): Layout | null {
  const p = preset(id)
  return p ? structuredClone(p.layout) : null
}
