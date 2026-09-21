// 레지스트리 레일 — **맵 + 표 한 면이 기본 화면**이고, 머리줄 하나가 "맵 옆에 어떤 표를 둘지"만 고른다.
// 화면 왼쪽을 차지한다.
//
// 전에는 탭이 여섯이었다(레이아웃+표 · 레이아웃 · 재고 · 셀 · 스테이션 · 품목). 그런데 `레이아웃`은
// `레이아웃+표`에서 표를 접은 것일 뿐이고, `셀`·`스테이션`·`재고`·`품목`은 나눠 보기 안 표 토글과
// **같은 네 가지**였다 — 같은 선택을 두 줄에서 두 번 물었고, 표 머리줄에는 그 둘을 오가는 버튼이
// 또 있었다. 그래서 줄을 하나로 합쳤다: 표 토글 넷 + 검색 + ⋯.
//
// 접고 펴는 일(맵만 · 표만 · 나눠 보기)과 나누는 방향은 ⋯ 안으로 들어갔고, 고른 값은 그대로
// 저장돼 다음에 열 때 같은 자리에서 시작한다(`gr-rail-tab` · `gr-rail-split`). 저장 값의 뜻은
// 예전과 같다 — `split`=둘 다, `layout`=맵만, 표 이름=그 표만.
//
// 맵 면은 나눠 보기와 맵만 사이에서 **같은 자리**에 남아 다시 마운트되지 않는다 — 맵의 확대·이동이
// 보기를 오갈 때 풀리지 않는다.
import { useEffect, useRef, useState, type ReactNode } from 'react'
import type { Registry } from '../../lib/registry'
import {
  DEFAULT_SPLIT,
  SPLIT_TABLES,
  parseRailTab,
  parseSplit,
  pxToRatio,
  ratioToPx,
  serializeSplit,
  splitBounds,
  tableForPick,
  type RailTab,
  type SplitState,
  type SplitTable,
} from '../../lib/task/railSplitModel'
import { ErrorBoundary } from '../../lib/ui/ErrorBoundary'
import { Input } from '../../lib/ui/Input'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import { menuItems, type MenuEntry } from '../../lib/task/menuEntries'
import { Segmented } from '../../lib/ui/Segmented'
import { cn } from '../../lib/utils'
import type { Cell, Item, Station, Target } from '../../lib/types'
import { Splitter } from '../workspace/Splitter'
import { CellRegistry } from './CellRegistry'
import { ItemRegistry } from './ItemRegistry'
import { StationRegistry } from './StationRegistry'
import { StockRegistry } from './StockRegistry'

export type { RailTab } from '../../lib/task/railSplitModel'

const TABLE_LABEL: Record<SplitTable, string> = {
  cell: '셀',
  station: '스테이션',
  stock: '재고',
  item: '품목',
}
/** 레일 탭의 저장 키 — 화면 루트(`TaskIssue`)도 이 값으로 복원한다(두 벌을 두면 한쪽만 고쳐진다). */
export const RAIL_KEY = 'gr-rail-tab'
const SPLIT_KEY = 'gr-rail-split'

const placeholderOf = (t: SplitTable) =>
  t === 'item'
    ? 'Code·Name'
    : t === 'cell' || t === 'stock'
      ? 'Id·Section·Row·Col'
      : 'Id·ConvNo·Group'

function SearchBox({
  table,
  q,
  onChange,
  className,
}: {
  table: SplitTable
  q: string
  onChange: (q: string) => void
  className?: string
}) {
  return (
    <Input
      className={className}
      placeholder={placeholderOf(table)}
      value={q}
      onValueChange={onChange}
      aria-label={`${TABLE_LABEL[table]} 검색`}
      data-testid="rail-search"
    />
  )
}

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
  /** 셀/스테이션 표의 선택 — 바깥이 쥐면 맵 강조와 한 상태가 된다. */
  selected?: Target | null
  onSelect?: (t: Target | null) => void
  /** 맵에서 고른 대상 — nonce 가 바뀔 때마다 나눠 보기의 표를 그 종류로 옮긴다. */
  reveal?: { target: Target; nonce: number } | null
}

