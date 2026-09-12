// 레지스트리 편집 폼 — 품목·셀·스테이션을 `Modal` 안에서 만들고 고친다.
//
// 검증은 백엔드와 같은 규칙(셀 1..1000, 스테이션 2001..2999 & mod 100 ∈ 1..32, 셀 X/Y ≥ 0·Z > 0, 스테이션 위치 > 0, 구역 1..3)을
// 제출 전에 한 번 더 돈다 — 서버 400을 기다리지 않게. 저장 버튼은 폼 안에 두고 Modal의 기본 버튼은 닫기다.
import type * as React from 'react'
import { useState } from 'react'
import { Save } from 'lucide-react'
import { Button } from '../../lib/ui/Button'
import { Input } from '../../lib/ui/Input'
import { Modal } from '../../lib/ui/Modal'
import { Switch } from '../../lib/ui/Switch'
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

function Errors({ list }: { list: string[] }) {
  if (list.length === 0) return null
  return (
    <ul className="list-disc rounded-md border border-fault bg-fault-soft py-1 pr-2 pl-6 text-xs text-fault-fg">
      {list.map((e) => (
        <li key={e}>{e}</li>
      ))}
    </ul>
  )
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <fieldset className="flex flex-col gap-2">
      <legend className="mb-1 text-2xs font-semibold text-content-muted">{title}</legend>
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">{children}</div>
    </fieldset>
  )
}

/** 공통 껍데기 — 제목·저장 버튼·오류 목록. */
function FormModal({
  open,
  onOpenChange,
  title,
  errors,
  saving,
  onSave,
  wide = false,
  children,
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
  title: string
  errors: string[]
  saving: boolean
  onSave: () => void
  wide?: boolean
  children: React.ReactNode
}) {
  return (
    <Modal open={open} onOpenChange={onOpenChange} title={title} wide={wide} closeLabel="취소">
      <form
        className="flex flex-col gap-3"
        onSubmit={(e) => {
          e.preventDefault()
          onSave()
        }}
        data-testid="registry-form"
      >
        {children}
        <Errors list={errors} />
        <div className="flex justify-end">
          <Button
            type="submit"
            intent="primary"
            size="sm"
            icon={<Save className="h-3.5 w-3.5" />}
            loading={saving}
            data-testid="form-save"
          >
            저장
          </Button>
        </div>
      </form>
    </Modal>
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

export function validateItem(i: ItemUpsert): string[] {
  const out: string[] = []
  if (!Number.isInteger(i.code) || i.code < 1) out.push('코드는 1 이상의 정수')
  if (!Number.isInteger(i.count) || i.count < 1 || i.count > 255) out.push('수량은 1..255')
  return out
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
        <Num
          label="구역(Section)"
          value={v.section}
          onChange={(section) => set({ section })}
          step="1"
        />
        <Num label="행(Row)" value={v.row} onChange={(row) => set({ row })} step="1" />
        <Num label="열(Col)" value={v.col} onChange={(col) => set({ col })} step="1" />
        <Bool label="사용(Use)" value={v.use} onChange={(use) => set({ use })} />
        <Bool
          label="블렌드(BlendUse)"
          value={v.blend_use}
          onChange={(blend_use) => set({ blend_use })}
        />
      </Section>
      <Section title="치수·위치 (mm)">
        <Num label="길이(Length)" value={v.length} onChange={(length) => set({ length })} />
        <Num label="폭(Width)" value={v.width} onChange={(width) => set({ width })} />
        <span />
        <Num label="X" value={v.position[0]} onChange={(n) => pos(0, n)} />
        <Num label="Y" value={v.position[1]} onChange={(n) => pos(1, n)} />
        <Num label="Z" value={v.position[2]} onChange={(n) => pos(2, n)} />
      </Section>
    </>
  )
}

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
        <Num
          label="컨베이어(ConvNo)"
          value={v.conv_no}
          onChange={(conv_no) => set({ conv_no })}
          step="1"
        />
        <Num
          label="작업 종류(TaskType)"
          value={v.task_type}
          onChange={(task_type) => set({ task_type })}
          step="1"
        />
        <Num
          label="회전(RotateType)"
          value={v.rotate_type}
          onChange={(rotate_type) => set({ rotate_type })}
          step="1"
        />
        <Num label="그룹(Group)" value={v.group} onChange={(group) => set({ group })} step="1" />
        <Num
          label="그룹 순번(GroupIndex)"
          value={v.group_index}
          onChange={(group_index) => set({ group_index })}
          step="1"
        />
        <Num
          label="이전 연결(ConnPrev)"
          value={v.connection_prev}
          onChange={(connection_prev) => set({ connection_prev })}
          step="1"
        />
        <Num
          label="다음 연결(ConnNext)"
          value={v.connection_next}
          onChange={(connection_next) => set({ connection_next })}
          step="1"
        />
        <Num
          label="IO 블록(IOBlockNo)"
          value={v.io_block_no}
          onChange={(io_block_no) => set({ io_block_no })}
          step="1"
        />
      </Section>
      <CellFields v={{ ...v.info, id: v.id }} set={setInfo} lockId />
      <Section title="센서(IO-Link)">
        <Num
          label="모듈"
          value={s.io_link_master_module}
          onChange={(n) => setSensor({ io_link_master_module: n })}
          step="1"
        />
        <Num
          label="포트 L"
          value={s.io_link_master_port_l}
          onChange={(n) => setSensor({ io_link_master_port_l: n })}
          step="1"
        />
        <Num
          label="포트 R"
          value={s.io_link_master_port_r}
          onChange={(n) => setSensor({ io_link_master_port_r: n })}
          step="1"
        />
        <Num
          label="검출 계수"
          value={s.detection_factor}
          onChange={(n) => setSensor({ detection_factor: n })}
        />
        <Num
          label="허용 범위"
          value={s.allow_range}
          onChange={(n) => setSensor({ allow_range: n })}
        />
        <span />
        <Num
          label="L 센서 오프셋"
          value={s.l_sensor_offset}
          onChange={(n) => setSensor({ l_sensor_offset: n })}
        />
        <Num
          label="R 센서 오프셋"
          value={s.r_sensor_offset}
          onChange={(n) => setSensor({ r_sensor_offset: n })}
        />
      </Section>
    </FormModal>
  )
}

