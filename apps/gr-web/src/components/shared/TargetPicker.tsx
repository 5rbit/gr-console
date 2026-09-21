// 대상 선택 — **Select 하나**. 셀과 스테이션을 묶음(optgroup)으로 한 목록에 두고, 고른 항목이 종류까지 정한다.
//
// 전에는 종류 Select + 검색칸 + 목록 Select 셋이었다. 종류는 고른 대상이 이미 말하고, 검색은 목록이
// 수십 개인 지금 Select 의 타이핑 찾기(항목이 Id 로 시작한다)로 충분하다 — 한 번 고르면 되는 일을
// 세 칸에 나눠 물을 이유가 없었다. 목록 아래 `N건` 힌트도 줄의 아래선을 깨서 뺐다(오류만 남긴다).
//
// 셀 항목에는 재고(품목 × 개수)를 붙이고, 고른 셀의 재고·화물 규격은 아래 한 줄로 보여 준다 — PICK 할
// 셀을 고르는 순간 "무엇이 몇 개 있나"가 같이 보여야 한다. 재고는 `stock` 스토어(SSE)를 읽는다.
// 목록은 `useRegistry`로 스스로 받는다; 이미 받아 둔 것이 있으면 `cells`/`stations`로 넘겨 재조회를 피한다.
import { useEffect } from 'react'
import { api } from '../../lib/api'
import { itemLabel } from '../../lib/items/model'
import { useRegistry } from '../../lib/registry'
import { stock as stockStore } from '../../lib/stock'
import { useStore } from '../../lib/store'
import { Select } from '../../lib/ui/Select'
import type { Cell, Item, Station, StockEntry, Target, TargetKind } from '../../lib/types'
import { TARGET_NAME, USE_FALSE } from '../../lib/fieldNames'

export interface TargetPickerProps {
  value: Target | null
  onChange: (next: Target | null) => void
  /** 미리 받아 둔 목록(선택). 없으면 스스로 받는다. */
  cells?: Cell[]
  stations?: Station[]
  /** 재고 줄에 화물 규격을 붙일 품목 목록(선택). 없으면 ItemCode 만 보인다. */
  items?: readonly Item[]
  /** 고를 수 있는 종류(예: MEASURE는 셀만). */
  kinds?: TargetKind[]
  disabled?: boolean
  dense?: boolean
}

export function cellText(c: Cell): string {
  return `#${c.id} · S${c.section} R${c.row} C${c.col}${c.use ? '' : ` (${USE_FALSE})`}`
}
export function stationText(s: Station): string {
  return `#${s.id} · CV${s.conv_no} G${s.group}-${s.group_index}`
}

/** 셀 항목 한 줄 — Id 로 시작해야 Select 타이핑 찾기가 먹는다. 재고가 있으면 `품목 ×개수`. */
function cellOption(c: Cell, s: StockEntry | null): string {
  const base = `${c.id} · S${c.section} R${c.row} C${c.col}${c.use ? '' : ` (${USE_FALSE})`}`
  return s && s.count > 0 ? `${base} — ${s.item_code || '?'} ×${s.count}` : base
}
function stationOption(s: Station): string {
  return `${s.id} · CV${s.conv_no} G${s.group}-${s.group_index}`
}

/**
 * 셀을 고를 때 비어 있는 ItemCode 를 채울 값 — 그 셀 재고의 품목(없으면 null). 이미 고른 품목은 건드리지
 * 않는다(부르는 쪽이 `item_code ?? stockItemFor(t)` 로 쓴다).
 */
export function stockItemFor(t: Target | null): number | null {
  if (t?.kind !== 'cell') return null
  const s = stockStore.get(t.id)
  return s && s.count > 0 && s.item_code ? s.item_code : null
}

const key = (t: Target) => `${t.kind}:${t.id}`
function parseKey(v: string): Target | null {
  const [k, id] = v.split(':')
  if ((k !== 'cell' && k !== 'station') || !id) return null
  return { kind: k, id: Number(id) }
}

export function TargetPicker({
  value,
  onChange,
  cells,
  stations,
  items,
  kinds = ['cell', 'station'],
  disabled = false,
  dense = false,
}: TargetPickerProps) {
  // 훅은 조건 없이 부른다 — 넘겨받은 목록이 있으면 그것을 쓰고 조회 결과는 버린다.
  const cellReg = useRegistry<Cell>(cells ? async () => cells : api.cells)
  const stationReg = useRegistry<Station>(stations ? async () => stations : api.stations)
  const cellList = cells ?? cellReg.items
  const stationList = stations ?? stationReg.items
  useStore(stockStore)
  useEffect(() => stockStore.start(), [])

  const allowCell = kinds.includes('cell')
  const allowStation = kinds.includes('station')
  const loading = (allowCell && cellReg.loading) || (allowStation && stationReg.loading)
  const error = (allowCell && cellReg.error) || (allowStation && stationReg.error) || ''
  // 고른 것이 목록에 없어도(삭제됨·아직 안 옴) 선택이 빈 것처럼 보이면 안 된다.
  const missing =
    value &&
    !(value.kind === 'cell' ? cellList : stationList).some((x) => x.id === value.id) &&
    kinds.includes(value.kind)

  const sel = value?.kind === 'cell' ? cellList.find((c) => c.id === value.id) : undefined
  const st = sel ? stockStore.get(sel.id) : null
  const item = st?.item_code ? items?.find((i) => i.code === st.item_code) : undefined

  return (
    <div className="flex flex-col gap-1" data-testid="target-picker">
      <Select
        label={allowCell && allowStation ? 'Target' : TARGET_NAME[kinds[0] ?? 'cell']}
        dense={dense}
        className="min-w-64 self-start"
        value={value ? key(value) : ''}
        disabled={disabled || loading}
        hint={error ? `목록 조회 실패 — ${error}` : ''}
        data-testid="target-id"
        onValueChange={(v) => onChange(parseKey(v))}
      >
        <option value="">{loading ? '불러오는 중…' : '(선택)'}</option>
        {missing && value ? (
          <option value={key(value)}>{`${value.id} (목록에 없음)`}</option>
        ) : null}
        {allowCell && cellList.length ? (
          <optgroup label={TARGET_NAME.cell}>
            {cellList.map((c) => (
              <option key={`c${c.id}`} value={`cell:${c.id}`}>
                {cellOption(c, stockStore.get(c.id))}
              </option>
            ))}
          </optgroup>
        ) : null}
        {allowStation && stationList.length ? (
          <optgroup label={TARGET_NAME.station}>
            {stationList.map((s) => (
              <option key={`s${s.id}`} value={`station:${s.id}`}>
                {stationOption(s)}
              </option>
            ))}
          </optgroup>
        ) : null}
      </Select>
      {sel ? (
        <div className="text-2xs text-content-muted tabular-nums" data-testid="target-stock">
          {st && st.count > 0 ? (
            <>
              재고 <b className="text-content-primary">{st.count}개</b>
              {' · '}
              {item ? itemLabel(item) : st.item_code ? `ItemCode ${st.item_code}` : 'ItemCode 없음'}
            </>
          ) : (
            '재고 없음'
          )}
        </div>
      ) : null}
    </div>
  )
}
