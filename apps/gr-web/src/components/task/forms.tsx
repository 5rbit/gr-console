// 레지스트리 편집 폼 — 품목·셀·스테이션을 `FormDialog` 안에서 만들고 고친다.
//
// 검증은 백엔드와 같은 규칙(셀 1..1000, 스테이션 2001..2999 & mod 100 ∈ 1..32, 셀 X/Y ≥ 0·Z > 0, 스테이션 위치 > 0, 구역 1..3)을
// 제출 전에 한 번 더 돈다 — 서버 400을 기다리지 않게.
//
// 바닥 띠는 **하나**다. 예전에는 폼이 자기 저장 줄을 그리고 `Modal`이 그 아래 닫기 줄을 또 그려
// 푸터가 두 겹이었고, 열자마자 포커스가 그 닫기에 앉아 첫 칸을 찾아 Tab 을 눌러야 했다(그래서
// 여기서 `data-autofocus`를 DOM 에 심는 이펙트가 있었다). `FormDialog`가 첫 입력 포커스 · Enter
// 제출 · 저장 안 한 입력 지킴이를 한 벌로 맡으므로 그 셋이 모두 사라졌다.
import type * as React from 'react'
import { useState } from 'react'
import { FormDialog } from '../../lib/ui/Dialog'
import { HelpTip } from '../../lib/ui/HelpTip'
import { Input } from '../../lib/ui/Input'
import { Switch } from '../../lib/ui/Switch'
import { DEFAULT_SPEC, specErrors, specOf } from '../../lib/items/levelsModel'
import { itemErrors } from '../../lib/items/model'
import type { CellUpsert, ItemUpsert, StationUpsert } from '../../lib/types'

// ── 공용 조각 ─────────────────────────────────────────────────────────────────

function Num({
  label,
  value,
  onChange,
  disabled,
  step = 'any',
  min,
}: {
  label: string
  value: number
  onChange: (n: number) => void
  disabled?: boolean
  step?: string
  min?: number
}) {
  return (
    <Input
      label={label}
      type="number"
      mono
      step={step}
      min={min}
      value={Number.isFinite(value) ? String(value) : ''}
      disabled={disabled}
      onValueChange={(s) => {
        const n = Number(s)
        if (s.trim() !== '' && Number.isFinite(n)) onChange(n)
      }}
    />
  )
}

function Bool({
  label,
  value,
  onChange,
}: {
  label: string
  value: boolean
  onChange: (b: boolean) => void
}) {
  return (
    <label className="flex items-center justify-between gap-2 rounded-md border border-line-default px-2 py-1.5 text-xs">
      <span>{label}</span>
      <Switch checked={value} label={label} onCheckedChange={onChange} />
    </label>
  )
}

/** 폼 오류 목록 — 대화상자 본문 맨 아래. 화면 폼도 같은 모양으로 쓴다(두 벌로 베끼지 않는다). */
export function FormErrors({ list }: { list: string[] }) {
  if (list.length === 0) return null
  return (
    <ul className="list-disc rounded-md border border-fault bg-fault-soft py-1 pr-2 pl-6 text-xs text-fault-fg">
      {list.map((e) => (
        <li key={e}>{e}</li>
      ))}
    </ul>
  )
}

function Section({
  title,
  hint,
  children,
}: {
  title: string
  /** 규칙·출처 설명 — 제목에 문장을 붙이지 않고 `?` 로 내린다. */
  hint?: string
  children: React.ReactNode
}) {
  return (
    <fieldset className="flex flex-col gap-2">
      <legend className="mb-1 flex items-center gap-1 text-2xs font-semibold text-content-muted">
        {title}
        {hint ? <HelpTip title={title} text={hint} /> : null}
      </legend>
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">{children}</div>
    </fieldset>
  )
}