export function ItemForm({ open, onOpenChange, initial, editing, onSave }: FormProps<ItemUpsert>) {
  const [v, setV] = useState<ItemUpsert>(initial)
  const [errors, setErrors] = useState<string[]>([])
  const [saving, setSaving] = useState(false)
  const set = (patch: Partial<ItemUpsert>) => setV((cur) => ({ ...cur, ...patch }))
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
      title={editing ? `품목 ${initial.code} 편집` : '품목 추가'}
      errors={errors}
      saving={saving}
      onSave={save}
    >
      <Section title="품목">
        <Num
          label="코드(Code)"
          value={v.code}
          onChange={(code) => set({ code })}
          disabled={editing}
          step="1"
          min={1}
        />
        <Input
          className="col-span-2"
          label="이름"
          value={v.name}
          onValueChange={(name) => set({ name })}
          placeholder="예: 225/45R17"
        />
        <Num
          label="적재 수량(Count)"
          value={v.count}
          onChange={(count) => set({ count })}
          step="1"
          min={1}
        />
        <Num
          label="내경(mm)"
          value={v.inner_diameter}
          onChange={(inner_diameter) => set({ inner_diameter })}
        />
        <Num
          label="외경(mm)"
          value={v.outer_diameter}
          onChange={(outer_diameter) => set({ outer_diameter })}
        />
        <Num
          label="하부 비드 높이"
          value={v.lower_bead_height}
          onChange={(lower_bead_height) => set({ lower_bead_height })}
        />
        <Num
          label="상부 비드 높이"
          value={v.upper_bead_height}
          onChange={(upper_bead_height) => set({ upper_bead_height })}
        />
        <Num label="높이(mm)" value={v.height} onChange={(height) => set({ height })} />
        <Num
          label="처짐 계수"
          value={v.deflection_factor}
          onChange={(deflection_factor) => set({ deflection_factor })}
        />
        <Input
          className="col-span-2"
          label="비고"
          value={v.note}
          onValueChange={(note) => set({ note })}
        />
      </Section>
    </FormModal>
  )
}
