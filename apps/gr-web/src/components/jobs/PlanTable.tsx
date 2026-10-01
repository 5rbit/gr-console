// 수동작업 계획 표 — 맵 클릭(PICK → DROP → …) · 측정 경로 · 팔렛이 쌓는 **보내기 전** 스텝. 옛 계획 카드의 표를 되살렸다
// (2026-10-01 운전자: 짝이 되자마자 대기열로 가 고칠 틈이 없다). 칸이 곧 편집 컨트롤이고 ✎ 는 팝업, 대기열에는 카드의
// [대기열로] · [다음 1건] 이 넣는다. 순서가 곧 뜻이라 정렬은 켜지 않는다.
import { useEffect, useMemo, useState } from 'react'
import { ArrowDown, ArrowUp, ChevronDown, Pencil, X } from 'lucide-react'
import { taskDataRows } from '../../lib/gr/plcShape'
import { itemLabel } from '../../lib/items/model'
import { f1 } from '../../lib/meas/format'
import { robotLabel, type RobotChipModel } from '../../lib/robotContext'
import { robots } from '../../lib/robots'
import { taskApi } from '../../lib/task/api'
import { MOVE_MODES, moveOf, moveUsesItem } from '../../lib/task/moveMode'
import {
  autoMeasureMode,
  move,
  patch,
  planRows,
  remove,
  removeWithPair,
  retype,
  toRequest,
  type PlanRow,
  type PlanStep,
} from '../../lib/task/plan'
import type { ComposePreview } from '../../lib/task/types'
import type {
  Cell,
  GripRef,
  Item,
  MeasureMode,
  Station,
  StockEntry,
  TaskType,
} from '../../lib/types'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { Input } from '../../lib/ui/Input'
import { Select } from '../../lib/ui/Select'
import type { Column } from '../../lib/ui/table'
import { cn } from '../../lib/utils'
import { PlcStructView } from '../shared/PlcStructView'
import { RobotChip } from '../shared/RobotChip'
import { PlanStepDialog } from '../task/PlanStepDialog'

const TYPES: TaskType[] = ['PICK', 'DROP', 'MEASURE', 'MOVE']

/** 종류 표식 — PICK/DROP 두 색이 번갈아 서는 것이 이 표를 훑는 기준이다. */
const TYPE_BAR: Record<string, string> = {
  PICK: 'bg-info',
  DROP: 'bg-ok',
}

function measureLabel(m: MeasureMode): string {
  return m === 'sku' ? 'SKU' : m === 'floor' ? 'Floor (Teaching)' : 'Item'
}

function moveLabel(s: PlanStep): string {
  const m = moveOf(s).mode
  return MOVE_MODES.find((x) => x.id === m)?.label ?? m
}

