// 품목 선택 — 코드·이름으로 고른다. 목록은 `useRegistry`로 스스로 받는다(넘겨받으면 그것을 쓴다).
import { api } from '../../lib/api'
import { useRegistry } from '../../lib/registry'
import { Select } from '../../lib/ui/Select'
import type { Item } from '../../lib/types'

export interface ItemPickerProps {
  value: number | null
  onChange: (code: number | null) => void
  /** 미리 받아 둔 목록(선택). */
  items?: Item[]
  label?: string
  disabled?: boolean
  dense?: boolean
  /** "(없음)" 항목을 둘지 — 품목이 필수인 자리(PICK)는 끈다. */
  allowNone?: boolean
}

export function ItemPicker({
  value,
  onChange,
  items,
  label = 'ItemCode',
  disabled = false,
  dense = false,
  allowNone = true,
}: ItemPickerProps) {
  const reg = useRegistry<Item>(items ? async () => items : api.items)
  const list = items ?? reg.items
  // 고른 코드가 목록에 없어도(삭제됨·아직 안 옴) 선택이 빈 것처럼 보이면 안 된다.
  const missing = value !== null && !list.some((i) => i.code === value)

  return (
    <Select
      label={label}
      dense={dense}
      className="min-w-48"
      value={value === null ? '' : String(value)}
      disabled={disabled || (!items && reg.loading)}
      hint={reg.error && !items ? `목록 조회 실패 — ${reg.error}` : ''}
      onValueChange={(v) => onChange(v === '' ? null : Number(v))}
      data-testid="item-picker"
    >
      {allowNone || value === null ? <option value="">(없음)</option> : null}
      {missing ? <option value={String(value)}>{`#${value} (목록에 없음)`}</option> : null}
      {list.map((i) => (
        <option key={i.code} value={String(i.code)}>
          {`${i.code} · ${i.name}`}
        </option>
      ))}
    </Select>
  )
}