/** 공통 껍데기 — 제목 · 저장(Enter) · 오류 목록. 바닥 띠는 `FormDialog` 하나뿐이다. */
function FormModal({
  open,
  onOpenChange,
  title,
  errors,
  saving,
  dirty,
  onSave,
  wide = false,
  children,
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
  title: string
  errors: string[]
  saving: boolean
  /** 입력이 남아 있는가 — 참이면 Escape·백드롭·취소가 한 번 되묻는다. */
  dirty: boolean
  onSave: () => void
  wide?: boolean
  children: React.ReactNode
}) {
  return (
    <FormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={title}
      size={wide ? 'lg' : 'md'}
      dirty={dirty}
      busy={saving}
      submitLabel="저장"
      onSubmit={onSave}
      testid="registry-form"
    >
      {children}
      <FormErrors list={errors} />
    </FormDialog>
  )
}

// ── 검증(백엔드 규칙 그대로) ─────────────────────────────────────────────────

/** 스테이션: PLC isValid_Station_Parameter 가 X/Y/Z ≤ 0 을 거부한다. */
const positionErrors = (p: [number, number, number]): string[] =>
  (['X', 'Y', 'Z'] as const)
    .filter((_, i) => !(p[i] > 0))
    .map((a) => `위치 ${a}는 0보다 커야 합니다`)

/** 셀: X/Y 는 0 이상이면 되고, 바닥 Z ≤ 0 은 PLC isValidTaskData 가 거부한다. */
export const cellPositionErrors = (p: [number, number, number]): string[] => [
  ...(['X', 'Y'] as const)
    .filter((_, i) => !(p[i] >= 0))
    .map((a) => `위치 ${a}는 0 이상이어야 합니다`),
  ...(p[2] > 0 ? [] : ['위치 Z(바닥)는 0보다 커야 합니다']),
]

export function validateCell(c: CellUpsert): string[] {
  const out: string[] = []
  if (!Number.isInteger(c.id) || c.id < 1 || c.id > 1000) out.push('셀 Id는 1..1000')
  if (![1, 2, 3].includes(c.section)) out.push('구역은 1..3')
  out.push(...cellPositionErrors(c.position))
  return out
}

export function validateStation(s: StationUpsert): string[] {
  const out: string[] = []
  const m = s.id % 100
  if (!Number.isInteger(s.id) || s.id < 2001 || s.id > 2999 || m < 1 || m > 32)
    out.push('스테이션 Id는 2001..2999 이고 Id mod 100 이 1..32')
  if (![1, 2, 3].includes(s.info.section)) out.push('구역은 1..3')
  out.push(...positionErrors(s.info.position))
  return out
}

/** 품목 규칙은 `lib/items/model.ts`·`levelsModel.ts`(테스트가 있는 순수 모듈)가 진실원이다. */
export function validateItem(i: ItemUpsert): string[] {
  return [...itemErrors(i), ...(i.spec ? specErrors(i.spec) : [])]
}

// ── 빈 값 ─────────────────────────────────────────────────────────────────────

export const EMPTY_CELL: CellUpsert = {
  id: 0,
  use: true,
  blend_use: false,
  section: 1,
  row: 1,
  col: 1,
  length: 0,
  width: 0,
  position: [0, 0, 0],
}

export const EMPTY_STATION: StationUpsert = {
  id: 0,
  conv_no: 1,
  task_type: 1,
  rotate_type: 1,
  group: 1,
  group_index: 1,
  connection_prev: 0,
  connection_next: 0,
  info: { ...EMPTY_CELL, section: 3 },
  sensor: {
    io_link_master_module: 0,
    io_link_master_port_l: 0,
    io_link_master_port_r: 0,
    detection_factor: 0,
    allow_range: 0,
    l_sensor_offset: 0,
    r_sensor_offset: 0,
  },
  io_block_no: 0,
}

export const EMPTY_ITEM: ItemUpsert = {
  code: 0,
  name: '',
  count: 1,
  inner_diameter: 0,
  outer_diameter: 0,
  lower_bead_height: 0,
  upper_bead_height: 0,
  height: 0,
  deflection_factor: 0,
  note: '',
  spec: DEFAULT_SPEC,
}

