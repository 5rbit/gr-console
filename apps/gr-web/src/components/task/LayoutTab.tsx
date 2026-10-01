// 레이아웃 탭 — 맵과 세 동작 모드. 모드 토글과 보기 조작은 모두 플롯 안(아이콘·팝업)에 둔다.
//   명령 생성:     좌클릭마다 PICK → DROP → … 순으로 계획에 쌓인다 (드롭다운 Cell Teaching = 셀마다 MEASURE Floor)
//   모니터링:      좌클릭 = 셀·화물 정보 카드(플롯 안), 우클릭 = 명령 팔레트
//   레이아웃 편집: 좌클릭 = 우측 사이드바 셀/스테이션 리스트에서 선택, 생성 예정 셀 표시
// 로봇이 작업 중인 셀은 그 로봇 색 테두리(대기 = 점선), 로봇 위치는 같은 색 십자.
// 로봇이 들고 있는 화물(콘솔 Hand · PLC HoldItem)은 로봇 위치에 원으로, 어긋나면 빨간 점선. 로봇 표식 우클릭 =
// 운전 명령 + 화물 처리(`RobotMapMenu`), 좌클릭 = 그 로봇 선택.
// 모니터링 모드는 스케줄러 준비 상태(PICK ▲ · DROP ▼)를 그린다 — 켜고 끄기는 모드 토글 옆, 브라우저에 저장.
import { useEffect, useMemo, useState } from 'react'
import { Eye, ListPlus, Wand2, X } from 'lucide-react'
import { allStatus } from '../../lib/feeds'
import { TASK_TYPE_CODE } from '../../lib/gr/const'
import { cellInfoRows, toPlcCell } from '../../lib/gr/plcShape'
import type { Registry } from '../../lib/registry'
import { robotColor, robots } from '../../lib/robots'
import { stock as stockStore } from '../../lib/stock'
import { stationLive } from '../../lib/task/stationLiveStore'
import { useReady } from '../../lib/task/readyStore'
import { holdLine, readyShort } from '../../lib/task/readyMarkModel'
import { readyMark, targetKey, type TargetReady } from '../../lib/sched'
import CondExpr from '../sched/CondExpr'
import { stationAsCell } from '../../lib/task/stationCell'
import { useStore } from '../../lib/store'
import { tasks } from '../../lib/tasks'
import type { Shape } from '../../lib/task/layoutModel'
import type { PreviewCell } from '../../lib/task/layoutGen'
import {
  MEASURE_ROUTES,
  PLAN_KINDS,
  gripOffset,
  nextType,
  routeKindOf,
  type PlanKind,
  type PlanStep,
} from '../../lib/task/plan'
import { Button } from '../../lib/ui/Button'
import { InfoRows } from '../../lib/ui/Pair'
import { f1 } from '../../lib/meas/format'
import { ctxMenu, type MenuItem } from '../../lib/ui/menu'
import { Segmented } from '../../lib/ui/Segmented'
import { Select } from '../../lib/ui/Select'
import { cn } from '../../lib/utils'
import type { Cell, GripRef, Item, Station, Target, TaskType } from '../../lib/types'
import { PlcStructView } from '../shared/PlcStructView'
import { CellMap, type RobotMarker, type WorkMark } from './CellMap'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import { StockEditDialog, type StockEdit } from './StockRegistry'
import { QuickStockDialog, type QuickStockTarget } from './QuickStockDialog'
import { cargoOf, useRobotMapMenu } from './RobotMapMenu'
import { cargoMismatch, cargoVisible } from '../../lib/task/robotCargoModel'

export type MapMode = 'plan' | 'monitor' | 'edit'
export const MAP_MODES: readonly MapMode[] = ['plan', 'monitor', 'edit']

const TYPE_NAME: Record<number, string> = Object.fromEntries(
  Object.entries(TASK_TYPE_CODE).map(([k, v]) => [v, k]),
)
const isStation = (id: number) => id >= 2001 && id <= 2999
const READY_KEY = 'gr-map-ready'

function loadShowReady(): boolean {
  try {
    return localStorage.getItem(READY_KEY) !== '0'
  } catch {
    return true
  }
}

