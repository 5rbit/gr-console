// 화물 규격 상세 패널 — 표에서 고른 품목 하나를 탭 셋(Spec · Beads · Usage)으로 고친다.
//
// 초안 하나(PLC 품목 필드 + 콘솔 전용 spec)를 세 탭이 같이 쓰고 저장·취소는 머리에 하나다 — 탭을 옮겨도
// 고친 값이 남는다. 저장하면 부모가 목록을 다시 받고 `key`(code:updated_at)가 바뀌어 새 초안으로 다시 선다.
//
// Beads 탭의 측정값은 서버(`GET /api/items/{code}/levels?robot=`, 사이드바에서 고른 로봇)에서 오고,
// 계산값·유효값·편차·옆모습 그림은 편집 중인 초안으로 여기서 바로 다시 계산한다(`levelsModel.ts`).
// 필드 이름은 PLC/데이터 이름 그대로(영문) 보인다 — 설명·버튼·메시지만 한국어.
//
// **본문에는 설명 문단을 두지 않는다.** 표와 열 머리가 이미 말하는 것은 지우고, 값이 있는 것(최신
// 측정 · ByCode · 표본 수)은 조작 띠의 칩 한 조각으로, 규칙·공식·용어는 `?` 팝오버로 접는다
// (글은 `lib/items/panelInfo.ts`, 그릇은 `Info.tsx`). 띠에 남는 것은 Stack n · 잰 크기 칩 ·
// AutoApplyMeasured · `?` · `⋯` 뿐이고, 가끔 쓰는 조작은 전부 `⋯` 뒤에 있다.
import type * as React from 'react'
import { useEffect, useMemo, useState } from 'react'
import { RotateCcw, Save, Trash2, X } from 'lucide-react'
import { api } from '../../lib/api'
import {
  DEFAULT_PICK_BEAD_OFFSET,
  LEVEL_MAX,
  PROFILE_KEYS,
  PROFILE_LABEL,
  autoApplyMeasured,
  buildGrid,
  deviationTone,
  fillComputed,
  limitLabel,
  measuredByLevel,
  measuredCounts,
  normalizeSpec,
  parseCell,
  pickBeadOffset,
  profileOf,
  removeProfile,
  removeProfileRow,
  round1,
  scenarioRefs,
  setProfileValue,
  signed,
  specEquals,
  specErrors,
  specOf,
  specWarnings,
  stackBoxes,
  stockCellsFor,
  type GridRow,
  type ProfileKey,
  type StepRef,
} from '../../lib/items/levelsModel'
import { mm, sampleLabel, sampleRows, samplesStats, type SampleRow } from '../../lib/items/samples'
import {
  SPEC_HELP,
  USAGE_HELP,
  beadsHelp,
  byCodeChip,
  byCodeRows,
  levelsState,
  measuredChip,
  measuredRows,
  profileChip,
  sourceChip,
  type HelpSection,
} from '../../lib/items/panelInfo'
import { itemErrors } from '../../lib/items/model'
import { robots } from '../../lib/robots'
import { withRobot } from '../../lib/robotContext'
import { RobotChip } from '../shared/RobotChip'
import { useStore } from '../../lib/store'
import { Button } from '../../lib/ui/Button'
import { HelpTip } from '../../lib/ui/HelpTip'
import { Input } from '../../lib/ui/Input'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import { Segmented } from '../../lib/ui/Segmented'
import { Switch } from '../../lib/ui/Switch'
import { toast } from '../../lib/ui/toast'
import { cn } from '../../lib/utils'
import type { Item, ItemLevels, ItemSpec, ItemUpsert, StockEntry } from '../../lib/types'
import type { DimField } from '../../lib/items/dims'
import { toItemUpsert } from '../task/ItemRegistry'
import { ChipPopover, InfoChip } from '../../lib/ui/InfoChip'
import { InfoRows } from '../../lib/ui/Pair'
import { DimsTab } from './DimsTab'
import { StackSvg } from './StackSvg'

type Tab = 'spec' | 'beads' | 'measured' | 'usage'
type Draft = ItemUpsert & { spec: ItemSpec }

const TAB_KEY = 'gr-items-detail-tab'
const TABS: { id: Tab; label: string; title: string; testid: string }[] = [
  {
    id: 'spec',
    label: 'Spec',
    title: '품목 필드 · StackMax · PalletMax · WeightKg · PickBeadOffset · Compression',
    testid: 'item-tab-spec',
  },
  {
    id: 'beads',
    label: 'Beads',
    title: '잰 스택 크기별 절대 AbsLowerBead · AbsUpperBead · StackHeight · PickZ 표',
    testid: 'item-tab-beads',
  },
  {
    id: 'measured',
    label: 'Measured',
    title:
      '측정 기록(MeasureItem)으로 InnerDiameter · UpperBeadHeight · Height 제안 · 검토 적용 · 변경 이력',
    testid: 'item-tab-measured',
  },
  {
    id: 'usage',
    label: 'Usage',
    title: '이 코드를 쓰는 재고 칸 · 시나리오 스텝',
    testid: 'item-tab-usage',
  },
]

const errMsg = (e: unknown) => (e instanceof Error ? e.message : String(e))
const fmt = (v: number | null) => (v === null ? '' : String(round1(v)))
const withoutSpec = (d: Draft) => JSON.stringify({ ...d, spec: null })

