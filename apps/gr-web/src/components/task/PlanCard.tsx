// 작업 카드 — 오른쪽 칸 **하나뿐인 작업면**이고 모드가 둘이다.
//   순차 계획: 레이아웃 클릭이 PICK → DROP → … 으로 쌓인 표. 순서/종류/품목/수량 편집, 다음 1건 즉시 제출.
//   단일 명령: 작성 카드(ComposeCard)를 같은 면에 끼운다.
// Z 는 재고 시뮬레이션으로 계산해 보여 준다.
//
// 전에는 카드 밖에 모드 토글이 한 줄 따로 서고 카드가 둘이었다 — 같은 자리에서 같은 일을 하는데
// 껍데기가 둘이면 머리띠도 둘이다. 모드를 카드 머리줄로 들여 한 줄을 없앴다.
// 그립 기준·되돌리기·다시실행·비우기는 ⋯ 로, 시나리오 이름은 저장 팝업으로 내렸다.
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import {
  ArrowDown,
  ArrowUp,
  ChevronDown,
  ListOrdered,
  Pause,
  Pencil,
  Play,
  Save,
  Send,
  X,
} from 'lucide-react'
import { api } from '../../lib/api'
import { visibleInterval } from '../../lib/poll'
import { taskDataRows } from '../../lib/gr/plcShape'
import { nav } from '../../lib/nav'
import { taskApi } from '../../lib/task/api'
import type { ComposePreview } from '../../lib/task/types'
import { PlcStructView } from '../shared/PlcStructView'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import { menuItems, type MenuEntry } from '../../lib/task/menuEntries'
import {
  GRIP_REFS,
  move,
  pairIssues,
  removeWithPair,
  patch,
  planRows,
  remove,
  retype,
  autoMeasureMode,
  toRequest,
  toScenario,
  type PlanRow,
  type PlanStep,
} from '../../lib/task/plan'
import { Button } from '../../lib/ui/Button'
import { DataTable } from '../../lib/ui/DataTable'
import { Card } from '../../lib/ui/Card'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { FormDialog } from '../../lib/ui/Dialog'
import { FieldList } from '../../lib/ui/FieldList'
import { Input } from '../../lib/ui/Input'
import { f1 } from '../../lib/meas/format'
import { itemLabel } from '../../lib/items/model'
import { Segmented } from '../../lib/ui/Segmented'
import { Select } from '../../lib/ui/Select'
import { Switch } from '../../lib/ui/Switch'
import { preQueueLabel, readPreQueue, writePreQueue } from '../../lib/task/preQueue'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'
import { robots } from '../../lib/robots'
import {
  robotFailure,
  robotLabel,
  withRobotChip,
  type RobotChipModel,
} from '../../lib/robotContext'
import { RobotChip, robotField } from '../shared/RobotChip'
import { MOVE_MODES, moveOf, moveUsesItem, sentItem } from '../../lib/task/moveMode'
import { PlanStepDialog } from './PlanStepDialog'
import { useStore } from '../../lib/store'
import { tasks } from '../../lib/tasks'
import {
  AUTO_LIMIT_DEFAULT,
  AUTO_LIMIT_MAX,
  AUTO_LIMIT_MIN,
  autoDecision,
  awaitingEcho,
  clampLimit,
  robotQueueCount,
} from '../../lib/task/autoSubmitModel'

const AUTO_LIMIT_KEY = 'gr-plan-auto-limit'
function loadLimit(): number {
  try {
    const v = localStorage.getItem(AUTO_LIMIT_KEY)
    return v === null ? AUTO_LIMIT_DEFAULT : clampLimit(Number(v))
  } catch {
    return AUTO_LIMIT_DEFAULT
  }
}
import type {
  MeasureMode,
  Cell,
  Gate,
  GripRef,
  HandEntry,
  Item,
  Station,
  StockEntry,
  SyncIssue,
  TaskType,
} from '../../lib/types'
import { SyncIssuesDialog } from './SyncIssuesDialog'
import { cn } from '../../lib/utils'

const TYPES: TaskType[] = ['PICK', 'DROP', 'MEASURE', 'MOVE']

/** 종류 표식 — PICK/DROP 두 색이 번갈아 서는 것이 이 표를 훑는 기준이다(뜻으로 부른다). */
const TYPE_BAR: Record<string, string> = {
  PICK: 'bg-info',
  DROP: 'bg-ok',
}

