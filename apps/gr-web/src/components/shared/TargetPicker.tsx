// 대상 선택 — 종류(셀/스테이션) Select + 검색칸 + 그 종류의 목록 Select.
//
// 셀은 수백 개라 한 번에 펼치면 못 찾는다 — 검색칸이 목록을 좁힌다(id·구역·행·열 어느 것으로든).
// 목록은 `useRegistry`로 스스로 받는다; 이미 받아 둔 것이 있으면 `cells`/`stations`로 넘겨 재조회를 피한다.
import { useState } from 'react'
import { api } from '../../lib/api'
import { useRegistry } from '../../lib/registry'
import { Input } from '../../lib/ui/Input'
import { Select } from '../../lib/ui/Select'
import type { Cell, Station, Target, TargetKind } from '../../lib/types'

export interface TargetPickerProps {
  value: Target | null
  onChange: (next: Target | null) => void
  /** 미리 받아 둔 목록(선택). 없으면 스스로 받는다. */
  cells?: Cell[]
  stations?: Station[]
  /** 고를 수 있는 종류(예: MEASURE는 셀만). */
  kinds?: TargetKind[]
  disabled?: boolean
  dense?: boolean
}

const KIND_LABEL: Record<TargetKind, string> = { cell: '셀', station: '스테이션' }

export function cellText(c: Cell): string {
  return `#${c.id} · S${c.section} R${c.row} C${c.col}${c.use ? '' : ' (미사용)'}`
}
export function stationText(s: Station): string {
  return `#${s.id} · CV${s.conv_no} G${s.group}-${s.group_index}`
}

export function TargetPicker({
  value,
  onChange,
  cells,
  stations,
  kinds = ['cell', 'station'],
  disabled = false,
  dense = false,
}: TargetPickerProps) {
  // 훅은 조건 없이 부른다 — 넘겨받은 목록이 있으면 그것을 쓰고 조회 결과는 버린다.
  const cellReg = useRegistry<Cell>(cells ? async () => cells : api.cells)
  const stationReg = useRegistry<Station>(stations ? async () => stations : api.stations)
  const cellList = cells ?? cellReg.items
  const stationList = stations ?? stationReg.items

  // 종류는 고른 대상이 있으면 그것을 따르고, 없으면(아직 id를 안 골랐으면) 로컬 선택을 쓴다.
  const [pickedKind, setPickedKind] = useState<TargetKind>(value?.kind ?? kinds[0] ?? 'cell')
  const kind: TargetKind = value?.kind ?? pickedKind
  const [q, setQ] = useState('')
  const needle = q.trim().toLowerCase()

  const options: { id: number; text: string }[] =
    kind === 'cell'
      ? cellList.map((c) => ({ id: c.id, text: cellText(c) }))
      : stationList.map((s) => ({ id: s.id, text: stationText(s) }))
  const shown = needle ? options.filter((o) => o.text.toLowerCase().includes(needle)) : options
  // 고른 것이 검색에 안 걸려도 목록에서 사라지면 안 된다(빈 선택으로 보인다).
  const picked =
    value && !shown.some((o) => o.id === value.id)
      ? (options.find((o) => o.id === value.id) ?? { id: value.id, text: `#${value.id} (목록에 없음)` })
      : null
  const loading = kind === 'cell' ? cellReg.loading : stationReg.loading
  const error = kind === 'cell' ? cellReg.error : stationReg.error

  return (
    <div className="flex flex-wrap items-end gap-2" data-testid="target-picker">
      <Select
        label="대상 종류"
        dense={dense}
        value={kind}
        disabled={disabled || kinds.length <= 1}
        data-testid="target-kind"
        onValueChange={(v) => {
          const k = v as TargetKind
          setPickedKind(k)
          setQ('')
          // 종류가 바뀌면 id는 비운다 — 다른 종류의 id가 남아 있으면 거짓 대상이 된다.
          if (value && value.kind !== k) onChange(null)
        }}
      >
        {kinds.map((k) => (
          <option key={k} value={k}>
            {KIND_LABEL[k]}
          </option>
        ))}
      </Select>
      <Input
        label="검색"
        placeholder={kind === 'cell' ? 'id·구역·행·열' : 'id·컨베이어·그룹'}
        value={q}
        disabled={disabled}
        data-testid="target-search"
        onValueChange={setQ}
      />
      <Select
        label={KIND_LABEL[kind]}
        dense={dense}
        className="min-w-48"
        value={value ? String(value.id) : ''}
        disabled={disabled || loading}
        hint={error ? `목록 조회 실패 — ${error}` : loading ? '불러오는 중…' : `${shown.length}건`}
        data-testid="target-id"
        onValueChange={(v) => onChange(v === '' ? null : { kind, id: Number(v) })}
      >
        <option value="">(선택)</option>
        {picked ? <option value={String(picked.id)}>{picked.text}</option> : null}
        {shown.map((o) => (
          <option key={o.id} value={String(o.id)}>
            {o.text}
          </option>
        ))}
      </Select>
    </div>
  )
}