function readTab(): Tab {
  try {
    const v = localStorage.getItem(TAB_KEY)
    return v === 'beads' || v === 'measured' || v === 'usage' ? v : 'spec'
  } catch {
    return 'spec'
  }
}

export interface ItemDetailProps {
  item: Item
  stock: readonly StockEntry[]
  /** 저장 뒤 — 부모가 목록을 다시 받는다. */
  onSaved: (code: number) => Promise<void>
  onClose: () => void
  /** 저장하지 않은 변경이 있는가(다른 행을 고르기 전에 묻는 데 쓴다). */
  onDirtyChange?: (dirty: boolean) => void
  /** 표의 측정 제안 칩을 눌렀을 때 — Measured 탭을 열고 그 필드 줄을 반짝인다(`n` 이 바뀔 때마다). */
  focus?: { field: DimField; n: number } | null
}

export function ItemDetail({
  item,
  stock,
  onSaved,
  onClose,
  onDirtyChange,
  focus,
}: ItemDetailProps) {
  useStore(robots)
  const robot = robots.selected
  const robotName = robots.current?.name ?? (robot === null ? '기본 로봇' : `로봇 ${robot}`)
  const original = useMemo<Draft>(() => ({ ...toItemUpsert(item), spec: specOf(item) }), [item])
  const [draft, setDraft] = useState<Draft>(original)
  const [tab, setTabState] = useState<Tab>(readTab)
  const [saving, setSaving] = useState(false)
  const [levels, setLevels] = useState<ItemLevels | null>(null)
  const [levelsErr, setLevelsErr] = useState<string | null>(null)
  const [levelsLoading, setLevelsLoading] = useState(false)
  /** 하중 곡선을 몇 개 쌓은 스택으로 볼지(null = StackMax). 저장값이 아니라 보기 설정이다. */
  const [preview, setPreview] = useState<number | null>(null)

  const spec = draft.spec
  const dirty = withoutSpec(draft) !== withoutSpec(original) || !specEquals(spec, original.spec)
  const errors = useMemo(
    () => [...itemErrors(draft), ...specErrors(spec, draft.height)],
    [draft, spec],
  )
  const measured = useMemo(() => measuredByLevel(levels?.rows), [levels])
  const grid = useMemo(
    () => buildGrid(draft, spec, preview, measured),
    [draft, spec, preview, measured],
  )

  const set = (patch: Partial<ItemUpsert>) => setDraft((d) => ({ ...d, ...patch }))
  const patchSpec = (patch: Partial<ItemSpec>) =>
    setDraft((d) => ({ ...d, spec: { ...d.spec, ...patch } }))
  const updateSpec = (fn: (s: ItemSpec) => ItemSpec) =>
    setDraft((d) => ({ ...d, spec: fn(d.spec) }))

  function setTab(t: Tab) {
    setTabState(t)
    try {
      localStorage.setItem(TAB_KEY, t)
    } catch {
      /* 저장 못 해도 동작 */
    }
  }

  // 표의 제안 칩 — Measured 탭으로(그 필드 줄은 DimsTab 이 반짝인다)
  useEffect(() => {
    if (focus) setTab('measured')
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 요청 번호가 바뀔 때만
  }, [focus?.n])

  async function loadLevels() {
    setLevelsLoading(true)
    try {
      setLevels(await api.itemLevels(item.code, robot, preview))
      setLevelsErr(null)
    } catch (e) {
      setLevels(null)
      setLevelsErr(errMsg(e))
    } finally {
      setLevelsLoading(false)
    }
  }

  useEffect(() => {
    void loadLevels()
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 코드·저장 시각·로봇이 바뀔 때만 다시 받는다
  }, [item.code, item.updated_at, robot])

  useEffect(() => {
    onDirtyChange?.(dirty)
  }, [dirty, onDirtyChange])
  useEffect(
    () => () => onDirtyChange?.(false),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 떠날 때 한 번
    [],
  )

  async function save() {
    if (errors.length) return
    setSaving(true)
    try {
      await api.itemUpdate(item.code, { ...draft, spec: normalizeSpec(spec) })
      toast.ok(`품목 ${item.code} 저장됨`)
      await onSaved(item.code)
    } catch (e) {
      toast.error(`품목 ${item.code} 저장 실패 — ${errMsg(e)}`)
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="item-detail">
      <div className="flex h-screen-header flex-none items-center gap-2 border-b border-line-default px-2">
        <span className="font-mono text-sm font-semibold">{item.code}</span>
        <span className="min-w-0 truncate text-xs text-content-muted">{item.name}</span>
        {dirty ? (
          <span
            className="rounded-sm bg-warn-soft px-1 text-2xs text-warn-fg"
            data-testid="item-detail-dirty"
          >
            저장 안 됨
          </span>
        ) : null}
        <span className="flex-1" />
        <Button
          size="sm"
          intent="ghost"
          icon={<RotateCcw className="h-3.5 w-3.5" />}
          disabled={!dirty || saving}
          onClick={() => setDraft(original)}
          data-testid="item-detail-cancel"
        >
          취소
        </Button>
        <Button
          size="sm"
          intent="primary"
          icon={<Save className="h-3.5 w-3.5" />}
          disabled={!dirty || errors.length > 0}
          loading={saving}
          onClick={() => void save()}
          title={errors.length ? errors[0] : ''}
          data-testid="item-detail-save"
        >
          저장
        </Button>
        <Button
          size="icon-sm"
          intent="ghost"
          icon={<X size={14} />}
          aria-label="상세 닫기"
          title="상세 닫기"
          onClick={onClose}
        />
      </div>
      <div className="flex flex-none items-center gap-2 border-b border-line-default px-2 py-1.5">
        <Segmented
          ariaLabel="품목 상세"
          value={tab}
          onChange={setTab}
          options={TABS.map((t) => ({
            id: t.id,
            label: t.label,
            title: t.title,
            testid: t.testid,
          }))}
        />
      </div>
      {errors.length ? (
        <ul
          className="mx-2 mt-2 list-disc rounded-md border border-fault bg-fault-soft py-1 pr-2 pl-6 text-xs text-fault-fg"
          data-testid="item-detail-errors"
        >
          {errors.map((e) => (
            <li key={e}>{e}</li>
          ))}
        </ul>
      ) : null}
      <div className="min-h-0 flex-1 overflow-auto p-2">
        {tab === 'spec' ? (
          <SpecTab draft={draft} set={set} patchSpec={patchSpec} />
        ) : tab === 'beads' ? (
          <BeadsTab
            draft={draft}
            grid={grid}
            levels={levels}
            levelsErr={levelsErr}
            levelsLoading={levelsLoading}
            robotName={robotName}
            robot={robot}
            preview={preview}
            setPreview={setPreview}
            reload={() => void loadLevels()}
            patchSpec={patchSpec}
            updateSpec={updateSpec}
          />
        ) : tab === 'measured' ? (
          <DimsTab
            code={item.code}
            dirty={dirty}
            onApplied={() => onSaved(item.code)}
            focus={focus ?? null}
          />
        ) : (
          <UsageTab code={item.code} spec={spec} stock={stock} />
        )}
      </div>
    </div>
  )
}

// ── Spec ─────────────────────────────────────────────────────────────────────

function NumField({
  label,
  value,
  onChange,
  step = 'any',
  min,
  hint,
}: {
  label: string
  value: number
  onChange: (n: number) => void
  step?: string
  min?: number
  hint?: string
}) {
  return (
    <Input
      label={label}
      type="number"
      mono
      step={step}
      min={min}
      hint={hint}
      // 서버 값은 PLC Real(f32)을 f64 로 편 것이라 457.20001220703125 처럼 온다 — f32 유효 자릿수(7)로 보인다.
      value={Number.isFinite(value) ? String(Number(value.toPrecision(7))) : ''}
      onValueChange={(s) => {
        const n = Number(s)
        if (s.trim() !== '' && Number.isFinite(n)) onChange(n)
      }}
    />
  )
}

/** 비워 둘 수 있는 숫자 칸 — 빈칸이면 `fallback`(해석된 기본값)을 흐리게 보인다. */
function OptNumField({
  label,
  value,
  fallback,
  hint,
  onChange,
  testid,
}: {
  label: string
  value: number | null
  fallback: number
  hint?: string
  onChange: (v: number | null) => void
  testid?: string
}) {
  return (
    <Input
      label={label}
      type="number"
      mono
      step="any"
      min={0}
      hint={hint}
      placeholder={`${fallback} (기본)`}
      value={value === null ? '' : String(Number(value.toPrecision(7)))}
      data-testid={testid}
      onValueChange={(v) => {
        const n = Number(v)
        if (v.trim() === '') onChange(null)
        else if (Number.isFinite(n)) onChange(n)
      }}
    />
  )
}

/** 묶음 하나 — 제목은 이름만, 왜·어디로 가는지는 제목 옆 `?` 안에 있다. */
function Group({
  title,
  help,
  children,
}: {
  title: string
  help?: readonly HelpSection[]
  children: React.ReactNode
}) {
  return (
    <fieldset className="flex flex-col gap-2">
      <legend className="mb-1 flex items-center gap-1 text-2xs font-semibold text-content-muted">
        {title}
        {help ? <HelpTip title={title} sections={help} /> : null}
      </legend>
      <div className="grid grid-cols-2 gap-2 xl:grid-cols-3">{children}</div>
    </fieldset>
  )
}

function SpecTab({
  draft,
  set,
  patchSpec,
}: {
  draft: Draft
  set: (patch: Partial<ItemUpsert>) => void
  patchSpec: (patch: Partial<ItemSpec>) => void
}) {
  const s = draft.spec
  return (
    <div className="flex flex-col gap-4" data-testid="item-spec">
      <Group title="PLC LGR_Stock_Item" help={SPEC_HELP.plc}>
        <Input
          className="col-span-2"
          label="Name"
          value={draft.name}
          onValueChange={(name) => set({ name })}
          placeholder="예: 225/45R17"
        />
        <NumField
          label="Count"
          value={draft.count}
          onChange={(count) => set({ count })}
          step="1"
          min={1}
        />
        <NumField
          label="InnerDiameter (mm)"
          value={draft.inner_diameter}
          onChange={(inner_diameter) => set({ inner_diameter })}
        />
        <NumField
          label="OuterDiameter (mm)"
          value={draft.outer_diameter}
          onChange={(outer_diameter) => set({ outer_diameter })}
        />
        <NumField label="Height (mm)" value={draft.height} onChange={(height) => set({ height })} />
        <NumField
          label="LowerBidHeight (mm)"
          value={draft.lower_bead_height}
          onChange={(lower_bead_height) => set({ lower_bead_height })}
          hint="타이어 1개 기준"
        />
        <NumField
          label="UpperBidHeight (mm)"
          value={draft.upper_bead_height}
          onChange={(upper_bead_height) => set({ upper_bead_height })}
          hint="타이어 1개 기준"
        />
        <NumField
          label="DeflectionFactor"
          value={draft.deflection_factor}
          onChange={(deflection_factor) => set({ deflection_factor })}
          hint="PLC 전용"
        />
        <Input
          className="col-span-2 xl:col-span-3"
          label="Note"
          value={draft.note}
          onValueChange={(note) => set({ note })}
        />
      </Group>
      <Group title="콘솔 전용" help={SPEC_HELP.console}>
        <NumField
          label="StackMax"
          value={s.stack_max}
          onChange={(stack_max) => patchSpec({ stack_max })}
          step="1"
          min={0}
          hint="0 = 제한 없음"
        />
        <NumField
          label="PalletMax"
          value={s.pallet_max}
          onChange={(pallet_max) => patchSpec({ pallet_max })}
          step="1"
          min={0}
          hint="0 = 제한 없음"
        />
        <Input
          label="WeightKg"
          type="number"
          mono
          step="any"
          min={0}
          // f32 를 편 값이 `11.399999618530273` 처럼 오면 칸 하나가 줄을 다 먹는다 — 유효 자릿수(7)로 자른다.
          value={s.weight_kg === null ? '' : String(Number(s.weight_kg.toPrecision(7)))}
          placeholder="미입력"
          onValueChange={(v) => {
            const n = Number(v)
            if (v.trim() === '') patchSpec({ weight_kg: null })
            else if (Number.isFinite(n)) patchSpec({ weight_kg: n })
          }}
        />
        <Input
          className="col-span-2"
          label="BeadSource"
          value={s.bead_source}
          placeholder="예: 2026-09 GR2 SKU 측정 평균"
          onValueChange={(bead_source) => patchSpec({ bead_source })}
        />
      </Group>
      <Group title="집는 높이" help={SPEC_HELP.pick}>
        <OptNumField
          label="PickBeadOffset (mm)"
          value={s.pick_bead_offset}
          fallback={DEFAULT_PICK_BEAD_OFFSET}
          hint={`기본 ${DEFAULT_PICK_BEAD_OFFSET} · 1단 PickZ ${round1(Math.max(draft.upper_bead_height - pickBeadOffset(s), 0))}`}
          onChange={(pick_bead_offset) => patchSpec({ pick_bead_offset })}
          testid="spec-pick-bead-offset"
        />
        <OptNumField
          label="Compression (mm)"
          value={s.compression}
          fallback={0}
          hint="기본 0 · 1개당"
          onChange={(compression) => patchSpec({ compression })}
          testid="spec-compression"
        />
        <Input
          className="col-span-2 xl:col-span-3"
          label="CompressionSource"
          value={s.compression_source ?? ''}
          placeholder="예: 측정 EachHeight 회귀 n=3"
          onValueChange={(compression_source) => patchSpec({ compression_source })}
        />
      </Group>
    </div>
  )
}

// ── Beads ────────────────────────────────────────────────────────────────────

/** 한 칸 입력 — 치는 동안의 글자("12.")를 지키려고 글자 상태를 따로 든다. 숫자가 되면 바로 반영. */
function LevelCell({
  value,
  placeholder,
  label,
  testid,
  onCommit,
}: {
  value: number | null
  placeholder: string
  label: string
  testid: string
  onCommit: (v: number | null) => void
}) {
  const [text, setText] = useState(fmt(value))
  useEffect(() => {
    setText((t) => (parseCell(t) === value ? t : fmt(value)))
  }, [value])
  const bad = parseCell(text) === undefined
  return (
    <input
      className={cn(
        'h-control-sm w-20 rounded-md border bg-transparent px-1.5 text-right font-mono text-xs tabular-nums placeholder:text-content-faint focus-visible:border-focus focus-visible:outline-none',
        bad ? 'border-fault' : value === null ? 'border-line-default' : 'border-line-strong',
      )}
      inputMode="decimal"
      value={text}
      placeholder={placeholder}
      aria-label={label}
      title={value === null ? `빈칸 = 계산값 ${placeholder}` : label}
      data-testid={testid}
      onChange={(e) => {
        const t = e.currentTarget.value
        setText(t)
        const n = parseCell(t)
        if (n !== undefined) onCommit(n)
      }}
    />
  )
}

function MeasuredLine({ m, d }: { m: number | null; d: number | null }) {
  if (m === null) return null
  const tone = deviationTone(d)
  return (
    <div
      className={cn(
        'pt-0.5 text-right font-mono text-2xs tabular-nums',
        tone === 'fault'
          ? 'text-fault-fg'
          : tone === 'warn'
            ? 'text-warn-fg'
            : 'text-content-muted',
      )}
      title="측정값 (측정 − 유효값)"
    >
      M {round1(m)}
      {d !== null ? ` (${signed(d)})` : ''}
    </div>
  )
}

function BeadsTab({
  draft,
  grid,
  levels,
  levelsErr,
  levelsLoading,
  robotName,
  robot,
  preview,
  setPreview,
  reload,
  patchSpec,
  updateSpec,
}: {
  draft: Draft
  grid: GridRow[]
  levels: ItemLevels | null
  levelsErr: string | null
  levelsLoading: boolean
  robotName: string
  robot: number | null
  preview: number | null
  setPreview: (n: number | null) => void
  reload: () => void
  patchSpec: (patch: Partial<ItemSpec>) => void
  updateSpec: (fn: (s: ItemSpec) => ItemSpec) => void
}) {
  const spec = draft.spec
  const warnings = specWarnings(spec, draft)
  const n = grid.length
  const boxes = stackBoxes(grid, n)
  const sizes = measuredCounts(spec)
  const stored = profileOf(spec, n)
  const src = levels?.measured ?? null
  const sum = levels?.measured_summary ?? null
  const suggestion = levels?.suggestion ?? null
  const rows = useMemo(() => sampleRows(levels?.bead_samples), [levels])
  const [applying, setApplying] = useState<number | null>(null)
  const setCell = (level: number, key: ProfileKey, v: number | null) =>
    updateSpec((s) => setProfileValue(s, n, level, key, v))
  const applySuggestedCompression = () => {
    if (!suggestion?.value && suggestion?.value !== 0) return
    patchSpec({
      compression: suggestion.value,
      compression_source: `측정 ${src?.plc ?? '?'} Seq ${src?.seq ?? '?'} — ${suggestion.method ?? '?'} n=${suggestion.samples}`,
    })
  }

  async function applySample(seq: number) {
    setApplying(seq)
    try {
      const r = await api.itemBeadSampleApply(draft.code, seq, robot)
      toast.ok(withRobot(robotName, `Seq ${seq} 반영 — ${r.reason}`))
      reload()
    } catch (e) {
      toast.error(withRobot(robotName, `Seq ${seq} 반영 실패 — ${errMsg(e)}`))
    } finally {
      setApplying(null)
    }
  }

  // 잰 로봇은 **값이 온 곳**이다(`src.plc`) — 사이드바 선택이 아니다. 칩이 그 호기를 색·이름으로 말한다.
  const measuredBy = src ? robots.chipOfPlc(src.plc) : null
  const mBase = measuredChip(src)
  const mChip =
    mBase && measuredBy
      ? {
          ...mBase,
          label: <RobotChip chip={measuredBy} bare testid="beads-measured-robot" />,
          title: withRobot(measuredBy.name, mBase.title ?? ''),
        }
      : mBase
  const bChip = byCodeChip(sum)
  const pChip = sourceChip(stored?.sample_plc, stored?.sample_seq, stored?.at)
  const state = levelsState(levelsErr, levelsLoading, !!src, robotName)
  const noSuggestion = suggestion === null || suggestion.value === null
  const actions = [
    {
      label: '계산값으로 채우기',
      run: () => updateSpec((s) => fillComputed(draft, s, n)),
    },
    {
      label: '측정값에서 Compression 계산',
      disabled: noSuggestion
        ? '되짚을 측정값이 없습니다 — 단별 비드나 EachHeight 가 필요합니다'
        : undefined,
      run: applySuggestedCompression,
    },
    {
      label: `스택 ${n} 프로파일 지우기`,
      danger: true,
      disabled: stored ? undefined : '이 크기는 잰 적이 없습니다',
      run: () => updateSpec((s) => removeProfile(s, n)),
    },
    {
      label: '측정값 다시 받기',
      disabled: levelsLoading ? '받는 중입니다' : undefined,
      run: reload,
    },
  ]

  return (
    <div className="flex flex-col gap-2" data-testid="item-beads">
      <div className="flex flex-wrap items-center gap-1.5" data-testid="beads-controls">
        <label className="flex items-center gap-1 text-2xs text-content-muted">
          <span className="font-mono">Stack n</span>
          <input
            className="h-control-sm w-12 rounded-md border border-line-default bg-transparent px-1.5 text-right font-mono text-xs tabular-nums focus-visible:border-focus focus-visible:outline-none"
            inputMode="numeric"
            value={preview === null ? '' : String(preview)}
            placeholder={String(n)}
            aria-label="스택 크기"
            title="이 크기의 스택을 잰 값을 보입니다 (잰 적 없으면 곡선에서 지어낸 값)"
            data-testid="beads-preview"
            onChange={(e) => {
              const v = parseCell(e.currentTarget.value)
              setPreview(
                v === undefined || v === null || v <= 0 ? null : Math.min(Math.round(v), LEVEL_MAX),
              )
            }}
          />
        </label>
        <InfoChip chip={profileChip(n, !!stored)} testid="beads-profile-chip" />
        <span className="flex items-center gap-1 text-2xs" data-testid="beads-sizes">
          {sizes.length ? (
            sizes.map((c) => (
              <button
                key={c}
                type="button"
                className={cn(
                  'h-control-sm rounded-sm px-1 font-mono',
                  c === n ? 'bg-ok-soft text-ok-fg' : 'text-content-muted hover:bg-surface-hover',
                )}
                title={`스택 ${c} 을(를) 잰 프로파일 보기`}
                data-testid={`beads-size-${c}`}
                onClick={() => setPreview(c)}
              >
                n={c}
              </button>
            ))
          ) : (
            <span className="text-content-faint">잰 스택 없음</span>
          )}
        </span>
        {mChip ? (
          <ChipPopover
            chip={mChip}
            title={`최신 SKU 측정 · ${measuredBy?.name ?? robotName}`}
            testid="beads-measured-chip"
          >
            <InfoRows rows={measuredRows(src)} />
            {bChip ? (
              <>
                <span className="mt-2 mb-1 block font-mono text-3xs text-content-muted">
                  ByCode {bChip.value}
                </span>
                <InfoRows rows={byCodeRows(sum)} />
              </>
            ) : null}
          </ChipPopover>
        ) : null}
        {state ? (
          <span
            className="text-2xs text-content-faint"
            title={state.title}
            data-testid="beads-state"
          >
            {state.text}
          </span>
        ) : null}
        {pChip ? <InfoChip chip={pChip} testid="beads-sample-chip" /> : null}
        <Switch
          inline
          checked={autoApplyMeasured(spec)}
          label="AutoApplyMeasured"
          onCheckedChange={(v) => patchSpec({ auto_apply_measured: v })}
          testid="beads-auto-apply"
        />
        {/* `?`·`⋯` 는 오른쪽 끝으로 밀지 않는다 — 밀면 띠가 넘칠 때 손잡이 하나만 둘째 줄에 혼자 선다. */}
        <HelpTip
          title="Beads"
          sections={beadsHelp(spec, suggestion)}
          align="right"
          testid="beads-help"
        />
        <OverflowMenu items={actions} title="Beads 조작" testid="beads-actions" />
      </div>
      <div className="flex flex-wrap items-start gap-3">
        <div className="min-w-0 flex-1 basis-80 overflow-x-auto">
          <table className="text-xs" data-testid="beads-grid">
            <thead>
              <tr className="text-content-muted">
                <th
                  className="px-1 py-1 text-right font-mono font-medium"
                  title={`스택 ${n} 에서의 단 번호(맨 아래가 1)`}
                >
                  Level
                </th>
                {PROFILE_KEYS.map((k) => (
                  <th
                    key={k}
                    className="px-1 py-1 text-right font-mono font-medium"
                    title="셀 바닥 기준 절대값"
                  >
                    {PROFILE_LABEL[k]} (mm)
                  </th>
                ))}
                <th
                  className="px-1 py-1 text-right font-mono font-medium"
                  title="이 타이어 위에 얹힌 개수(파생 곡선의 키)"
                >
                  Above
                </th>
                <th
                  className="px-1 py-1 text-right font-mono font-medium"
                  title="그 하중에서 먹은 총 눌림 = Height − PressedHeight"
                >
                  Compression@Level (mm)
                </th>
                <th
                  className="px-1 py-1 text-left font-mono font-medium"
                  title="measured = 측정 그대로 · manual = 손으로 고침 · interpolated = 이웃 점 보간 · computed = Compression 모형"
                >
                  Source
                </th>
                <th
                  className="px-1 py-1 text-left font-mono font-medium"
                  title="이 프로파일을 채운 SKU 표본"
                >
                  SampleSeq
                </th>
                <th
                  className="px-1 py-1 text-right font-mono font-medium"
                  title={`AbsUpperBead − PickBeadOffset ${round1(pickBeadOffset(spec))} · mid = 비드를 몰라 타이어 중간(H/2)`}
                >
                  PickZ (mm)
                </th>
                <th className="sr-only">행 지우기</th>
              </tr>
            </thead>
            <tbody>
              {grid.map((g) => (
                <tr
                  key={g.level}
                  className="border-t border-line-default align-top"
                  data-testid="beads-row"
                >
                  <td className="px-1 py-1.5 font-mono tabular-nums">{g.level}</td>
                  {PROFILE_KEYS.map((k) => (
                    <td key={k} className="px-1 py-1">
                      <LevelCell
                        value={g.stored?.[k] ?? null}
                        placeholder={fmt(
                          k === 'lower_bead'
                            ? g.absLowerBead
                            : k === 'upper_bead'
                              ? g.absUpperBead
                              : g.absStackHeight,
                        )}
                        label={`스택 ${n} ${g.level}단 ${PROFILE_LABEL[k]}`}
                        testid={`beads-${g.level}-${k}`}
                        onCommit={(v) => setCell(g.level, k, v)}
                      />
                      {k === 'upper_bead' && g.fromProfile ? (
                        <MeasuredLine
                          m={g.measured?.upper_bead ?? null}
                          d={g.deviation?.upper_bead ?? null}
                        />
                      ) : null}
                    </td>
                  ))}
                  <td className="px-1 py-1.5 text-right font-mono tabular-nums text-content-muted">
                    {g.above}
                  </td>
                  <td
                    className="px-1 py-1.5 text-right font-mono tabular-nums"
                    data-testid={`beads-${g.level}-compression-at`}
                  >
                    {g.compressionAt === null ? (
                      <span className="text-content-faint">—</span>
                    ) : (
                      g.compressionAt
                    )}
                  </td>
                  <td className="px-1 py-1.5" data-testid={`beads-${g.level}-source`}>
                    <SourceTag source={g.source} />
                  </td>
                  <td className="px-1 py-1.5 font-mono text-2xs tabular-nums text-content-muted">
                    {sampleLabel(
                      g.fromProfile ? (stored?.sample_plc ?? '') : (g.effective.sample?.plc ?? ''),
                      g.fromProfile ? (stored?.sample_seq ?? 0) : (g.effective.sample?.seq ?? 0),
                    )}
                  </td>
                  <td
                    className="px-1 py-1.5 text-right font-mono tabular-nums"
                    data-testid={`beads-${g.level}-pick-z`}
                  >
                    {g.pickZ === null ? (
                      <span className="text-content-faint">—</span>
                    ) : g.pickZRef === 'mid' ? (
                      <span
                        className="text-content-muted"
                        title="이 크기·하중의 비드를 아직 재지 않아 타이어 중간(H/2)으로 잡습니다"
                      >
                        {round1(g.pickZ)} mid
                      </span>
                    ) : (
                      <span className={g.fromProfile ? '' : 'text-content-muted'}>
                        {round1(g.pickZ)}
                      </span>
                    )}
                  </td>
                  <td className="px-1 py-1">
                    {g.stored ? (
                      <Button
                        size="icon-sm"
                        intent="ghost"
                        icon={<Trash2 size={13} />}
                        aria-label={`${g.level}단 지우기`}
                        title="이 단의 저장값 지우기(곡선 환산값으로 돌아감)"
                        onClick={() => updateSpec((s) => removeProfileRow(s, n, g.level))}
                      />
                    ) : null}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <StackSvg
          boxes={boxes}
          outer={draft.outer_diameter}
          inner={draft.inner_diameter}
          stackMax={spec.stack_max}
        />
      </div>
      <MeasuredSamples rows={rows} applying={applying} onApply={(seq) => void applySample(seq)} />
      {warnings.length ? (
        <ul className="list-disc pl-4 text-xs text-warn-fg" data-testid="beads-warnings">
          {warnings.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
      ) : null}
    </div>
  )
}
const SOURCE_TONE: Record<string, string> = {
  measured: 'bg-ok-soft text-ok-fg',
  manual: 'bg-warn-soft text-warn-fg',
  interpolated: 'bg-info-soft text-info-fg',
  computed: 'text-content-faint',
}

function SourceTag({ source }: { source: string }) {
  return (
    <span className={cn('rounded-sm px-1 font-mono text-2xs', SOURCE_TONE[source] ?? '')}>
      {source}
    </span>
  )
}

/** 라벨+값 한 짝 — 값은 고정폭. 설명 문장 대신 이 짝을 쓴다(`docs/DESIGN.md` 5절 ②). */
function Count({ label, v }: { label: string; v: number | string }) {
  return (
    <span className="mr-2 inline-flex items-baseline gap-1 last:mr-0">
      {label}
      <span className="font-mono tabular-nums text-content-tertiary">{v}</span>
    </span>
  )
}

/** 최근 SKU 측정 표본 — 어느 것이 규격에 들어갔고 어느 것이 왜 거부됐는지. */
function MeasuredSamples({
  rows,
  applying,
  onApply,
}: {
  rows: readonly SampleRow[]
  applying: number | null
  onApply: (seq: number) => void
}) {
  const stats = samplesStats(rows)
  return (
    <section className="flex flex-col gap-1" data-testid="beads-samples">
      {/* 머리는 이름 + 개수 짝이다 — "표본 5개 · 반영 3 · …" 같은 문장은 값 하나를 뽑으려고 매번 다 읽게 만든다. */}
      <h3 className="flex items-center gap-2 text-xs font-semibold">
        Measured
        <span className="font-normal text-2xs text-content-muted" data-testid="beads-samples-stats">
          <Count label="표본" v={stats.total} />
          <Count label="반영" v={stats.applied} />
          <Count label="거부" v={stats.rejected} />
        </span>
      </h3>
      {rows.length === 0 ? (
        <p className="text-2xs text-content-faint" title="measureSKU 를 한 번 돌리면 여기 쌓입니다">
          기록 없음
        </p>
      ) : null}
      {rows.length ? (
        <div className="w-full overflow-x-auto">
          <table className="text-xs">
            <thead>
              <tr className="text-content-muted">
                <th className="px-1 py-1 text-left font-mono font-medium">Time</th>
                <th className="px-1 py-1 text-left font-mono font-medium">Robot</th>
                <th className="px-1 py-1 text-right font-mono font-medium">Seq</th>
                <th className="px-1 py-1 text-right font-mono font-medium">TotalCount</th>
                <th className="px-1 py-1 text-right font-mono font-medium">EachHeight</th>
                <th className="px-1 py-1 text-right font-mono font-medium">TotalHeight</th>
                <th className="px-1 py-1 text-right font-mono font-medium">Status</th>
                <th
                  className="px-1 py-1 text-left font-mono font-medium"
                  title="이 표본이 채우는 하중"
                >
                  Above
                </th>
                <th
                  className="px-1 py-1 text-left font-mono font-medium"
                  title="반영 내용 또는 거부 사유"
                >
                  Result
                </th>
                <th className="sr-only">반영</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr
                  key={r.id}
                  className="border-t border-line-default"
                  data-testid="beads-sample-row"
                >
                  <td className="px-1 py-1 font-mono text-2xs tabular-nums whitespace-nowrap">
                    {r.time}
                  </td>
                  <td className="px-1 py-1 font-mono text-2xs">{r.plc}</td>
                  <td className="px-1 py-1 text-right font-mono tabular-nums">{r.seq}</td>
                  <td className="px-1 py-1 text-right font-mono tabular-nums">{r.totalCount}</td>
                  <td className="px-1 py-1 text-right font-mono tabular-nums">
                    {mm(r.eachHeight)}
                  </td>
                  <td className="px-1 py-1 text-right font-mono tabular-nums">
                    {mm(r.totalHeight)}
                  </td>
                  <td className="px-1 py-1 text-right font-mono tabular-nums">{r.status}</td>
                  <td className="px-1 py-1 font-mono text-2xs tabular-nums">
                    {r.aboves.length ? `${r.aboves[0]}..${r.aboves[r.aboves.length - 1]}` : '—'}
                  </td>
                  <td
                    className={cn(
                      'max-w-72 px-1 py-1 text-2xs',
                      r.state === 'applied'
                        ? 'text-ok-fg'
                        : r.state === 'rejected'
                          ? 'text-fault-fg'
                          : 'text-content-muted',
                    )}
                    title={r.label}
                    data-testid={`beads-sample-${r.seq}-state`}
                  >
                    {r.label}
                  </td>
                  <td className="px-1 py-1">
                    <Button
                      size="sm"
                      intent="outline"
                      disabled={!r.canApply}
                      loading={applying === r.seq}
                      onClick={() => onApply(r.seq)}
                      title={
                        r.canApply
                          ? '이 표본을 규격에 넣습니다(자동 반영 스위치와 상관없이)'
                          : r.reason
                      }
                      data-testid={`beads-sample-${r.seq}-apply`}
                    >
                      Apply
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : null}
    </section>
  )
}

// ── Usage ────────────────────────────────────────────────────────────────────

function UsageTab({
  code,
  spec,
  stock,
}: {
  code: number
  spec: ItemSpec
  stock: readonly StockEntry[]
}) {
  const cells = stockCellsFor(code, stock, spec.stack_max)
  const over = cells.filter((c) => c.over).length
  const [refs, setRefs] = useState<StepRef[] | null>(null)
  const [err, setErr] = useState<string | null>(null)

  useEffect(() => {
    let alive = true
    api
      .scenarios()
      .then((s) => {
        if (alive) setRefs(scenarioRefs(s, code))
      })
      .catch((e: unknown) => {
        if (alive) setErr(errMsg(e))
      })
    return () => {
      alive = false
    }
  }, [code])

  const th = 'px-1.5 py-1 text-left font-mono font-medium'
  return (
    <div className="flex flex-col gap-4" data-testid="item-usage">
      <section className="flex flex-col gap-1">
        <h3 className="flex items-center gap-2 text-2xs font-semibold text-content-muted">
          재고 칸
          <span className="font-normal">
            <Count label="StockCells" v={cells.length} />
            <Count label="StackMax" v={limitLabel(spec.stack_max)} />
            {over ? <Count label="초과" v={over} /> : null}
          </span>
          <HelpTip title="재고 칸" sections={[USAGE_HELP[0]]} />
        </h3>
        {cells.length === 0 ? (
          <p className="text-xs text-content-faint">재고 칸 없음</p>
        ) : (
          <div className="w-full overflow-x-auto">
            <table className="text-xs">
              <thead>
                <tr className="text-content-muted">
                  <th className={th}>Cell</th>
                  <th className={th}>Count</th>
                  <th className={th} title="0 = 제한 없음">
                    StackMax
                  </th>
                  <th className={th}>
                    <span className="sr-only">상태</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {cells.map((c) => (
                  <tr
                    key={c.cell_id}
                    className={cn(
                      'border-t border-line-default',
                      c.over && 'bg-warn-soft text-warn-fg',
                    )}
                    data-testid="usage-cell"
                  >
                    <td className="px-1.5 py-1 font-mono tabular-nums">{c.cell_id}</td>
                    <td className="px-1.5 py-1 font-mono tabular-nums">{c.count}</td>
                    <td className="px-1.5 py-1 font-mono tabular-nums">
                      {limitLabel(spec.stack_max)}
                    </td>
                    <td className="px-1.5 py-1">{c.over ? 'StackMax 초과' : ''}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
      <section className="flex flex-col gap-1">
        <h3 className="flex items-center gap-2 text-2xs font-semibold text-content-muted">
          시나리오 스텝
          {refs ? (
            <span className="font-normal">
              <Count label="Steps" v={refs.length} />
            </span>
          ) : null}
          <HelpTip title="시나리오 스텝" sections={[USAGE_HELP[1]]} />
        </h3>
        {err ? (
          <p className="text-xs text-fault-fg">시나리오를 못 받았습니다 — {err}</p>
        ) : refs === null ? (
          <p className="text-xs text-content-faint">받는 중…</p>
        ) : refs.length === 0 ? (
          <p className="text-xs text-content-faint">스텝 없음</p>
        ) : (
          <div className="w-full overflow-x-auto">
            <table className="text-xs">
              <thead>
                <tr className="text-content-muted">
                  <th className={th}>Scenario</th>
                  <th className={th}>Step</th>
                  <th className={th}>Type</th>
                  <th className={th}>Target</th>
                  <th className={th}>Label</th>
                </tr>
              </thead>
              <tbody>
                {refs.map((r) => (
                  <tr key={`${r.scenario_id}:${r.step}`} className="border-t border-line-default">
                    <td className="px-1.5 py-1">{r.scenario}</td>
                    <td className="px-1.5 py-1 font-mono tabular-nums">{r.step}</td>
                    <td className="px-1.5 py-1 font-mono">{r.type}</td>
                    <td className="px-1.5 py-1">{r.target}</td>
                    <td className="px-1.5 py-1 text-content-muted">{r.label}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  )
}
