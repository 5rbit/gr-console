// 팔렛 패턴 편집 — 편집 저장소(`/api/pallet/flows`)의 흐름·패턴·슬롯을 고친다. 사양서 R4 는 기준본으로만
// 남고(되돌리기·비교), 생성기·계획·작업 명령은 저장된 패턴을 쓴다.
//
// 왼쪽: 흐름 목록(만들기·복사·이름 바꾸기·삭제·사양서로 되돌리기) + 패턴 목록(추가·복제·삭제)
// 가운데: 끌어서 옮기는 그림(스냅, 회전·미러, Seq 재정렬) · 오른쪽: 패턴 필드 + 슬롯 표(단위 norm|mm) + DragDir 격자.
// 한 번에 패턴 하나를 초안으로 고치고 "저장" 으로 `PUT /api/pallet/flows/{id}/patterns/{원래 번호}`.
// 저장하지 않은 변경이 있으면 다른 흐름·패턴으로 가거나 편집을 끌 때 묻는다.
import { useCallback, useEffect, useRef, useState } from 'react'
import type * as React from 'react'
import { Copy, GitCompare, GripVertical, ListOrdered, Pencil, Plus, Save, Trash2, Undo2 } from 'lucide-react'
import { palletApi } from '../../lib/pallet/api'
import {
  addSlot,
  cycleDir,
  diffPattern,
  duplicatePattern,
  isDirty,
  liveIssues,
  mmToNorm,
  newPattern,
  normMinDistance,
  normToMm,
  normalizeFlowId,
  patternBody,
  pitchOf,
  removeSlot,
  reorderSeq,
  rescaleToUnit,
  round4,
  scaleOf,
  seqByClicks,
  slotsBySeq,
  toDraft,
  transformSlots,
  validatePattern,
  type DraftPattern,
  type DraftSlot,
  type SnapMode,
  type Units,
} from '../../lib/pallet/editorModel'
import {
  dragTypeByte,
  hexByte,
  viewAxes,
  type DragKind,
  type EditFlowView,
  type FlowStatus,
  type ViewMode,
} from '../../lib/pallet/model'
import { Badge } from '../../lib/ui/Badge'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { FormDialog } from '../../lib/ui/Dialog'
import { EmptyState } from '../../lib/ui/EmptyState'
import { ErrorBoundary } from '../../lib/ui/ErrorBoundary'
import { Field } from '../../lib/ui/Field'
import { HelpTip } from '../../lib/ui/HelpTip'
import { Input } from '../../lib/ui/Input'
import type { MenuItem } from '../../lib/ui/menu'
import { Pairs } from '../../lib/ui/Pair'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import { Segmented } from '../../lib/ui/Segmented'
import { Select } from '../../lib/ui/Select'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'
import { cn } from '../../lib/utils'
import { DRAG_DIR_HELP, EDIT_CANVAS_HELP, SLOT_ROW_HELP } from './help'
import { DirPicker } from './DirPicker'
import { PalletJsonDialog } from './PalletJsonDialog'
import { PatternDiffDialog } from './PatternDiffDialog'
import { PatternEditCanvas } from './PatternEditCanvas'

const errMsg = (e: unknown) => (e instanceof Error ? e.message : String(e))
const num = (s: string): number | undefined => {
  if (s.trim() === '') return undefined
  const n = Number(s)
  return Number.isFinite(n) ? n : undefined
}
const h3 = 'text-2xs font-semibold text-content-muted'

// 고른 단위·스냅은 **사람의 습관**이지 패턴의 성질이 아니다 — 화면을 다시 열 때마다 되묻지 않는다
// (보기 방향 `gr-pallet-view` 와 같은 관용구).
const UNITS_KEY = 'gr-pallet-units'
const SNAP_KEY = 'gr-pallet-snap'
const SNAPS: SnapMode[] = ['off', 'mm10', 'norm005']

function readUnits(): Units {
  try {
    return localStorage.getItem(UNITS_KEY) === 'norm' ? 'norm' : 'mm'
  } catch {
    return 'mm'
  }
}
function readSnap(): SnapMode {
  try {
    const v = localStorage.getItem(SNAP_KEY)
    return SNAPS.find((s) => s === v) ?? 'off'
  } catch {
    return 'off'
  }
}
function remember(key: string, value: string): void {
  try {
    localStorage.setItem(key, value)
  } catch {
    /* 저장 못 해도 동작 */
  }
}
const cellInput =
  'h-control-sm w-full min-w-0 rounded border border-line-default bg-surface-inset px-1.5 font-mono text-2xs tabular-nums text-content-primary'

const STATUS_TONE: Record<FlowStatus, string> = {
  spec: 'bg-surface-inset',
  modified: 'bg-warn-soft text-warn-fg',
  custom: 'bg-surface-active text-accent-text',
}
const PATTERN_TONE: Record<string, string> = {
  same: '',
  changed: 'bg-warn-soft text-warn-fg',
  added: 'bg-surface-active text-accent-text',
  custom: '',
}

/** 미리보기 외경 — 범위 가운데(아래가 0 이면 위 끝). */
function midOd(p: DraftPattern): number {
  if (!Number.isFinite(p.od_min) || !Number.isFinite(p.od_max) || p.od_min >= p.od_max) return 700
  return Math.round(p.od_min > 0 ? (p.od_min + p.od_max) / 2 : p.od_max)
}

