// 작성 카드 — 종류·대상·품목·수량·파라미터 덮어쓰기 → 미리보기 → 게이트 → 확인 → 제출 → Ack.
//
// 초안(`Draft`)이 진실원이고 `TaskRequest`는 그 투영이다(`buildRequest`). 파라미터 폼은 **덮어쓰기**
// 모드(`partial`)라 체크 안 한 키는 요청에 실리지 않고 백엔드 기본값(공통 ← 종류·대상별)이 쓰인다 —
// 체크박스를 끄는 것이 곧 "기본값 사용"이다. 흐리게 보이는 값은 `effectiveParams`가 계산한 그 기본값이다.
import { useEffect, useMemo, useRef, useState } from 'react'
import { RotateCcw, Send, Settings2 } from 'lucide-react'
import { api } from '../../lib/api'
import { TASK_TYPES } from '../../lib/gr/const'
import { robots } from '../../lib/robots'
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
import { Dialog } from '../../lib/ui/Dialog'
import { FieldList } from '../../lib/ui/FieldList'
import { HelpTip } from '../../lib/ui/HelpTip'
import { Input } from '../../lib/ui/Input'
import { Select } from '../../lib/ui/Select'
import { toast } from '../../lib/ui/toast'
import type {
  Cell,
  Defaults,
  Gate,
  GripRef,
  Item,
  Station,
  Target,
  Task,
  TaskRequest,
  TaskType,
} from '../../lib/types'
import { gripLabel } from '../../lib/task/plan'
import { f1 } from '../../lib/meas/format'
import { ItemPicker } from '../shared/ItemPicker'
import { TargetPicker } from '../shared/TargetPicker'
import { TaskParamFields } from '../shared/TaskParamFields'
import { AckBanner, type AckPhase } from './AckBanner'
import { PositionPreview } from './PositionPreview'
import { StationOffsetBlock } from './StationOffsetBlock'

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
  /** 바깥(레이아웃 맵·명령 팔레트)에서 고른 대상 — nonce 가 바뀔 때마다 초안에 적용. `type` 이 있으면 종류도. */
  pickedTarget?: { target: Target; type?: TaskType; nonce: number } | null
  /** 초안 대상이 바뀔 때 알린다(레이아웃 강조용). */
  onTargetChange?: (t: Target | null) => void
  /** 자기 껍데기(카드 테두리 + 머리줄)를 그릴지.
   *
   *  `false` 는 **바깥에 이미 카드가 하나 있다**는 뜻이다(`PlanCard` 의 단일 명령 모드). 전에는
   *  바깥 카드가 CSS 로 안쪽 카드의 테두리를 벗겨 냈는데, 그래도 머리줄은 두 겹으로 남았다 —
   *  카드 안 카드도 머리띠 두 겹도 만들지 않는다(`docs/DESIGN.md` 5절). */
  chrome?: boolean
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
  chrome = true,
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
  // 미리보기가 StackMax 초과 DROP 을 알리면 서버가 409 로 거부한다 — 무시 플래그 없이는 누르지 못하게.
  const stackBlocked = !!request && !!preview?.stack_limit?.blocked
  const canSubmit = !!request && !!gate?.can_submit && !submitting && !stackBlocked

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
    const forced = pickedTarget.type
    setDraft((d) => {
      const type = forced ?? (targetKindsFor(d.type).includes(t.kind) ? d.type : 'PICK')
      return { ...d, target: t, type: targetKindsFor(type).includes(t.kind) ? type : 'PICK' }
    })
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

  const inner = (
    <>
      <div ref={cardRef} />
      <div className="flex flex-col gap-3 px-3 py-3">
        <div className="flex flex-wrap items-end gap-2">
          <Select
            label="Type"
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
            label="Count"
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
            label="Note"
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
            <span className="pb-2">
              <HelpTip title="UP" text="UP 은 대상 없이 제출할 수 있습니다." />
            </span>
          ) : null}
        </div>

        {/* 덮어쓰기는 **한 줄 요약이 곧 손잡이**다 — 접었다 폈다 해도 같은 값을 다시 보여 줄 뿐인
            토글은 자리만 먹는다. 값은 여기서 읽고 고치는 일은 팝업에서 한다. */}
        <div
          className="flex flex-wrap items-center gap-x-2 gap-y-1 rounded-md border border-line-default px-2 py-1.5 text-xs"
          data-testid="params-row"
        >
          <span className="font-medium">파라미터</span>
          <span className="text-content-faint tabular-nums">
            {overrideCount ? `덮어씀 ${overrideCount}` : '모두 기본값'}
          </span>
          <span className="text-content-faint">
            기본 {draft.type}·{kind === 'cell' ? 'Cell' : 'Station'}
          </span>
          <HelpTip
            title="파라미터 덮어쓰기"
            text="체크를 끈 항목은 기본값(공통 ← 종류·대상별)을 씁니다. 켠 항목만 이 작업에 실립니다."
          />
          <span className="flex-1" />
          <Button
            size="sm"
            intent="ghost"
            onClick={() => setShowParams(true)}
            data-testid="toggle-params"
          >
            덮어쓰기…
          </Button>
        </div>

        <div className="rounded-md border border-line-default px-2 py-2">
          <div className="mb-1 text-2xs font-semibold text-content-muted">위치 미리보기</div>
          {problems.length ? (
            <ul className="list-disc pl-4 text-xs text-content-faint" data-testid="draft-problems">
              {problems.map((p) => (
                <li key={p}>{p}</li>
              ))}
            </ul>
          ) : null}
          <PositionPreview request={request} onPreview={setPreview} />
          {request && preview ? (
            <StackNote
              preview={preview}
              asked={request.grip_ref}
              ignore={!!draft.ignore_stack_max}
              onIgnore={(on) => set({ ignore_stack_max: on })}
            />
          ) : null}
          {draft.target?.kind === 'station' ? (
            <div className="mt-2 border-t border-line-default pt-2">
              <StationOffsetBlock
                audit={request ? preview?.station_offset : null}
                enabled={draft.station_offset !== false}
                onToggle={(on) => set({ station_offset: on })}
              />
            </div>
          ) : null}
        </div>

        {ack ? (
          <AckBanner task={ack.task} phase={ack.phase} onDismiss={() => setAck(null)} />
        ) : null}

        <div className="flex items-center gap-2">
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
          <span className="flex-1" />
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
                : stackBlocked
                  ? `StackMax 초과 — ${preview?.stack_limit?.blocked ?? ''}`
                  : request
                    ? ''
                    : '초안이 완성되지 않았습니다'
            }
          >
            제출
          </Button>
        </div>
      </div>

      <Dialog
        open={showParams}
        onOpenChange={setShowParams}
        title="파라미터 덮어쓰기"
        size="lg"
        meta={`${draft.type} · ${kind === 'cell' ? 'Cell' : 'Station'} · 덮어씀 ${overrideCount}`}
        footer={
          <>
            <Button
              size="sm"
              intent="ghost"
              disabled={!overrideCount}
              title={overrideCount ? undefined : '덮어쓴 항목이 없습니다'}
              onClick={() => set({ params: {} })}
            >
              모두 기본값 사용
            </Button>
            <Button size="sm" intent="primary" onClick={() => setShowParams(false)}>
              닫기
            </Button>
          </>
        }
        testid="params-dialog"
      >
        <TaskParamFields
          value={draft.params}
          base={base}
          onChange={(params) => set({ params })}
          partial
        />
      </Dialog>

      <ConfirmDialog
        open={confirm}
        onOpenChange={setConfirm}
        scope="single-robot"
        danger
        title={`${draft.type} 작업 제출`}
        confirmLabel="PLC로 제출"
        onConfirm={() => void submit()}
      >
        <div className="flex flex-col gap-2 text-xs">
          <p className="m-0">PLC 로 제출할까요?</p>
          <FieldList
            columns={2}
            dense
            labelWidth={72}
            items={[
              { label: '종류', value: draft.type },
              {
                label: '대상',
                value: draft.target
                  ? `${draft.target.kind === 'cell' ? 'Cell' : 'Station'} #${draft.target.id}`
                  : '',
                missing: '대상 없음',
              },
              {
                label: '품목',
                value: draft.item_code !== null ? `${draft.item_code} × ${draft.count}` : '',
                missing: '품목 없음',
              },
              { label: '로봇', value: robots.current?.name ?? '', missing: '기본 로봇' },
              ...(preview
                ? [
                    { label: 'X (mm)', value: f1(preview.task.Position[0]) },
                    { label: 'Y (mm)', value: f1(preview.task.Position[1]) },
                    { label: 'Z (mm)', value: f1(preview.task.Position[2]) },
                    { label: 'G (mm)', value: f1(preview.task.Position[3]) },
                  ]
                : []),
              ...(preview && preview.warnings.length
                ? [
                    {
                      label: '경고',
                      value: `${preview.warnings.length}건`,
                      status: 'warn' as const,
                    },
                  ]
                : []),
              ...(preview?.station_offset?.blocked
                ? [
                    {
                      label: '보정 거부',
                      value: preview.station_offset.blocked,
                      status: 'fault' as const,
                      wide: true,
                    },
                  ]
                : []),
              ...(preview?.stack_limit?.ignored
                ? [
                    {
                      label: 'StackMax',
                      value: `${preview.stack_limit.stack_max} 초과 · ${preview.stack_limit.count_after}개 · 무시`,
                      status: 'warn' as const,
                      wide: true,
                    },
                  ]
                : []),
            ]}
          />
          <p className="m-0 text-fault-fg">로봇이 실제로 움직입니다.</p>
        </div>
      </ConfirmDialog>
    </>
  )

  return chrome ? (
    <Card padded={false} className="flex flex-col" data-testid="compose-card">
      <div className="flex h-screen-header flex-none items-center gap-2 border-b border-line-default px-3">
        <Send className="h-4 w-4 text-content-muted" />
        <span className="text-sm font-semibold">작업 작성</span>
      </div>
      {inner}
    </Card>
  ) : (
    <div className="flex flex-col" data-testid="compose-card">
      {inner}
    </div>
  )
}

