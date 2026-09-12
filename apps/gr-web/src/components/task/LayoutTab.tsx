// 레이아웃 탭 — 맵 + 동작 모드 토글 + 편집기 패널 + 정보 패널.
//   명령 생성 모드: 좌클릭마다 PICK → DROP → … 으로 순차 계획에 쌓인다(우클릭은 팔레트).
//   모니터링 모드:  좌클릭 = 셀 정보·화물 정보 패널, 우클릭 = 명령 팔레트(PICK/DROP/MEASURE/MOVE 작성, 계획 추가, 재고·셀 편집).
import { useCallback, useState } from 'react'
import { Eye, ListPlus, Wand2 } from 'lucide-react'
import type { Registry } from '../../lib/registry'
import { stock as stockStore } from '../../lib/stock'
import { useStore } from '../../lib/store'
import type { Shape } from '../../lib/task/layoutModel'
import type { PlanStep } from '../../lib/task/plan'
import { nextType } from '../../lib/task/plan'
import { cellInfoRows, toPlcCell } from '../../lib/gr/plcShape'
import { Button } from '../../lib/ui/Button'
import { ctxMenu, type MenuItem } from '../../lib/ui/menu'
import type { Cell, CellUpsert, Item, Station, Target, TaskType } from '../../lib/types'
import { cn } from '../../lib/utils'
import { PlcStructView } from '../shared/PlcStructView'
import { CellMap } from './CellMap'
import { LayoutEditor } from './LayoutEditor'
import { StockEditDialog, type StockEdit } from './StockRegistry'

export type MapMode = 'plan' | 'monitor'
const MODE_KEY = 'gr-cellmap-mode'

export interface LayoutTabProps {
  cells: Registry<Cell>
  stations: Registry<Station>
  items: readonly Item[]
  plan: readonly PlanStep[]
  /** 작성 카드 대상(강조). */
  selected: Target | null
  /** 명령 생성 모드 좌클릭 / 팔레트 "계획에 추가". */
  onPlanAdd: (target: Target, shape: Shape, type?: TaskType) => void
  /** 팔레트 "단일 명령 작성" — 작성 카드에 종류+대상. */
  onCompose: (target: Target, shape: Shape, type: TaskType) => void
  /** 셀 편집(셀 탭의 폼을 연다). */
  onEditCell?: (cell: Cell) => void
  /** 재고 편집에서 품목을 새로 등록했을 때. */
  onItemsChanged?: () => void
}

