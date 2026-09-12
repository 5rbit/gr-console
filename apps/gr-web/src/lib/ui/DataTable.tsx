// 표 — 정렬·행 클릭·행 액션·빈 상태를 한 곳에.
//
// 컬럼이 렌더를 소유한다: 값 포맷(시각·배지·진행률)은 컬럼 정의의 `cell`이 정하고,
// 표는 배치와 상호작용만 안다.
import { useMemo, useState } from 'react'
import type { ReactNode } from 'react'
import { ChevronDown, ChevronUp } from 'lucide-react'
import type { Column } from './table'
import { cn } from '../utils'
import { EmptyState } from './EmptyState'
import { Skeleton } from './Skeleton'

export interface DataTableProps<T> {
  rows: T[]
  columns: Column<T>[]
  /** 행의 안정적 식별자 — 없으면 인덱스를 쓰지만 정렬 시 DOM이 튄다 */
  rowKey?: (row: T) => string
  /** 행 클릭 — 있으면 행이 버튼처럼 동작한다(드릴다운) */
  onPick?: (row: T) => void
  /** 선택된 행의 키 — 하이라이트 */
  selected?: string | null
  /** 행 끝 액션 열(선택) */
  actions?: (row: T) => ReactNode
  empty?: string
  emptyHint?: string
  loading?: boolean
  testid?: string
}

export function DataTable<T>({
  rows,
  columns,
  rowKey,
  onPick,
  selected,
  actions,
  empty = '기록 없음',
  emptyHint,
  loading = false,
  testid,
}: DataTableProps<T>) {
  const [sortKey, setSortKey] = useState<string | null>(null)
  const [desc, setDesc] = useState(false)

  const sorted = useMemo(() => {
    const col = columns.find((c) => c.key === sortKey)
    if (!col?.get) return rows
    const g = col.get
    // **정렬은 원본을 건드리지 않는다** — 폴링이 배열을 갈아끼우는 화면이라 부작용이 곧 깜빡임이다.
    return [...rows].sort((a, b) => {
      const x = g(a)
      const y = g(b)
      // 빈 값은 **항상 뒤로**. 정렬 방향에 따라 위아래로 튀면 "없는 것"이 데이터인 양 보인다.
      if (x === null || x === undefined) return y === null || y === undefined ? 0 : 1
      if (y === null || y === undefined) return -1
      const c =
        typeof x === 'number' && typeof y === 'number' ? x - y : String(x).localeCompare(String(y))
      return desc ? -c : c
    })
  }, [rows, columns, sortKey, desc])

  function toggle(c: Column<T>): void {
    if (!(c.sortable ?? !!c.get)) return
    if (sortKey === c.key) setDesc((d) => !d)
    else {
      setSortKey(c.key)
      setDesc(false)
    }
  }
  function keyOf(row: T, i: number): string {
    return rowKey ? rowKey(row) : String(i)
  }

  if (loading && rows.length === 0) {
    return (
      <div className="space-y-2 p-2">
        {Array.from({ length: 5 }, (_, i) => i).map((i) => (
          <Skeleton key={i} />
        ))}
      </div>
    )
  }

  if (rows.length === 0) return <EmptyState title={empty} hint={emptyHint} />

  return (
    <div className="overflow-x-auto">
      <table className="w-full text-xs" data-testid={testid}>
        <thead>
          <tr className="text-left text-slate-600 dark:text-slate-300">
            {columns.map((c) => {
              const can = c.sortable ?? !!c.get
              return (
                <th
                  key={c.key}
                  className={cn(
                    'px-2 py-1 font-medium whitespace-nowrap',
                    c.class ?? '',
                    c.numeric && 'text-right',
                  )}
                >
                  {can ? (
                    <button
                      className="inline-flex items-center gap-1 hover:text-slate-900 dark:hover:text-slate-100"
                      onClick={() => toggle(c)}
                      aria-label={`${c.label} 정렬`}
                    >
                      {c.label}
                      {sortKey === c.key &&
                        (desc ? <ChevronDown size={12} /> : <ChevronUp size={12} />)}
                    </button>
                  ) : (
                    c.label
                  )}
                </th>
              )
            })}
            {actions && (
              <th className="px-2 py-1">
                <span className="sr-only">조작</span>
              </th>
            )}
          </tr>
        </thead>
        <tbody>
          {sorted.map((row, i) => {
            const k = keyOf(row, i)
            return (
              <tr
                key={k}
                className={cn(
                  'border-t border-slate-200 dark:border-slate-700',
                  !!onPick && 'cursor-pointer',
                  selected === k && 'bg-indigo-50 dark:bg-indigo-950',
                )}
                data-testid="dt-row"
              >
                {columns.map((c) => (
                  <td
                    key={c.key}
                    className={cn(
                      'px-2 py-1 whitespace-nowrap',
                      c.class ?? '',
                      c.numeric && 'tabular-nums text-right',
                    )}
                    onClick={() => onPick?.(row)}
                  >
                    {c.cell ? c.cell(row) : (c.get?.(row) ?? '—')}
                  </td>
                ))}
                {actions && <td className="px-2 py-1 text-right">{actions(row)}</td>}
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}
