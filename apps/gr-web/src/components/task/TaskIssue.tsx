// 작업 명령 화면 — 왼쪽 레지스트리 레일(맵 + 표 한 면), 오른쪽 작업 카드 하나.
//   레이아웃 편집 모드: 오른쪽 = 셀/스테이션 리스트 + 생성 규칙
//   그 밖:              오른쪽 = 작업 카드(순차 계획 ↔ 단일 명령 두 모드)
//
// 게이트는 머리띠 오른쪽 한 줄(`GateChip`)로 들어갔다 — 배너 한 덩이가 카드를 아래로 밀지 않는다.
//
// 목록·계획(되돌리기 스택)·맵 모드·선택·화면 이동은 여기서 들어 레일·맵·사이드바가 한 상태를 본다.
// 계획·맵 모드·레일 탭은 브라우저에 저장돼 새로고침에도 남는다.
// 레일의 레이아웃+표(나눠 보기)에서 표 행을 고르면 맵이 그 대상으로 이동·강조하고, 맵에서 고르면
// 표가 그 행으로 따라간다(`tableSel` + `reveal`).
import { useCallback, useEffect, useMemo, useState } from 'react'
import { Send, ShieldAlert, ShieldCheck } from 'lucide-react'
import { api } from '../../lib/api'
import { useRegistry } from '../../lib/registry'
import { robots } from '../../lib/robots'
import { gateFor, robotLabel, withRobot } from '../../lib/robotContext'
import { RobotChip } from '../shared/RobotChip'
import { stock as stockStore } from '../../lib/stock'
import { useStore } from '../../lib/store'
import type { PreviewCell } from '../../lib/task/layoutGen'
import type { Shape } from '../../lib/task/layoutModel'
import { parseRailTab } from '../../lib/task/railSplitModel'
import {
  EMPTY_HISTORY,
  commit,
  redo,
  stepForClick,
  undo,
  type History,
  type PlanStep,
} from '../../lib/task/plan'
import { planInbox } from '../../lib/task/planInbox'
import { ScreenHeader } from '../../lib/ui/ScreenHeader'
import { toast } from '../../lib/ui/toast'
import { cn } from '../../lib/utils'
import type {
  Cell,
  Defaults,
  Gate,
  GripRef,
  Item,
  Station,
  Target,
  TaskType,
} from '../../lib/types'
import { ComposeCard } from './ComposeCard'
import { DefaultsDialog } from './DefaultsDialog'
import { useGate } from './GateBanner'
import { LayoutSidePanel, type SideTab } from './LayoutSidePanel'
import { LayoutTab, MAP_MODES, type MapMode } from './LayoutTab'
import { PlanCard, type PlanMode } from './PlanCard'
import { RAIL_KEY, RegistryRail, type RailTab } from './RegistryRail'
import { TaskManagerCard } from './TaskManagerCard'

const PLAN_KEY = 'gr-plan'
const MODE_KEY = 'gr-cellmap-mode'
type Side = PlanMode

function loadPlan(): PlanStep[] {
  try {
    const s = localStorage.getItem(PLAN_KEY)
    const v = s ? (JSON.parse(s) as PlanStep[]) : []
    return Array.isArray(v) ? v.filter((x) => x && x.id && x.type && x.target) : []
  } catch {
    return []
  }
}
function loadChoice<T extends string>(key: string, allowed: readonly T[], def: T): T {
  try {
    const v = localStorage.getItem(key) as T | null
    return v && allowed.includes(v) ? v : def
  } catch {
    return def
  }
}
function persist(key: string, v: string) {
  try {
    localStorage.setItem(key, v)
  } catch {
    /* 저장 못 해도 동작 */
  }
}

/**
 * 게이트 한 줄 — 머리띠 오른쪽에 색으로 선다.
 *
 * 전에는 오른쪽 칸 맨 위에 배너 한 덩이(닫혔을 때 사유 목록까지)가 서서, 계획 카드를 아래로
 * 밀어냈다. 게이트는 **제출 버튼을 누를 수 있나**를 말하는 한 비트라 색 하나면 충분하다 —
 * 막힌 사유는 손이 멈췄을 때(툴팁) 읽고, 제출 버튼 자신도 같은 이유로 잠긴다.
 */
