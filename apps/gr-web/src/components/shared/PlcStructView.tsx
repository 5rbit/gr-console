// PLC UDT 구조 표 — 멤버 · 타입 · 값 (· 주석). 셀 정보 패널과 계획 스텝 펼침에서 쓴다.
//
// 표는 킷의 `DataTable` 이다(손으로 짠 `<table>` 이었다): 좁은 패널에서 주석·타입이 접히고, 값이
// 없을 때 빈 상자 대신 빈 상태가 서고, 정렬이 필요하면 머리글이 이미 그것을 안다.
// 값의 소수는 `lib/meas/format.pos`(위치·치수 1자리) 한 벌을 쓴다 — 여기만 제 규칙을 갖고 있었다.
import { pos } from '../../lib/meas/format'
import { DataTable } from '../../lib/ui/DataTable'
import type { Column } from '../../lib/ui/table'
import type { PlcRow } from '../../lib/gr/plcShape'

function valueText(v: PlcRow['value']): string {
  if (typeof v === 'boolean') return v ? 'TRUE' : 'FALSE'
  if (typeof v === 'number') return Number.isInteger(v) ? String(v) : pos(v)
  return String(v ?? '')
}

// 멤버·값이 1(행을 알아보는 것), 타입이 2, 주석이 3 — 좁은 패널에서는 접히고 행을 펼치면 나온다.
const COLS: Column<PlcRow>[] = [
  { key: 'member', label: 'Member', get: (r) => r.member, class: 'font-mono', priority: 1 },
  { key: 'type', label: 'Type', get: (r) => r.type, priority: 2 },
  { key: 'value', label: 'Value', get: (r) => valueText(r.value), numeric: true, priority: 1 },
  { key: 'comment', label: 'Comment', get: (r) => r.comment ?? '', priority: 3 },
]

export function PlcStructView({
  rows,
  title,
}: {
  rows: readonly PlcRow[]
  title?: string
}) {
  return (
    <div className="rounded border border-line-default" data-testid="plc-struct">
      {title ? (
        <div className="border-b border-line-default bg-surface-panel px-2 py-1 font-mono text-2xs font-semibold">
          {title}
        </div>
      ) : null}
      <DataTable
        rows={[...rows]}
        columns={COLS}
        rowKey={(r) => r.member}
        density="compact"
        empty="멤버 없음"
      />
    </div>
  )
}