// ── 폼 ────────────────────────────────────────────────────────────────────────

export interface FormProps<T> {
  open: boolean
  onOpenChange: (o: boolean) => void
  /** 편집이면 기존 값, 추가면 빈 값. `id`/`code`는 편집 때 잠근다. */
  initial: T
  editing: boolean
  onSave: (v: T) => Promise<void>
}

function CellFields({
  v,
  set,
  lockId,
}: {
  v: CellUpsert
  set: (patch: Partial<CellUpsert>) => void
  lockId: boolean
}) {
  const pos = (i: 0 | 1 | 2, n: number) => {
    const p: [number, number, number] = [...v.position] as [number, number, number]
    p[i] = n
    set({ position: p })
  }
  return (
    <>
      <Section title="식별">
        <Num
          label="Id"
          value={v.id}
          onChange={(id) => set({ id })}
          disabled={lockId}
          step="1"
          min={1}
        />
        <Num label="Section" value={v.section} onChange={(section) => set({ section })} step="1" />
        <Num label="Row" value={v.row} onChange={(row) => set({ row })} step="1" />
        <Num label="Col" value={v.col} onChange={(col) => set({ col })} step="1" />
        <Bool label="Use" value={v.use} onChange={(use) => set({ use })} />
        <Bool label="BlendUse" value={v.blend_use} onChange={(blend_use) => set({ blend_use })} />
      </Section>
      <Section title="치수·위치 (mm)">
        <Num label="Length" value={v.length} onChange={(length) => set({ length })} />
        <Num label="Width" value={v.width} onChange={(width) => set({ width })} />
        <span />
        <Num label="X" value={v.position[0]} onChange={(n) => pos(0, n)} />
        <Num label="Y" value={v.position[1]} onChange={(n) => pos(1, n)} />
        <Num label="Z" value={v.position[2]} onChange={(n) => pos(2, n)} />
      </Section>
    </>
  )
}

/** 입력이 남아 있나 — 값 하나라도 처음과 다르면 닫기가 한 번 되묻는다. */
const changed = (a: unknown, b: unknown) => JSON.stringify(a) !== JSON.stringify(b)

export function CellForm({ open, onOpenChange, initial, editing, onSave }: FormProps<CellUpsert>) {
  const [v, setV] = useState<CellUpsert>(initial)
  const [errors, setErrors] = useState<string[]>([])
  const [saving, setSaving] = useState(false)
  const set = (patch: Partial<CellUpsert>) => setV((cur) => ({ ...cur, ...patch }))
  const save = async () => {
    const errs = validateCell(v)
    setErrors(errs)
    if (errs.length) return
    setSaving(true)
    try {
      await onSave(v)
    } catch (e) {
      setErrors([e instanceof Error ? e.message : String(e)])
    } finally {
      setSaving(false)
    }
  }
  return (
    <FormModal
      open={open}
      onOpenChange={onOpenChange}
      title={editing ? `셀 #${initial.id} 편집` : '셀 추가'}
      errors={errors}
      saving={saving}
      dirty={changed(v, initial)}
      onSave={save}
    >
      <CellFields v={v} set={set} lockId={editing} />
    </FormModal>
  )
}