export interface LayoutTabProps {
  cells: Registry<Cell>
  stations: Registry<Station>
  items: readonly Item[]
  plan: readonly PlanStep[]
  /** 강조할 대상. */
  selected: Target | null
  /** 이 품목이 든 셀을 모두 강조한다(레일 품목 표 선택). */
  highlightItem?: number | null
  mode: MapMode
  onModeChange: (m: MapMode) => void
  /** 생성 예정 셀(편집 모드) — 충돌 판정이 실려 있다. */
  preview: readonly PreviewCell[]
  /** 이 대상으로 화면 이동. */
  focus?: { target: Target; nonce: number } | null
  gripRef?: GripRef
  /** 명령 생성 모드 좌클릭 / 팔레트 "계획에 추가". */
  onPlanAdd: (target: Target, shape: Shape, type?: TaskType | 'TEACH') => void
  /** 명령 생성 모드의 생성 방식 — PICK/DROP 교대 · Cell Teaching(맵 왼쪽 위 드롭다운). */
  planKind: PlanKind
  onPlanKindChange: (k: PlanKind) => void
  /** Cell Teaching — 모든 셀을 한 번씩 도는 경로를 계획에 넣는다. */
  onTeachAll: () => void
  /** 팔레트 "명령 작성" — 작성 카드에 종류+대상. */
  /** `item` = 품목·개수까지(로봇 화물 DROP). */
  onCompose: (
    target: Target,
    shape: Shape,
    type: TaskType,
    item?: { code: number; count: number },
  ) => void
  /** 편집 모드 좌클릭. */
  onEditSelect?: (target: Target) => void
  /** 모니터링 모드 좌클릭 — 정보 카드와 함께 바깥(셀/스테이션 표)에도 알린다. */
  onTargetPick?: (target: Target) => void
  onItemsChanged?: () => void
  /** 편집 그리드의 저장 전 초안(있으면 맵·정보 카드는 이것을 쓴다). */
  mapCells?: readonly Cell[] | null
  mapStations?: readonly Station[] | null
}

