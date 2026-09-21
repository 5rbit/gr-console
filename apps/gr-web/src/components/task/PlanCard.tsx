// 작업 카드 — 오른쪽 칸 **하나뿐인 작업면**이고 모드가 둘이다.
//   순차 계획: 레이아웃 클릭이 PICK → DROP → … 으로 쌓인 표. 순서/종류/품목/수량 편집, 다음 1건 즉시 제출.
//   단일 명령: 작성 카드(ComposeCard)를 같은 면에 끼운다.
// Z 는 재고 시뮬레이션으로 계산해 보여 준다.
//
// 전에는 카드 밖에 모드 토글이 한 줄 따로 서고 카드가 둘이었다 — 같은 자리에서 같은 일을 하는데
// 껍데기가 둘이면 머리띠도 둘이다. 모드를 카드 머리줄로 들여 한 줄을 없앴다.
// 그립 기준·되돌리기·다시실행·비우기는 ⋯ 로, 시나리오 이름은 저장 팝업으로 내렸다.
import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { ArrowDown, ArrowUp, ListOrdered, Play, Save, Send, X } from 'lucide-react'
import { api } from '../../lib/api'
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
  patch,
  planRows,
  remove,
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
import { Segmented } from '../../lib/ui/Segmented'
import { Select } from '../../lib/ui/Select'
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
import { useStore } from '../../lib/store'
import type { Cell, Gate, GripRef, Item, Station, StockEntry, TaskType } from '../../lib/types'
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
  const req = toRequest(row)
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

