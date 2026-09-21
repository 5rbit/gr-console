// 셸 탭 레지스트리 — 탭 정의와 그룹이 여기 한 곳에만 있다. 탭바는 그룹 드롭다운으로 접힌다.
//
// GR 콘솔은 그룹이 하나(`엔지니어링`)다 — sh4w의 다섯 그룹 구조를 그대로 쓰되 서랍을 하나만 연다.
// 화면이 늘어 축이 필요해지면 여기서 그룹을 가른다(App은 그룹 수에 매이지 않는다).
import { Activity, Grid3x3, ListChecks, ListOrdered, Ruler, Send, Waves } from 'lucide-react'
import type { Tab } from './nav'

/** 탭 그룹 id — 메뉴바의 최상위 항목. */
export type TabGroupId = 'engineering'

/** 탭 1건(라벨·아이콘·소속 그룹). */
export interface TabDef {
  id: Tab
  label: string
  icon: typeof Send
  group: TabGroupId
  /**
   * 백엔드 Profile(`/api/console/info` 의 `tabs`)에 없어도 **늘 보이는** 탭.
   *
   * Profile 은 "이 백엔드가 내는 화면"의 목록이라 기본값은 거기에 없으면 숨기는 것이다. 그런데
   * 트레이스처럼 백엔드 라우터(`/api/trace/*`)는 이미 있는데 Profile 목록에 아직 안 실린 화면이
   * 생기면, 그 한 줄 때문에 화면 전체가 닿지 않는 자리에 갇힌다. 그 틈을 프런트에서 메운다 —
   * 엔드포인트가 없으면 화면이 조회 오류를 그대로 보여 주므로 잘못 보여도 길을 잃지는 않는다.
   */
  local?: boolean
}

/** 그룹 1건(메뉴바 버튼). */
export interface TabGroupDef {
  id: TabGroupId
  label: string
}

/** 메뉴바 그룹 — 표시 순서. */
export const TAB_GROUPS: TabGroupDef[] = [{ id: 'engineering', label: '엔지니어링' }]

/** 전 탭 — 그룹 순서대로 늘어놓는다(단축키 1~9가 보이는 탭의 이 순서를 쓴다). */
export const ALL_TABS: TabDef[] = [
  { id: 'task', label: '작업 명령', icon: Send, group: 'engineering' },
  { id: 'items', label: '화물 규격', icon: Ruler, group: 'engineering' },
  { id: 'taskmgr', label: 'Task 관리', icon: ListChecks, group: 'engineering' },
  { id: 'measure', label: '측정 모니터', icon: Activity, group: 'engineering' },
  { id: 'scenario', label: '시나리오', icon: ListOrdered, group: 'engineering' },
  { id: 'pallet', label: '팔렛 패턴', icon: Grid3x3, group: 'engineering' },
  { id: 'trace', label: '트레이스', icon: Waves, group: 'engineering' },
]
