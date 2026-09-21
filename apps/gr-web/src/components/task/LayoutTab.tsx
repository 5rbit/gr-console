// 레이아웃 탭 — 맵과 세 동작 모드. 모드 토글과 보기 조작은 모두 플롯 안(아이콘·팝업)에 둔다.
//   명령 생성:     좌클릭마다 PICK → DROP → … 순으로 계획에 쌓인다
//   모니터링:      좌클릭 = 셀·화물 정보 카드(플롯 안), 우클릭 = 명령 팔레트
//   레이아웃 편집: 좌클릭 = 우측 사이드바 셀/스테이션 리스트에서 선택, 생성 예정 셀 표시
// 로봇이 작업 중인 셀은 그 로봇 색 테두리(대기 = 점선), 로봇 위치는 같은 색 십자.
import { useEffect, useMemo, useState } from 'react'
import { Eye, ListPlus, Wand2, X } from 'lucide-react'
import { allStatus } from '../../lib/feeds'
import { TASK_TYPE_CODE } from '../../lib/gr/const'
import { cellInfoRows, toPlcCell } from '../../lib/gr/plcShape'
import type { Registry } from '../../lib/registry'
import { robotColor, robots } from '../../lib/robots'
import { stock as stockStore } from '../../lib/stock'
import { useStore } from '../../lib/store'
import { tasks } from '../../lib/tasks'
import type { Shape } from '../../lib/task/layoutModel'
import { gripOffset, nextType, type PlanStep } from '../../lib/task/plan'
import { Button } from '../../lib/ui/Button'
import { InfoRows } from '../../lib/ui/Pair'
import { f1 } from '../../lib/meas/format'
import { ctxMenu, type MenuItem } from '../../lib/ui/menu'
import { Segmented } from '../../lib/ui/Segmented'
import type { Cell, CellUpsert, GripRef, Item, Station, Target, TaskType } from '../../lib/types'
import { PlcStructView } from '../shared/PlcStructView'
import { CellMap, type RobotMarker, type WorkMark } from './CellMap'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import { StockEditDialog, type StockEdit } from './StockRegistry'

export type MapMode = 'plan' | 'monitor' | 'edit'
export const MAP_MODES: readonly MapMode[] = ['plan', 'monitor', 'edit']

const TYPE_NAME: Record<number, string> = Object.fromEntries(
  Object.entries(TASK_TYPE_CODE).map(([k, v]) => [v, k]),
)
const isStation = (id: number) => id >= 2001 && id <= 2999

export interface LayoutTabProps {
  cells: Registry<Cell>
  stations: Registry<Station>
  items: readonly Item[]
  plan: readonly PlanStep[]
  /** 강조할 대상. */
  selected: Target | null
  mode: MapMode
  onModeChange: (m: MapMode) => void
  /** 생성 예정 셀(편집 모드). */
  preview: readonly CellUpsert[]
  /** 이 대상으로 화면 이동. */
  focus?: { target: Target; nonce: number } | null
  gripRef?: GripRef
  /** 명령 생성 모드 좌클릭 / 팔레트 "계획에 추가". */
  onPlanAdd: (target: Target, shape: Shape, type?: TaskType) => void
  /** 팔레트 "명령 작성" — 작성 카드에 종류+대상. */
  onCompose: (target: Target, shape: Shape, type: TaskType) => void
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
  mode,
  onModeChange,
  preview,
  focus,
  gripRef = 'mid',
  onPlanAdd,
  onCompose,
  onEditSelect,
  onTargetPick,
  onItemsChanged,
  mapCells,
  mapStations,
}: LayoutTabProps) {
  const cellList = mapCells ?? cells.items
  const stationList = mapStations ?? stations.items
  useStore(stockStore, tasks, robots, allStatus)
  useEffect(() => allStatus.start(), [])
  useEffect(() => tasks.start(), [])
  const [info, setInfo] = useState<Shape | null>(null)
  const [stockEdit, setStockEdit] = useState<StockEdit | null>(null)
  const next = nextType(plan)

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
      const rb = robots.list.find((r) => r.plc === t.plc_name) ?? robots.current
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
    return [
      {
        id: r?.id ?? 0,
        name: r?.name ?? ev?.plc ?? 'GR',
        color: robotColor(r?.id),
        x: axis[0].Position,
        y: axis[1].Position,
        moving: axis[0].Running || axis[1].Running,
      },
    ]
  })
  const robotLegend = robots.list.map((r) => ({ name: r.name, color: robotColor(r.id) }))

  const infoCell = info?.kind === 'cell' ? (cellList.find((c) => c.id === info.id) ?? null) : null
  const infoStation =
    info?.kind === 'station' ? (stationList.find((s) => s.id === info.id) ?? null) : null
  const infoStock = infoCell ? stockStore.get(infoCell.id) : null
  const infoItem = infoStock?.item_code
    ? items.find((i) => i.code === infoStock.item_code)
    : undefined
  const infoWork = info ? work.get(`${info.kind}-${info.id}`) : undefined

  /**
   * 한 대상에 걸 수 있는 명령 목록 — 우클릭 팔레트와 정보 카드의 ⋯ 가 **같은 것**을 쓴다.
   * (두 자리에서 항목이 갈라지면 "오른쪽 클릭으로만 되는 것"이 생긴다.)
   */
  function taskMenu(t: Target, shape: Shape, from: 'palette' | 'info' = 'palette'): MenuItem[] {
    const cell = t.kind === 'cell' ? cellList.find((c) => c.id === t.id) : undefined
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
    ]
    // 정보 카드에서 연 메뉴에는 재고 편집·정보 보기를 넣지 않는다 — 그 둘은 이미 카드가 하고 있다.
    if (from === 'palette') {
      if (cell)
        menu.push({
          label: '재고 편집…',
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
          label: '명령 생성',
          badge: mode === 'plan' ? next : '',
          title: '좌클릭마다 PICK → DROP 순으로 계획에 쌓인다',
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

  const g = infoItem ? gripOffset(gripRef, infoItem) : 0

  return (
    <div className="h-full min-h-0" data-testid="layout-tab">
      <CellMap
        cells={cellList}
        stations={stationList}
        selected={mode === 'monitor' && info ? { kind: info.kind, id: info.id } : selected}
        stock={stockStore.map}
        plan={mode === 'plan' ? plan : undefined}
        preview={mode === 'edit' ? preview : undefined}
        work={work}
        robots={markers}
        robotLegend={robotLegend}
        focus={focus}
        topLeft={modeToggle}
        onPick={(t, shape) => {
          if (mode === 'plan') onPlanAdd(t, shape)
          else if (mode === 'monitor') {
            setInfo(shape)
            onTargetPick?.(t)
          } else onEditSelect?.(t)
        }}
        onContext={palette}
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
                <div className="text-content-muted">
                  CV{infoStation.conv_no} · G{infoStation.group}-{infoStation.group_index} ·
                  TaskType {infoStation.task_type}
                </div>
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
      <StockEditDialog
        edit={stockEdit}
        items={items}
        onClose={() => setStockEdit(null)}
        onItemsChanged={onItemsChanged}
      />
    </div>
  )
}
