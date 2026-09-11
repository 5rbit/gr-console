// 셸 탭 레지스트리 — 탭 정의와 그룹이 여기 한 곳에만 있다. 탭바는 그룹 드롭다운으로 접힌다.
//
// GR 콘솔은 그룹이 하나(`엔지니어링`)다 — sh4w의 다섯 그룹 구조를 그대로 쓰되 서랍을 하나만 연다.
// 화면이 늘어 축이 필요해지면 여기서 그룹을 가른다(App은 그룹 수에 매이지 않는다).
import { Activity, ListChecks, ListOrdered, Send } from 'lucide-react'
import type { Tab } from './nav'

/** 탭 그룹 id — 메뉴바의 최상위 항목. */
export type TabGroupId = 'engineering'

/** 탭 1건(라벨·아이콘·소속 그룹). */
export interface TabDef {
  id: Tab
  label: string
  icon: typeof Send
  group: TabGroupId
}

/** 그룹 1건(메뉴바 버튼). */
export interface TabGroupDef {
  id: TabGroupId
  label: string
}

/** 메뉴바 그룹 — 표시 순서. */
export const TAB_GROUPS: TabGroupDef[] = [{ id: 'engineering', label: '엔지니어링' }]

/** 전 탭 — 그룹 순서대로 늘어놓는다(단축키 1~4가 이 순서를 쓴다). */
export const ALL_TABS: TabDef[] = [
  { id: 'task', label: '작업 명령', icon: Send, group: 'engineering' },
  { id: 'taskmgr', label: 'Task 관리', icon: ListChecks, group: 'engineering' },
  { id: 'measure', label: '측정 모니터', icon: Activity, group: 'engineering' },
  { id: 'scenario', label: '시나리오', icon: ListOrdered, group: 'engineering' },
]
