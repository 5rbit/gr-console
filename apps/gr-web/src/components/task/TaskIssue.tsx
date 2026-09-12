// 작업 명령 화면 — 왼쪽 레지스트리 레일(레이아웃 맵·재고·셀·스테이션·품목), 오른쪽 "단일 명령" 작성 카드 / "순차 계획" 카드.
//
// 목록은 여기서 한 번 받아 레일과 카드가 나눠 쓴다. 순차 계획(되돌리기 스택)과 작성 카드에 잡힌 대상도 여기 있어
// 레이아웃 클릭·팔레트·표 편집이 한 상태를 본다. 계획은 브라우저에 저장돼 새로고침에도 남는다.
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
import { toast } from '../../lib/ui/toast'
import { cn } from '../../lib/utils'
import type { Cell, Defaults, GripRef, Item, Station, Target, TaskType } from '../../lib/types'
import { ComposeCard } from './ComposeCard'
import { DefaultsDialog } from './DefaultsDialog'
import { GateBanner, useGate } from './GateBanner'
import { LayoutTab } from './LayoutTab'
import { PlanCard } from './PlanCard'
import { RegistryRail } from './RegistryRail'

const PLAN_KEY = 'gr-plan'
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

  useEffect(() => {
    try {
      localStorage.setItem(PLAN_KEY, JSON.stringify(plan))
    } catch {
      /* 저장 못 해도 동작 */
    }
  }, [plan])

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
    toast.info(
      `${type} · ${shape.label} → 작성 카드 (X ${shape.x.toFixed(0)} · Y ${shape.y.toFixed(0)} · Z ${shape.z.toFixed(0)})`,
    )
  }, [])

  // 레이아웃 강조: 단일 모드면 작성 카드 대상, 계획 모드면 표에서 고른 스텝
  const selected = side === 'single' ? current : (focusStep?.target ?? null)
  const layout = useMemo(
    () => (
      <LayoutTab
        cells={cells}
        stations={stations}
        items={items.items}
        plan={plan}
        selected={selected}
        onPlanAdd={planAdd}
        onCompose={compose}
      />
    ),
    [cells, stations, items, plan, selected, planAdd, compose],
  )

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="screen-task">
      <ScreenHeader
        title="작업 명령"
        icon={<Send className="h-4 w-4" />}
        items={[
          { label: '품목', value: String(items.items.length) },
          { label: '셀', value: String(cells.items.length) },
          { label: '스테이션', value: String(stations.items.length) },
          { label: '재고', value: String(stockStore.all.reduce((a, s) => a + s.count, 0)) },
          { label: '계획', value: String(plan.length) },
          {
            label: '게이트',
            value: gate ? (gate.can_submit ? '열림' : `닫힘 ${gate.reasons.length}`) : '…',
          },
        ]}
        trailing={
          <span
            className="text-xs text-slate-500"
            data-testid="task-robot"
            title="사이드바 로봇 목록에서 바꿉니다"
          >
            로봇 <b className="text-slate-800 dark:text-slate-100">{robots.current?.name ?? '…'}</b>
          </span>
        }
      />
      <div className="flex min-h-0 flex-1">
        <section
          className="flex min-h-0 min-w-0 flex-[3] flex-col border-r border-slate-200 dark:border-slate-700"
          aria-label="레지스트리"
        >
          <RegistryRail items={items} cells={cells} stations={stations} layout={layout} />
        </section>
        <section
          className="flex min-h-0 w-[460px] min-w-[400px] flex-[2] flex-col gap-3 overflow-y-auto p-3"
          aria-label="작업 작성"
        >
          <GateBanner gate={gate} error={gateError} />
          <div
            className="inline-flex self-start rounded-md border border-slate-300 p-0.5 dark:border-slate-600"
            role="tablist"
            aria-label="작성 방식"
          >
            {(
              [
                ['plan', `순차 계획 ${plan.length ? `(${plan.length})` : ''}`],
                ['single', '단일 명령'],
              ] as [Side, string][]
            ).map(([id, label]) => (
              <button
                key={id}
                type="button"
                role="tab"
                aria-selected={side === id}
                className={cn(
                  'rounded px-2.5 py-0.5 text-xs',
                  side === id
                    ? 'bg-indigo-600 text-white'
                    : 'text-slate-600 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800',
                )}
                onClick={() => setSide(id)}
                data-testid={`side-${id}`}
              >
                {label}
              </button>
            ))}
          </div>
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
              onFocus={setFocusStep}
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
