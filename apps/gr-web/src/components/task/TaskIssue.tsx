// 작업 명령 화면 — 왼쪽 레지스트리 레일(레이아웃 맵·재고·셀·스테이션·품목), 오른쪽 사이드바.
//   레이아웃 편집 모드: 오른쪽 = 셀/스테이션 리스트 + 생성 규칙
//   그 밖:              오른쪽 = 게이트 + 순차 계획 / 단일 명령
//
// 목록·계획(되돌리기 스택)·맵 모드·선택·화면 이동은 여기서 들어 레일·맵·사이드바가 한 상태를 본다.
// 계획·맵 모드·레일 탭은 브라우저에 저장돼 새로고침에도 남는다.
import { useCallback, useEffect, useMemo, useState } from 'react'
import { Send } from 'lucide-react'
import { api } from '../../lib/api'
import { useRegistry } from '../../lib/registry'
import { robots } from '../../lib/robots'
import { stock as stockStore } from '../../lib/stock'
import { useStore } from '../../lib/store'
import type { Shape } from '../../lib/task/layoutModel'
import {
  EMPTY_HISTORY,
  commit,
  redo,
  stepForClick,
  undo,
  type History,
  type PlanStep,
} from '../../lib/task/plan'
import { ScreenHeader } from '../../lib/ui/ScreenHeader'
import { Segmented } from '../../lib/ui/Segmented'
import { toast } from '../../lib/ui/toast'
import { cn } from '../../lib/utils'
import type {
  Cell,
  CellUpsert,
  Defaults,
  GripRef,
  Item,
  Station,
  Target,
  TaskType,
} from '../../lib/types'
import { ComposeCard } from './ComposeCard'
import { DefaultsDialog } from './DefaultsDialog'
import { GateBanner, useGate } from './GateBanner'
import { LayoutSidePanel, type SideTab } from './LayoutSidePanel'
import { LayoutTab, MAP_MODES, type MapMode } from './LayoutTab'
import { PlanCard } from './PlanCard'
import { RegistryRail, type RailTab } from './RegistryRail'

const PLAN_KEY = 'gr-plan'
const MODE_KEY = 'gr-cellmap-mode'
const RAIL_KEY = 'gr-rail-tab'
const RAIL_TABS: readonly RailTab[] = ['layout', 'stock', 'cell', 'station', 'item']
type Side = 'single' | 'plan'

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

export default function TaskIssue() {
  const items = useRegistry<Item>(api.items)
  const cells = useRegistry<Cell>(api.cells)
  const stations = useRegistry<Station>(api.stations)
  useStore(stockStore, robots)
  useEffect(() => stockStore.start(), [])
  useEffect(() => robots.start(), [])
  const robot = robots.selected
  const { gate, error: gateError } = useGate(robot)
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
  const [railTab, setRailTab] = useState<RailTab>(() => loadChoice(RAIL_KEY, RAIL_TABS, 'layout'))
  const [editSel, setEditSel] = useState<Target | null>(null)
  const [sideTab, setSideTab] = useState<SideTab>('cell')
  const [preview, setPreview] = useState<CellUpsert[]>([])
  const onPreview = useCallback((c: CellUpsert[]) => setPreview(c), [])
  const [focus, setFocus] = useState<{ target: Target; nonce: number } | null>(null)
  // 레이아웃 편집 그리드의 저장 전 초안 — 맵에 바로 그린다.
  const [cellDraft, setCellDraft] = useState<Cell[] | null>(null)
  const [stationDraft, setStationDraft] = useState<Station[] | null>(null)
  const editing = mapMode === 'edit' && railTab === 'layout'

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
        toast.ok(g === 'bead' ? '그립 기준: 상부 비드 높이' : '그립 기준: 타이어 중간(H/2)')
      } catch (e) {
        toast.error(`그립 기준 저장 실패 — ${e instanceof Error ? e.message : String(e)}`)
      }
    },
    [defaults],
  )

  const setPlan = useCallback((next: PlanStep[]) => setHist((h) => commit(h, next)), [])
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
        toast.info(
          `#${h.present.length + 1} ${step.type} ${shape.label}${step.item_code !== null ? ` · 품목 ${step.item_code}` : ''} → 계획`,
        )
        return commit(h, [...h.present, step])
      })
      setSide('plan')
    },
    [items.items],
  )

  const compose = useCallback((target: Target, shape: Shape, type: TaskType) => {
    setPicked({ target, type, nonce: Date.now() })
    setSide('single')
    toast.info(`${type} · ${shape.label} → 작성 카드`)
  }, [])

  const changeMode = useCallback((m: MapMode) => {
    setMapMode(m)
    persist(MODE_KEY, m)
  }, [])
  const changeRail = useCallback((t: RailTab) => {
    setRailTab(t)
    persist(RAIL_KEY, t)
  }, [])

  // 맵 강조: 편집 = 리스트 선택, 단일 명령 = 작성 카드 대상, 계획 = 표에서 고른 스텝
  const selected = editing ? editSel : side === 'single' ? current : (focusStep?.target ?? null)
  const layout = useMemo(
    () => (
      <LayoutTab
        cells={cells}
        stations={stations}
        items={items.items}
        plan={plan}
        selected={selected}
        mode={mapMode}
        onModeChange={changeMode}
        preview={preview}
        focus={focus}
        gripRef={gripRef}
        mapCells={editing ? cellDraft : null}
        mapStations={editing ? stationDraft : null}
        onPlanAdd={planAdd}
        onCompose={compose}
        onEditSelect={(t) => {
          setEditSel(t)
          setSideTab(t.kind)
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
        items={[
          { label: '품목', value: String(items.items.length) },
          { label: '셀', value: String(cells.items.length) },
          { label: '재고', value: String(stockStore.all.reduce((a, s) => a + s.count, 0)) },
          { label: '계획', value: String(plan.length) },
          {
            label: '게이트',
            value: gate ? (gate.can_submit ? '열림' : `닫힘 ${gate.reasons.length}`) : '…',
          },
        ]}
        trailing={
          <span
            className="text-xs text-content-muted"
            data-testid="task-robot"
            title="사이드바 로봇 목록에서 바꿉니다"
          >
            로봇 <b className="text-content-primary">{robots.current?.name ?? '…'}</b>
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
          />
        </section>
        <section
          className={cn(
            editing
              ? 'flex min-h-0 w-[620px] min-w-[560px] flex-[2] flex-col'
              : 'flex min-h-0 w-[460px] min-w-[400px] flex-[2] flex-col',
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
              <GateBanner gate={gate} error={gateError} />
              <Segmented
                ariaLabel="작성 방식"
                value={side}
                onChange={setSide}
                className="self-start"
                options={[
                  { id: 'plan', label: '순차 계획', badge: plan.length || '', testid: 'side-plan' },
                  { id: 'single', label: '단일 명령', testid: 'side-single' },
                ]}
              />
              <div className={side === 'plan' ? '' : 'hidden'}>
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
                  onFocus={(s) => {
                    setFocusStep(s)
                    if (s) setFocus({ target: s.target, nonce: Date.now() })
                  }}
                  gripRef={gripRef}
                  onGripRefChange={(g) => void setGripRef(g)}
                />
              </div>
              <div className={side === 'single' ? '' : 'hidden'}>
                <ComposeCard
                  items={items.items}
                  cells={cells.items}
                  stations={stations.items}
                  defaults={defaults}
                  gate={gate}
                  onOpenDefaults={() => setDefaultsOpen(true)}
                  pickedTarget={picked}
                  onTargetChange={setCurrent}
                />
              </div>
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