function GateChip({ gate, error, robot }: { gate: Gate | null; error: string | null; robot: string }) {
  const tone = error
    ? 'border-warn bg-warn-soft text-warn-fg'
    : !gate
      ? 'border-line-default text-content-faint'
      : gate.can_submit
        ? 'border-ok bg-ok-soft text-ok-fg'
        : 'border-fault bg-fault-soft text-fault-fg'
  // 게이트는 **한 로봇의** 비트다 — 글자와 툴팁 양쪽에서 그 이름을 말한다(사고: GR2 앞에서 GR1 의
  // `Accept = FALSE` 를 읽었다). 사유 자체도 백엔드가 `GR1: …` 으로 낸다.
  const text = error
    ? `${robot} 게이트 조회 실패`
    : !gate
      ? `${robot} 게이트 확인 중…`
      : gate.can_submit
        ? `${robot} 제출 가능`
        : `${robot} 제출 불가 ${gate.reasons.length}`
  const why = error
    ? withRobot(robot, `게이트 조회 실패 — ${error}`)
    : gate && !gate.can_submit
      ? gate.reasons.join(' · ')
      : withRobot(robot, 'PLC가 새 작업을 받을 수 있습니다')
  return (
    <span
      className={cn(
        'flex items-center gap-1.5 rounded-md border px-2 py-0.5 text-2xs whitespace-nowrap',
        tone,
      )}
      title={why}
      data-testid="gate-banner"
      data-robot={robot}
      data-state={error ? 'error' : !gate ? 'wait' : gate.can_submit ? 'open' : 'closed'}
    >
      {error || (gate && !gate.can_submit) ? (
        <ShieldAlert className="h-3.5 w-3.5 shrink-0" />
      ) : (
        <ShieldCheck className="h-3.5 w-3.5 shrink-0" />
      )}
      {text}
    </span>
  )
}

