// 레이아웃 편집 모드의 우측 사이드바 — 셀 / 스테이션 그리드 편집기(엑셀형)와 생성 규칙.
// 맵에서 도형을 누르면 그리드 행이 선택되고, 행을 누르면 맵이 그 도형으로 이동한다.
// 그리드에서 고친 값은 "적용" 전에도 맵에 바로 그려진다(onCellDraft / onStationDraft).
// 생성 규칙 탭은 숨겨도 마운트를 유지해 맵의 생성 예정 셀이 남는다(편집 모드를 나가면 사라진다).
import { useState } from 'react'
import { Wand2 } from 'lucide-react'
import type { Registry } from '../../lib/registry'
import { Input } from '../../lib/ui/Input'
import { Segmented } from '../../lib/ui/Segmented'
import type { Cell, CellUpsert, Station, Target } from '../../lib/types'
import { CellGridEditor } from './CellGridEditor'
import { LayoutEditor } from './LayoutEditor'
import { StationGridEditor } from './StationGridEditor'

export type SideTab = 'cell' | 'station' | 'rule'

export interface LayoutSidePanelProps {
  cells: Registry<Cell>
  stations: Registry<Station>
  tab: SideTab
  onTabChange: (t: SideTab) => void
  selected: Target | null
  onSelect: (t: Target | null) => void
  onPreview: (cells: CellUpsert[]) => void
  previewCount: number
  /** 셀 그리드 초안(저장 전) — 맵 미리보기. */
  onCellDraft: (cells: Cell[] | null) => void
  /** 스테이션 그리드 초안(저장 전) — 맵 미리보기. */
  onStationDraft: (stations: Station[] | null) => void
}

export function LayoutSidePanel({
  cells,
  stations,
  tab,
  onTabChange,
  selected,
  onSelect,
  onPreview,
  previewCount,
  onCellDraft,
  onStationDraft,
}: LayoutSidePanelProps) {
  const [q, setQ] = useState('')
  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="layout-side">
      <div className="flex min-h-screen-header flex-none items-center gap-2 border-b border-line-default px-3 py-1">
        <Wand2 className="h-4 w-4 text-content-muted" />
        <span className="text-sm font-semibold">레이아웃 편집</span>
        <span className="flex-1" />
        <Segmented
          ariaLabel="편집 목록"
          value={tab}
          onChange={onTabChange}
          options={[
            { id: 'cell', label: '셀', badge: cells.items.length, testid: 'side-tab-cell' },
            {
              id: 'station',
              label: '스테이션',
              badge: stations.items.length,
              testid: 'side-tab-station',
            },
            { id: 'rule', label: '생성 규칙', badge: previewCount || '', testid: 'side-tab-rule' },
          ]}
        />
      </div>
      {tab !== 'rule' ? (
        <div className="flex min-h-screen-header flex-none items-center border-b border-line-default px-3 py-1">
          <Input
            className="w-full"
            placeholder={tab === 'cell' ? 'Id·Section·Row·Col' : 'Id·ConvNo·Group'}
            value={q}
            onValueChange={setQ}
            aria-label={tab === 'cell' ? '셀 검색' : '스테이션 검색'}
            data-testid="side-search"
          />
        </div>
      ) : null}
      <div className="min-h-0 flex-1">
        {tab === 'cell' ? (
          <CellGridEditor
            reg={cells}
            q={q}
            selectedId={selected?.kind === 'cell' ? selected.id : null}
            onSelect={(id) => onSelect(id === null ? null : { kind: 'cell', id })}
            onDraft={onCellDraft}
          />
        ) : null}
        {tab === 'station' ? (
          <StationGridEditor
            reg={stations}
            q={q}
            selectedId={selected?.kind === 'station' ? selected.id : null}
            onSelect={(id) => onSelect(id === null ? null : { kind: 'station', id })}
            onDraft={onStationDraft}
          />
        ) : null}
        <div className={tab === 'rule' ? 'h-full overflow-y-auto' : 'hidden'}>
          <LayoutEditor
            embedded
            cells={cells.items}
            onPreview={onPreview}
            onApplied={() => void cells.reload()}
          />
        </div>
      </div>
    </div>
  )
}
