// 작성 카드 — 종류·대상·품목·수량·파라미터 덮어쓰기 → 미리보기 → 게이트 → 확인 → 제출 → Ack.
//
// 초안(`Draft`)이 진실원이고 `TaskRequest`는 그 투영이다(`buildRequest`). 파라미터 폼은 **덮어쓰기**
// 모드(`partial`)라 체크 안 한 키는 요청에 실리지 않고 백엔드 기본값(공통 ← 종류·대상별)이 쓰인다 —
// 체크박스를 끄는 것이 곧 "기본값 사용"이다. 흐리게 보이는 값은 `effectiveParams`가 계산한 그 기본값이다.
import { useEffect, useMemo, useRef, useState } from 'react'
import { ChevronDown, ChevronRight, RotateCcw, Send, Settings2 } from 'lucide-react'
import { api } from '../../lib/api'
import { TASK_TYPES } from '../../lib/gr/const'
import {
  EMPTY_DRAFT,
  buildRequest,
  effectiveParams,
  itemRequired,
  targetKindsFor,
  targetRequired,
  validateDraft,
} from '../../lib/task/compose'
import type { ComposePreview, Draft } from '../../lib/task/types'
import { Button } from '../../lib/ui/Button'
import { Card } from '../../lib/ui/Card'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { Input } from '../../lib/ui/Input'
import { Select } from '../../lib/ui/Select'
import { toast } from '../../lib/ui/toast'
import type {
  Cell,
  Defaults,
  Gate,
  Item,
  Station,
  Target,
  Task,
  TaskRequest,
  TaskType,
} from '../../lib/types'
import { ItemPicker } from '../shared/ItemPicker'
import { TargetPicker } from '../shared/TargetPicker'
import { TaskParamFields } from '../shared/TaskParamFields'
import { AckBanner, type AckPhase } from './AckBanner'
import { PositionPreview } from './PositionPreview'

const ACK_POLL_MS = 500
const ACK_MAX_MS = 8000
const TERMINAL = new Set(['rejected', 'completed', 'canceled', 'failed', 'lost'])

const TYPE_LABEL: Record<TaskType, string> = {
  UP: 'UP — 상승',
  PICK: 'PICK — 집기',
  DROP: 'DROP — 놓기',
  MOVE: 'MOVE — 이동',
  MEASURE: 'MEASURE — 측정',
}

export interface ComposeCardProps {
  items: Item[]
  cells: Cell[]
  stations: Station[]
  defaults: Defaults | null
  gate: Gate | null
  onOpenDefaults: () => void
  /** 바깥(레이아웃 맵)에서 고른 대상 — nonce 가 바뀔 때마다 초안에 적용. */
  pickedTarget?: { target: Target; nonce: number } | null
  /** 초안 대상이 바뀔 때 알린다(레이아웃 강조용). */
  onTargetChange?: (t: Target | null) => void
}