export function StationForm({
  open,
  onOpenChange,
  initial,
  editing,
  onSave,
}: FormProps<StationUpsert>) {
  const [v, setV] = useState<StationUpsert>(initial)
  const [errors, setErrors] = useState<string[]>([])
  const [saving, setSaving] = useState(false)
  const set = (patch: Partial<StationUpsert>) => setV((cur) => ({ ...cur, ...patch }))
  const setInfo = (patch: Partial<CellUpsert>) =>
    setV((cur) => ({ ...cur, info: { ...cur.info, ...patch } }))
  const setSensor = (patch: Partial<StationUpsert['sensor']>) =>
    setV((cur) => ({ ...cur, sensor: { ...cur.sensor, ...patch } }))
  const save = async () => {
    const next = { ...v, info: { ...v.info, id: v.id } }
    const errs = validateStation(next)
    setErrors(errs)
    if (errs.length) return
    setSaving(true)
    try {
      await onSave(next)
    } catch (e) {
      setErrors([e instanceof Error ? e.message : String(e)])
    } finally {
      setSaving(false)
    }
  }
  const s = v.sensor
  return (
    <FormModal
      open={open}
      onOpenChange={onOpenChange}
      title={editing ? `스테이션 #${initial.id} 편집` : '스테이션 추가'}
      errors={errors}
      saving={saving}
      dirty={changed(v, initial)}
      onSave={save}
      wide
    >
      <Section title="스테이션">
        <Num
          label="Id"
          value={v.id}
          onChange={(id) => set({ id })}
          disabled={editing}
          step="1"
          min={2001}
        />
        <Num label="ConvNo" value={v.conv_no} onChange={(conv_no) => set({ conv_no })} step="1" />
        <Num
          label="TaskType"
          value={v.task_type}
          onChange={(task_type) => set({ task_type })}
          step="1"
        />
        <Num
          label="RotateType"
          value={v.rotate_type}
          onChange={(rotate_type) => set({ rotate_type })}
          step="1"
        />
        <Num label="Group" value={v.group} onChange={(group) => set({ group })} step="1" />
        <Num
          label="GroupIndex"
          value={v.group_index}
          onChange={(group_index) => set({ group_index })}
          step="1"
        />
        <Num
          label="ConnectionPrev"
          value={v.connection_prev}
          onChange={(connection_prev) => set({ connection_prev })}
          step="1"
        />
        <Num
          label="ConnectionNext"
          value={v.connection_next}
          onChange={(connection_next) => set({ connection_next })}
          step="1"
        />
        <Num
          label="IOBlockNo"
          value={v.io_block_no}
          onChange={(io_block_no) => set({ io_block_no })}
          step="1"
        />
      </Section>
      <CellFields v={{ ...v.info, id: v.id }} set={setInfo} lockId />
      <Section title="센서(IO-Link)">
        <Num
          label="IOLinkMasterModule"
          value={s.io_link_master_module}
          onChange={(n) => setSensor({ io_link_master_module: n })}
          step="1"
        />
        <Num
          label="IOLinkMasterPort_L"
          value={s.io_link_master_port_l}
          onChange={(n) => setSensor({ io_link_master_port_l: n })}
          step="1"
        />
        <Num
          label="IOLinkMasterPort_R"
          value={s.io_link_master_port_r}
          onChange={(n) => setSensor({ io_link_master_port_r: n })}
          step="1"
        />
        <Num
          label="DetectionFactor"
          value={s.detection_factor}
          onChange={(n) => setSensor({ detection_factor: n })}
        />
        <Num
          label="AllowRange"
          value={s.allow_range}
          onChange={(n) => setSensor({ allow_range: n })}
        />
        <span />
        <Num
          label="L_SensorOffset"
          value={s.l_sensor_offset}
          onChange={(n) => setSensor({ l_sensor_offset: n })}
        />
        <Num
          label="R_SensorOffset"
          value={s.r_sensor_offset}
          onChange={(n) => setSensor({ r_sensor_offset: n })}
        />
      </Section>
    </FormModal>
  )
}

/**
 * 품목 입력 칸들 — **대화상자 없이** 쓰는 몸통이다.
 *
 * 재고 편집 팝업이 "새 품목 등록" 을 모달 위에 모달로 띄우고 있었다(오버레이 예산 위반: 두 번째
 * 모달 금지). 칸들을 껍데기에서 떼어 두면 같은 폼이 **한 대화상자 안의 다음 단계**로도 설 수 있다 —
 * 이 파일의 `ItemForm` 과 재고 팝업이 같은 칸·같은 검증을 쓴다.
 */