export default function TaskIssue() {
  const items = useRegistry<Item>(api.items)
  const cells = useRegistry<Cell>(api.cells)
  const stations = useRegistry<Station>(api.stations)
  useStore(stockStore, robots)
  useEffect(() => stockStore.start(), [])
  useEffect(() => robots.start(), [])
  // 게이트·제출·계획 스텝이 모두 **이 하나**를 본다. 화면 어디도 다른 호기를 겨냥하지 않는다.
  const robot = robots.selected
  const chip = robots.chip
  const polled = useGate(robot)
  // 응답이 다른 로봇 것이면(선택을 막 바꾼 직후) 없는 것으로 본다 — 칩과 게이트가 늘 같은 호기다.
  const gate = gateFor(polled.gate, chip.name)
  const gateError = polled.error
  const [defaults, setDefaults] = useState<Defaults | null>(null)
  const [defaultsOpen, setDefaultsOpen] = useState(false)
  const [side, setSide] = useState<Side>('plan')
  // 단일 명령: 레이아웃/팔레트에서 고른 대상. nonce 로 같은 대상을 다시 눌러도 전달된다.
  const [picked, setPicked] = useState<{ target: Target; type?: TaskType; nonce: number } | null>(
    null,
  )
  const [current, setCurrent] = useState<Target | null>(null)
  // 순차 계획 + 되돌리기 스택
  const [hist, setHist] = useState<History>(() => ({ ...EMPTY_HISTORY, present: loadPlan() }))
  const plan = hist.present
  const [focusStep, setFocusStep] = useState<PlanStep | null>(null)
  // 맵 모드 · 레일 탭 · 편집 선택 · 생성 예정 셀 · 화면 이동
  const [mapMode, setMapMode] = useState<MapMode>(() => loadChoice(MODE_KEY, MAP_MODES, 'plan'))
  const [railTab, setRailTab] = useState<RailTab>(() => {
    try {
      return parseRailTab(localStorage.getItem(RAIL_KEY))
    } catch {
      return 'split'
    }
  })
  const [editSel, setEditSel] = useState<Target | null>(null)
  const [sideTab, setSideTab] = useState<SideTab>('cell')
  const [preview, setPreview] = useState<PreviewCell[]>([])
  const onPreview = useCallback((c: PreviewCell[]) => setPreview(c), [])
  const [focus, setFocus] = useState<{ target: Target; nonce: number } | null>(null)
  // 레이아웃 편집 그리드의 저장 전 초안 — 맵에 바로 그린다.
  const [cellDraft, setCellDraft] = useState<Cell[] | null>(null)
  const [stationDraft, setStationDraft] = useState<Station[] | null>(null)
  // 편집은 맵이 보일 때만 — 맵만(레이아웃)이든 나눠 보기든.
  const editing = mapMode === 'edit' && (railTab === 'layout' || railTab === 'split')
  // 레일 표(셀/스테이션)의 선택. 편집 중에는 편집 선택(`editSel`)이 그 자리를 맡는다.
  const [tableSel, setTableSel] = useState<Target | null>(null)
  // 레일 품목 표의 선택 — 맵이 그 품목이 든 셀들을 강조한다.
  const [itemSel, setItemSel] = useState<number | null>(null)
  // 맵·계획에서 고른 대상을 표로 보내는 신호(표 토글 전환 + 행 스크롤).
  const [reveal, setReveal] = useState<{ target: Target; nonce: number } | null>(null)
  const pickTarget = useCallback((t: Target) => {
    setTableSel(t)
    setReveal({ target: t, nonce: Date.now() })
  }, [])
  // 작성 카드 대상이 바뀌면 그쪽이 강조를 가져간다(표에서 고른 것보다 나중 일이므로).
  const onComposeTarget = useCallback((t: Target | null) => {
    setCurrent(t)
    setTableSel(null)
  }, [])

  useEffect(() => persist(PLAN_KEY, JSON.stringify(plan)), [plan])

  const loadDefaults = useCallback(async () => {
    try {
      setDefaults(await api.defaults())
    } catch {
      /* 카드가 "기본값 없음"으로 그린다 */
    }
  }, [])
  useEffect(() => {
    void loadDefaults()
  }, [loadDefaults])

  const gripRef: GripRef = defaults?.grip_ref ?? 'mid'
  const setGripRef = useCallback(
    async (g: GripRef) => {
      if (!defaults) return
      try {
        setDefaults(await api.defaultsSave({ ...defaults, grip_ref: g }))
        toast.ok(
          g === 'pick_bead'
            ? '그립 기준: bead+offset — 잰 상부 비드 − PickBeadOffset (못 잰 품목은 타이어 중간)'
            : '그립 기준: 타이어 중간(H/2)',
        )
      } catch (e) {
        toast.error(`그립 기준 저장 실패 — ${e instanceof Error ? e.message : String(e)}`)
      }
    },
    [defaults],
  )

  const setPlan = useCallback((next: PlanStep[]) => setHist((h) => commit(h, next)), [])

  // 다른 화면(팔렛 패턴)이 보낸 스텝 — 마운트 때와 도착할 때 한 번의 편집(되돌리기 한 칸)으로 끝에 붙인다.
  useEffect(() => {
    const drain = () => {
      const steps = planInbox.take()
      if (steps.length === 0) return
      setHist((h) => commit(h, [...h.present, ...steps]))
      setSide('plan')
    }
    drain()
    return planInbox.subscribe(drain)
  }, [])
  const doUndo = useCallback(() => setHist((h) => undo(h)), [])
  const doRedo = useCallback(() => setHist((h) => redo(h)), [])

  // Ctrl+Z / Ctrl+Y (입력 필드 밖에서)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null
      if (
        el &&
        (el.tagName === 'INPUT' ||
          el.tagName === 'TEXTAREA' ||
          el.tagName === 'SELECT' ||
          el.isContentEditable)
      )
        return
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.key.toLowerCase() === 'z') {
        e.preventDefault()
        doUndo()
      } else if (
        (e.ctrlKey || e.metaKey) &&
        (e.key.toLowerCase() === 'y' || (e.shiftKey && e.key.toLowerCase() === 'z'))
      ) {
        e.preventDefault()
        doRedo()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [doUndo, doRedo])

  const planAdd = useCallback(
    (target: Target, shape: Shape, type?: TaskType) => {
      setHist((h) => {
        const step = stepForClick(
          h.present,
          target,
          stockStore.map,
          type,
          items.items[0]?.code ?? null,
          robots.selected,
        )
        // 스텝은 지금 고른 로봇을 싣는다(`stepForClick`) — 토스트도 그 이름을 말한다.
        toast.info(
          withRobot(
            robots.chip.name,
            `#${h.present.length + 1} ${step.type} ${shape.label}${step.item_code !== null ? ` · 품목 ${step.item_code}` : ''} → 계획`,
          ),
        )
        return commit(h, [...h.present, step])
      })
      setSide('plan')
      pickTarget(target)
    },
    [items.items, pickTarget],
  )

  const compose = useCallback(
    (target: Target, shape: Shape, type: TaskType) => {
      setPicked({ target, type, nonce: Date.now() })
      setSide('single')
      toast.info(withRobot(robots.chip.name, `${type} · ${shape.label} → 작성 카드`))
      pickTarget(target)
    },
    [pickTarget],
  )

  const changeMode = useCallback((m: MapMode) => {
    setMapMode(m)
    persist(MODE_KEY, m)
  }, [])
  const changeRail = useCallback((t: RailTab) => {
    setRailTab(t)
    persist(RAIL_KEY, t)
  }, [])

  // 맵 강조: 편집 = 리스트 선택, 단일 명령 = 작성 카드 대상, 계획 = 계획 표에서 고른 스텝.
  // 레일 표에서 고른 행이 있으면 그것이 먼저다(계획 스텝을 고르면 `tableSel`도 그 대상으로 옮긴다).
  const selected = editing
    ? editSel
    : side === 'single'
      ? (tableSel ?? current)
      : (tableSel ?? focusStep?.target ?? null)
  const layout = useMemo(
    () => (
      <LayoutTab
        cells={cells}
        stations={stations}
        items={items.items}
        plan={plan}
        selected={selected}
        highlightItem={itemSel}
        mode={mapMode}
        onModeChange={changeMode}
        preview={preview}
        focus={focus}
        gripRef={gripRef}
        mapCells={editing ? cellDraft : null}
        mapStations={editing ? stationDraft : null}
        onPlanAdd={planAdd}
        onCompose={compose}
        onTargetPick={pickTarget}
        onEditSelect={(t) => {
          setEditSel(t)
          setSideTab(t.kind)
          setReveal({ target: t, nonce: Date.now() })
        }}
        onItemsChanged={() => void items.reload()}
      />
    ),
    [
      cellDraft,
      stationDraft,
      editing,
      cells,
      stations,
      items,
      plan,
      selected,
      itemSel,
      mapMode,
      changeMode,
      preview,
      focus,
      gripRef,
      planAdd,
      compose,
    ],
  )

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="screen-task">
      <ScreenHeader
        title="작업 명령"
        icon={<Send className="h-4 w-4" />}
        items={[{ label: 'Plan', value: String(plan.length) }]}
        trailing={
          <span className="flex items-center gap-2">
            <GateChip gate={gate} error={gateError} robot={chip.name} />
            <RobotChip
              chip={chip}
              prefix="대상"
              testid="task-robot"
              title={`이 화면의 작업은 ${robotLabel(chip)} 로 갑니다 — 사이드바 로봇 목록에서 바꿉니다`}
            />
          </span>
        }
      />
      <div className="flex min-h-0 flex-1">
        <section
          className="flex min-h-0 min-w-0 flex-[3] flex-col border-r border-line-default"
          aria-label="레지스트리"
        >
          <RegistryRail
            items={items}
            cells={cells}
            stations={stations}
            layout={layout}
            tab={railTab}
            onTabChange={changeRail}
            selected={editing ? editSel : tableSel}
            onSelect={(t) => {
              if (editing) {
                setEditSel(t)
                if (t) setSideTab(t.kind)
              } else setTableSel(t)
              if (t) setFocus({ target: t, nonce: Date.now() })
            }}
            itemSel={itemSel}
            onItemSelect={setItemSel}
            reveal={reveal}
          />
        </section>
        <section
          className={cn(
            // `min-w-0` 이 없으면 표·카드 안의 긴 값이 이 칸을 밀어 1280px 에서 왼쪽 레일을 잡아먹는다.
            editing
              ? 'flex min-h-0 w-[620px] min-w-0 flex-[2] flex-col'
              : 'flex min-h-0 w-[460px] min-w-0 flex-[2] flex-col',
            editing ? '' : 'gap-3 overflow-y-auto p-3',
          )}
          aria-label={editing ? '레이아웃 편집' : '작업 작성'}
        >
          {editing ? (
            <LayoutSidePanel
              cells={cells}
              stations={stations}
              tab={sideTab}
              onTabChange={setSideTab}
              selected={editSel}
              onSelect={(t) => {
                setEditSel(t)
                if (t) setFocus({ target: t, nonce: Date.now() })
              }}
              onPreview={onPreview}
              previewCount={preview.length}
              onCellDraft={setCellDraft}
              onStationDraft={setStationDraft}
            />
          ) : (
            <>
            <PlanCard
              steps={plan}
              onChange={setPlan}
              canUndo={hist.past.length > 0}
              canRedo={hist.future.length > 0}
              onUndo={doUndo}
              onRedo={doRedo}
              cells={cells.items}
              stations={stations.items}
              items={items.items}
              stockNow={stockStore.map}
              gate={gate}
              robot={chip}
              onFocus={(s) => {
                setFocusStep(s)
                if (s) {
                  setFocus({ target: s.target, nonce: Date.now() })
                  pickTarget(s.target)
                }
              }}
              gripRef={gripRef}
              onGripRefChange={(g) => void setGripRef(g)}
              mode={side}
              onModeChange={setSide}
              single={
                <ComposeCard
                  chrome={false}
                  items={items.items}
                  cells={cells.items}
                  stations={stations.items}
                  defaults={defaults}
                  gate={gate}
                  robot={chip}
                  onOpenDefaults={() => setDefaultsOpen(true)}
                  pickedTarget={picked}
                  onTargetChange={onComposeTarget}
                />
              }
            />
            {/* 작업 카드 아래 — Task Manager 원장(진행 · 히스토리), 고른 로봇 것만. */}
            <TaskManagerCard />
            </>
          )}
        </section>
      </div>
      <DefaultsDialog
        open={defaultsOpen}
        onOpenChange={setDefaultsOpen}
        defaults={defaults}
        onSaved={setDefaults}
      />
    </div>
  )
}
