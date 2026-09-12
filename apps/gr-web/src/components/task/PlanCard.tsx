// 순차 계획 카드(생성 예정 작업) — 레이아웃 클릭이 PICK → DROP → … 으로 쌓인 표. 순서/종류/품목/수량 편집,
// 되돌리기·다시실행, 시나리오로 저장·실행, 다음 1건 즉시 제출. Z 는 재고 시뮬레이션으로 계산해 보여 준다.
import { Fragment, useEffect, useMemo, useState } from 'react'
import {
  ArrowDown,
  ArrowUp,
  ChevronDown,
  ChevronRight,
  ListOrdered,
  Play,
  Redo2,
  Save,
  Send,
  Trash2,
  Undo2,
  X,
} from 'lucide-react'
import { api } from '../../lib/api'
import { taskDataRows } from '../../lib/gr/plcShape'
import { nav } from '../../lib/nav'
import { taskApi } from '../../lib/task/api'
import type { ComposePreview } from '../../lib/task/types'
import { PlcStructView } from '../shared/PlcStructView'
import {
  move,
  patch,
  planRows,
  remove,
  toRequest,
  toScenario,
  toggleType,
  type PlanRow,
  type PlanStep,
} from '../../lib/task/plan'
import { Button } from '../../lib/ui/Button'
import { Card } from '../../lib/ui/Card'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { Input } from '../../lib/ui/Input'
import { Select } from '../../lib/ui/Select'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import { toast } from '../../lib/ui/toast'
import type { Cell, Gate, Item, Station, StockEntry, TaskType } from '../../lib/types'
import { cn } from '../../lib/utils'