/**
 * 펼친 스텝의 `LGR_Task_Data` 미리보기 — 백엔드 compose(그 스텝 시점의 재고 기준).
 *
 * 펼쳤을 때만 마운트되므로 **여는 것이 곧 요청**이다. 전에는 카드가 "지금 열린 행" 을 제 상태로
 * 들고 이펙트를 돌렸는데, 표가 펼치기를 소유하면 그 상태가 두 곳에 생긴다.
 */
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

/** MOVE 방식의 짧은 이름(Top · Avoid · Stack). */
function measureLabel(m: MeasureMode): string {
  return m === 'sku' ? 'SKU' : m === 'floor' ? 'Floor (Teaching)' : 'Item'
}

function moveLabel(s: PlanStep): string {
  const m = moveOf(s).mode
  return MOVE_MODES.find((x) => x.id === m)?.label ?? m
}

/** 이 카드의 두 모드. 전에는 카드 밖 토글 + 카드 둘이었다 — 한 작업면의 두 모드로 합쳤다. */
/**
 * 계획 표의 ItemCode 칸 — **펼친 목록은 사양까지**(`itemLabel`), **닫힌 칸은 코드만**.
 * 네이티브 Select 는 고른 항목의 글자를 그대로 칸에 그리고 폭도 가장 긴 항목에 맞추므로, 긴 사양 문구가
 * 표 폭을 밀어낸다. 그래서 코드만 쓴 얇은 칸 위에 투명한 Select 를 겹친다 — 누르면 네이티브 목록이
 * 그대로 열리고(키보드·스크린 리더도 Select 그대로), 칸 폭은 코드 폭이다. 전체 사양은 title(호버)에.
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

export type PlanMode = 'auto' | 'plan' | 'single'

export interface PlanCardProps {
  steps: PlanStep[]
  onChange: (next: PlanStep[]) => void
  canUndo: boolean
  canRedo: boolean
  onUndo: () => void
  onRedo: () => void
  cells: readonly Cell[]
  stations: readonly Station[]
  items: readonly Item[]
  stockNow: ReadonlyMap<number, StockEntry>
  gate: Gate | null
  /** 표에서 행을 고르면 레이아웃 강조에 쓴다. */
  onFocus?: (step: PlanStep | null) => void
  /** 그립 기준(전역 기본값) + 변경. */
  gripRef: GripRef
  onGripRefChange: (g: GripRef) => void
  /** 지금 모드 — `plan` 이 아니면 그 모드의 내용(`single`/`auto`)이 작업면을 채운다. */
  mode: PlanMode
  onModeChange: (m: PlanMode) => void
  /** 이 카드가 겨냥한 로봇 — 머리줄 칩·제출 확인·토스트가 모두 이것을 말한다. */
  robot: RobotChipModel
  /** 단일 생성 모드의 내용(작성 카드). */
  single?: ReactNode
  /** 자동 생성 모드의 내용(생성 규칙 · 모니터). */
  autoGen?: ReactNode
  /** 계획 시작 때의 예상 Hand(진행 중 PICK/DROP 반영) — 표의 "들고 있음" 출발점. */
  hand?: { item_code: number; count: number } | null
  /** 지금 Hand(표 값) — 머리줄에 품목 × 개수와 이송 지시를 보인다. */
  handNow?: HandEntry | null
  /** 이 로봇의 동기화 경고(PLC 실제 상태 vs Hand · 이송 지시). */
  sync?: readonly SyncIssue[]
  /** 두 로봇 영역 간격(mm) — 이웃한 다른 로봇 스텝의 X 거리 경고. */
  anticolSep?: number | null
}