export function LayoutTab({
  cells,
  stations,
  items,
  plan,
  selected,
  onPlanAdd,
  onCompose,
  onEditCell,
  onItemsChanged,
}: LayoutTabProps) {
  useStore(stockStore)
  const [mode, setMode] = useState<MapMode>(() => {
    try {
      return (localStorage.getItem(MODE_KEY) as MapMode) || 'plan'
    } catch {
      return 'plan'
    }
  })
  const [editor, setEditor] = useState(false)
  const [preview, setPreview] = useState<CellUpsert[]>([])
  const [info, setInfo] = useState<Shape | null>(null)
  const [stockEdit, setStockEdit] = useState<StockEdit | null>(null)
  const onPreview = useCallback((c: CellUpsert[]) => setPreview(c), [])

  function switchMode(m: MapMode) {
    setMode(m)
    try {
      localStorage.setItem(MODE_KEY, m)
    } catch {
      /* 저장 못 해도 동작 */
    }
  }

  const infoCell =
    info?.kind === 'cell' ? (cells.items.find((c) => c.id === info.id) ?? null) : null
  const infoStation =
    info?.kind === 'station' ? (stations.items.find((s) => s.id === info.id) ?? null) : null
  const infoStock = infoCell ? stockStore.get(infoCell.id) : null
  const infoItem = infoStock?.item_code
    ? items.find((i) => i.code === infoStock.item_code)
    : undefined
  const next = nextType(plan)

  function palette(t: Target, shape: Shape, e: React.MouseEvent) {
    const cell = t.kind === 'cell' ? cells.items.find((c) => c.id === t.id) : undefined
    const st = cell ? stockStore.get(cell.id) : null
    const name = `${t.kind === 'cell' ? '셀' : '스테이션'} #${t.id}`
    const items_: MenuItem[] = [
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
    if (cell) {
      items_.push({
        label: '재고 편집…',
        run: () => setStockEdit({ cell, item: st?.item_code || null, count: st?.count ?? 0 }),
      })
      if (onEditCell) items_.push({ label: '셀 편집…', run: () => onEditCell(cell) })
    }
    items_.push({ label: '정보 보기', run: () => setInfo(shape) })
    ctxMenu.show(e, items_)
  }

  const modeBar = (
    <div
      className="ml-2 inline-flex rounded-md border border-slate-300 p-0.5 dark:border-slate-600"
      role="tablist"
      aria-label="맵 동작"
    >
      <button
        type="button"
        role="tab"
        aria-selected={mode === 'plan'}
        className={cn(
          'flex items-center gap-1 rounded px-2 py-0.5 text-xs',
          mode === 'plan'
            ? 'bg-indigo-600 text-white'
            : 'text-slate-600 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800',
        )}
        onClick={() => switchMode('plan')}
        data-testid="map-mode-plan"
        title="좌클릭마다 PICK → DROP 순으로 계획에 쌓는다"
      >
        <ListPlus className="h-3.5 w-3.5" /> 명령 생성{' '}
        <span className="font-mono opacity-80">{next}</span>
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={mode === 'monitor'}
        className={cn(
          'flex items-center gap-1 rounded px-2 py-0.5 text-xs',
          mode === 'monitor'
            ? 'bg-indigo-600 text-white'
            : 'text-slate-600 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800',
        )}
        onClick={() => switchMode('monitor')}
        data-testid="map-mode-monitor"
        title="좌클릭 = 셀·화물 정보, 우클릭 = 명령 팔레트"
      >
        <Eye className="h-3.5 w-3.5" /> 모니터링
      </button>
    </div>
  )

  return (
    <div className="flex h-full min-h-0" data-testid="layout-tab">
      <div className="min-w-0 flex-1">
        <CellMap
          cells={cells.items}
          stations={stations.items}
          selected={selected}
          stock={stockStore.map}
          plan={plan}
          preview={preview}
          onPick={(t, shape) => {
            if (mode === 'plan') onPlanAdd(t, shape)
            else setInfo(shape)
          }}
          onContext={palette}
          extra={
            <>
              {modeBar}
              <Button
                size="sm"
                intent={editor ? 'primary' : 'ghost'}
                active={editor}
                icon={<Wand2 className="h-3.5 w-3.5" />}
                onClick={() => setEditor((e) => !e)}
                data-testid="map-editor-toggle"
                title="레이아웃 생성 규칙 편집기"
              >
                편집기
              </Button>
            </>
          }
        />
      </div>
      {editor ? (
        <LayoutEditor
          cells={cells.items}
          onPreview={onPreview}
          onApplied={() => void cells.reload()}
          onClose={() => setEditor(false)}
        />
      ) : null}
      {info && !editor ? (
        <div
          className="flex w-[280px] flex-none flex-col gap-2 overflow-y-auto border-l border-slate-200 p-3 text-xs dark:border-slate-700"
          data-testid="map-info"
        >
          <div className="flex items-center gap-2">
            <span className="text-sm font-semibold">
              {info.kind === 'cell' ? '셀' : '스테이션'} #{info.id}
            </span>
            <span className="flex-1" />
            <Button size="sm" intent="ghost" onClick={() => setInfo(null)}>
              닫기
            </Button>
          </div>
          <PlcStructView
            title="LGR_Cell_Info"
            rows={cellInfoRows(toPlcCell(infoCell ?? infoStation!.info))}
          />
          <div className="text-[11px] text-slate-500">
            상태:{' '}
            {infoCell
              ? infoCell.dirty
                ? '로컬 수정 (PLC 미반영)'
                : infoCell.source === 'plc'
                  ? 'PLC 동일'
                  : '로컬'
              : infoStation
                ? infoStation.dirty
                  ? '로컬 수정 (PLC 미반영)'
                  : infoStation.source === 'plc'
                    ? 'PLC 동일'
                    : '로컬'
                : ''}
            {infoStation
              ? ` · CV${infoStation.conv_no} · G${infoStation.group}-${infoStation.group_index} · TaskType ${infoStation.task_type}`
              : ''}
          </div>
          {infoCell ? (
            <>
              <div className="mt-1 text-[11px] font-semibold text-slate-500">화물 정보</div>
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
                      ['타이어 높이', infoItem ? `${infoItem.height} mm` : '-'],
                      [
                        '적재 높이',
                        infoItem && infoStock
                          ? `${(infoItem.height * infoStock.count).toFixed(0)} mm`
                          : '-',
                      ],
                      [
                        'PICK Z (맨 위)',
                        infoItem && infoStock?.count
                          ? `${(info.z + infoItem.height * (infoStock.count - 1) + infoItem.height / 2).toFixed(0)}`
                          : '-',
                      ],
                      [
                        'DROP Z (다음)',
                        infoItem
                          ? `${(info.z + infoItem.height * (infoStock?.count ?? 0) + infoItem.height / 2).toFixed(0)}`
                          : '-',
                      ],
                      ['갱신', infoStock?.updated_at?.slice(0, 19).replace('T', ' ') ?? '-'],
                    ] as [string, string][]
                  ).map(([k, v]) => (
                    <tr key={k} className="border-b border-slate-100 dark:border-slate-800">
                      <td className="py-1 pr-2 text-slate-500">{k}</td>
                      <td className="py-1 font-mono">{v}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <div className="flex flex-wrap gap-1">
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
                <Button
                  size="sm"
                  intent="outline"
                  onClick={() => onPlanAdd({ kind: 'cell', id: infoCell.id }, info)}
                >
                  계획에 추가 ({next})
                </Button>
              </div>
            </>
          ) : null}
          <p className="text-[11px] text-slate-400">우클릭하면 명령 팔레트가 열립니다.</p>
        </div>
      ) : null}
      <StockEditDialog
        edit={stockEdit}
        items={items}
        onClose={() => setStockEdit(null)}
        onItemsChanged={onItemsChanged}
      />
    </div>
  )
}