const TYPES: TaskType[] = ['PICK', 'DROP', 'MEASURE', 'MOVE']

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
}: PlanCardProps) {
  const rows = useMemo(
    () => planRows(steps, { cells, stations, items, stockNow }),
    [steps, cells, stations, items, stockNow],
  )
  const [name, setName] = useState('')
  const [confirmNext, setConfirmNext] = useState(false)
  const [confirmClear, setConfirmClear] = useState(false)
  const [busy, setBusy] = useState(false)
  const [focus, setFocus] = useState<string | null>(null)
  const [open, setOpen] = useState<string | null>(null)
  const [preview, setPreview] = useState<{
    id: string
    p: ComposePreview | null
    err: string | null
  } | null>(null)
  // 펼친 스텝의 LGR_Task_Data 미리보기 — 백엔드 compose(현재 재고 기준)
  useEffect(() => {
    const s = steps.find((x) => x.id === open)
    if (!s) return
    const row = rows.find((x) => x.id === s.id)
    let alive = true
    setPreview({ id: s.id, p: null, err: null })
    taskApi
      .compose(toRequest(s), row?.stockBefore ?? null)
      .then((p) => alive && setPreview({ id: s.id, p, err: null }))
      .catch(
        (e) =>
          alive &&
          setPreview({ id: s.id, p: null, err: e instanceof Error ? e.message : String(e) }),
      )
    return () => {
      alive = false
    }
  }, [open, steps, rows])
  const warnCount = rows.reduce((a, r) => a + r.warnings.length, 0)
  const first = rows[0] ?? null

  function pick(id: string | null) {
    setFocus(id)
    onFocus?.(steps.find((s) => s.id === id) ?? null)
  }

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
        toast.info('시나리오 실행 시작 — 진행은 시나리오 탭에서')
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
      toast.info(
        `#${t.seq} 제출됨 — ${first.type} ${first.target.kind === 'cell' ? '셀' : '스테이션'} #${first.target.id}`,
      )
      onChange(remove(steps, first.id))
    } catch (e) {
      toast.error(`제출 실패 — ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card padded={false} className="flex flex-col" data-testid="plan-card">
      <div className="flex items-center gap-2 border-b border-slate-200 px-3 py-2 dark:border-slate-700">
        <ListOrdered className="h-4 w-4 text-slate-500" />
        <span className="text-sm font-semibold">순차 계획</span>
        <span className="text-xs text-slate-400">
          {steps.length}스텝{warnCount ? ` · 경고 ${warnCount}` : ''}
        </span>
        <span className="flex-1" />
        <Button
          size="icon-sm"
          intent="ghost"
          icon={<Undo2 className="h-3.5 w-3.5" />}
          title="되돌리기 (Ctrl+Z)"
          disabled={!canUndo}
          onClick={onUndo}
          data-testid="plan-undo"
        />
        <Button
          size="icon-sm"
          intent="ghost"
          icon={<Redo2 className="h-3.5 w-3.5" />}
          title="다시실행 (Ctrl+Y)"
          disabled={!canRedo}
          onClick={onRedo}
          data-testid="plan-redo"
        />
        <Button
          size="sm"
          intent="ghost"
          icon={<Trash2 className="h-3.5 w-3.5" />}
          disabled={!steps.length}
          onClick={() => setConfirmClear(true)}
        >
          비우기
        </Button>
      </div>

      <div className="px-3 py-2 text-[11px] text-slate-500">
        레이아웃에서 셀/스테이션을 누르면 <b>PICK → DROP → PICK …</b> 순으로 쌓입니다. 품목은 셀
        재고(PICK) / 들고 있는 화물(DROP)에서 잇고, Z 는 재고로 계산합니다: PICK = 바닥 + H×(n−c) +
        H/2, DROP = 바닥 + H×n + H/2.
      </div>

      <div className="max-h-[42vh] overflow-auto border-t border-slate-200 dark:border-slate-700">
        <table className="w-full text-xs">
          <thead className="sticky top-0 bg-surface-panel text-slate-500">
            <tr>
              <th className="w-5" />
              <th className="px-1 py-1 text-left">#</th>
              <th className="px-1 text-left">종류</th>
              <th className="px-1 text-left">대상</th>
              <th className="px-1 text-left">품목</th>
              <th className="px-1 text-right">수량</th>
              <th className="px-1 text-right">재고</th>
              <th className="px-1 text-right">Z</th>
              <th className="px-1 text-left">경고</th>
              <th className="px-1" />
            </tr>
          </thead>
          <tbody>
            {rows.length === 0 ? (
              <tr>
                <td colSpan={10} className="px-2 py-4 text-center text-slate-400">
                  계획이 비어 있습니다 — 레이아웃에서 셀을 누르세요
                </td>
              </tr>
            ) : (
              rows.map((r: PlanRow, i) => (
                <Fragment key={r.id}>
                  <tr
                    className={cn(
                      'border-t border-slate-100 dark:border-slate-800',
                      focus === r.id && 'bg-indigo-50 dark:bg-indigo-950',
                    )}
                    onClick={() => pick(r.id)}
                    data-testid={`plan-row-${r.no}`}
                  >
                    <td className="px-0.5">
                      <button
                        type="button"
                        className="rounded p-0.5 hover:bg-slate-100 dark:hover:bg-slate-800"
                        title="PLC 구조(LGR_Task_Data) 보기"
                        onClick={(e) => {
                          e.stopPropagation()
                          setOpen(open === r.id ? null : r.id)
                        }}
                        data-testid={`plan-expand-${r.no}`}
                      >
                        {open === r.id ? (
                          <ChevronDown className="h-3.5 w-3.5" />
                        ) : (
                          <ChevronRight className="h-3.5 w-3.5" />
                        )}
                      </button>
                    </td>
                    <td className="px-1 py-0.5 font-mono tabular-nums">{r.no}</td>
                    <td className="px-1">
                      <button
                        type="button"
                        className={cn(
                          'rounded px-1.5 py-0.5 text-[11px] font-bold text-white',
                          r.type === 'PICK'
                            ? 'bg-indigo-600'
                            : r.type === 'DROP'
                              ? 'bg-emerald-600'
                              : 'bg-slate-600',
                        )}
                        title="클릭: PICK ↔ DROP"
                        onClick={(e) => {
                          e.stopPropagation()
                          onChange(toggleType(steps, r.id))
                        }}
                        data-testid={`plan-type-${r.no}`}
                      >
                        {r.type}
                      </button>
                      <Select
                        dense
                        className="ml-1 inline-block w-auto"
                        value={r.type}
                        onValueChange={(v) => onChange(patch(steps, r.id, { type: v as TaskType }))}
                        aria-label="종류"
                      >
                        {TYPES.map((t) => (
                          <option key={t} value={t}>
                            {t}
                          </option>
                        ))}
                      </Select>
                    </td>
                    <td className="px-1 whitespace-nowrap">
                      {r.target.kind === 'cell' ? '셀' : '스테이션'}{' '}
                      <b className="font-mono">#{r.target.id}</b>
                    </td>
                    <td className="px-1">
                      <Select
                        dense
                        value={r.item_code === null ? '' : String(r.item_code)}
                        onValueChange={(v) =>
                          onChange(patch(steps, r.id, { item_code: v === '' ? null : Number(v) }))
                        }
                        aria-label="품목"
                      >
                        <option value="">(없음)</option>
                        {items.map((it) => (
                          <option key={it.code} value={String(it.code)}>
                            {it.code} {it.name}
                          </option>
                        ))}
                      </Select>
                    </td>
                    <td className="px-1 text-right">
                      <Input
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
                        aria-label="수량"
                      />
                    </td>
                    <td className="px-1 text-right font-mono tabular-nums text-slate-500">
                      {r.stockBefore === null ? '-' : `${r.stockBefore}→${r.stockAfter}`}
                    </td>
                    <td
                      className="px-1 text-right font-mono tabular-nums"
                      title={r.height !== null ? `바닥 ${r.floor} · H ${r.height}` : ''}
                    >
                      {r.z === null ? <span className="text-slate-400">?</span> : r.z.toFixed(0)}
                    </td>
                    <td className="px-1">
                      {r.warnings.length ? (
                        <span className="flex flex-wrap gap-1">
                          {r.warnings.map((w) => (
                            <StatusBadge key={w} status="warn">
                              {w}
                            </StatusBadge>
                          ))}
                        </span>
                      ) : null}
                    </td>
                    <td className="px-1 whitespace-nowrap">
                      <Button
                        size="icon-sm"
                        intent="ghost"
                        icon={<ArrowUp className="h-3.5 w-3.5" />}
                        title="위로"
                        disabled={i === 0}
                        onClick={(e) => {
                          e.stopPropagation()
                          onChange(move(steps, i, i - 1))
                        }}
                      />
                      <Button
                        size="icon-sm"
                        intent="ghost"
                        icon={<ArrowDown className="h-3.5 w-3.5" />}
                        title="아래로"
                        disabled={i === rows.length - 1}
                        onClick={(e) => {
                          e.stopPropagation()
                          onChange(move(steps, i, i + 1))
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
                    </td>
                  </tr>
                  {open === r.id ? (
                    <tr className="bg-slate-50 dark:bg-slate-900/40">
                      <td colSpan={10} className="px-2 py-2">
                        {preview?.id === r.id && preview.p ? (
                          <>
                            <PlcStructView
                              title={`LGR_Task_Data — 스텝 ${r.no} (재고 ${r.stockBefore ?? '?'} 가정 compose)`}
                              rows={taskDataRows(preview.p.task)}
                            />
                            {preview.p.warnings.length ? (
                              <div className="mt-1 text-[11px] text-amber-700 dark:text-amber-300">
                                경고: {preview.p.warnings.join(' · ')}
                              </div>
                            ) : null}
                          </>
                        ) : preview?.id === r.id && preview.err ? (
                          <span className="text-red-600">{preview.err}</span>
                        ) : (
                          <span className="text-slate-400">compose 중…</span>
                        )}
                      </td>
                    </tr>
                  ) : null}
                </Fragment>
              ))
            )}
          </tbody>
        </table>
      </div>

      <div className="flex flex-wrap items-end gap-2 border-t border-slate-200 px-3 py-2 dark:border-slate-700">
        <Input
          label="시나리오 이름"
          className="min-w-40 flex-1"
          value={name}
          onValueChange={setName}
          placeholder="비우면 날짜로"
        />
        <Button
          size="sm"
          icon={<Save className="h-3.5 w-3.5" />}
          disabled={!steps.length || busy}
          onClick={() => void saveScenario(false)}
          data-testid="plan-save"
        >
          시나리오로 저장
        </Button>
        <Button
          size="sm"
          intent="primary"
          icon={<Play className="h-3.5 w-3.5" />}
          disabled={!steps.length || busy}
          onClick={() => void saveScenario(true)}
          data-testid="plan-run"
        >
          저장 후 실행
        </Button>
        <Button
          size="sm"
          intent="outline"
          icon={<Send className="h-3.5 w-3.5" />}
          disabled={!first || busy || !gate?.can_submit}
          title={!gate?.can_submit ? '게이트가 닫혀 있습니다' : '첫 스텝만 지금 제출'}
          onClick={() => setConfirmNext(true)}
          data-testid="plan-next"
        >
          다음 1건 제출
        </Button>
      </div>

      <ConfirmDialog
        open={confirmNext}
        onOpenChange={setConfirmNext}
        scope="single-robot"
        danger
        title="다음 스텝 제출"
        confirmLabel="PLC로 제출"
        onConfirm={() => void submitNext()}
      >
        {first ? (
          <p className="text-xs">
            <b>{first.type}</b> · {first.target.kind === 'cell' ? '셀' : '스테이션'} #
            {first.target.id}
            {first.item_code !== null ? ` · 품목 ${first.item_code} × ${first.count}` : ''}
            {first.z !== null ? ` · Z ${first.z.toFixed(0)}` : ''}. 로봇이 실제로 움직입니다.
          </p>
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
        <p className="text-xs">{steps.length}개 스텝을 지웁니다(되돌리기 가능).</p>
      </ConfirmDialog>
    </Card>
  )
}