export function ItemFields({
  value,
  onChange,
  lockCode = false,
}: {
  value: ItemUpsert
  onChange: (v: ItemUpsert) => void
  /** 편집이면 Code 를 잠근다(키다). */
  lockCode?: boolean
}) {
  const v = value
  const editing = lockCode
  const set = (patch: Partial<ItemUpsert>) => onChange({ ...v, ...patch })
  const spec = specOf(v)
  const setSpec = (patch: Partial<ItemUpsert['spec'] & object>) =>
    onChange({ ...v, spec: { ...specOf(v), ...patch } })
  return (
    <>
      <Section title="품목" hint="PLC LGR_Stock_Item 한 행과 같은 값입니다.">
        <Num
          label="Code"
          value={v.code}
          onChange={(code) => set({ code })}
          disabled={editing}
          step="1"
          min={1}
        />
        <Input
          className="col-span-2"
          label="Name"
          value={v.name}
          onValueChange={(name) => set({ name })}
          placeholder="예: 225/45R17"
        />
        <Num label="Count" value={v.count} onChange={(count) => set({ count })} step="1" min={1} />
        <Num
          label="InnerDiameter (mm)"
          value={v.inner_diameter}
          onChange={(inner_diameter) => set({ inner_diameter })}
        />
        <Num
          label="OuterDiameter (mm)"
          value={v.outer_diameter}
          onChange={(outer_diameter) => set({ outer_diameter })}
        />
        <Num
          label="LowerBidHeight (mm)"
          value={v.lower_bead_height}
          onChange={(lower_bead_height) => set({ lower_bead_height })}
        />
        <Num
          label="UpperBidHeight (mm)"
          value={v.upper_bead_height}
          onChange={(upper_bead_height) => set({ upper_bead_height })}
        />
        <Num label="Height (mm)" value={v.height} onChange={(height) => set({ height })} />
        <Num
          label="DeflectionFactor"
          value={v.deflection_factor}
          onChange={(deflection_factor) => set({ deflection_factor })}
        />
        <Input
          className="col-span-2"
          label="Note"
          value={v.note}
          onValueChange={(note) => set({ note })}
        />
      </Section>
      <Section
        title="적재"
        hint="콘솔 전용 값이라 PLC 로 가지 않습니다. 0 은 제한 없음이고, 단별 비드 표는 화물 규격 화면 Beads 탭에 있습니다."
      >
        <Num
          label="StackMax"
          value={spec.stack_max}
          onChange={(stack_max) => setSpec({ stack_max })}
          step="1"
          min={0}
        />
        <Num
          label="PalletMax"
          value={spec.pallet_max}
          onChange={(pallet_max) => setSpec({ pallet_max })}
          step="1"
          min={0}
        />
        <Input
          label="WeightKg"
          type="number"
          mono
          step="any"
          min={0}
          value={spec.weight_kg ?? ''}
          placeholder="미입력"
          onValueChange={(s) => {
            const n = Number(s)
            if (s.trim() === '') setSpec({ weight_kg: null })
            else if (Number.isFinite(n)) setSpec({ weight_kg: n })
          }}
        />
      </Section>
    </>
  )
}

export function ItemForm({ open, onOpenChange, initial, editing, onSave }: FormProps<ItemUpsert>) {
  const [v, setV] = useState<ItemUpsert>(initial)
  const [errors, setErrors] = useState<string[]>([])
  const [saving, setSaving] = useState(false)
  const save = async () => {
    const errs = validateItem(v)
    setErrors(errs)
    if (errs.length) return
    setSaving(true)
    try {
      await onSave(v)
    } catch (e) {
      setErrors([e instanceof Error ? e.message : String(e)])
    } finally {
      setSaving(false)
    }
  }
  return (
    <FormModal
      open={open}
      onOpenChange={onOpenChange}
      title={
        editing
          ? `품목 ${initial.code} 편집`
          : initial.code
            ? `품목 추가 — ${initial.code}`
            : '품목 추가'
      }
      errors={errors}
      saving={saving}
      dirty={changed(v, initial)}
      onSave={save}
    >
      <ItemFields value={v} onChange={setV} lockCode={editing} />
    </FormModal>
  )
}