/** 숫자 칸 — 입력 중 글자는 그대로 두고, 숫자로 읽힐 때만 값을 올린다. 초점이 빠지면 값으로 되돌린다. */
function NumField({
  value,
  onChange,
  label,
  className,
  testid,
}: {
  value: number
  onChange: (v: number) => void
  label: string
  className?: string
  testid?: string
}) {
  const [text, setText] = useState(Number.isFinite(value) ? String(value) : '')
  const focused = useRef(false)
  useEffect(() => {
    if (!focused.current) setText(Number.isFinite(value) ? String(value) : '')
  }, [value])
  return (
    <input
      aria-label={label}
      inputMode="decimal"
      className={cn(cellInput, className)}
      value={text}
      onFocus={() => {
        focused.current = true
      }}
      onBlur={() => {
        focused.current = false
        setText(Number.isFinite(value) ? String(value) : '')
      }}
      onChange={(e) => {
        const t = e.currentTarget.value
        setText(t)
        const n = t.trim() === '' ? NaN : Number(t)
        if (Number.isFinite(n)) onChange(n)
        else if (t.trim() === '') onChange(NaN)
      }}
      data-testid={testid}
    />
  )
}

/** 패턴 목록의 한 행 — 저장된 패턴과 저장 전 초안을 같은 모양으로 만든다. */
type PatternRow = {
  key: string
  pattern: number
  od_min: number
  od_max: number
  slots: number
  min_distance: number | null
  status: string
  draft: boolean
}

type FlowForm = { mode: 'create' | 'copy' | 'edit'; id: string; name: string; drag_kind: DragKind; note: string; copy_from: string }
type Ask = { title: string; body: string; label: string; danger?: boolean; run: () => void | Promise<void> }

export interface PatternEditorProps {
  view: ViewMode
  /** 생성 화면의 Gap — 미리보기 기본값. */
  gap: number
  palletSize: number
  dragDist: number
  initialFlow: string
  /** 저장소가 바뀌었다(생성 화면의 사양·계획을 다시 읽는다). */
  onChanged: () => void
  onDirtyChange: (dirty: boolean) => void
}