export function ComposeCard({
  items,
  cells,
  stations,
  defaults,
  gate,
  onOpenDefaults,
  pickedTarget = null,
  onTargetChange,
}: ComposeCardProps) {
  const [draft, setDraft] = useState<Draft>(EMPTY_DRAFT)
  const [showParams, setShowParams] = useState(false)
  const [preview, setPreview] = useState<ComposePreview | null>(null)
  const [confirm, setConfirm] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const [ack, setAck] = useState<{ task: Task; phase: AckPhase } | null>(null)
  const pollRef = useRef<number>(0)
  const cardRef = useRef<HTMLDivElement>(null)

  const set = (patch: Partial<Draft>) => setDraft((d) => ({ ...d, ...patch }))
  const kinds = targetKindsFor(draft.type)
  const kind = draft.target?.kind ?? kinds[0]
  const problems = useMemo(() => validateDraft(draft), [draft])
  const request: TaskRequest | null = problems.length === 0 ? buildRequest(draft) : null
  const base = useMemo(
    () => effectiveParams(defaults, draft.type, kind, {}),
    [defaults, draft.type, kind],
  )
  const overrideCount = Object.keys(draft.params).length
  const canSubmit = !!request && !!gate?.can_submit && !submitting

  // 종류가 바뀌면 그 종류에 맞지 않는 대상은 비운다(MEASURE → 스테이션).
  useEffect(() => {
    if (draft.target && !kinds.includes(draft.target.kind)) set({ target: null })
    // eslint-disable-next-line react-hooks/exhaustive-deps -- kinds는 type에서 파생
  }, [draft.type])

  useEffect(() => () => clearTimeout(pollRef.current), [])

  // 레이아웃 맵에서 고른 대상 적용 — 지금 종류가 그 대상을 못 받으면(MEASURE → 스테이션) PICK 으로 바꾼다.
  useEffect(() => {
    if (!pickedTarget) return
    const t = pickedTarget.target
    setDraft((d) => ({
      ...d,
      target: t,
      type: targetKindsFor(d.type).includes(t.kind) ? d.type : 'PICK',
    }))
    cardRef.current?.scrollIntoView?.({ block: 'nearest', behavior: 'smooth' })
  }, [pickedTarget])

  useEffect(() => {
    onTargetChange?.(draft.target)
  }, [draft.target, onTargetChange])

  /** 제출 뒤 `GET /api/tasks/{id}`를 0.5초마다 — `ack`이 앉거나 상태가 끝나거나 8초. */
  function watchAck(task: Task) {
    const started = Date.now()
    const tick = async () => {
      try {
        const t = await api.task(task.id)
        if (t.ack || TERMINAL.has(t.state)) {
          setAck({ task: t, phase: 'done' })
          toast[t.ack?.accepted === false ? 'warn' : 'ok'](
            t.ack?.accepted === false
              ? `#${t.seq} 거부됨 (코드 ${t.ack.code})`
              : `#${t.seq} ${t.ack ? '수락됨' : t.state}`,
          )
          return
        }
        setAck({ task: t, phase: 'waiting' })
      } catch {
        /* 다음 tick에 다시 */
      }
      if (Date.now() - started >= ACK_MAX_MS) {
        setAck((cur) => (cur ? { ...cur, phase: 'timeout' } : cur))
        return
      }
      pollRef.current = window.setTimeout(() => void tick(), ACK_POLL_MS)
    }
    pollRef.current = window.setTimeout(() => void tick(), ACK_POLL_MS)
  }

  async function submit() {
    if (!request) return
    setSubmitting(true)
    clearTimeout(pollRef.current)
    try {
      const t = await api.taskCreate(request, true)
      setAck({ task: t, phase: t.ack || TERMINAL.has(t.state) ? 'done' : 'waiting' })
      toast.info(`#${t.seq} 제출됨 — Work ${t.work_id} / Task ${t.task_id}`)
      if (!(t.ack || TERMINAL.has(t.state))) watchAck(t)
    } catch (e) {
      toast.error(`제출 실패 — ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <Card padded={false} className="flex flex-col" data-testid="compose-card">
      <div ref={cardRef} />
      <div className="flex items-center gap-2 border-b border-slate-200 px-3 py-2 dark:border-slate-700">
        <Send className="h-4 w-4 text-slate-500" />
        <span className="text-sm font-semibold">작업 작성</span>
        <span className="flex-1" />
        <Button
          size="sm"
          intent="ghost"
          icon={<Settings2 className="h-3.5 w-3.5" />}
          onClick={onOpenDefaults}
          data-testid="open-defaults"
        >
          기본값
        </Button>
        <Button
          size="sm"
          intent="ghost"
          icon={<RotateCcw className="h-3.5 w-3.5" />}
          onClick={() => setDraft(EMPTY_DRAFT)}
          title="초안 비우기"
        >
          초기화
        </Button>
      </div>

      <div className="flex flex-col gap-3 px-3 py-3">
        <div className="flex flex-wrap items-end gap-2">
          <Select
            label="작업 종류"
            value={draft.type}
            onValueChange={(v) => set({ type: v as TaskType })}
            data-testid="compose-type"
          >
            {TASK_TYPES.map((t) => (
              <option key={t} value={t}>
                {TYPE_LABEL[t]}
              </option>
            ))}
          </Select>
          <Input
            label="수량"
            type="number"
            mono
            min={1}
            max={255}
            step="1"
            className="w-20"
            value={String(draft.count)}
            onValueChange={(s) => set({ count: Number(s) })}
            data-testid="compose-count"
          />
          <Input
            label="메모"
            className="min-w-40 flex-1"
            value={draft.note}
            onValueChange={(note) => set({ note })}
            placeholder="원장에 남는 한 줄"
          />
        </div>

        <TargetPicker
          value={draft.target}
          onChange={(target) => set({ target })}
          cells={cells}
          stations={stations}
          kinds={kinds}
        />

        <div className="flex flex-wrap items-end gap-2">
          <ItemPicker
            value={draft.item_code}
            onChange={(item_code) => set({ item_code })}
            items={items}
            allowNone={!itemRequired(draft.type)}
          />
          {!targetRequired(draft.type) ? (
            <span className="pb-2 text-xs text-slate-400">UP은 대상 없이 제출할 수 있습니다</span>
          ) : null}
        </div>

        <div className="rounded-md border border-slate-200 dark:border-slate-700">
          <button
            type="button"
            className="flex w-full items-center gap-2 px-2 py-1.5 text-left text-xs font-medium"
            onClick={() => setShowParams((s) => !s)}
            aria-expanded={showParams}
            data-testid="toggle-params"
          >
            {showParams ? (
              <ChevronDown className="h-3.5 w-3.5" />
            ) : (
              <ChevronRight className="h-3.5 w-3.5" />
            )}
            파라미터 덮어쓰기
            <span className="font-normal text-slate-400">
              {overrideCount ? `${overrideCount}개 덮어씀` : '모두 기본값 사용'} · 기본값 = 공통 ←{' '}
              {draft.type}·{kind === 'cell' ? '셀' : '스테이션'}
            </span>
            <span className="flex-1" />
            {overrideCount ? (
              <Button
                size="sm"
                intent="ghost"
                onClick={(e) => {
                  e.stopPropagation()
                  set({ params: {} })
                }}
              >
                모두 기본값 사용
              </Button>
            ) : null}
          </button>
          {showParams ? (
            <div className="border-t border-slate-200 px-2 py-2 dark:border-slate-700">
              <p className="mb-2 text-[11px] text-slate-500">
                체크를 끄면 그 항목은 기본값을 씁니다(흐린 값). 켠 항목만 이 작업에 실립니다.
              </p>
              <TaskParamFields
                value={draft.params}
                base={base}
                onChange={(params) => set({ params })}
                partial
              />
            </div>
          ) : null}
        </div>

        <div className="rounded-md border border-slate-200 px-2 py-2 dark:border-slate-700">
          <div className="mb-1 text-[11px] font-semibold text-slate-500">위치 미리보기</div>
          {problems.length ? (
            <ul className="list-disc pl-4 text-xs text-slate-400" data-testid="draft-problems">
              {problems.map((p) => (
                <li key={p}>{p}</li>
              ))}
            </ul>
          ) : null}
          <PositionPreview request={request} onPreview={setPreview} />
        </div>

        {ack ? (
          <AckBanner task={ack.task} phase={ack.phase} onDismiss={() => setAck(null)} />
        ) : null}

        <div className="flex items-center justify-end gap-2">
          <Button
            intent="primary"
            icon={<Send className="h-4 w-4" />}
            disabled={!canSubmit}
            loading={submitting}
            onClick={() => setConfirm(true)}
            data-testid="compose-submit"
            title={
              !gate?.can_submit
                ? '게이트가 닫혀 있습니다'
                : request
                  ? ''
                  : '초안이 완성되지 않았습니다'
            }
          >
            제출
          </Button>
        </div>
      </div>

      <ConfirmDialog
        open={confirm}
        onOpenChange={setConfirm}
        scope="single-robot"
        danger
        title={`${draft.type} 작업 제출`}
        confirmLabel="PLC로 제출"
        onConfirm={() => void submit()}
      >
        <div className="flex flex-col gap-1 text-xs">
          <p>
            <b>{draft.type}</b>
            {draft.target
              ? ` · ${draft.target.kind === 'cell' ? '셀' : '스테이션'} #${draft.target.id}`
              : ' · 대상 없음'}
            {draft.item_code !== null ? ` · 품목 ${draft.item_code} × ${draft.count}` : ''}
          </p>
          {preview ? (
            <p className="font-mono text-slate-500">
              X {preview.task.Position[0]} · Y {preview.task.Position[1]} · Z{' '}
              {preview.task.Position[2]} · G {preview.task.Position[3]}
            </p>
          ) : null}
          {preview && preview.warnings.length ? (
            <p className="text-amber-700 dark:text-amber-300">
              경고 {preview.warnings.length}건 — 미리보기를 확인하세요
            </p>
          ) : null}
          <p className="text-slate-500">
            GRM OPC UA를 거쳐 GR2에 명령이 갑니다. 로봇이 실제로 움직입니다.
          </p>
        </div>
      </ConfirmDialog>
    </Card>
  )
}
