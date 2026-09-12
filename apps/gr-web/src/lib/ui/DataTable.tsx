// 표 — 정렬·행 클릭·행 액션·빈 상태·**좁은 폭에서 열 접기**를 한 곳에.
//
// 컬럼이 렌더를 소유한다: 값 포맷(시각·배지·진행률)은 컬럼 정의의 `cell`이 정하고,
// 표는 배치와 상호작용만 안다.
//
// 열 접기(`Column.priority`): 표가 든 **컨테이너 폭**이 좁으면 우선순위가 낮은 열을 내린다. 접힌
// 열은 **행 펼치기**로 되살아난다(라벨+값 짝 — 이 콘솔의 요약 규칙과 같은 모양). 접고 끝내면
// 좁은 자리에서는 값이 없는 표가 되고, 가로 스크롤로 두면 오른쪽 끝 열이 없는 것과 같다.
// 판정은 순수 함수(`table.ts`의 `columnLevel`·`splitColumns`)가 하고 테스트가 못 박아 둔다.
import { Fragment, useMemo, useState } from 'react'
import type { ReactNode } from 'react'
import { ChevronDown, ChevronRight, ChevronUp } from 'lucide-react'
import { columnLevel, splitColumns, type Column } from './table'
import { cn } from '../utils'
import { EmptyState } from './EmptyState'
import { Skeleton } from './Skeleton'
import { useElementWidth } from './useElementWidth'

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
  /** 펼친 행의 키 — 접힌 열을 보려고 연 행들. */
  const [open, setOpen] = useState<ReadonlySet<string>>(() => new Set())

  // 콜백 ref다 — 표는 로딩·빈 상태를 거쳐 **뒤늦게** 마운트될 수 있고, 그때 관찰이 다시 걸려야 한다.
  const [box, width] = useElementWidth<HTMLDivElement>()
  const { visible, hidden } = useMemo(
    () => splitColumns(columns, columnLevel(width)),
    [columns, width],
  )

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
  function toggleRow(k: string): void {
    setOpen((prev) => {
      const next = new Set(prev)
      if (next.has(k)) next.delete(k)
      else next.add(k)
      return next
    })
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

  /** 펼치기 손잡이 열 + 보이는 열 + 액션 열 — 자식 행의 `colSpan`이 이 수를 쓴다. */
  const span = visible.length + (hidden.length > 0 ? 1 : 0) + (actions ? 1 : 0)

  return (
    <div className="overflow-x-auto" ref={box}>
      <table className="w-full text-xs" data-testid={testid}>
        <thead>
          <tr className="text-left text-slate-600 dark:text-slate-300">
            {hidden.length > 0 ? (
              <th className="w-5 px-1 py-1">
                <span className="sr-only">접힌 열 펼치기</span>
              </th>
            ) : null}
            {visible.map((c) => {
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
              <Fragment key={k}>
              <tr
                className={cn(
                  'border-t border-slate-200 dark:border-slate-700',
                  !!onPick && 'cursor-pointer',
                  selected === k && 'bg-indigo-50 dark:bg-indigo-950',
                )}
                data-testid="dt-row"
              >
                {hidden.length > 0 ? (
                  // 손잡이는 행 클릭(드릴다운)과 **갈라야 한다** — 값을 더 보려고 눌렀는데 상세가
                  // 열리면 방금 보던 자리를 잃는다.
                  <td className="px-1 py-1 align-top">
                    <button
                      type="button"
                      className="rounded p-0.5 text-slate-400 hover:bg-slate-200 hover:text-slate-700 dark:hover:bg-slate-700"
                      aria-expanded={open.has(k)}
                      aria-label={open.has(k) ? '접힌 열 숨기기' : `접힌 열 ${hidden.length}개 보기`}
                      title={`좁아서 접힌 열 ${hidden.length}개 — ${hidden.map((c) => c.label).join(' · ')}`}
                      data-testid="dt-expand"
                      onClick={(e) => {
                        e.stopPropagation()
                        toggleRow(k)
                      }}
                    >
                      {open.has(k) ? (
                        <ChevronDown className="h-3 w-3" />
                      ) : (
                        <ChevronRight className="h-3 w-3" />
                      )}
                    </button>
                  </td>
                ) : null}
                {visible.map((c) => (
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
              {hidden.length > 0 && open.has(k) ? (
                <tr className="bg-slate-50 dark:bg-slate-800/40" data-testid="dt-row-detail">
                  <td colSpan={span} className="px-2 py-1.5">
                    {/* 접힌 값은 **라벨+값 짝**으로 — 표의 머리글이 없으니 각 값이 자기 이름을 들고
                        있어야 한다(`docs/DESIGN.md` 4절 ②와 같은 규칙). */}
                    <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-[11px] sm:grid-cols-[auto_1fr_auto_1fr]">
                      {hidden.map((c) => (
                        <Fragment key={c.key}>
                          <dt className="text-slate-400">{c.label}</dt>
                          <dd className={cn('min-w-0', c.numeric && 'tabular-nums')}>
                            {c.cell ? c.cell(row) : (c.get?.(row) ?? '—')}
                          </dd>
                        </Fragment>
                      ))}
                    </dl>
                  </td>
                </tr>
              ) : null}
              </Fragment>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}