/** 펼친 스텝의 `LGR_Task_Data` 미리보기 — 백엔드 compose(그 스텝 시점의 재고 기준). 펼쳤을 때만 요청한다. */
function StepPreview({ row }: { row: PlanRow }) {
  const [p, setP] = useState<ComposePreview | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const req = toRequest(row, robots.selected, row.multiPick)
  const before = row.stockBefore
  useEffect(() => {
    let alive = true
    setP(null)
    setErr(null)
    taskApi
      .compose(req, before ?? null)
      .then((v) => alive && setP(v))
      .catch((e) => alive && setErr(e instanceof Error ? e.message : String(e)))
    return () => {
      alive = false
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 요청 본문은 매 렌더 새 객체라 값으로 비교한다
  }, [JSON.stringify(req), before])
  if (err) return <span className="text-fault-fg">{err}</span>
  if (!p) return <span className="text-content-faint">compose 중…</span>
  return (
    <>
      <PlcStructView
        title={`LGR_Task_Data — 스텝 ${row.no} (재고 ${row.stockBefore ?? '?'} 가정 compose)`}
        rows={taskDataRows(p.task)}
      />
      {p.warnings.length ? (
        <div className="mt-1 text-2xs text-warn-fg">경고: {p.warnings.join(' · ')}</div>
      ) : null}
    </>
  )
}

/**
 * ItemCode 칸 — 펼친 목록은 사양까지(`itemLabel`), 닫힌 칸은 코드만. 네이티브 Select 는 가장 긴 항목에 폭을 맞춰
 * 표를 밀어내므로 코드만 쓴 칸 위에 투명한 select 를 겹친다.
 */
function ItemCodeSelect({
  value,
  items,
  onChange,
}: {
  value: number | null
  items: readonly Item[]
  onChange: (code: number | null) => void
}) {
  const it = value === null ? undefined : items.find((i) => i.code === value)
  const full = it ? itemLabel(it) : value === null ? '' : `${value} (목록에 없음)`
  return (
    <span
      className="relative inline-flex h-control-sm min-w-16 items-center gap-1 rounded-md border border-line-strong px-2 text-xs focus-within:border-focus focus-within:ring-2 focus-within:ring-focus"
      title={full || undefined}
      data-testid="plan-item-code"
    >
      <span className={cn('tabular-nums', value === null && 'text-content-faint')}>
        {value ?? '(없음)'}
      </span>
      <ChevronDown className="ml-auto h-3 w-3 text-content-muted" aria-hidden="true" />
      <select
        className="absolute inset-0 h-full w-full cursor-pointer opacity-0"
        value={value === null ? '' : String(value)}
        onChange={(e) => onChange(e.target.value === '' ? null : Number(e.target.value))}
        aria-label="ItemCode"
      >
        <option value="">(없음)</option>
        {value !== null && !it ? <option value={String(value)}>{full}</option> : null}
        {items.map((i) => (
          <option key={i.code} value={String(i.code)}>
            {itemLabel(i)}
          </option>
        ))}
      </select>
    </span>
  )
}

export function PlanTable({
  steps,
  onChange,
  cells,
  stations,
  items,
  stockNow,
  gripRef,
  hand,
  robot,
}: {
  steps: readonly PlanStep[]
  onChange: (next: PlanStep[]) => void
  cells: readonly Cell[]
  stations: readonly Station[]
  items: readonly Item[]
  stockNow: ReadonlyMap<number, StockEntry>
  gripRef: GripRef
  /** 계획 시작 때 로봇이 들고 있을 화물(예상 Hand). */
  hand: { item_code: number; count: number } | null
  /** 로봇 없는 스텝이 갈 곳(고른 로봇). */
  robot: RobotChipModel
}) {
  const runRobot = robots.selected
  const rows = useMemo(
    () =>
      planRows(steps, {
        cells,
        stations,
        items,
        stockNow,
        gripRef,
        robot: runRobot,
        hand,
        robotName: (id) => (id === null ? robot.name : robots.nameOf(id)),
      }),
    [steps, cells, stations, items, stockNow, gripRef, runRobot, hand, robot.name],
  )
  const [editing, setEditing] = useState<string | null>(null)
  const [deleting, setDeleting] = useState<string | null>(null)
  const editRow = editing ? (rows.find((r) => r.id === editing) ?? null) : null
  const set = (id: string, p: Partial<Omit<PlanStep, 'id'>>) => onChange(patch(steps, id, p))

  const cols: Column<PlanRow>[] = [
    {
      key: 'no',
      label: '#',
      name: '순서',
      sortable: false,
      priority: 1,
      cell: (r) => (
        <span className="flex items-center gap-1.5">
          <span
            aria-hidden="true"
            className={cn('inline-block h-3.5 w-1 rounded', TYPE_BAR[r.type] ?? 'bg-line-strong')}
          />
          <span className="font-mono tabular-nums">{r.no}</span>
        </span>
      ),
    },
    {
      key: 'type',
      label: 'Type',
      sortable: false,
      priority: 1,
      cell: (r) => (
        <span className="flex items-center gap-1">
          <Select
            dense
            className="w-auto"
            value={r.type}
            onValueChange={(v) => set(r.id, retype(r, v as TaskType))}
            aria-label="Type"
          >
            {TYPES.map((t) => (
              <option key={t} value={t}>
                {t}
              </option>
            ))}
          </Select>
          {r.type === 'MOVE' ? (
            <span className="text-3xs text-content-muted">{moveLabel(r)}</span>
          ) : null}
          {r.type === 'MEASURE' ? (
            <Select
              dense
              className="w-auto"
              value={r.measure ?? ''}
              onValueChange={(v) => set(r.id, { measure: v === '' ? null : (v as MeasureMode) })}
              aria-label="Measure"
              title="MEASURE 종류 — auto: 셀 재고 1개 = Item, 2개 이상 = SKU · Floor = 바닥 측정(Cell Teaching)"
            >
              <option value="">{`auto(${measureLabel(autoMeasureMode(r.stockBefore))})`}</option>
              <option value="item">Item</option>
              <option value="sku">SKU</option>
              <option value="floor">Floor (Teaching)</option>
            </Select>
          ) : null}
        </span>
      ),
    },
    {
      key: 'target',
      label: 'Target',
      sortable: false,
      priority: 1,
      cell: (r) => (
        <span className="whitespace-nowrap">
          {r.target.kind === 'cell' ? 'Cell' : 'Station'}{' '}
          <b className="font-mono">#{r.target.id}</b>
          {r.multiPick ? (
            <span
              className="ml-1 text-3xs text-accent-text"
              title="Multi-Picking — 다음 스텝이 같은 스테이션 그룹: 기본값 Multi-Pick 층(부분 리프트)을 얹어 보냅니다"
            >
              MP
            </span>
          ) : null}
        </span>
      ),
    },
    ...(robots.multi
      ? [
          {
            key: 'robot',
            label: 'Robot',
            sortable: false,
            priority: 2 as const,
            cell: (r: PlanRow) => (
              <Select
                dense
                value={r.robot === null || r.robot === undefined ? '' : String(r.robot)}
                onValueChange={(v) => set(r.id, { robot: v === '' ? null : Number(v) })}
                aria-label="Robot"
              >
                <option value="">기본</option>
                {robots.list.map((rb) => (
                  <option key={rb.id} value={String(rb.id)}>
                    {rb.name}
                  </option>
                ))}
              </Select>
            ),
          },
        ]
      : []),
    {
      key: 'item',
      label: 'ItemCode',
      sortable: false,
      priority: 2,
      cell: (r) =>
        r.type === 'MOVE' && !moveUsesItem(moveOf(r)) ? (
          <span className="text-content-faint">-</span>
        ) : (
          <ItemCodeSelect
            value={r.item_code}
            items={items}
            onChange={(item_code) => set(r.id, { item_code })}
          />
        ),
    },
    {
      key: 'count',
      label: 'Count',
      sortable: false,
      numeric: true,
      priority: 1,
      cell: (r) =>
        r.type === 'MOVE' ? (
          <span className="text-content-faint">-</span>
        ) : (
          <Input
            dense
            type="number"
            mono
            min={1}
            max={20}
            step="1"
            className="w-14"
            value={String(r.count)}
            onValueChange={(s) => set(r.id, { count: Math.max(1, Number(s) || 1) })}
            aria-label="Count"
          />
        ),
    },
    {
      key: 'stock',
      label: 'Stock',
      sortable: false,
      numeric: true,
      priority: 3,
      help: '이 스텝 **직전 → 직후**의 재고. 앞 스텝들을 순서대로 적용한 시뮬레이션.',
      cell: (r) => (
        <span className="font-mono tabular-nums text-content-muted">
          {r.stockBefore === null ? '-' : `${r.stockBefore}→${r.stockAfter}`}
        </span>
      ),
    },
    {
      key: 'z',
      label: 'Z (mm)',
      sortable: false,
      numeric: true,
      priority: 2,
      help: '바닥 + 품목 높이 × 재고 + 그립 기준(⋯ 메뉴). 품목 높이를 모르면 `?` 로 두고 계산하지 않는다.',
      cell: (r) => (
        <span
          className="font-mono tabular-nums"
          title={r.height !== null ? `Floor ${r.floor} · H ${r.height}` : ''}
        >
          {r.z === null ? <span className="text-content-faint">?</span> : f1(r.z)}
        </span>
      ),
    },
    {
      key: 'warn',
      label: 'Warnings',
      sortable: false,
      priority: 2,
      cell: (r) =>
        r.warnings.length ? (
          <span
            className="inline-flex h-5 min-w-5 items-center justify-center rounded bg-warn-soft px-1 text-3xs font-semibold text-warn-fg"
            title={r.warnings.join(' · ')}
            data-testid={`plan-warn-${r.no}`}
          >
            {r.warnings.length}
          </span>
        ) : null,
    },
  ]

  const doomed = deleting ? removeWithPair(steps, deleting, runRobot) : null
  const doomedStep = deleting ? steps.find((x) => x.id === deleting) : undefined
  const doomedChip =
    doomedStep?.robot !== null && doomedStep?.robot !== undefined
      ? robots.chipOf(doomedStep.robot)
      : robot

  return (
    <>
      <DataTable
        rows={rows}
        columns={cols}
        rowKey={(r) => r.id}
        density="compact"
        stickyHeader
        empty="계획 없음"
        emptyDense
        testid="plan-table"
        rowDetail={(r) => <StepPreview row={r} />}
        actions={(r) => (
          <>
            <Button
              size="icon-sm"
              intent="ghost"
              icon={<Pencil className="h-3.5 w-3.5" />}
              title="편집"
              onClick={(e) => {
                e.stopPropagation()
                setEditing(r.id)
              }}
              data-testid={`plan-edit-${r.no}`}
            />
            <Button
              size="icon-sm"
              intent="ghost"
              icon={<ArrowUp className="h-3.5 w-3.5" />}
              title="위로"
              disabled={r.no === 1}
              onClick={(e) => {
                e.stopPropagation()
                onChange(move(steps, r.no - 1, r.no - 2))
              }}
            />
            <Button
              size="icon-sm"
              intent="ghost"
              icon={<ArrowDown className="h-3.5 w-3.5" />}
              title="아래로"
              disabled={r.no === rows.length}
              onClick={(e) => {
                e.stopPropagation()
                onChange(move(steps, r.no - 1, r.no))
              }}
            />
            <Button
              size="icon-sm"
              intent="ghost"
              icon={<X className="h-3.5 w-3.5" />}
              title="삭제"
              onClick={(e) => {
                e.stopPropagation()
                // PICK/DROP 은 짝을 같이 지우므로 한 번 묻는다.
                if (r.type === 'PICK' || r.type === 'DROP') setDeleting(r.id)
                else onChange(remove(steps, r.id))
              }}
              data-testid={`plan-del-${r.no}`}
            />
          </>
        )}
      />
      {editRow ? (
        <PlanStepDialog
          key={editRow.id}
          step={steps.find((s) => s.id === editRow.id) ?? editRow}
          no={editRow.no}
          onClose={() => setEditing(null)}
          onApply={(next) => onChange(patch(steps, next.id, next))}
          cells={cells}
          stations={stations}
          items={items}
          robot={robot}
        />
      ) : null}
      {doomed ? (
        <ConfirmDialog
          open
          onOpenChange={(o) => {
            if (!o) setDeleting(null)
          }}
          scope="single"
          title={`스텝 삭제 — ${doomedChip.name}`}
          confirmLabel="삭제"
          onConfirm={() => {
            onChange(doomed.next)
            setDeleting(null)
          }}
        >
          <div className="flex flex-col gap-2 text-xs">
            <span className="flex items-center gap-2">
              <RobotChip chip={doomedChip} title={robotLabel(doomedChip)} />
              {doomed.removed
                .map(
                  (x) =>
                    `${steps.indexOf(x) + 1}. ${x.type} ${x.target.kind === 'cell' ? 'Cell' : 'Station'} #${x.target.id}`,
                )
                .join(' · ')}
            </span>
            {doomed.removed.length > 1 ? (
              <span className="text-warn-fg">PICK/DROP 짝이라 같이 지웁니다</span>
            ) : null}
          </div>
        </ConfirmDialog>
      ) : null}
    </>
  )
}
