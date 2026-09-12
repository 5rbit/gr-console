// 레이아웃 탭 — 맵과 세 동작 모드. 모드 토글과 보기 조작은 모두 플롯 안(아이콘·팝업)에 둔다.
//   명령 생성:     좌클릭마다 PICK → DROP → … 순으로 계획에 쌓인다
//   모니터링:      좌클릭 = 셀·화물 정보 카드(플롯 안), 우클릭 = 명령 팔레트
//   레이아웃 편집: 좌클릭 = 우측 사이드바 셀/스테이션 리스트에서 선택, 생성 예정 셀 표시
// 로봇이 작업 중인 셀은 그 로봇 색 테두리(대기 = 점선), 로봇 위치는 같은 색 십자.
import { useEffect, useMemo, useState } from 'react'
import { Eye, ListPlus, Wand2, X } from 'lucide-react'
import { statusFeed } from '../../lib/feeds'
import { TASK_TYPE_CODE } from '../../lib/gr/const'
import { cellInfoRows, toPlcCell } from '../../lib/gr/plcShape'
import type { Registry } from '../../lib/registry'
import { robotColor, robots } from '../../lib/robots'
import { useSse } from '../../lib/sse'
import { stock as stockStore } from '../../lib/stock'
import { useStore } from '../../lib/store'
import { tasks } from '../../lib/tasks'
import type { Shape } from '../../lib/task/layoutModel'
import { gripOffset, nextType, type PlanStep } from '../../lib/task/plan'
import { Button } from '../../lib/ui/Button'
import { ctxMenu, type MenuItem } from '../../lib/ui/menu'
import { Segmented } from '../../lib/ui/Segmented'
import type { Cell, CellUpsert, GripRef, Item, Station, Target, TaskType } from '../../lib/types'
import { PlcStructView } from '../shared/PlcStructView'
import { CellMap, type RobotMarker, type WorkMark } from './CellMap'
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
  onItemsChanged,
  mapCells,
  mapStations,
}: LayoutTabProps) {
  const cellList = mapCells ?? cells.items
  const stationList = mapStations ?? stations.items
  useStore(stockStore, tasks, robots)
  useSse(statusFeed)
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

  // 로봇 위치 — 상태 스트림은 기본 로봇(상태 PLC) 것.
  const axis = statusFeed.data?.webmon?.Axis
  const def = robots.list.find((r) => r.default) ?? robots.current
  const markers: RobotMarker[] =
    axis && axis.length >= 2
      ? [
          {
            id: def?.id ?? 0,
            name: def?.name ?? 'GR',
            color: robotColor(def?.id),
            x: axis[0].Position,
            y: axis[1].Position,
            moving: axis[0].Running || axis[1].Running,
          },
        ]
      : []
  const robotLegend = robots.list.map((r) => ({ name: r.name, color: robotColor(r.id) }))

  const infoCell = info?.kind === 'cell' ? (cellList.find((c) => c.id === info.id) ?? null) : null
  const infoStation =
    info?.kind === 'station' ? (stationList.find((s) => s.id === info.id) ?? null) : null
  const infoStock = infoCell ? stockStore.get(infoCell.id) : null
  const infoItem = infoStock?.item_code
    ? items.find((i) => i.code === infoStock.item_code)
    : undefined
  const infoWork = info ? work.get(`${info.kind}-${info.id}`) : undefined

  function palette(t: Target, shape: Shape, e: React.MouseEvent) {
    const cell = t.kind === 'cell' ? cellList.find((c) => c.id === t.id) : undefined
    const st = cell ? stockStore.get(cell.id) : null
    const name = `${t.kind === 'cell' ? '셀' : '스테이션'} #${t.id}`
    const menu: MenuItem[] = [
      {
        label: `${name}${st ? ` · 재고 ${st.count}${st.item_code ? ` (품목 ${st.item_code})` : ''}` : ''}`,
      },
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
    if (cell)
      menu.push({
        label: '재고 편집…',
        run: () => setStockEdit({ cell, item: st?.item_code || null, count: st?.count ?? 0 }),
      })
    menu.push({ label: '정보 보기', run: () => setInfo(shape) })
    ctxMenu.show(e, menu)
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
          title: '명령 생성 — 좌클릭마다 PICK → DROP 순으로 계획에 쌓는다',
          testid: 'map-mode-plan',
        },
        {
          id: 'monitor',
          icon: <Eye size={14} />,
          label: '모니터링',
          title: '모니터링 — 좌클릭 = 셀·화물 정보, 우클릭 = 명령 팔레트',
          testid: 'map-mode-monitor',
        },
        {
          id: 'edit',
          icon: <Wand2 size={14} />,
          label: '레이아웃 편집',
          title: '레이아웃 편집 — 우측에 셀/스테이션 리스트와 생성 규칙',
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
          else if (mode === 'monitor') setInfo(shape)
          else onEditSelect?.(t)
        }}
        onContext={palette}
      >
        {mode === 'monitor' && info ? (
          <div
            className="absolute top-2 right-12 z-10 flex max-h-[calc(100%-1rem)] w-72 flex-col overflow-hidden rounded-lg border border-slate-200 bg-white/95 shadow-lg dark:border-slate-700 dark:bg-slate-900/95"
            data-testid="map-info"
          >
            <div className="flex h-10 flex-none items-center gap-2 border-b border-slate-200 px-3 dark:border-slate-700">
              <span className="text-sm font-semibold">
                {info.kind === 'cell' ? '셀' : '스테이션'} #{info.id}
              </span>
              {infoWork ? (
                <span
                  className="rounded px-1.5 py-0.5 text-3xs font-semibold text-white"
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
                  <table className="w-full">
                    <tbody>
                      {(
                        [
                          ['재고', String(infoStock?.count ?? 0)],
                          [
                            '품목',
                            infoStock?.item_code
                              ? `${infoStock.item_code} ${infoItem?.name ?? ''}`
                              : '-',
                          ],
                          [
                            '적재 높이',
                            infoItem && infoStock
                              ? `${(infoItem.height * infoStock.count).toFixed(0)} mm`
                              : '-',
                          ],
                          [
                            'PICK Z',
                            infoItem && infoStock?.count
                              ? (info.z + infoItem.height * (infoStock.count - 1) + g).toFixed(0)
                              : '-',
                          ],
                          [
                            'DROP Z',
                            infoItem
                              ? (info.z + infoItem.height * (infoStock?.count ?? 0) + g).toFixed(0)
                              : '-',
                          ],
                        ] as [string, string][]
                      ).map(([k, val]) => (
                        <tr
                          key={k}
                          className="border-b border-slate-100 last:border-0 dark:border-slate-800"
                        >
                          <td className="h-7 pr-2 text-slate-500">{k}</td>
                          <td className="h-7 text-right font-mono tabular-nums">{val}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                  <div className="grid grid-cols-2 gap-1">
                    <Button
                      size="sm"
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
                    <Button
                      size="sm"
                      intent="outline"
                      onClick={() => onPlanAdd({ kind: 'cell', id: infoCell.id }, info)}
                    >
                      계획 추가 ({next})
                    </Button>
                    <Button
                      size="sm"
                      intent="outline"
                      onClick={() => onCompose({ kind: 'cell', id: infoCell.id }, info, 'PICK')}
                      disabled={!infoStock?.count}
                    >
                      PICK 작성
                    </Button>
                    <Button
                      size="sm"
                      intent="outline"
                      onClick={() => onCompose({ kind: 'cell', id: infoCell.id }, info, 'DROP')}
                    >
                      DROP 작성
                    </Button>
                  </div>
                </>
              ) : null}
              {infoStation ? (
                <div className="text-slate-500">
                  CV{infoStation.conv_no} · G{infoStation.group}-{infoStation.group_index} ·
                  TaskType {infoStation.task_type}
                </div>
              ) : null}
              {infoCell || infoStation ? (
                <details>
                  <summary className="cursor-pointer text-2xs font-semibold text-slate-500">
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