export function PlanCard({
  steps,
  onChange,
  canUndo,
  canRedo,
  onUndo,
  onRedo,
  cells,
  stations,
  items,
  stockNow,
  gate,
  onFocus,
  gripRef,
  onGripRefChange,
  mode,
  onModeChange,
  robot,
  single,
  autoGen,
  hand = null,
  handNow = null,
  sync = [],
  anticolSep = null,
}: PlanCardProps) {
  const [syncOpen, setSyncOpen] = useState(false)
  // 지울 스텝(짝은 같이) — 확인 창이 로봇과 짝을 말한다.
  const [deleting, setDeleting] = useState<string | null>(null)
  useStore(robots, tasks)
  useEffect(() => tasks.start(), [])
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
        anticolSep,
        robotName: (id) => (id === null ? robot.name : robots.nameOf(id)),
      }),
    [steps, cells, stations, items, stockNow, gripRef, runRobot, hand, anticolSep, robot.name],
  )
  // PICK/DROP 짝이 어긋난 계획은 실행하지 않는다(백엔드도 시작 때 거부) — 저장은 된다.
  const pairs = useMemo(() => pairIssues(steps, runRobot), [steps, runRobot])
  const pairBlock = pairs.length
    ? `PICK/DROP 짝 — ${pairs
        .slice(0, 3)
        .map((p) => `스텝 ${p.no}: ${p.message}`)
        .join(' · ')}${pairs.length > 3 ? ` 외 ${pairs.length - 3}건` : ''}`
    : undefined
  const [name, setName] = useState('')
  const [saveOpen, setSaveOpen] = useState(false)
  const [confirmNext, setConfirmNext] = useState(false)
  const [confirmClear, setConfirmClear] = useState(false)
  // 실행 방식 — 켜면 스텝마다 접수까지만 기다리고 다음 스텝을 GR 버퍼에 미리 넣는다(브라우저별 기억, 기본 끔).
  const [preQueue, setPreQueueState] = useState(readPreQueue)
  function setPreQueue(on: boolean) {
    setPreQueueState(on)
    writePreQueue(on)
  }
  const [busy, setBusy] = useState(false)
  const [focus, setFocus] = useState<string | null>(null)
  const [editing, setEditing] = useState<string | null>(null)
  const editRow = editing ? (rows.find((r) => r.id === editing) ?? null) : null
  const warnCount = rows.reduce((a, r) => a + r.warnings.length, 0)
  const first = rows[0] ?? null
  // 스텝이 제 로봇을 들고 있으면 그 호기로 간다 — 확인 창은 **실제로 갈 곳**을 말해야 한다.
  const nextRobot =
    first && first.robot !== null && first.robot !== undefined ? robots.chipOf(first.robot) : robot

  // ── 자동 제출: 로봇 큐(원장: 제출됨·수락·대기·실행 중)가 N 개 이하이면 다음 스텝을 한 건씩 보낸다.
  // 큐는 **다음 스텝이 갈 로봇** 것이다(스텝이 로봇을 들고 있으면 그 호기). 판단은 `autoSubmitModel`.
  const [auto, setAuto] = useState(false)
  const [autoLimit, setAutoLimitState] = useState(loadLimit)
  const [confirmAuto, setConfirmAuto] = useState(false)
  /** 직전에 자동으로 보낸 Task — 원장(SSE)에 보일 때까지 다음을 보내지 않는다(큐 수가 낡았다). */
  const [echoId, setEchoId] = useState<string | null>(null)
  const inFlight = useRef(false)
  const setAutoLimit = (v: number) => {
    const n = clampLimit(v)
    setAutoLimitState(n)
    try {
      localStorage.setItem(AUTO_LIMIT_KEY, String(n))
    } catch {
      /* 기억 못 해도 동작 */
    }
  }
  const nextRobotId = first?.robot ?? robots.selected
  // 게이트는 **다음 스텝이 갈 로봇** 것 — 카드의 `gate` 는 사이드바 선택 로봇이라, 선택이 GR1(닫힘)이고 스텝이
  // GR2 면 GR2 가 열려 있어도 제출·자동 제출이 서 있었다(2026-09-21). 다르면 그 로봇 게이트를 따로 읽는다.
  const [stepGate, setStepGate] = useState<Gate | null>(null)
  const otherRobot =
    nextRobotId !== null && nextRobotId !== undefined && nextRobotId !== robots.selected
  useEffect(() => {
    setStepGate(null)
    if (!otherRobot) return
    let alive = true
    const load = () =>
      void api
        .taskGate(nextRobotId)
        .then((g) => alive && setStepGate(g))
        .catch(() => alive && setStepGate(null))
    load()
    const t = visibleInterval(load, 1000)
    return () => {
      alive = false
      clearInterval(t)
    }
  }, [otherRobot, nextRobotId])
  const nextGate = otherRobot ? stepGate : gate
  const nextPlc = robots.byId(nextRobotId)?.plc ?? null
  const queue = robotQueueCount(tasks.list, nextPlc)
  const decision = autoDecision({
    on: auto,
    busy,
    remaining: steps.length,
    queue,
    limit: autoLimit,
    gateOk: !!nextGate?.can_submit,
    gateReason: nextGate?.reasons.join(' · '),
    awaitingEcho: awaitingEcho(tasks.list, echoId, nextPlc),
  })
  const decisionKey = JSON.stringify(decision) + (first?.id ?? '')
  useEffect(() => {
    if ('done' in decision && auto) {
      setAuto(false)
      toast.ok(withRobotChip(nextRobot, '자동 제출 완료 — 계획의 스텝을 모두 보냈습니다'))
      return
    }
    if (decision.go && !inFlight.current) void submitNext(true)
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 판단이 바뀔 때만 움직인다(매 렌더 새 객체)
  }, [decisionKey])

  function pick(id: string | null) {
    setFocus(id)
    onFocus?.(steps.find((s) => s.id === id) ?? null)
  }

  // 계획 표의 열 — **정렬을 켜지 않는다**(`sortable: false`). 이 표는 순서가 곧 뜻이고, 행을 섞으면
  // 위/아래 버튼이 딴 행을 옮긴다. 좁은 존(460px 아래)에서는 우선순위 3·2 가 차례로 접히고, 접힌
  // 열도 행을 펼치면 **그대로 고칠 수 있다**(셀이 곧 컨트롤이라 짝 자리에서도 편집이 산다).
  const planCols: Column<PlanRow>[] = [
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
            onValueChange={(v) => onChange(patch(steps, r.id, retype(r, v as TaskType)))}
            aria-label="Type"
          >
            {TYPES.map((t) => (
              <option key={t} value={t}>
                {t}
              </option>
            ))}
          </Select>
          {r.type === 'MOVE' ? (
            <span className="text-3xs text-content-muted" data-testid={`plan-move-${r.no}`}>
              {moveLabel(r)}
            </span>
          ) : null}
          {r.type === 'MEASURE' ? (
            // 비우면 자동 — 그 시점 셀 재고 1개 = Item, 2개 이상 = SKU. 자동이 고른 값을 옵션 글자로 보인다.
            <Select
              dense
              className="w-auto"
              value={r.measure ?? ''}
              onValueChange={(v) =>
                onChange(patch(steps, r.id, { measure: v === '' ? null : (v as MeasureMode) }))
              }
              aria-label="Measure"
              title="MEASURE 종류 — auto: 셀 재고 1개 = Item, 2개 이상 = SKU · Floor = 바닥 측정(Cell Teaching)"
              data-testid={`plan-measure-${r.no}`}
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
              data-testid={`plan-mp-${r.no}`}
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
                onValueChange={(v) =>
                  onChange(patch(steps, r.id, { robot: v === '' ? null : Number(v) }))
                }
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
            onChange={(item_code) => onChange(patch(steps, r.id, { item_code }))}
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
            onValueChange={(s) =>
              onChange(patch(steps, r.id, { count: Math.max(1, Number(s) || 1) }))
            }
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
      help: '이 스텝 **직전 → 직후**의 셀 재고. 앞 스텝들을 순서대로 적용한 시뮬레이션이고, 스테이션은 재고를 세지 않는다.',
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
      help: '셀 바닥 + 품목 높이 × 재고 + 그립 기준(⋯ 메뉴). 품목 높이를 모르면 `?` 로 두고 계산하지 않는다.',
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

  async function saveScenario(run: boolean) {
    if (!steps.length) return
    setBusy(true)
    try {
      const sc = await api.scenarioSave(
        toScenario(steps, name.trim() || `계획 ${new Date().toLocaleString()}`, '', { preQueue }),
      )
      toast.ok(`시나리오 "${sc.name}" 저장 (${sc.steps.length}스텝)`)
      if (run) {
        // 로봇을 안 든("기본") 스텝은 카드 대상 로봇으로 — "다음 1건 제출"과 같은 곳으로 간다.
        await api.scenarioRun(sc.id, { repeat: 1, robot: robots.selected })
        // 실행은 로봇이 움직이는 일이다 — 어느 호기인지 토스트가 말한다.
        toast.info(
          withRobotChip(
            robot,
            `시나리오 실행 시작${preQueue ? ' (Pre-queue)' : ''} — 진행은 시나리오 탭에서`,
          ),
        )
      }
      nav.goScenario(sc.id)
    } catch (e) {
      toast.error(`시나리오 저장 실패 — ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  async function submitNext(isAuto = false) {
    if (!first || inFlight.current) return
    inFlight.current = true
    setBusy(true)
    try {
      const t = await api.taskCreate(toRequest(first, robots.selected, first.multiPick), true)
      if (isAuto) setEchoId(t.id)
      // 스텝이 제 로봇을 들고 있으면(계획 표의 Robot 열) 그쪽, 아니면 카드 대상.
      const who =
        first.robot === null || first.robot === undefined ? robot : robots.chipOf(first.robot)
      toast.info(
        withRobotChip(
          who,
          `#${t.seq} 제출됨 — ${first.type} ${first.target.kind === 'cell' ? 'Cell' : 'Station'} #${first.target.id}`,
        ),
      )
      onChange(remove(steps, first.id))
    } catch (e) {
      // 자동 제출은 실패하면 멈춘다 — 같은 스텝을 계속 두드리지 않는다.
      if (isAuto) setAuto(false)
      toast.error(
        robotFailure(
          nextRobot.name,
          isAuto ? '자동 제출 멈춤 — 제출 실패' : '제출 실패',
          e instanceof Error ? e.message : String(e),
        ),
      )
    } finally {
      inFlight.current = false
      setBusy(false)
    }
  }

  // 카드 넘침 메뉴 — 그립 기준(세그먼트 셋) · 되돌리기 · 다시실행 · 비우기.
  // 되돌리기는 Ctrl+Z/Y 가 이미 있으니 띠에 아이콘 둘을 세워 둘 이유가 없다(메뉴가 단축키를 가르친다).
  const cardMenu: MenuEntry[] = [
    { label: '그립 기준 — Z 의 잡는 높이' },
    ...GRIP_REFS.map((g) => ({
      label: g.label,
      hint: g.id === gripRef ? '지금' : undefined,
      run: () => onGripRefChange(g.id),
    })),
    mode === 'plan' && { label: '계획' },
    mode === 'plan' && {
      label: '되돌리기',
      hint: 'Ctrl+Z',
      run: onUndo,
      disabled: canUndo ? undefined : '되돌릴 편집이 없습니다',
    },
    mode === 'plan' && {
      label: '다시실행',
      hint: 'Ctrl+Y',
      run: onRedo,
      disabled: canRedo ? undefined : '다시실행할 편집이 없습니다',
    },
    mode === 'plan' && {
      label: '계획 비우기…',
      danger: true,
      run: () => setConfirmClear(true),
      disabled: steps.length ? undefined : '계획이 이미 비어 있습니다',
    },
  ]

  return (
    <Card padded={false} className="flex flex-col" data-testid="plan-card">
      <div className="flex min-h-screen-header flex-none items-center gap-2 border-b border-line-default px-3 py-1">
        <ListOrdered className="h-4 w-4 text-content-muted" />
        <Segmented
          ariaLabel="작성 방식"
          value={mode}
          onChange={onModeChange}
          options={[
            { id: 'auto', label: '자동 생성', testid: 'side-auto' },
            { id: 'plan', label: '순차 생성', badge: steps.length || '', testid: 'side-plan' },
            { id: 'single', label: '단일 생성', testid: 'side-single' },
          ]}
        />
        {/* 설명 한 줄은 늘 서 있을 값이 아니다 — 손이 멈췄을 때만 읽히게 title 로 내린다. */}
        <span
          className="truncate text-2xs text-content-faint"
          title={
            mode === 'plan'
              ? '레이아웃 클릭 = PICK/DROP 교대 · Z = 바닥 + H×재고 + 그립'
              : mode === 'single'
                ? '한 건을 만들어 바로 제출한다 — 계획에 쌓지 않는다'
                : '규칙의 조건이 참이면 콘솔이 스스로 만들어 보낸다'
          }
        >
          {mode === 'plan'
            ? `${steps.length}스텝${warnCount ? ` · 경고 ${warnCount}` : ''}`
            : mode === 'single'
              ? '한 건 작성 → 제출'
              : '규칙 → 조건 → 자동 제출'}
        </span>
        <span className="flex-1" />
        {/* 그리퍼에 든 화물 — PICK/DROP 짝 사이(또는 DROP 이 취소돼 남은) 타이어. */}
        <span
          className={cn(
            'text-2xs whitespace-nowrap',
            handNow && handNow.count > 0 ? 'text-content-secondary' : 'text-content-faint',
          )}
          title={
            handNow && handNow.count > 0
              ? `그리퍼에 든 화물${handNow.transfer_order_id ? ` — 이송 지시 ${handNow.transfer_order_id}` : ''}`
              : '그리퍼 비어 있음'
          }
          data-testid="plan-hand"
        >
          Hand: {handNow && handNow.count > 0 ? `${handNow.item_code} ×${handNow.count}` : '–'}
        </span>
        {sync.length ? (
          <Button
            size="sm"
            intent="outline"
            className="text-warn-fg"
            title={sync.map((i) => i.message).join(' · ')}
            onClick={() => setSyncOpen(true)}
            data-testid="plan-sync"
          >
            동기화 {sync.length}
          </Button>
        ) : null}
        {/* 이 카드가 만드는 명령은 전부 이 호기로 간다 — 놓칠 수 없는 자리(머리줄 오른쪽)에 늘 선다. */}
        <RobotChip
          chip={robot}
          testid="plan-robot"
          title={`이 카드의 제출 대상 — ${robotLabel(robot)} (사이드바 로봇 목록에서 바꿉니다)`}
        />
        <OverflowMenu
          items={menuItems(cardMenu)}
          title={mode === 'plan' ? '계획 — 그립 기준 · 되돌리기 · 비우기' : '그립 기준'}
          testid="plan-more"
        />
      </div>

      {/* 두 모드를 **둘 다 마운트한 채** 감춘다 — 모드를 오갈 때 작성 중이던 초안이 사라지면
          "잠깐 계획을 확인하는 일"이 초안을 버리는 일이 된다(전에도 같은 이유로 둘 다 떠 있었다).
          작성 카드는 `chrome={false}` 로 들어온다 — 카드도 머리줄도 이 카드의 것 하나뿐이다
          (예전에는 CSS 로 안쪽 카드의 테두리만 벗기고 머리줄은 두 겹으로 남겨 두었다). */}
      <div className={mode === 'single' ? 'min-w-0' : 'hidden'} data-testid="plan-card-single">
        {single}
      </div>
      {mode === 'auto' ? (
        <div className="min-h-0 min-w-0 flex-1 overflow-auto p-3" data-testid="plan-card-auto">
          {autoGen}
        </div>
      ) : null}
      <div className={mode === 'plan' ? 'contents' : 'hidden'}>
        <div className="min-h-0 flex-1 overflow-auto border-t border-line-default">
          <DataTable
            rows={rows}
            columns={planCols}
            rowKey={(r) => r.id}
            onPick={(r) => pick(r.id)}
            selected={focus}
            density="compact"
            stickyHeader
            empty="계획 없음"
            emptyHint="레이아웃 맵에서 셀을 누르면 PICK → DROP 순으로 쌓입니다."
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
                    // 짝이 없는 스텝은 바로, PICK/DROP 은 짝을 같이 지우므로 한 번 묻는다.
                    if (r.type === 'PICK' || r.type === 'DROP') setDeleting(r.id)
                    else onChange(remove(steps, r.id))
                  }}
                  data-testid={`plan-del-${r.no}`}
                />
              </>
            )}
          />
        </div>

        <div className="flex items-center gap-2 border-t border-line-default px-3 py-2">
          <Button
            size="sm"
            intent="outline"
            icon={<Save className="h-3.5 w-3.5" />}
            disabled={!steps.length || busy}
            onClick={() => setSaveOpen(true)}
            data-testid="plan-save"
          >
            시나리오로 저장…
          </Button>
          <Switch
            inline
            label="Pre-queue"
            checked={preQueue}
            onCheckedChange={setPreQueue}
            title={
              '켜면 시나리오 실행 때 앞 Task 가 실행에 들어가면 다음 1건을 미리 넣습니다(실행 중 + 다음 1건, 나머지는 예정). ' +
              'Z 는 진행 중 Task 를 반영한 예상 재고로 작성되고, 앞 Task 가 실패·취소되면 실행이 멈춥니다. ' +
              'PICK/DROP 짝 사이에서는 멈추지 않고, 마지막 Task 가 완료돼야 실행이 끝납니다. 끄면 스텝마다 완료를 기다립니다.'
            }
            testid="plan-prequeue"
          />
          <span className="flex-1" />
          {auto ? (
            <span
              className="truncate text-2xs text-content-muted tabular-nums"
              data-testid="plan-auto-status"
              title="로봇 큐 = 원장의 제출됨·수락·대기·실행 중 Task 수"
            >
              자동 · 큐 {queue}/{autoLimit}
              {'wait' in decision && decision.wait !== '꺼짐' ? ` · ${decision.wait}` : ''}
            </span>
          ) : null}
          <Input
            dense
            type="number"
            mono
            min={AUTO_LIMIT_MIN}
            max={AUTO_LIMIT_MAX}
            step="1"
            className="w-12"
            value={String(autoLimit)}
            onValueChange={(v) => setAutoLimit(Number(v))}
            title={`자동 제출 — 로봇 큐가 이 개수 이하일 때 다음 스텝을 보냅니다(${AUTO_LIMIT_MIN}~${AUTO_LIMIT_MAX})`}
            aria-label="자동 제출 큐 한도"
            data-testid="plan-auto-limit"
          />
          <Button
            size="sm"
            intent={auto ? 'primary' : 'outline'}
            icon={auto ? <Pause className="h-3.5 w-3.5" /> : <Play className="h-3.5 w-3.5" />}
            disabled={!auto && (!first || busy)}
            title={
              auto
                ? '자동 제출 멈춤(보낸 Task 는 그대로)'
                : `로봇 큐가 ${autoLimit}개 이하일 때마다 다음 스텝을 자동으로 보냅니다`
            }
            onClick={() => (auto ? setAuto(false) : setConfirmAuto(true))}
            data-testid="plan-auto"
          >
            {auto ? '자동 멈춤' : '자동 제출'}
          </Button>
          <Button
            size="sm"
            intent="primary"
            icon={<Send className="h-3.5 w-3.5" />}
            disabled={!first || busy || auto || !nextGate?.can_submit || first.type === 'PICK'}
            title={
              first?.type === 'PICK'
                ? 'PICK 은 DROP 과 짝으로만 보냅니다 — 시나리오로 저장 → 저장 후 실행'
                : !nextGate?.can_submit
                  ? // 비활성은 침묵하지 않고 **누가 왜** 막았는지 말한다.
                    (nextGate?.reasons.join(' · ') ?? withRobotChip(nextRobot, '게이트 확인 중'))
                  : `첫 스텝만 지금 ${robotLabel(nextRobot)} 로 제출`
            }
            onClick={() => setConfirmNext(true)}
            data-testid="plan-next"
          >
            다음 1건 제출
          </Button>
        </div>
      </div>

      {/* 시나리오 이름은 저장할 때만 필요한 값이다 — 늘 서 있던 입력칸을 이 팝업으로 옮겼다.
          Enter 는 **저장까지만** 한다: 실행은 로봇이 움직이는 일이라 따로 누르게 둔다. */}
      <FormDialog
        open={saveOpen}
        onOpenChange={setSaveOpen}
        title="시나리오로 저장"
        size="sm"
        meta={`${steps.length}스텝`}
        dirty={name.trim() !== ''}
        busy={busy}
        submitLabel="저장"
        disabledReason={steps.length ? undefined : '계획이 비어 있습니다'}
        onSubmit={() => {
          setSaveOpen(false)
          void saveScenario(false)
        }}
        extra={
          <Button
            size="sm"
            intent="outline"
            icon={<Play className="h-3.5 w-3.5" />}
            disabled={!steps.length || busy || pairs.length > 0}
            title={
              pairBlock ?? (steps.length ? '저장한 뒤 바로 실행합니다' : '계획이 비어 있습니다')
            }
            onClick={() => {
              setSaveOpen(false)
              void saveScenario(true)
            }}
            data-testid="plan-run"
          >
            저장 후 실행
          </Button>
        }
        testid="plan-save-dialog"
      >
        <Input
          label="시나리오 이름"
          value={name}
          onValueChange={setName}
          placeholder="비우면 날짜로"
          data-testid="plan-name"
        />
        <FieldList
          columns={1}
          dense
          labelWidth={72}
          items={[
            { label: '실행 방식', value: preQueueLabel(preQueue) },
            ...(pairBlock ? [{ label: '실행 불가', value: pairBlock }] : []),
          ]}
        />
      </FormDialog>

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

      {deleting
        ? (() => {
            const plan = removeWithPair(steps, deleting, runRobot)
            const s = steps.find((x) => x.id === deleting)
            const chip =
              s?.robot !== null && s?.robot !== undefined ? robots.chipOf(s.robot) : robot
            return (
              <ConfirmDialog
                open
                onOpenChange={(o) => {
                  if (!o) setDeleting(null)
                }}
                scope="single"
                title={`스텝 삭제 — ${chip.name}`}
                confirmLabel="삭제"
                onConfirm={() => {
                  onChange(plan.next)
                  setDeleting(null)
                }}
              >
                <div className="flex flex-col gap-2 text-xs">
                  <span className="flex items-center gap-2">
                    <RobotChip chip={chip} title={robotLabel(chip)} />
                    {plan.removed
                      .map(
                        (x) =>
                          `${steps.indexOf(x) + 1}. ${x.type} ${x.target.kind === 'cell' ? 'Cell' : 'Station'} #${x.target.id}`,
                      )
                      .join(' · ')}
                  </span>
                  {plan.removed.length > 1 ? (
                    <span className="text-warn-fg">PICK/DROP 짝이라 같이 지웁니다</span>
                  ) : null}
                </div>
              </ConfirmDialog>
            )
          })()
        : null}
      <SyncIssuesDialog
        open={syncOpen}
        onOpenChange={setSyncOpen}
        robot={robot}
        robotId={runRobot}
        issues={sync}
      />
      <ConfirmDialog
        open={confirmAuto}
        onOpenChange={setConfirmAuto}
        scope="single-robot"
        danger
        title={`자동 제출 — ${nextRobot.name}`}
        confirmLabel="자동 제출 시작"
        onConfirm={() => {
          setEchoId(null)
          setAuto(true)
        }}
      >
        <div className="text-xs">
          남은 {steps.length}스텝을 로봇 큐가 <b>{autoLimit}개 이하</b>일 때마다 한 건씩
          보냅니다(스텝의 로봇, 없으면 {robotLabel(robot)}). 제출이 실패하면 멈추고, 게이트가 닫히면
          열릴 때까지 기다립니다.
        </div>
      </ConfirmDialog>

      <ConfirmDialog
        open={confirmNext}
        onOpenChange={setConfirmNext}
        scope="single-robot"
        danger
        title={`다음 스텝 제출 — ${nextRobot.name}`}
        confirmLabel={`${nextRobot.name} 로 제출`}
        onConfirm={() => void submitNext()}
      >
        {first ? (
          <div className="flex flex-col gap-2 text-xs">
            {/* 질문이 로봇을 말한다 — "PLC 로" 가 아니라 "어느 호기로". */}
            <p className="m-0">첫 스텝을 {robotLabel(nextRobot)} 로 제출할까요?</p>
            <FieldList
              columns={2}
              dense
              labelWidth={72}
              items={[
                robotField(nextRobot, '대상 로봇'),
                {
                  label: '종류',
                  value:
                    first.type === 'MOVE'
                      ? `MOVE · ${moveLabel(first)}`
                      : first.type === 'MEASURE' && first.measureMode
                        ? `MEASURE · ${first.measure ? measureLabel(first.measureMode) : `auto(${measureLabel(first.measureMode)})`}`
                        : first.type,
                },
                {
                  label: '대상',
                  value: `${first.target.kind === 'cell' ? 'Cell' : 'Station'} #${first.target.id}`,
                },
                {
                  label: '품목',
                  value: sentItem(first) !== null ? `${first.item_code} × ${first.count}` : '',
                  missing: '품목 없음',
                },
                {
                  label: 'Z (mm)',
                  value: first.z !== null ? f1(first.z) : '',
                  missing: '계산 불가',
                },
              ]}
            />
            <p className="m-0 text-fault-fg">로봇이 실제로 움직입니다.</p>
          </div>
        ) : null}
      </ConfirmDialog>
      <ConfirmDialog
        open={confirmClear}
        onOpenChange={setConfirmClear}
        scope="single"
        title="계획 비우기"
        confirmLabel="비우기"
        onConfirm={() => onChange([])}
      >
        <div className="flex flex-col gap-2 text-xs">
          <p className="m-0">계획을 비울까요?</p>
          <FieldList
            columns={2}
            dense
            labelWidth={56}
            items={[
              { label: '스텝', value: `${steps.length}개` },
              { label: '되돌리기', value: 'Ctrl+Z 로 가능' },
            ]}
          />
        </div>
      </ConfirmDialog>
    </Card>
  )
}