export function LayoutTab({
  cells,
  stations,
  items,
  plan,
  selected,
  highlightItem = null,
  mode,
  onModeChange,
  preview,
  focus,
  gripRef = 'mid',
  onPlanAdd,
  planKind,
  onPlanKindChange,
  onTeachAll,
  onCompose,
  onEditSelect,
  onTargetPick,
  onItemsChanged,
  mapCells,
  mapStations,
}: LayoutTabProps) {
  const cellList = mapCells ?? cells.items
  const stationList = mapStations ?? stations.items
  useStore(stockStore, tasks, robots, allStatus, stationLive)
  useEffect(() => stationLive.start(), [])
  useEffect(() => allStatus.start(), [])
  useEffect(() => tasks.start(), [])
  const [info, setInfo] = useState<Shape | null>(null)
  const [showReady, setShowReady] = useState(loadShowReady)
  // 준비 상태는 모니터링 모드에서만 받는다(정보 카드는 표식을 꺼도 보인다).
  const { index: ready } = useReady(mode === 'monitor')
  const toggleReady = () => {
    const b = !showReady
    setShowReady(b)
    try {
      localStorage.setItem(READY_KEY, b ? '1' : '0')
    } catch {
      /* 저장 못 해도 동작 */
    }
  }
  const stockVer = stockStore.getSnapshot()
  const highlight = useMemo(() => {
    if (highlightItem === null) return undefined
    const ids = cellList
      .filter((c) => {
        const s = stockStore.get(c.id)
        return s?.item_code === highlightItem && s.count > 0
      })
      .map((c) => c.id)
    return { label: `ItemCode ${highlightItem}`, cells: new Set(ids) }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- stockVer 가 재고 변경을 대표한다
  }, [highlightItem, cellList, stockVer])
  const [stockEdit, setStockEdit] = useState<StockEdit | null>(null)
  const [quick, setQuick] = useState<QuickStockTarget | null>(null)
  const next = planKind === 'teach' ? 'TEACH' : nextType(plan)

  // 로봇 작업 테두리 — 진행 중(running)은 실선, 제출~대기는 점선.
  const taskVer = tasks.getSnapshot()
  const robotVer = robots.getSnapshot()
  const work = useMemo(() => {
    const m = new Map<string, WorkMark>()
    for (const t of tasks.active) {
      const id = t.plc_task?.Cell?.Id
      if (!id) continue
      const key = `${isStation(id) ? 'station' : 'cell'}-${id}`
      const running = t.state === 'running'
      if (m.get(key)?.running && !running) continue
      // 못 찾으면 "선택한 로봇"이 아니라 Task 가 적어 둔 이름 그대로(다른 로봇 작업을 선택 로봇으로 칠하지 않는다).
      const rb = robots.list.find((r) => r.plc === t.plc_name || r.name === t.plc_name)
      const name = rb?.name ?? t.plc_name ?? '로봇'
      m.set(key, {
        color: robotColor(rb?.id),
        robot: name,
        running,
        label: `${name} ${TYPE_NAME[t.plc_task?.TaskType ?? 0] ?? ''} ${running ? '작업 중' : '대기'}`,
      })
    }
    return m
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 스토어 버전이 변화를 대표한다
  }, [taskVer, robotVer])

  // 로봇 위치 — 로봇마다 **자기** 상태 스트림의 X/Y(같은 레일 위 두 대가 한 맵에 같이 보인다).
  // 로봇 목록이 오기 전에는 기본 로봇 스트림 하나만 그린다.
  const markers: RobotMarker[] = (robots.list.length ? robots.list : [null]).flatMap((r) => {
    const ev = allStatus.get(r?.id ?? null)
    const axis = ev?.webmon?.Axis
    if (!axis || axis.length < 2) return []
    const c = r ? cargoOf(r) : null
    const it = c?.itemCode ? items.find((i) => i.code === c.itemCode) : undefined
    return [
      {
        id: r?.id ?? 0,
        name: r?.name ?? ev?.plc ?? 'GR',
        color: robotColor(r?.id),
        x: axis[0].Position,
        y: axis[1].Position,
        moving: axis[0].Running || axis[1].Running,
        cargo:
          c && cargoVisible(c)
            ? {
                label:
                  c.count > 0
                    ? `${c.itemCode || '코드?'}${c.count > 1 ? ` ×${c.count}` : ''}${cargoMismatch(c) ? ' ?' : ''}`
                    : 'Hand?',
                od: it?.outer_diameter ?? 0,
                id: it?.inner_diameter ?? 0,
                tone: cargoMismatch(c) ? 'fault' : c.state === 'unknown_plc' ? 'warn' : 'ok',
                title: c.text,
              }
            : null,
      },
    ]
  })

  // 로봇 우클릭 → DROP 작성: 다음 맵 클릭(셀·스테이션)이 그 로봇의 DROP 작성 카드가 된다. Esc = 취소.
  const [dropFor, setDropFor] = useState<number | null>(null)
  useEffect(() => {
    if (dropFor === null) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setDropFor(null)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [dropFor])
  const robotMenu = useRobotMapMenu({
    items,
    onDropPick: (id) => {
      robots.select(id)
      setDropFor(id)
    },
  })
  const dropRobot = dropFor === null ? null : robots.list.find((r) => r.id === dropFor)
  const robotLegend = robots.list.map((r) => ({ name: r.name, color: robotColor(r.id) }))

  const infoCell = info?.kind === 'cell' ? (cellList.find((c) => c.id === info.id) ?? null) : null
  const infoStation =
    info?.kind === 'station' ? (stationList.find((s) => s.id === info.id) ?? null) : null
  const infoStock = infoCell ? stockStore.get(infoCell.id) : null
  const infoStationStock = infoStation ? stockStore.get(infoStation.id) : null
  const infoStationItem = infoStationStock?.item_code
    ? items.find((i) => i.code === infoStationStock.item_code)
    : undefined
  const infoItem = infoStock?.item_code
    ? items.find((i) => i.code === infoStock.item_code)
    : undefined
  const infoWork = info ? work.get(`${info.kind}-${info.id}`) : undefined
  const infoReady = info ? ready.get(targetKey(info)) : undefined

  /**
   * 한 대상에 걸 수 있는 명령 목록 — 우클릭 팔레트와 정보 카드의 ⋯ 가 **같은 것**을 쓴다.
   * (두 자리에서 항목이 갈라지면 "오른쪽 클릭으로만 되는 것"이 생긴다.)
   */
  function taskMenu(t: Target, shape: Shape, from: 'palette' | 'info' = 'palette'): MenuItem[] {
    const station = t.kind === 'station' ? stationList.find((s) => s.id === t.id) : undefined
    // 스테이션은 재고 편집 창이 쓰는 Cell 모양으로(편집 창은 id 만 쓴다) — 컨베이어에 손으로 올린 화물의 코드 지정.
    const cell =
      t.kind === 'cell'
        ? cellList.find((c) => c.id === t.id)
        : station
          ? stationAsCell(station)
          : undefined
    const st = cell ? stockStore.get(cell.id) : null
    const name = `${t.kind === 'cell' ? 'Cell' : 'Station'} #${t.id}`
    const menu: MenuItem[] = [
      ...(from === 'palette'
        ? [
            {
              label: `${name}${st ? ` · Count ${st.count}${st.item_code ? ` (ItemCode ${st.item_code})` : ''}` : ''}`,
            },
          ]
        : []),
      {
        label: 'PICK 명령 작성',
        hint: 'P',
        run: () => onCompose(t, shape, 'PICK'),
        disabled: st && st.count === 0 ? '재고가 없습니다' : undefined,
      },
      { label: 'DROP 명령 작성', hint: 'D', run: () => onCompose(t, shape, 'DROP') },
      {
        label: 'MEASURE 명령 작성',
        hint: 'S',
        run: () => onCompose(t, shape, 'MEASURE'),
        disabled: t.kind === 'station' ? 'MEASURE 는 셀만' : undefined,
      },
      { label: 'MOVE 명령 작성', hint: 'M', run: () => onCompose(t, shape, 'MOVE') },
      { label: `계획에 추가 (${next})`, run: () => onPlanAdd(t, shape) },
      { label: '계획에 PICK 추가', run: () => onPlanAdd(t, shape, 'PICK') },
      { label: '계획에 DROP 추가', run: () => onPlanAdd(t, shape, 'DROP') },
      {
        label: '계획에 MEASURE 추가',
        hint: '재고 1 = Item · 2+ = SKU',
        run: () => onPlanAdd(t, shape, 'MEASURE'),
        disabled: t.kind === 'station' ? 'MEASURE 는 셀만' : undefined,
      },
      {
        label: '계획에 Cell Teaching 추가',
        hint: 'MEASURE Floor',
        run: () => onPlanAdd(t, shape, 'TEACH'),
      },
    ]
    // 정보 카드에서 연 메뉴에는 재고 편집·정보 보기를 넣지 않는다 — 그 둘은 이미 카드가 하고 있다.
    if (from === 'palette') {
      if (cell)
        menu.push({
          label: station ? '화물 코드 지정…' : '재고 편집…',
          run: () => setStockEdit({ cell, item: st?.item_code || null, count: st?.count ?? 0 }),
        })
      menu.push({ label: '정보 보기', run: () => setInfo(shape) })
    }
    return menu
  }

  function palette(t: Target, shape: Shape, e: React.MouseEvent) {
    ctxMenu.show(e, taskMenu(t, shape))
  }

  const modeToggle = (
    <Segmented
      ariaLabel="맵 동작"
      compact
      value={mode}
      onChange={(m) => {
        onModeChange(m)
        if (m !== 'monitor') setInfo(null)
      }}
      className="shadow-sm"
      options={[
        {
          id: 'plan',
          icon: <ListPlus size={14} />,
          label: '작업 추가',
          badge: mode === 'plan' ? next : '',
          title: PLAN_KINDS.find((k) => k.id === planKind)?.title,
          testid: 'map-mode-plan',
        },
        {
          id: 'monitor',
          icon: <Eye size={14} />,
          label: '모니터링',
          title: '좌클릭 = 셀·화물 정보 · 우클릭 = 명령 팔레트',
          testid: 'map-mode-monitor',
        },
        {
          id: 'edit',
          icon: <Wand2 size={14} />,
          label: '레이아웃 편집',
          title: '우측에 셀/스테이션 리스트와 생성 규칙',
          testid: 'map-mode-edit',
        },
      ]}
    />
  )

  // 명령 생성 모드에서만 — 좌클릭이 만드는 스텝 종류. 고른 값은 TaskIssue 가 기억한다.
  const kindSelect =
    mode === 'plan' ? (
      <span className="rounded-md bg-surface-panel shadow-sm">
        <Select
          dense
          value={planKind}
          onValueChange={(v) => onPlanKindChange(v as PlanKind)}
          aria-label="생성 방식"
          title={PLAN_KINDS.find((k) => k.id === planKind)?.title}
          data-testid="map-plan-kind"
        >
          {PLAN_KINDS.map((k) => (
            <option key={k.id} value={k.id}>
              {k.label}
            </option>
          ))}
        </Select>
      </span>
    ) : null

  // 측정 방식(Teaching · Item · SKU)은 보통 **대상 셀 전체**를 돈다 — 하나씩 누르지 않게 경로를 통째로 넣는 버튼을
  // 드롭다운 옆에. 종류·차례는 확인 창에서 고른다.
  const route = MEASURE_ROUTES.find((r) => r.id === routeKindOf(planKind))
  const teachAllButton =
    mode === 'plan' && route ? (
      <Button
        size="sm"
        intent="outline"
        className="bg-surface-panel shadow-sm"
        icon={<ListPlus className="h-3.5 w-3.5" />}
        title={`${route.label} — ${route.cells}을 차례대로 계획에 넣는다 (같은 측정이 이미 든 셀은 건너뜀)`}
        onClick={onTeachAll}
        data-testid="map-teach-all"
      >
        {route.cells}
      </Button>
    ) : null

  const readyToggle =
    mode === 'monitor' ? (
      <Button
        size="sm"
        intent="outline"
        active={showReady}
        aria-pressed={showReady}
        className={cn('bg-surface-panel shadow-sm', !showReady && 'text-content-faint')}
        title={`PICK·DROP 표시 ${showReady ? '끄기' : '켜기'} — ▲ PICK · ▼ DROP · 초록 가능 · 주황 품목 미정 · 로봇 색 예약 · 속 빈 회색 막힘`}
        onClick={toggleReady}
        data-testid="map-ready-toggle"
      >
        ▲▼
      </Button>
    ) : null

  const g = infoItem ? gripOffset(gripRef, infoItem) : 0

  return (
    <div className="h-full min-h-0" data-testid="layout-tab">
      <CellMap
        cells={cellList}
        stations={stationList}
        selected={mode === 'monitor' && info ? { kind: info.kind, id: info.id } : selected}
        stock={stockStore.map}
        stationLive={stationLive.map}
        ready={mode === 'monitor' && showReady ? ready : undefined}
        items={items}
        showDirty={mode === 'edit'}
        highlight={highlight}
        plan={mode === 'plan' ? plan : undefined}
        preview={mode === 'edit' ? preview : undefined}
        work={work}
        robots={markers}
        robotLegend={robotLegend}
        focus={focus}
        topLeft={
          <>
            {modeToggle}
            {kindSelect}
            {teachAllButton}
            {readyToggle}
            {dropRobot ? (
              <span
                className="flex items-center gap-2 rounded-md bg-surface-panel px-2 py-1 text-xs shadow-sm ring-1 ring-accent"
                data-testid="map-drop-pick"
              >
                <span className="font-semibold">{dropRobot.name} DROP</span>
                <span className="text-content-muted">놓을 셀·스테이션을 클릭 (Esc 취소)</span>
                <Button
                  size="icon-sm"
                  intent="ghost"
                  icon={<X size={14} />}
                  title="취소"
                  onClick={() => setDropFor(null)}
                />
              </span>
            ) : null}
          </>
        }
        onRobotContext={robotMenu.open}
        onRobotClick={(id, e) => {
          robots.select(id)
          robotMenu.open(id, e)
        }}
        onPick={(t, shape) => {
          if (dropFor !== null) {
            // 들고 있는 품목·개수를 그대로 싣는다(콘솔 Hand 를 모르면 작성 카드에서 고른다).
            const c = dropRobot ? cargoOf(dropRobot) : null
            setDropFor(null)
            onCompose(
              t,
              shape,
              'DROP',
              c && c.count > 0 && c.itemCode ? { code: c.itemCode, count: c.count } : undefined,
            )
            return
          }
          if (mode === 'plan') onPlanAdd(t, shape)
          else if (mode === 'monitor') {
            setInfo(shape)
            onTargetPick?.(t)
          } else onEditSelect?.(t)
        }}
        onContext={palette}
        // 더블 클릭 = 빠른 재고 팝업(셀·스테이션 공통): 비어 있으면 품목·개수(1), 있으면 수량·비우기.
        // 단일 클릭은 잠시(`CellMap`) 미뤄 두 번 눌러도 계획이 두 번 쌓이지 않는다.
        deferClick={() => true}
        onDouble={(t) => setQuick({ kind: t.kind, id: t.id, stock: stockStore.get(t.id) ?? null })}
      >
        {mode === 'monitor' && info ? (
          <div
            className="absolute top-2 right-12 z-10 flex max-h-[calc(100%-1rem)] w-72 flex-col overflow-hidden rounded-lg border border-line-default bg-surface-panel/95 shadow-lg"
            data-testid="map-info"
          >
            <div className="flex h-screen-header flex-none items-center gap-2 border-b border-line-default px-3">
              <span className="text-sm font-semibold">
                {info.kind === 'cell' ? 'Cell' : 'Station'} #{info.id}
              </span>
              {infoWork ? (
                <span
                  className="rounded px-1.5 py-0.5 text-3xs font-semibold text-content-on-accent"
                  style={{ background: infoWork.color }}
                >
                  {infoWork.label}
                </span>
              ) : null}
              <span className="flex-1" />
              <Button
                size="icon-sm"
                intent="ghost"
                icon={<X size={14} />}
                title="닫기"
                onClick={() => setInfo(null)}
              />
            </div>
            <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-3 text-xs">
              {infoReady && (infoCell || infoStation) ? <ReadyRow t={infoReady} /> : null}
              {infoCell ? (
                <>
                  {/* 라벨+값 짝은 킷의 `InfoRows` 하나다(손으로 짠 `<table>` 이었다) — 좁은 정보
                      패널에서 값 열이 오른쪽에 서서 자릿수가 맞는다. */}
                  <InfoRows
                    rows={[
                      { label: 'Count', value: String(infoStock?.count ?? 0) },
                      {
                        label: 'ItemCode',
                        value: infoStock?.item_code
                          ? `${infoStock.item_code} ${infoItem?.name ?? ''}`
                          : '-',
                      },
                      {
                        label: 'StackHeight (mm)',
                        value: infoItem && infoStock ? f1(infoItem.height * infoStock.count) : '-',
                      },
                      {
                        label: 'PICK Z (mm)',
                        value:
                          infoItem && infoStock?.count
                            ? f1(info.z + infoItem.height * (infoStock.count - 1) + g)
                            : '-',
                      },
                      {
                        label: 'DROP Z (mm)',
                        value: infoItem
                          ? f1(info.z + infoItem.height * (infoStock?.count ?? 0) + g)
                          : '-',
                      },
                    ]}
                  />
                  {/* 여기서 실제로 손이 가는 것은 재고를 고치는 일 하나다 — 명령을 만드는 길은
                      우클릭 팔레트와 같은 메뉴로 접어 둔다(같은 항목, 한 자리). */}
                  <div className="flex items-center gap-1">
                    <Button
                      size="sm"
                      intent="primary"
                      className="flex-1"
                      onClick={() =>
                        setStockEdit({
                          cell: infoCell,
                          item: infoStock?.item_code || null,
                          count: infoStock?.count ?? 0,
                        })
                      }
                      data-testid="info-stock-edit"
                    >
                      재고 편집
                    </Button>
                    <OverflowMenu
                      items={taskMenu({ kind: 'cell', id: infoCell.id }, info, 'info')}
                      title="명령 — 계획에 추가 · PICK/DROP/MEASURE/MOVE 작성"
                      testid="info-more"
                    />
                  </div>
                </>
              ) : null}
              {infoStation ? (
                <>
                  <div className="text-content-muted">
                    CV{infoStation.conv_no} · G{infoStation.group}-{infoStation.group_index} ·
                    TaskType {infoStation.task_type}
                  </div>
                  {/* 컨베이어 화물 코드 — 손으로 올린 화물은 여기서 코드를 지정하면 다음 스테이션으로 따라간다. */}
                  <InfoRows
                    rows={[
                      {
                        label: 'ItemCode',
                        value: infoStationStock?.count
                          ? `${infoStationStock.item_code || '?'} ${infoStationItem?.name ?? ''}`
                          : '-',
                      },
                      { label: 'Count', value: String(infoStationStock?.count ?? 0) },
                    ]}
                  />
                  <div className="flex items-center gap-1">
                    <Button
                      size="sm"
                      intent="primary"
                      className="flex-1"
                      onClick={() =>
                        setStockEdit({
                          cell: stationAsCell(infoStation),
                          item: infoStationStock?.item_code || null,
                          count: infoStationStock?.count ?? 0,
                        })
                      }
                      data-testid="info-station-stock-edit"
                    >
                      화물 코드 지정
                    </Button>
                    <OverflowMenu
                      items={taskMenu({ kind: 'station', id: infoStation.id }, info, 'info')}
                      title="명령 — 계획에 추가 · PICK/DROP/MOVE 작성"
                      testid="info-station-more"
                    />
                  </div>
                </>
              ) : null}
              {infoCell || infoStation ? (
                <details>
                  <summary className="cursor-pointer text-2xs font-semibold text-content-muted">
                    LGR_Cell_Info
                  </summary>
                  <div className="mt-1">
                    <PlcStructView rows={cellInfoRows(toPlcCell(infoCell ?? infoStation!.info))} />
                  </div>
                </details>
              ) : null}
            </div>
          </div>
        ) : null}
      </CellMap>
      <QuickStockDialog
        target={quick}
        items={items}
        onClose={() => setQuick(null)}
        onDetail={(q) => {
          const c =
            q.kind === 'cell'
              ? cellList.find((x) => x.id === q.id)
              : (() => {
                  const stn = stationList.find((x) => x.id === q.id)
                  return stn ? stationAsCell(stn) : undefined
                })()
          setQuick(null)
          if (c)
            setStockEdit({ cell: c, item: q.stock?.item_code || null, count: q.stock?.count ?? 0 })
        }}
      />
      {robotMenu.dialogs}
      <StockEditDialog
        edit={stockEdit}
        items={items}
        onClose={() => setStockEdit(null)}
        onItemsChanged={onItemsChanged}
      />
    </div>
  )
}

const READY_TONE_CLS = {
  ok: 'text-ok-fg',
  warn: 'text-warn-fg',
  bad: 'text-content-muted',
} as const

/** 정보 카드의 준비 한 줄 — `PICK ✓ · DROP ✗ 이유` + 예약. */
function ReadyRow({ t }: { t: TargetReady }) {
  const ready = t
  const pick = readyShort(t, 'pick')
  const drop = readyShort(t, 'drop')
  return (
    <div className="flex flex-col gap-0.5 text-2xs" data-testid="info-ready">
      <div className="flex min-w-0 flex-wrap gap-x-1.5">
        <span className={READY_TONE_CLS[pick.tone]}>{pick.text}</span>
        <span className="text-content-faint">·</span>
        <span className={READY_TONE_CLS[drop.tone]}>{drop.text}</span>
      </div>
      {(['pick', 'drop'] as const)
        .filter(
          (op) =>
            ready &&
            ready.kind === 'station' &&
            readyMark(ready, op) !== 'none' &&
            ready[op].expr?.length,
        )
        .map((op) => (
          <span key={op} className="flex min-w-0 items-center gap-1.5">
            <span className="text-content-muted">{op.toUpperCase()}</span>
            <CondExpr expr={ready?.[op].expr} />
          </span>
        ))}

      {t.holds.map((h, i) => (
        <span key={i} className="truncate text-content-muted">
          {holdLine(h)}
        </span>
      ))}
    </div>
  )
}
