// 레지스트리 레일 — 품목 | 셀 | 스테이션 | 재고 | 레이아웃 세그먼트 + 검색 + 해당 표/맵. 화면 왼쪽을 차지한다.
import { useState, type ReactNode } from 'react'
import { Search } from 'lucide-react'
import type { Registry } from '../../lib/registry'
import { Segmented } from '../../lib/ui/Segmented'
import type { Cell, Item, Station } from '../../lib/types'
import { CellRegistry } from './CellRegistry'
import { ItemRegistry } from './ItemRegistry'
import { StationRegistry } from './StationRegistry'
import { StockRegistry } from './StockRegistry'

export type RailTab = 'item' | 'cell' | 'station' | 'stock' | 'layout'

const TABS: { id: RailTab; label: string }[] = [
  { id: 'layout', label: '레이아웃' },
  { id: 'stock', label: '재고' },
  { id: 'cell', label: '셀' },
  { id: 'station', label: '스테이션' },
  { id: 'item', label: '품목' },
]
const RAIL_KEY = 'gr-rail-tab'

export interface RegistryRailProps {
  items: Registry<Item>
  cells: Registry<Cell>
  stations: Registry<Station>
  /** 레이아웃 탭 내용(맵·편집기·정보 패널) — 상태는 화면 루트가 든다. */
  layout: ReactNode
  /** 셀 탭을 열고 특정 셀 편집 폼을 띄우라는 요청(레이아웃 팔레트 → 셀 편집). */
  editCell?: Cell | null
  /** 바깥이 탭을 쥘 때(맵 모드와 사이드바 연동). */
  tab?: RailTab
  onTabChange?: (t: RailTab) => void
}

export function RegistryRail({
  items,
  cells,
  stations,
  layout,
  tab: tabProp,
  onTabChange,
}: RegistryRailProps) {
  const [tabLocal, setTab] = useState<RailTab>(() => {
    try {
      return (localStorage.getItem(RAIL_KEY) as RailTab) || 'layout'
    } catch {
      return 'layout'
    }
  })
  const tab = tabProp ?? tabLocal
  const [q, setQ] = useState('')
  const count = (t: RailTab) =>
    t === 'item'
      ? items.items.length
      : t === 'cell' || t === 'stock'
        ? cells.items.length
        : t === 'station'
          ? stations.items.length
          : cells.items.length + stations.items.length
  const go = (t: RailTab) => {
    setTab(t)
    onTabChange?.(t)
    try {
      localStorage.setItem(RAIL_KEY, t)
    } catch {
      /* 저장 못 해도 동작 */
    }
  }
  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="registry-rail">
      <div className="flex h-10 flex-none items-center gap-2 border-b border-slate-200 px-2 dark:border-slate-700">
        <Segmented
          ariaLabel="레지스트리"
          value={tab}
          onChange={go}
          options={TABS.map((t) => ({
            id: t.id,
            label: t.label,
            badge: count(t.id),
            testid: `rail-${t.id}`,
          }))}
        />
        {tab !== 'layout' ? (
          <label className="relative ml-auto flex items-center">
            <Search className="pointer-events-none absolute left-2 h-3.5 w-3.5 text-slate-400" />
            <input
              className="h-7 w-44 rounded-md border border-slate-300 bg-transparent pr-2 pl-7 text-xs focus-visible:border-focus focus-visible:ring-2 focus-visible:ring-focus focus-visible:outline-none dark:border-slate-600"
              placeholder={
                tab === 'item'
                  ? '코드·이름'
                  : tab === 'cell' || tab === 'stock'
                    ? 'id·구역·행·열'
                    : 'id·컨베이어·그룹'
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
        ) : tab === 'stock' ? (
          <StockRegistry
            cells={cells.items}
            items={items.items}
            q={q.trim()}
            onItemsChanged={() => void items.reload()}
          />
        ) : (
          layout
        )}
      </div>
    </div>
  )
}