/** 이 카드의 두 모드. 전에는 카드 밖 토글 + 카드 둘이었다 — 한 작업면의 두 모드로 합쳤다. */
export type PlanMode = 'plan' | 'single'

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
  /** 지금 모드 — `single` 이면 표 대신 `single` 이 작업면을 채운다. */
  mode: PlanMode
  onModeChange: (m: PlanMode) => void
  /** 이 카드가 겨냥한 로봇 — 머리줄 칩·제출 확인·토스트가 모두 이것을 말한다. */
  robot: RobotChipModel
  /** 단일 명령 모드의 내용(작성 카드). */
  single?: ReactNode
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
}: PlanCardProps) {
  useStore(robots)
  const rows = useMemo(
    () => planRows(steps, { cells, stations, items, stockNow, gripRef }),
    [steps, cells, stations, items, stockNow, gripRef],
  )
  const [name, setName] = useState('')
  const [saveOpen, setSaveOpen] = useState(false)
  const [confirmNext, setConfirmNext] = useState(false)
  const [confirmClear, setConfirmClear] = useState(false)
  const [busy, setBusy] = useState(false)
  const [focus, setFocus] = useState<string | null>(null)
  const warnCount = rows.reduce((a, r) => a + r.warnings.length, 0)
  const first = rows[0] ?? null
  // 스텝이 제 로봇을 들고 있으면 그 호기로 간다 — 확인 창은 **실제로 갈 곳**을 말해야 한다.
  const nextRobot =
    first && first.robot !== null && first.robot !== undefined ? robots.chipOf(first.robot) : robot

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
        <Select
          dense
          className="w-auto"
          value={r.type}
          onValueChange={(v) => onChange(patch(steps, r.id, { type: v as TaskType }))}
          aria-label="Type"
        >
          {TYPES.map((t) => (
            <option key={t} value={t}>
              {t}
            </option>
          ))}
        </Select>
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
      cell: (r) => (
        <Select
          dense
          value={r.item_code === null ? '' : String(r.item_code)}
          onValueChange={(v) =>
            onChange(patch(steps, r.id, { item_code: v === '' ? null : Number(v) }))
          }
          aria-label="ItemCode"
        >
          <option value="">(없음)</option>
          {items.map((it) => (
            <option key={it.code} value={String(it.code)}>
              {it.code}
            </option>
          ))}
        </Select>
      ),
    },
    {
      key: 'count',
      label: 'Count',
      sortable: false,
      numeric: true,
      priority: 1,
      cell: (r) => (
        <Input
          dense
          type="number"
          mono
          min={1}
          max={20}
          step="1"
          className="w-14"
          value={String(r.count)}
          onValueChange={(s) => onChange(patch(steps, r.id, { count: Math.max(1, Number(s) || 1) }))}
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
        toScenario(steps, name.trim() || `계획 ${new Date().toLocaleString()}`),
      )
      toast.ok(`시나리오 "${sc.name}" 저장 (${sc.steps.length}스텝)`)
      if (run) {
        await api.scenarioRun(sc.id, { repeat: 1 })
        // 실행은 로봇이 움직이는 일이다 — 어느 호기인지 토스트가 말한다.
        toast.info(withRobotChip(robot, '시나리오 실행 시작 — 진행은 시나리오 탭에서'))
      }
      nav.goScenario(sc.id)
    } catch (e) {
      toast.error(`시나리오 저장 실패 — ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  async function submitNext() {
    if (!first) return
    setBusy(true)
    try {
      const t = await api.taskCreate(toRequest(first), true)
      // 스텝이 제 로봇을 들고 있으면(계획 표의 Robot 열) 그쪽, 아니면 카드 대상.
      const who = first.robot === null || first.robot === undefined ? robot : robots.chipOf(first.robot)
      toast.info(
        withRobotChip(
          who,
          `#${t.seq} 제출됨 — ${first.type} ${first.target.kind === 'cell' ? 'Cell' : 'Station'} #${first.target.id}`,
        ),
      )
      onChange(remove(steps, first.id))
    } catch (e) {
      toast.error(robotFailure(nextRobot.name, '제출 실패', e instanceof Error ? e.message : String(e)))
    } finally {
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
            { id: 'plan', label: '순차 계획', badge: steps.length || '', testid: 'side-plan' },
            { id: 'single', label: '단일 명령', testid: 'side-single' },
          ]}
        />
        {/* 설명 한 줄은 늘 서 있을 값이 아니다 — 손이 멈췄을 때만 읽히게 title 로 내린다. */}
        <span
          className="truncate text-2xs text-content-faint"
          title={
            mode === 'plan'
              ? '레이아웃 클릭 = PICK/DROP 교대 · Z = 바닥 + H×재고 + 그립'
              : '한 건을 만들어 바로 제출한다 — 계획에 쌓지 않는다'
          }
        >
          {mode === 'plan'
            ? `${steps.length}스텝${warnCount ? ` · 경고 ${warnCount}` : ''}`
            : '한 건 작성 → 제출'}
        </span>
        <span className="flex-1" />
        {/* 이 카드가 만드는 명령은 전부 이 호기로 간다 — 놓칠 수 없는 자리(머리줄 오른쪽)에 늘 선다. */}
        <RobotChip
          chip={robot}
          testid="plan-robot"
          title={`이 카드의 제출 대상 — ${robotLabel(robot)} (사이드바 로봇 목록에서 바꿉니다)`}
        />
        <OverflowMenu
          items={menuItems(cardMenu)}
          title={mode === 'plan' ? '계획 — 그립 기준 · 되돌리기 · 비우기' : '작성 — 그립 기준'}
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
                    onChange(remove(steps, r.id))
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
          <span className="flex-1" />
          <Button
            size="sm"
            intent="primary"
            icon={<Send className="h-3.5 w-3.5" />}
            disabled={!first || busy || !gate?.can_submit}
            title={
              !gate?.can_submit
                ? // 비활성은 침묵하지 않고 **누가 왜** 막았는지 말한다.
                  (gate?.reasons.join(' · ') ?? withRobotChip(robot, '게이트 확인 중'))
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
            disabled={!steps.length || busy}
            title={steps.length ? '저장한 뒤 바로 실행합니다' : '계획이 비어 있습니다'}
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
      </FormDialog>

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
                { label: '종류', value: first.type },
                {
                  label: '대상',
                  value: `${first.target.kind === 'cell' ? 'Cell' : 'Station'} #${first.target.id}`,
                },
                {
                  label: '품목',
                  value: first.item_code !== null ? `${first.item_code} × ${first.count}` : '',
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