export function PatternEditor({ view, gap: gapDefault, palletSize, dragDist, initialFlow, onChanged, onDirtyChange }: PatternEditorProps) {
  const [flows, setFlows] = useState<EditFlowView[]>([])
  const [loadError, setLoadError] = useState<string | null>(null)
  const [flowId, setFlowId] = useState(initialFlow)
  const [base, setBase] = useState<DraftPattern | null>(null)
  const [draft, setDraft] = useState<DraftPattern | null>(null)
  const [originalNo, setOriginalNo] = useState<number | null>(null)
  const [units, setUnits] = useState<Units>(readUnits)
  const [snap, setSnap] = useState<SnapMode>(readSnap)
  const [odText, setOdText] = useState('')
  const [gapText, setGapText] = useState(String(gapDefault))
  const [selected, setSelected] = useState<string | null>(null)
  const [seqMode, setSeqMode] = useState(false)
  const [clicked, setClicked] = useState<string[]>([])
  const [flowForm, setFlowForm] = useState<FlowForm | null>(null)
  const [ask, setAsk] = useState<Ask | null>(null)
  const [diffOpen, setDiffOpen] = useState(false)
  const [jsonOpen, setJsonOpen] = useState(false)
  const [busy, setBusy] = useState(false)
  const [dragRow, setDragRow] = useState<number | null>(null)
  const [revision, setRevision] = useState(0)
  const started = useRef(false)

  const flow = flows.find((f) => f.id === flowId) ?? null
  const dirty = draft !== null && (originalNo === null || isDirty(draft, base))

  const changeUnits = (v: Units) => {
    setUnits(v)
    remember(UNITS_KEY, v)
  }
  const changeSnap = (v: SnapMode) => {
    setSnap(v)
    remember(SNAP_KEY, v)
  }

  useEffect(() => onDirtyChange(dirty), [dirty, onDirtyChange])
  useEffect(() => () => onDirtyChange(false), [onDirtyChange])
  useEffect(() => {
    if (!dirty) return
    const h = (e: BeforeUnloadEvent) => e.preventDefault()
    window.addEventListener('beforeunload', h)
    return () => window.removeEventListener('beforeunload', h)
  }, [dirty])

  const reload = useCallback(async () => {
    const v = await palletApi.flows()
    setFlows(v)
    setLoadError(null)
    return v
  }, [])

  const openPattern = useCallback((f: EditFlowView | null, no: number | null, keepPreview = false) => {
    const p = f ? (no === null ? f.patterns[0] : f.patterns.find((x) => x.pattern === no)) : undefined
    const d = p ? toDraft(p) : null
    setBase(d)
    setDraft(d ? toDraft(d) : null)
    setOriginalNo(p ? p.pattern : null)
    setSelected(null)
    setSeqMode(false)
    setClicked([])
    if (!keepPreview) setOdText(d ? String(midOd(d)) : '')
  }, [])

  useEffect(() => {
    reload().then(
      (v) => {
        if (started.current) return
        started.current = true
        const f = v.find((x) => x.id.toUpperCase() === initialFlow.toUpperCase()) ?? v[0] ?? null
        if (f) setFlowId(f.id)
        openPattern(f, null)
      },
      (e) => setLoadError(errMsg(e)),
    )
  }, [reload, openPattern, initialFlow])

  /** 저장하지 않은 변경이 있으면 묻고 진행. */
  function guard(action: () => void) {
    if (!dirty) return action()
    setAsk({
      title: '저장하지 않은 변경',
      body: `${flowId} Pattern ${draft?.pattern ?? ''} 에 저장하지 않은 변경이 있습니다. 버리고 계속할까요?`,
      label: '버리고 계속',
      danger: true,
      run: action,
    })
  }

  async function act(label: string, f: () => Promise<void>) {
    setBusy(true)
    try {
      await f()
    } catch (e) {
      toast.error(`${label} 실패 — ${errMsg(e)}`)
    } finally {
      setBusy(false)
    }
  }

  // ── 초안 조작 ──
  const patch = (fn: (d: DraftPattern) => DraftPattern) => setDraft((d) => (d ? fn(d) : d))
  const setSlots = (fn: (s: DraftSlot[]) => DraftSlot[]) => patch((d) => ({ ...d, slots: fn(d.slots) }))
  const updateSlot = (name: string, fn: (s: DraftSlot) => DraftSlot) =>
    setSlots((ss) => ss.map((s) => (s.slot === name ? fn(s) : s)))

  const od = num(odText) ?? (draft ? midOd(draft) : 700)
  const gap = num(gapText) ?? gapDefault
  const pitch = pitchOf(od, gap)
  const axes = viewAxes(view, flow)
  const savedDrafts = flow ? flow.patterns.map(toDraft) : []
  const others = savedDrafts.filter((p) => p.pattern !== originalNo)
  const validation = draft && flow ? validatePattern(draft, flow.drag_kind, others) : { errors: [], warnings: [] }
  const issues = draft ? liveIssues(draft.slots, od, gap, palletSize) : []
  const change = draft ? diffPattern(draft, base) : null
  const minD = draft ? normMinDistance(draft.slots) : null
  const selectedSlot = draft?.slots.find((s) => s.slot === selected) ?? null
  const savedPattern = flow && originalNo !== null ? flow.patterns.find((p) => p.pattern === originalNo) : undefined

  function seqClick(name: string) {
    if (!draft) return
    const next = clicked.includes(name) ? clicked : [...clicked, name]
    if (next.length >= draft.slots.length) {
      setSlots((ss) => seqByClicks(ss, next))
      setSeqMode(false)
      setClicked([])
      toast.ok(`Seq 를 ${next.join(' → ')} 순서로 바꿨습니다 (저장 전)`)
    } else setClicked(next)
  }

  // ── 저장 ──
  async function save() {
    if (!flow || !draft || validation.errors.length > 0) return
    await act('패턴 저장', async () => {
      const r = await palletApi.savePattern(flow.id, originalNo ?? draft.pattern, patternBody(draft))
      const list = await reload()
      openPattern(list.find((x) => x.id === flow.id) ?? null, r.pattern.pattern, true)
      setRevision((n) => n + 1)
      onChanged()
      const w = r.warnings ?? []
      toast.ok(`${flow.id} Pattern ${r.pattern.pattern} 저장${w.length ? ` — 경고 ${w.length}건` : ''}`)
    })
  }

  function cancel() {
    if (originalNo === null) openPattern(flow, null, true)
    else if (base) {
      setDraft(toDraft(base))
      setSeqMode(false)
      setClicked([])
    }
  }

  // ── 흐름 ──
  async function submitFlow(form: FlowForm) {
    const { id, error } = normalizeFlowId(form.id)
    if (error) {
      toast.error(error)
      return
    }
    await act('흐름 저장', async () => {
      const r =
        form.mode === 'edit'
          ? await palletApi.updateFlow(flowId, { id, name: form.name, drag_kind: form.drag_kind, note: form.note })
          : await palletApi.createFlow({
              id,
              name: form.name.trim() || undefined,
              note: form.note,
              ...(form.mode === 'copy' ? { copy_from: form.copy_from } : { drag_kind: form.drag_kind }),
            })
      const list = await reload()
      const f = list.find((x) => x.id === r.flow.id) ?? null
      setFlowId(r.flow.id)
      openPattern(f, form.mode === 'edit' ? originalNo : null, form.mode === 'edit')
      setFlowForm(null)
      setRevision((n) => n + 1)
      onChanged()
      toast.ok(form.mode === 'edit' ? `흐름 ${r.flow.id} 저장` : `흐름 ${r.flow.id} 를 만들었습니다`)
    })
  }

  function deleteFlow() {
    if (!flow) return
    setAsk({
      title: '흐름 삭제',
      body: `흐름 ${flow.id} (${flow.name}) 와 패턴 ${flow.patterns.length}개를 지웁니다.${flow.based_on ? ' 사양서 흐름이면 JSON 가져오기로 다시 넣을 수 있습니다.' : ''}`,
      label: '삭제',
      danger: true,
      run: () =>
        act('흐름 삭제', async () => {
          await palletApi.deleteFlow(flow.id)
          const list = await reload()
          const next = list[0] ?? null
          setFlowId(next?.id ?? '')
          openPattern(next, null)
          onChanged()
          toast.ok(`흐름 ${flow.id} 를 지웠습니다`)
        }),
    })
  }

  function resetFlow(pattern?: number) {
    if (!flow) return
    const what = pattern === undefined ? `흐름 ${flow.id} 전체(이름·DragKind·모든 패턴)` : `${flow.id} Pattern ${pattern}`
    setAsk({
      title: '사양서로 되돌리기',
      body: `${what} 를 사양서 R4 (${flow.based_on}) 값으로 되돌립니다. 지금 저장된 편집은 사라집니다.`,
      label: '되돌리기',
      danger: true,
      run: () =>
        act('되돌리기', async () => {
          await palletApi.resetFlow(flow.id, pattern)
          const list = await reload()
          openPattern(list.find((x) => x.id === flow.id) ?? null, pattern ?? originalNo, true)
          setRevision((n) => n + 1)
          onChanged()
          toast.ok(`${what} 를 사양서 값으로 되돌렸습니다`)
        }),
    })
  }

  function deletePattern() {
    if (!flow || originalNo === null) return
    setAsk({
      title: '패턴 삭제',
      body: `${flow.id} Pattern ${originalNo} 를 지웁니다. 그 OD 범위의 타이어는 이 흐름으로 계획·작업을 만들 수 없게 됩니다.`,
      label: '삭제',
      danger: true,
      run: () =>
        act('패턴 삭제', async () => {
          await palletApi.deletePattern(flow.id, originalNo)
          const list = await reload()
          openPattern(list.find((x) => x.id === flow.id) ?? null, null)
          setRevision((n) => n + 1)
          onChanged()
          toast.ok(`${flow.id} Pattern ${originalNo} 를 지웠습니다`)
        }),
    })
  }

  function startNew(fromCurrent: boolean) {
    guard(() => {
      if (!flow) return
      const d = fromCurrent && base ? duplicatePattern(base, savedDrafts) : newPattern(savedDrafts)
      setBase(null)
      setDraft(d)
      setOriginalNo(null)
      setSelected(null)
      setOdText(String(od))
      toast.ok(`새 Pattern ${d.pattern} 초안 — OdMin/OdMax 를 고친 뒤 저장하세요`)
    })
  }

  // ── 넘침 메뉴 ── 띠에는 매번 쓰는 것만 남기고, 가끔 쓰는 것은 `⋯` 뒤로 모은다(자리는 알람의 것이다).
  const flowMenu = (): MenuItem[] => [
    {
      label: '이름 · 설정…',
      disabled: !flow ? '흐름이 없습니다' : busy ? '처리 중입니다' : undefined,
      run: () =>
        flow && guard(() => setFlowForm({ mode: 'edit', id: flow.id, name: flow.name, drag_kind: flow.drag_kind, note: flow.note, copy_from: '' })),
    },
    {
      label: '사양서로 되돌리기',
      danger: true,
      disabled: !flow?.based_on ? '사양서 기준이 없는 흐름입니다' : flow.status === 'spec' ? '사양서와 같습니다' : busy ? '처리 중입니다' : undefined,
      run: () => resetFlow(),
    },
    {
      label: '흐름 삭제',
      danger: true,
      disabled: !flow
        ? '흐름이 없습니다'
        : flow.used_by.length > 0
          ? `스테이션 ${flow.used_by.join(', ')} 프로파일이 씁니다 — 먼저 프로파일을 바꾸세요`
          : busy
            ? '처리 중입니다'
            : undefined,
      run: deleteFlow,
    },
    { label: 'JSON 내보내기 · 가져오기…', run: () => setJsonOpen(true) },
  ]

  const patternMenu = (): MenuItem[] => [
    { label: '현재 패턴 복제', disabled: !base ? '고른 패턴이 없습니다' : busy ? '처리 중입니다' : undefined, run: () => startNew(true) },
    {
      label: '이 패턴만 사양서로',
      danger: true,
      disabled: !flow?.based_on
        ? '사양서 기준이 없는 흐름입니다'
        : originalNo === null
          ? '저장된 패턴이 아닙니다'
          : patternStatus === 'same'
            ? '사양서와 같습니다'
            : undefined,
      run: () => originalNo !== null && resetFlow(originalNo),
    },
    { label: '패턴 삭제', danger: true, disabled: originalNo === null ? '저장된 패턴이 아닙니다' : undefined, run: deletePattern },
  ]

  const shapeMenu = (): MenuItem[] => [
    { label: '패턴 전체 변환 (기계 축 · DragDir 도 같이)' },
    ...(
      [
        ['MirrorX', { rotation: 0, mirror_x: true, mirror_y: false }],
        ['MirrorY', { rotation: 0, mirror_x: false, mirror_y: true }],
        ['90° 회전', { rotation: 90, mirror_x: false, mirror_y: false }],
        ['180° 회전', { rotation: 180, mirror_x: false, mirror_y: false }],
      ] as const
    ).map(
      ([label, t]): MenuItem => ({
        label,
        disabled: draft ? undefined : '패턴이 없습니다',
        run: () => setSlots((ss) => transformSlots(ss, t)),
      }),
    ),
    {
      label: 'MinDistance 를 1 로',
      hint: minD === null ? '—' : String(round4(minD)),
      disabled: !draft ? '패턴이 없습니다' : minD === null || Math.abs(minD - 1) <= 1e-3 ? '이미 1 입니다' : undefined,
      run: () => setSlots(rescaleToUnit),
    },
  ]

  // ── 패턴 목록(읽는 표) ── 저장하지 않은 새 패턴은 `draft` 행으로 같은 표에 선다.
  const patternRows: PatternRow[] = flow
    ? [
        ...flow.patterns.map(
          (p): PatternRow => ({
            key: `p${p.pattern}`,
            pattern: p.pattern,
            od_min: p.od_min,
            od_max: p.od_max,
            slots: p.slots.length,
            min_distance: p.min_distance,
            status: p.status,
            draft: false,
          }),
        ),
        ...(originalNo === null && draft
          ? [
              {
                key: 'draft',
                pattern: draft.pattern,
                od_min: draft.od_min,
                od_max: draft.od_max,
                slots: draft.slots.length,
                min_distance: minD === null ? null : round4(minD),
                status: 'new',
                draft: true,
              } satisfies PatternRow,
            ]
          : []),
      ]
    : []

  const patternColumns: Column<PatternRow>[] = [
    {
      key: 'pattern',
      label: 'Pattern',
      get: (r) => r.pattern,
      cell: (r) => (
        <span className="inline-flex items-center gap-1">
          {r.pattern}
          {r.status !== 'same' && r.status !== 'custom' && (
            <Badge className={r.draft ? 'bg-surface-inset' : PATTERN_TONE[r.status]}>{r.status}</Badge>
          )}
        </span>
      ),
      priority: 1,
    },
    { key: 'od_min', label: 'OdMin (mm)', get: (r) => r.od_min, numeric: true, priority: 1 },
    { key: 'od_max', label: 'OdMax (mm)', get: (r) => r.od_max, numeric: true, priority: 1 },
    { key: 'slots', label: 'Slots', get: (r) => r.slots, numeric: true, priority: 2 },
    {
      key: 'min_distance',
      label: 'MinDistance (u)',
      get: (r) => r.min_distance,
      cell: (r) => (
        <span className={cn(r.min_distance !== null && Math.abs(r.min_distance - 1) > 1e-3 && 'text-warn-fg')}>{r.min_distance ?? '—'}</span>
      ),
      numeric: true,
      priority: 2,
    },
  ]

  // ── 표 행 ──
  const offsetCell = (s: DraftSlot, axis: 0 | 1) => (
    <NumField
      label={`${s.slot} ${axis === 0 ? 'OffsetX' : 'OffsetY'} (${units})`}
      value={units === 'mm' ? normToMm(s.u[axis], pitch) : s.u[axis]}
      onChange={(v) =>
        updateSlot(s.slot, (x) => {
          const u: [number, number] = [x.u[0], x.u[1]]
          u[axis] = units === 'mm' ? mmToNorm(v, pitch) : Number.isFinite(v) ? round4(v) : NaN
          return { ...x, u }
        })
      }
      testid={`pallet-edit-${axis === 0 ? 'x' : 'y'}-${s.slot}`}
    />
  )

  if (loadError)
    return <EmptyState title="패턴 저장소를 읽지 못했습니다" hint={loadError} icon={<Pencil size={20} />} />

  const patternStatus = savedPattern?.status ?? (originalNo === null ? 'added' : 'custom')

  return (
    <div className="flex min-h-0 flex-1 flex-wrap content-start gap-4 overflow-auto p-3" data-testid="pallet-editor">
      {/* 흐름 · 패턴 목록 */}
      <section className="flex w-72 shrink-0 flex-col gap-3" aria-label="흐름과 패턴">
        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-1">
            <h3 className={h3}>Flow</h3>
            <span className="flex-1" />
            <OverflowMenu items={flowMenu()} testid="pallet-flow-menu" />
          </div>
          <ul className="flex max-h-72 flex-col gap-1 overflow-y-auto" data-testid="pallet-flow-list">
            {flows.map((f) => (
              <li key={f.id}>
                <button
                  type="button"
                  onClick={() =>
                    f.id !== flowId &&
                    guard(() => {
                      setFlowId(f.id)
                      openPattern(f, null)
                    })
                  }
                  className={cn(
                    'flex w-full flex-col gap-0.5 rounded border px-2 py-1 text-left',
                    f.id === flowId ? 'border-accent bg-surface-active' : 'border-line-default hover:bg-surface-inset',
                  )}
                  aria-pressed={f.id === flowId}
                  data-testid="pallet-flow-item"
                  data-flow={f.id}
                >
                  <span className="flex items-center gap-1.5">
                    <span className="font-mono text-2xs text-content-primary">{f.id}</span>
                    <Badge className={STATUS_TONE[f.status]}>{f.status}</Badge>
                    <span className="ml-auto text-3xs text-content-faint">{`Drag-${f.drag_kind} · P${f.patterns.length}`}</span>
                  </span>
                  <span className="truncate text-3xs text-content-muted">
                    {f.name}
                    {f.used_by.length > 0 && ` · Station ${f.used_by.join(', ')}`}
                  </span>
                </button>
              </li>
            ))}
          </ul>
          <div className="flex flex-wrap gap-1">
            <Button
              size="sm"
              intent="neutral"
              icon={<Plus size={14} />}
              disabled={busy}
              onClick={() => guard(() => setFlowForm({ mode: 'create', id: '', name: '', drag_kind: 'in', note: '', copy_from: '' }))}
            >
              새 흐름
            </Button>
            <Button
              size="sm"
              intent="neutral"
              icon={<Copy size={14} />}
              disabled={busy || !flow}
              onClick={() =>
                flow &&
                guard(() => setFlowForm({ mode: 'copy', id: `${flow.id}_SITE`.slice(0, 32), name: '', drag_kind: flow.drag_kind, note: '', copy_from: flow.id }))
              }
              data-testid="pallet-flow-copy"
            >
              복사
            </Button>
          </div>
          {flow && (
            <Pairs
              className="gap-x-3"
              items={[
                { label: 'source', value: flow.source },
                ...(flow.based_on ? [{ label: 'based_on', value: flow.based_on }] : []),
                { label: 'updated', value: flow.updated_at.slice(0, 19).replace('T', ' ') },
                ...(flow.reference ? [{ label: 'reference', value: '참고본' }] : []),
              ]}
            />
          )}
        </div>

        {flow && (
          <div className="flex flex-col gap-2 border-t border-line-default pt-3">
            <h3 className={h3}>Pattern</h3>
            <DataTable
              rows={patternRows}
              columns={patternColumns}
              rowKey={(r) => r.key}
              selected={originalNo === null ? (draft ? 'draft' : null) : `p${originalNo}`}
              onPick={(r) => !r.draft && originalNo !== r.pattern && guard(() => openPattern(flow, r.pattern))}
              density="compact"
              fit
              empty="패턴 없음"
              emptyHint="아래 추가로 만드세요"
              testid="pallet-pattern-list"
            />
            {flow.removed_spec_patterns.length > 0 && (
              <p className="text-3xs text-warn-fg">{`사양서에는 있지만 지운 Pattern: ${flow.removed_spec_patterns.join(', ')}`}</p>
            )}
            <div className="flex flex-wrap items-center gap-1">
              <Button size="sm" intent="neutral" icon={<Plus size={14} />} disabled={busy} onClick={() => startNew(false)} data-testid="pallet-pattern-add">
                추가
              </Button>
              <span className="flex-1" />
              <OverflowMenu items={patternMenu()} testid="pallet-pattern-menu" />
            </div>
            {flow.warnings.length > 0 && (
              <details className="text-3xs text-warn-fg">
                <summary className="cursor-pointer">{`저장된 흐름 경고 ${flow.warnings.length}건`}</summary>
                <ul className="mt-1 flex flex-col gap-0.5">
                  {flow.warnings.map((w) => (
                    <li key={w}>{w}</li>
                  ))}
                </ul>
              </details>
            )}
          </div>
        )}
      </section>

      {/* 그림 */}
      <section className="flex min-w-0 flex-1 basis-80 flex-col gap-2" aria-label="패턴 그림">
        <div className="flex flex-wrap items-end gap-2">
          <div className="w-28">
            <Input label="Preview OD" value={odText} onValueChange={setOdText} mono hint="mm" data-testid="pallet-edit-od" />
          </div>
          <div className="w-20">
            <Input label="Gap" value={gapText} onValueChange={setGapText} mono hint="mm" />
          </div>
          <Field
            label="Units"
            hint={
              <span className="inline-flex items-center gap-1">
                mm = u × {pitch}
                <HelpTip
                  title="Units"
                  text={`mm = u × (Preview OD ${od} + Gap ${gap}) = u × ${pitch}. mm 로 넣으면 이 값으로 u 를 다시 계산합니다. norm 은 최소 중심거리 단위의 정규화 오프셋입니다.`}
                />
              </span>
            }
          >
            <Segmented
              value={units}
              onChange={changeUnits}
              ariaLabel="Units"
              compact
              options={[
                { id: 'norm', label: 'norm', testid: 'pallet-units-norm' },
                { id: 'mm', label: 'mm', testid: 'pallet-units-mm' },
              ]}
            />
          </Field>
          <Field label="Snap">
            <Segmented
              value={snap}
              onChange={changeSnap}
              ariaLabel="Snap"
              compact
              options={[
                { id: 'off', label: 'off' },
                { id: 'mm10', label: '10 mm' },
                { id: 'norm005', label: '0.05' },
              ]}
            />
          </Field>
        </div>
        <div className="flex flex-wrap items-center gap-1">
          <Button size="sm" intent="ghost" icon={<Plus size={14} />} disabled={!draft || draft.slots.length >= 20} onClick={() => setSlots(addSlot)} data-testid="pallet-slot-add">
            슬롯 추가
          </Button>
          <Button
            size="sm"
            intent={seqMode ? 'primary' : 'ghost'}
            icon={<ListOrdered size={14} />}
            disabled={!draft}
            title={seqMode ? '누른 순서까지만 적용합니다' : '그림에서 슬롯을 작업 순서대로 누르세요'}
            onClick={() => {
              if (!seqMode) {
                setSeqMode(true)
                setClicked([])
                setSelected(null)
              } else {
                if (clicked.length > 0) setSlots((ss) => seqByClicks(ss, clicked))
                setSeqMode(false)
                setClicked([])
              }
            }}
            data-testid="pallet-seq-mode"
          >
            {seqMode ? `Seq 재정렬 적용 (${clicked.length})` : 'Seq 재정렬'}
          </Button>
          <span className="flex-1" />
          <HelpTip title="그림 조작" text={EDIT_CANVAS_HELP} align="right" />
          <OverflowMenu items={shapeMenu()} title="패턴 변환" testid="pallet-shape-menu" />
        </div>
        <ErrorBoundary label="패턴 편집 그림">
          {draft && flow ? (
            <PatternEditCanvas
              slots={draft.slots}
              axes={axes}
              pitch={pitch}
              od={od}
              palletSize={palletSize}
              dragKind={flow.drag_kind}
              dragDist={dragDist}
              snap={snap}
              issues={issues}
              selected={selected}
              onSelect={setSelected}
              onMove={(name, u) => updateSlot(name, (s) => ({ ...s, u }))}
              onCycleDir={(name) => updateSlot(name, (s) => ({ ...s, drag_dir: cycleDir(s.drag_dir) }))}
              seqMode={seqMode}
              clicked={clicked}
              onSeqClick={seqClick}
            />
          ) : (
            <EmptyState title="패턴이 없습니다" hint="흐름을 고르거나 패턴을 추가하세요" icon={<Pencil size={20} />} />
          )}
        </ErrorBoundary>
        {/* 문단이 아니라 라벨+값 — 끌면서 봐야 하는 숫자만 남긴다(조작 설명은 위의 `?`). */}
        <Pairs
          testid="pallet-edit-scale"
          items={[
            { label: 'MinDistance', value: minD === null ? '—' : round4(minD) },
            { label: '생성 배율', value: `×${draft ? round4(scaleOf(draft.slots)) : 1}` },
            { label: 'Pitch (mm)', value: pitch },
          ]}
        />
      </section>

      {/* 필드 · 슬롯 표 */}
      <section className="flex min-w-0 flex-1 basis-80 flex-col gap-2" aria-label="슬롯 편집">
        {draft && flow ? (
          <>
            <div className="grid grid-cols-4 gap-2">
              {(
                [
                  ['Pattern', draft.pattern, (v: number) => patch((d) => ({ ...d, pattern: v }))],
                  ['OdMin', draft.od_min, (v: number) => patch((d) => ({ ...d, od_min: v }))],
                  ['OdMax', draft.od_max, (v: number) => patch((d) => ({ ...d, od_max: v }))],
                ] as const
              ).map(([label, value, set]) => (
                <label key={label} className="flex flex-col gap-1">
                  <span className="text-2xs font-medium text-content-muted">{label}</span>
                  <NumField label={label} value={value} onChange={set} testid={`pallet-field-${label.toLowerCase()}`} />
                </label>
              ))}
              <label className="flex flex-col gap-1">
                <span className="text-2xs font-medium text-content-muted">MinDistance</span>
                <span className={cn('flex h-control-sm items-center font-mono text-2xs tabular-nums', minD !== null && Math.abs(minD - 1) > 1e-3 && 'text-warn-fg')}>
                  {minD === null ? '—' : round4(minD)}
                </span>
              </label>
            </div>
            <Input label="Note" value={draft.note} onValueChange={(v) => patch((d) => ({ ...d, note: v }))} />
            <div className="flex flex-wrap items-center gap-2">
              <Button
                size="sm"
                intent="primary"
                icon={<Save size={14} />}
                disabled={busy || !dirty || validation.errors.length > 0}
                title={validation.errors.length > 0 ? validation.errors[0] : dirty ? '저장' : '바뀐 것이 없습니다'}
                onClick={() => void save()}
                data-testid="pallet-pattern-save"
              >
                저장
              </Button>
              <Button size="sm" intent="ghost" icon={<Undo2 size={14} />} disabled={busy || !dirty} onClick={cancel} data-testid="pallet-pattern-cancel">
                취소
              </Button>
              <Button size="sm" intent="neutral" icon={<GitCompare size={14} />} disabled={!flow.based_on} title={flow.based_on ? '사양서 R4 와 비교' : '사양서 기준이 없는 흐름입니다'} onClick={() => setDiffOpen(true)} data-testid="pallet-diff-open">
                사양서 비교
              </Button>
              <span className="text-3xs text-content-faint" data-testid="pallet-edit-state">
                {originalNo === null
                  ? '새 패턴 — 저장하지 않음'
                  : dirty
                    ? `변경 있음 (${[...(change?.fields ?? []), ...(change?.slots.filter((s) => s.status !== 'same').map((s) => s.slot) ?? [])].join(', ')})`
                    : `저장됨 ${savedPattern?.updated_at.slice(0, 19).replace('T', ' ') ?? ''}`}
              </span>
            </div>
            {validation.errors.length > 0 && (
              <ul className="flex flex-col gap-0.5 text-2xs text-danger-text" data-testid="pallet-edit-errors">
                {validation.errors.map((e) => (
                  <li key={e}>{e}</li>
                ))}
              </ul>
            )}
            {(validation.warnings.length > 0 || issues.length > 0) && (
              <ul className="flex flex-col gap-0.5 text-2xs text-warn-fg" data-testid="pallet-edit-warnings">
                {validation.warnings.map((w) => (
                  <li key={w}>{w}</li>
                ))}
                {[...new Set(issues.map((i) => i.text))].map((t) => (
                  <li key={t}>{t}</li>
                ))}
              </ul>
            )}
            <div className="overflow-x-auto">
              <table className="w-full border-collapse text-2xs" data-testid="pallet-slot-table">
                <thead>
                  <tr className="text-left text-content-faint">
                    <th className="w-5" aria-label="끌어서 Seq 바꾸기" />
                    <th className="px-1 py-0.5 font-medium">Seq</th>
                    <th className="px-1 py-0.5 font-medium">Slot</th>
                    <th className="px-1 py-0.5 font-medium">{`OffsetX (${units})`}</th>
                    <th className="px-1 py-0.5 font-medium">{`OffsetY (${units})`}</th>
                    <th className="px-1 py-0.5 font-medium">DragDir</th>
                    <th className="px-1 py-0.5 font-medium">DragType</th>
                    <th className="w-8" aria-label="삭제" />
                  </tr>
                </thead>
                <tbody>
                  {slotsBySeq(draft.slots).map((s) => {
                    const st = change?.slots.find((c) => c.slot === s.slot)
                    const on = selected === s.slot
                    return (
                      <tr
                        key={s.slot}
                        draggable
                        onDragStart={(e: React.DragEvent) => {
                          setDragRow(s.seq)
                          e.dataTransfer.effectAllowed = 'move'
                        }}
                        onDragOver={(e) => e.preventDefault()}
                        onDrop={(e) => {
                          e.preventDefault()
                          if (dragRow !== null && dragRow !== s.seq) setSlots((ss) => reorderSeq(ss, dragRow, s.seq))
                          setDragRow(null)
                        }}
                        onClick={() => setSelected(on ? null : s.slot)}
                        className={cn(
                          'border-t border-line-subtle',
                          on && 'bg-surface-active',
                          !on && st && st.status !== 'same' && 'bg-warn-soft',
                        )}
                        data-testid="pallet-slot-row"
                        data-slot={s.slot}
                      >
                        <td className="cursor-grab px-0.5 text-content-faint" title={SLOT_ROW_HELP}>
                          <GripVertical size={12} aria-hidden />
                        </td>
                        <td className="px-1 py-0.5 font-mono tabular-nums">{s.seq}</td>
                        <td className="px-1 py-0.5" onClick={(e) => e.stopPropagation()}>
                          <input
                            aria-label={`${s.slot} Slot`}
                            className={cn(cellInput, 'w-20')}
                            value={s.slot}
                            onChange={(e) => {
                              const v = e.currentTarget.value
                              updateSlot(s.slot, (x) => ({ ...x, slot: v }))
                              if (selected === s.slot) setSelected(v)
                            }}
                          />
                        </td>
                        <td className="px-1 py-0.5" onClick={(e) => e.stopPropagation()}>
                          {offsetCell(s, 0)}
                        </td>
                        <td className="px-1 py-0.5" onClick={(e) => e.stopPropagation()}>
                          {offsetCell(s, 1)}
                        </td>
                        <td className="px-1 py-0.5" onClick={(e) => e.stopPropagation()}>
                          <select
                            aria-label={`${s.slot} DragDir`}
                            className={cn(cellInput, 'w-14')}
                            value={s.drag_dir}
                            onChange={(e) => {
                              const v = Number(e.currentTarget.value)
                              updateSlot(s.slot, (x) => ({ ...x, drag_dir: v }))
                            }}
                            data-testid={`pallet-edit-dir-${s.slot}`}
                          >
                            {[0, 1, 2, 3, 4, 5, 6, 7, 8].map((d) => (
                              <option key={d} value={d}>
                                {d}
                              </option>
                            ))}
                          </select>
                        </td>
                        <td className="px-1 py-0.5 font-mono tabular-nums text-content-muted">{hexByte(dragTypeByte(s.drag_dir))}</td>
                        <td className="px-0.5" onClick={(e) => e.stopPropagation()}>
                          <Button
                            size="icon-sm"
                            intent="ghost"
                            aria-label={`${s.slot} 삭제`}
                            title="슬롯 삭제 (뒤 Seq 를 당깁니다)"
                            disabled={draft.slots.length <= 1}
                            onClick={() => {
                              setSlots((ss) => removeSlot(ss, s.slot))
                              if (selected === s.slot) setSelected(null)
                            }}
                          >
                            <Trash2 size={12} />
                          </Button>
                        </td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            </div>
            <div className="flex flex-col gap-1">
              <div className="flex items-center gap-1">
                <span className={h3}>DragDir</span>
                <HelpTip title="DragDir" text={DRAG_DIR_HELP} />
                <span className="text-3xs text-content-faint">
                  {selectedSlot ? `${selectedSlot.slot} · Seq ${selectedSlot.seq}` : '슬롯 미선택'}
                </span>
              </div>
              <DirPicker
                axes={axes}
                value={selectedSlot?.drag_dir ?? 0}
                disabled={!selectedSlot}
                onChange={(d) => selectedSlot && updateSlot(selectedSlot.slot, (s) => ({ ...s, drag_dir: d }))}
              />
            </div>
          </>
        ) : (
          <EmptyState title="편집할 패턴이 없습니다" hint="왼쪽에서 흐름·패턴을 고르거나 추가하세요" icon={<Pencil size={20} />} />
        )}
      </section>

      {flowForm && (
        <FormDialog
          open
          onOpenChange={(o) => !o && setFlowForm(null)}
          title={flowForm.mode === 'edit' ? `흐름 ${flowId} 설정` : flowForm.mode === 'copy' ? `흐름 ${flowForm.copy_from} 복사` : '새 흐름'}
          size="sm"
          dirty={flowForm.id.trim() !== '' || flowForm.name.trim() !== '' || flowForm.note.trim() !== ''}
          busy={busy}
          submitLabel={flowForm.mode === 'edit' ? '저장' : '만들기'}
          disabledReason={normalizeFlowId(flowForm.id).error ?? undefined}
          onSubmit={() => void submitFlow(flowForm)}
          testid="pallet-flow-form"
        >
          <Input
            label="Flow id"
            value={flowForm.id}
            onValueChange={(v) => setFlowForm({ ...flowForm, id: v })}
            mono
            hint={normalizeFlowId(flowForm.id).error ?? `저장 id: ${normalizeFlowId(flowForm.id).id}${flowForm.mode === 'edit' && flow?.used_by.length ? ` · 바꾸면 스테이션 ${flow.used_by.join(', ')} 프로파일도 따라갑니다` : ''}`}
            data-testid="pallet-flow-id"
          />
          <Input label="Name" value={flowForm.name} onValueChange={(v) => setFlowForm({ ...flowForm, name: v })} placeholder={flowForm.mode === 'copy' ? '비우면 "(copy of …)"' : ''} />
          {flowForm.mode !== 'copy' && (
            <Select label="DragKind" value={flowForm.drag_kind} onValueChange={(v) => setFlowForm({ ...flowForm, drag_kind: v === 'out' ? 'out' : 'in' })}>
              <option value="in">in — 입고 PICK + Drag-in</option>
              <option value="out">out — 출하 DROP + Drag-out</option>
            </Select>
          )}
          <Input label="Note" value={flowForm.note} onValueChange={(v) => setFlowForm({ ...flowForm, note: v })} />
        </FormDialog>
      )}

      <ConfirmDialog
        open={ask !== null}
        onOpenChange={(o) => !o && setAsk(null)}
        scope="single"
        title={ask?.title}
        danger={ask?.danger}
        confirmLabel={ask?.label}
        onConfirm={() => {
          const a = ask
          setAsk(null)
          void a?.run()
        }}
      >
        {ask?.body}
      </ConfirmDialog>

      {flow && (
        <PatternDiffDialog
          open={diffOpen}
          onOpenChange={setDiffOpen}
          flowId={flow.id}
          axes={axes}
          dragKind={flow.drag_kind}
          od={od}
          gap={gap}
          palletSize={palletSize}
          revision={revision}
        />
      )}
      <PalletJsonDialog
        open={jsonOpen}
        onOpenChange={setJsonOpen}
        onImported={() => {
          void reload().then((list) => openPattern(list.find((x) => x.id === flowId) ?? list[0] ?? null, originalNo, true))
          setRevision((n) => n + 1)
          onChanged()
        }}
      />
    </div>
  )
}
