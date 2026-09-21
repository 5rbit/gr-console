// 패널 레지스트리 — 도킹할 수 있는 것의 **전체 목록**. 라벨·아이콘·컴포넌트가 여기 한 곳에만 있다.
//
// `lib/workspace/*`는 id(문자열)만 안다 — lib이 화면을 import하면 순수 모델 테스트가 React를 끌고
// 오고, 화면 하나가 깨질 때 레이아웃 계산까지 함께 못 돌게 된다. 그 경계가 이 파일이다.
//
// 화면 넷(작업 명령·Task 관리·측정·시나리오)의 라벨·아이콘은 `lib/tabs.ts`에서 가져온다. 같은 이름을
// 두 벌 두면 한쪽만 고쳐지고, 메뉴와 탭이 다른 이름으로 같은 화면을 부른다.
import type { ComponentType } from 'react'
import { Boxes, Cpu, Gauge } from 'lucide-react'
import { ALL_TABS, type TabDef } from '../../lib/tabs'
import type { ZoneId } from '../../lib/workspace/model'

import TaskIssue from '../task/TaskIssue'
import ItemsPage from '../items/ItemsPage'
import TaskManager from '../taskmgr/TaskManager'
import MeasureMonitor from '../measure/MeasureMonitor'
import ScenarioPage from '../scenario/ScenarioPage'
import PalletPage from '../pallet/PalletPage'
import TracePage from '../trace/TracePage'
import PlcPane, { PlcSummary } from '../panes/PlcPane'
import RobotsPane, { RobotsSummary } from '../panes/RobotsPane'
import StatusPane, { StatusSummary } from '../panes/StatusPane'

/** 패널 1건. */
export interface PaneDef {
  id: string
  label: string
  icon: TabDef['icon']
  /**
   * `screen` 작업면(표·맵·폼이 든 화면 — 중앙에 서는 것이 기본) · `aux` 보조 면(목록·값 — 사이드).
   * 종류가 금지를 만들지는 않는다: 하단에 Task 관리를 깔든 오른쪽에 측정을 붙이든 사용자의 몫이다.
   * 이 값은 **기본 자리와 최소 크기**를 정할 뿐이다.
   */
  kind: 'screen' | 'aux'
  /** 팔레트·메뉴에서 열 때 들어가는 존. */
  defaultZone: ZoneId
  component: ComponentType
  /** 머리띠 제목 옆의 한 줄 요약(선택) — 접기 전에도 "지금 몇 개"가 보이게. */
  summary?: ComponentType
  /** 팔레트 검색 별칭. */
  keywords?: string
}

/** 화면 id → 컴포넌트. 탭이 늘면 `lib/tabs.ts`와 여기 둘을 손댄다(라벨은 저쪽이 진실원). */
const SCREEN_COMPONENT: Record<string, ComponentType> = {
  task: TaskIssue,
  items: ItemsPage,
  taskmgr: TaskManager,
  measure: MeasureMonitor,
  scenario: ScenarioPage,
  pallet: PalletPage,
  trace: TracePage,
}

const SCREEN_KEYWORDS: Record<string, string> = {
  task: 'issue command 명령 작성 제출',
  items: 'item tire spec 품목 타이어 규격 코드 치수 내경 외경',
  taskmgr: 'manager 목록 이력 history',
  measure: 'monitor 측정 트렌드 trend',
  scenario: 'runner 러너 순차 시퀀스',
  pallet: 'palletizing pattern drag 팔렛 패턴 적재 드래그 입고 출하',
  trace: 'trace waveform scope 파형 스코프 채널 사이클 오실로 로깅',
}

/** 보조 패널 — 예전 사이드바의 세 섹션. 이제는 어디에나 도킹된다. */
const AUX: PaneDef[] = [
  {
    id: 'robots',
    label: '로봇',
    icon: Boxes,
    kind: 'aux',
    defaultZone: 'left',
    component: RobotsPane,
    summary: RobotsSummary,
    keywords: 'robot 기종 선택 gr grm',
  },
  {
    id: 'plcs',
    label: 'PLC',
    icon: Cpu,
    kind: 'aux',
    defaultZone: 'left',
    component: PlcPane,
    summary: PlcSummary,
    keywords: 'plc 접속 레이아웃 s7 opcua',
  },
  {
    id: 'status',
    label: '상태',
    icon: Gauge,
    kind: 'aux',
    defaultZone: 'right',
    component: StatusPane,
    summary: StatusSummary,
    keywords: 'status webmon 모드 알람 대기열',
  },
]

/** 도킹 가능한 패널 전부 — 화면 넷 + 보조 셋. 순서가 메뉴·팔레트의 순서다. */
export const PANES: readonly PaneDef[] = [
  ...ALL_TABS.map(
    (t): PaneDef => ({
      id: t.id,
      label: t.label,
      icon: t.icon,
      kind: 'screen',
      defaultZone: 'center',
      component: SCREEN_COMPONENT[t.id],
      keywords: SCREEN_KEYWORDS[t.id],
    }),
  ),
  ...AUX,
]

/** id로 찾는다(없으면 `null` — 레이아웃에 남은 옛 id를 만질 수 있다). */
export function paneDef(id: string): PaneDef | null {
  return PANES.find((p) => p.id === id) ?? null
}

/** 화면 패널인가 — 셸이 `nav.tab`과 맞물릴 대상을 가릴 때 쓴다. */
export function isScreenPane(id: string): boolean {
  return paneDef(id)?.kind === 'screen'
}