/** 스택 Z 근거(z_source)와 StackMax 검사 — 미리보기 아래 한두 줄. 초과 DROP 이면 무시 체크가 붙는다. */
function StackNote({
  preview,
  asked,
  ignore,
  onIgnore,
}: {
  preview: ComposePreview
  /** 요청이 시킨 그립 기준(없으면 Defaults) — 실제로 쓴 기준과 다르면 그 까닭을 보인다. */
  asked?: GripRef | null
  ignore: boolean
  onIgnore: (on: boolean) => void
}) {
  const z = preview.stack_z
  const l = preview.stack_limit
  if (!z && !l) return null
  const dropOver = !!l && (l.blocked !== null || l.ignored)
  const verdict = !l
    ? null
    : l.blocked
      ? { text: '제출 거부', tone: 'fault' as const }
      : l.ignored
        ? { text: '초과 · 무시하고 제출', tone: 'warn' as const }
        : l.over_limit
          ? { text: '셀 재고가 이미 초과', tone: 'warn' as const }
          : { text: '통과', tone: undefined }
  return (
    <div
      className="mt-2 flex flex-col gap-1.5 border-t border-line-default pt-2"
      data-testid="stack-note"
    >
      {z ? (
        <FieldList
          columns={2}
          dense
          labelWidth={96}
          items={[
            { label: 'z_source', value: z.z_source, mono: true, tooltip: z.used.join(' · ') },
            { label: 'GripRef', value: gripLabel(asked, z.grip_ref) },
            { label: 'Level', value: `${z.level} (위 ${z.above})` },
            { label: 'Base (mm)', value: f1(z.base) },
            { label: 'Grip (mm)', value: f1(z.grip) },
            {
              label: 'UpperBead (mm)',
              value: z.upper_bead === null ? '' : f1(z.upper_bead),
              missing: '잰 값 없음',
            },
            {
              label: 'PickBeadOffset',
              value: z.pick_bead_offset === null ? '' : f1(z.pick_bead_offset),
              missing: '지정 없음',
            },
            { label: 'Compression (mm)', value: f1(z.compression) },
            { label: 'Used', value: z.used.join(', '), missing: '없음', wide: true, mono: true },
          ]}
        />
      ) : null}
      {z && z.warnings.length ? (
        <ul className="m-0 list-disc pl-4 text-2xs text-warn-fg">
          {z.warnings.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
      ) : null}
      {l && verdict ? (
        <div data-testid="stack-limit">
          <FieldList
            columns={3}
            dense
            labelWidth={72}
            items={[
              { label: 'StackMax', value: String(l.stack_max) },
              { label: 'count_after', value: String(l.count_after), status: verdict.tone },
              { label: '판정', value: verdict.text, status: verdict.tone },
              ...(l.blocked ? [{ label: '사유', value: l.blocked, wide: true }] : []),
            ]}
          />
        </div>
      ) : null}
      {dropOver ? (
        <label className="flex items-center gap-1.5 text-2xs text-warn-fg">
          <input
            type="checkbox"
            checked={ignore}
            onChange={(e) => onIgnore(e.currentTarget.checked)}
            data-testid="stack-ignore"
          />
          StackMax 무시
          <HelpTip
            title="ignore_stack_max"
            text="셀의 StackMax 를 넘겨도 제출합니다(요청에 ignore_stack_max 가 실립니다). 끄면 서버가 409 로 거부합니다."
          />
        </label>
      ) : null}
    </div>
  )
}
