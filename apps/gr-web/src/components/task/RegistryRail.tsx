// 레지스트리 레일 — 품목 | 셀 | 스테이션 세그먼트 + 검색 + 해당 표. 화면 왼쪽을 차지한다.
import { useState } from 'react'
import { Search } from 'lucide-react'
import type { Registry } from '../../lib/registry'
import { cn } from '../../lib/utils'
import type { Cell, Item, Station, Target } from '../../lib/types'
import { CellMap } from './CellMap'
import type { Shape } from '../../lib/task/layoutModel'
import { CellRegistry } from './CellRegistry'
import { ItemRegistry } from './ItemRegistry'
import { StationRegistry } from './StationRegistry'

export type RailTab = 'item' | 'cell' | 'station' | 'layout'

const TABS: { id: RailTab; label: string }[] = [
  { id: 'item', label: '품목' },
  { id: 'cell', label: '셀' },
  { id: 'station', label: '스테이션' },
  { id: 'layout', label: '레이아웃' },
]

export interface RegistryRailProps {
  items: Registry<Item>
  cells: Registry<Cell>
  stations: Registry<Station>
  /** 작성 카드에 잡힌 대상(레이아웃 강조용). */
  selected?: Target | null
  /** 레이아웃에서 도형을 눌렀을 때. */
  onPick?: (t: Target, shape: Shape) => void
}

export function RegistryRail({
  items,
  cells,
  stations,
  selected = null,
  onPick,
}: RegistryRailProps) {
  const [tab, setTab] = useState<RailTab>('cell')
  const [q, setQ] = useState('')
  const count = (t: RailTab) =>
    (t === 'item'
      ? items
      : t === 'cell'
        ? cells
        : t === 'station'
          ? stations
          : { items: [...cells.items, ...stations.items] }
    ).items.length
  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="registry-rail">
      <div className="flex flex-none items-center gap-2 border-b border-slate-200 px-2 py-1.5 dark:border-slate-700">
        <div
          className="inline-flex rounded-md border border-slate-300 p-0.5 dark:border-slate-600"
          role="tablist"
          aria-label="레지스트리"
        >
          {TABS.map((t) => (
            <button
              key={t.id}
              type="button"
              role="tab"
              aria-selected={tab === t.id}
              data-testid={`rail-${t.id}`}
              className={cn(
                'rounded px-2.5 py-0.5 text-xs transition-colors',
                tab === t.id
                  ? 'bg-indigo-600 text-white'
                  : 'text-slate-600 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800',
              )}
              onClick={() => setTab(t.id)}
            >
              {t.label}{' '}
              <span className={cn('tabular-nums', tab === t.id ? 'opacity-80' : 'text-slate-400')}>
                {count(t.id)}
              </span>
            </button>
          ))}
        </div>
        {tab !== 'layout' ? (
          <label className="relative ml-auto flex items-center">
            <Search className="pointer-events-none absolute left-2 h-3.5 w-3.5 text-slate-400" />
            <input
              className="h-7 w-44 rounded-md border border-slate-300 bg-transparent pr-2 pl-7 text-xs focus-visible:border-focus focus-visible:ring-2 focus-visible:ring-focus focus-visible:outline-none dark:border-slate-600"
              placeholder={
                tab === 'item' ? '코드·이름' : tab === 'cell' ? 'id·구역·행·열' : 'id·컨베이어·그룹'
              }
              value={q}
              onChange={(e) => setQ(e.currentTarget.value)}
              data-testid="rail-search"
            />
          </label>
        ) : null}
      </div>
      <div className="min-h-0 flex-1">
        {tab === 'item' ? (
          <ItemRegistry reg={items} q={q.trim()} />
        ) : tab === 'cell' ? (
          <CellRegistry reg={cells} q={q.trim()} />
        ) : tab === 'station' ? (
          <StationRegistry reg={stations} q={q.trim()} />
        ) : (
          <CellMap
            cells={cells.items}
            stations={stations.items}
            selected={selected}
            onPick={(t, s) => onPick?.(t, s)}
          />
        )}
      </div>
    </div>
  )
}