export function RegistryRail({
  items,
  cells,
  stations,
  layout,
  tab: tabProp,
  onTabChange,
  selected,
  onSelect,
  reveal,
}: RegistryRailProps) {
  const [tabLocal, setTab] = useState<RailTab>(() => {
    try {
      return parseRailTab(localStorage.getItem(RAIL_KEY))
    } catch {
      return 'split'
    }
  })
  const tab = tabProp ?? tabLocal
  const [q, setQ] = useState('')
  const [sp, setSp] = useState<SplitState>(() => {
    try {
      return parseSplit(localStorage.getItem(SPLIT_KEY))
    } catch {
      return { ...DEFAULT_SPLIT }
    }
  })
  const split = tab === 'split'
  const mapOnly = tab === 'layout'
  const side = sp.orient === 'side'
  const showMap = split || mapOnly
  // 표만 보기일 때는 탭 자체가 표 이름이다 — 머리줄 토글은 늘 "지금 보는 표"를 가리킨다.
  const table: SplitTable = showMap ? sp.table : (tab as SplitTable)

  const go = (t: RailTab) => {
    setTab(t)
    onTabChange?.(t)
    try {
      localStorage.setItem(RAIL_KEY, t)
    } catch {
      /* 저장 못 해도 동작 */
    }
  }
  const saveSplit = (next: SplitState) => {
    setSp(next)
    try {
      localStorage.setItem(SPLIT_KEY, serializeSplit(next))
    } catch {
      /* 저장 못 해도 동작 */
    }
  }
  /**
   * 표 토글 — 맵만 보던 중이면 표를 다시 펴면서 그 표로 간다(한 번 눌러 돌아온다).
   * 어느 보기에서 골랐든 `sp.table`에 적어 둔다: 표만 보기에서 고른 표가 나눠 보기로 돌아올 때
   * 그대로 남아야 "내가 보던 표"가 유지된다.
   */
  const pickTable = (t: SplitTable) => {
    saveSplit({ ...sp, table: t })
    if (mapOnly) go('split')
    else if (!split) go(t)
  }

  // 맵에서 셀·스테이션을 누르면 표를 그 종류로. 자동으로 따라간 것이라 저장하지 않는다(사용자가 고른
  // 표 토글만 새로고침에 남는다).
  useEffect(() => {
    if (!reveal) return
    setSp((s) => {
      const t = tableForPick(s.table, reveal.target.kind)
      return t === s.table ? s : { ...s, table: t }
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps -- nonce 가 요청 한 번을 대표한다
  }, [reveal?.nonce])

  // 스플리터는 px 를 말한다 — 나누는 방향의 컨테이너 길이를 재서 비율과 오간다.
  const box = useRef<HTMLDivElement>(null)
  const [total, setTotal] = useState(0)
  useEffect(() => {
    const el = box.current
    if (!el || !split) return
    const measure = () => {
      const r = el.getBoundingClientRect()
      setTotal(side ? r.width : r.height)
    }
    measure()
    if (typeof ResizeObserver === 'undefined') {
      window.addEventListener('resize', measure)
      return () => window.removeEventListener('resize', measure)
    }
    const ro = new ResizeObserver(measure)
    ro.observe(el)
    return () => ro.disconnect()
  }, [split, side])
  const bounds = splitBounds(total)

  const cellSel =
    selected === undefined ? undefined : selected?.kind === 'cell' ? selected.id : null
  const stationSel =
    selected === undefined ? undefined : selected?.kind === 'station' ? selected.id : null
  const pick = (kind: Target['kind']) =>
    onSelect ? (id: number | null) => onSelect(id === null ? null : { kind, id }) : undefined

  function body(t: SplitTable, compact: boolean) {
    const needle = q.trim()
    if (t === 'item') return <ItemRegistry reg={items} q={needle} />
    if (t === 'cell')
      return (
        <CellRegistry
          reg={cells}
          q={needle}
          selectedId={cellSel}
          onSelect={pick('cell')}
          compact={compact}
        />
      )
    if (t === 'station')
      return (
        <StationRegistry
          reg={stations}
          q={needle}
          selectedId={stationSel}
          onSelect={pick('station')}
          compact={compact}
        />
      )
    return (
      <StockRegistry
        cells={cells.items}
        items={items.items}
        q={needle}
        onItemsChanged={() => void items.reload()}
      />
    )
  }

  const viewMenu: MenuEntry[] = [
    { label: '보기' },
    {
      label: '맵과 표 나눠 보기',
      hint: split ? '지금' : undefined,
      run: () => go('split'),
    },
    { label: '맵만 보기', hint: mapOnly ? '지금' : undefined, run: () => go('layout') },
    {
      label: `${TABLE_LABEL[table]} 표만 보기`,
      hint: !showMap ? '지금' : undefined,
      run: () => go(table),
    },
    { label: '나누는 방향' },
    {
      label: '위아래 — 맵 위, 표 아래',
      hint: sp.orient === 'stack' ? '지금' : undefined,
      run: () => saveSplit({ ...sp, orient: 'stack' }),
      disabled: split ? undefined : '나눠 보기에서만',
    },
    {
      label: '좌우 — 맵 왼쪽, 표 오른쪽',
      hint: sp.orient === 'side' ? '지금' : undefined,
      run: () => saveSplit({ ...sp, orient: 'side' }),
      disabled: split ? undefined : '나눠 보기에서만',
    },
  ]

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="registry-rail">
      <div className="flex min-h-screen-header flex-none items-center gap-2 border-b border-line-default px-2 py-1">
        <Segmented
          ariaLabel="표 종류"
          value={table}
          onChange={pickTable}
          options={SPLIT_TABLES.map((t) => ({
            id: t,
            label: TABLE_LABEL[t],
            testid: `rail-${t}`,
          }))}
        />
        <SearchBox table={table} q={q} onChange={setQ} className="w-44 flex-initial" />
        <span className="flex-1" />
        <OverflowMenu
          items={menuItems(viewMenu)}
          title="보기 — 맵만 · 표만 · 나누는 방향"
          testid="rail-more"
        />
      </div>
      <div className="min-h-0 flex-1">
        {showMap ? (
          <div
            ref={box}
            className={cn('flex h-full min-h-0', side ? 'flex-row' : 'flex-col')}
            data-testid="rail-split"
          >
            <div
              className={cn('relative min-h-0 min-w-0', split ? 'flex-none' : 'flex-1')}
              style={split ? { flexBasis: `${sp.ratio * 100}%` } : undefined}
              data-testid="rail-map-pane"
            >
              <div className="absolute inset-0">
                <ErrorBoundary label="레이아웃 맵">{layout}</ErrorBoundary>
              </div>
            </div>
            {split ? (
              <Splitter
                axis={side ? 'col' : 'row'}
                size={ratioToPx(sp.ratio, total)}
                min={bounds.min}
                max={bounds.max}
                label="맵과 표 경계"
                onResize={(px) => setSp((s) => ({ ...s, ratio: pxToRatio(px, total, s.ratio) }))}
                onCommit={(px) => saveSplit({ ...sp, ratio: pxToRatio(px, total, sp.ratio) })}
              />
            ) : null}
            {split ? (
              <section
                className="flex min-h-0 min-w-0 flex-1 flex-col"
                aria-label="표"
                data-testid="rail-table-pane"
              >
                <ErrorBoundary label="표" resetKey={sp.table}>
                  {body(sp.table, side)}
                </ErrorBoundary>
              </section>
            ) : null}
          </div>
        ) : (
          <ErrorBoundary label="표" resetKey={tab}>
            {body(tab as SplitTable, false)}
          </ErrorBoundary>
        )}
      </div>
    </div>
  )
}
