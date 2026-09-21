// 기록 — 이력 표 · 추세 · 집계를 **한 탭에서** 본다. 전에는 상위 탭 넷이었다(이력·추세·규격별·통계).
//
// 넷을 한 탭으로 묶은 근거: 셋은 같은 필터(Kind·Code)가 걸린 같은 기록 묶음이고, 넷째(집계)는 그
// 필터를 고르는 자리다. 탭이 갈려 있으면 필터를 건드릴 때마다 "이 필터가 지금 보고 있는 것에
// 걸리나"를 매번 다시 확인해야 했다. 보기 전환은 세그먼트 하나이고 **마지막에 본 보기를 기억한다** —
// 같은 일을 하러 다시 들어온 사람에게 같은 질문을 두 번 하지 않는다.
import { useState } from 'react'
import { Segmented } from '../../lib/ui/Segmented'
import type { MeasRow } from '../../lib/meas/rows'
import type { MeasLogSnapshot } from '../../lib/types'
import { Aggregate } from './Aggregate'
import { History } from './History'
import { Trend } from './Trend'

const VIEWS = [
  { id: 'table', label: '이력' },
  { id: 'trend', label: '추세' },
  { id: 'agg', label: '집계' },
] as const
export type RecordsView = (typeof VIEWS)[number]['id']
const VIEW_KEY = 'gr-measure-records-view'

function loadView(): RecordsView {
  try {
    const v = localStorage.getItem(VIEW_KEY)
    return VIEWS.some((x) => x.id === v) ? (v as RecordsView) : 'table'
  } catch {
    return 'table'
  }
}

export function Records({
  rows,
  allCount,
  snap,
  kind,
  selected,
  onSelect,
  onPickCode,
  filtered,
}: {
  rows: MeasRow[]
  allCount: number
  snap: MeasLogSnapshot | null
  kind: string
  selected: number | null
  onSelect: (seq: number | null) => void
  onPickCode: (code: number) => void
  filtered: boolean
}) {
  const [view, setView] = useState<RecordsView>(loadView)

  function go(v: RecordsView) {
    setView(v)
    try {
      localStorage.setItem(VIEW_KEY, v)
    } catch {
      /* 저장 못 해도 동작 */
    }
  }

  return (
    // 보기 셋이 **남는 높이를 나눠 쓴다** — 표의 높이를 뷰포트 비율로 잡으면 도킹 존에서 어긋난다.
    <div className="flex h-full min-h-0 flex-col gap-2">
      <Segmented
        value={view}
        onChange={go}
        options={VIEWS.map((v) => ({
          id: v.id,
          label: v.label,
          testid: `measure-records-${v.id}`,
          badge: v.id === 'table' ? String(rows.length) : undefined,
        }))}
        ariaLabel="기록 보기"
      />
      <div className="min-h-0 flex-1 overflow-auto">
        {view === 'table' ? (
          <History
            rows={rows}
            selected={selected}
            onSelect={onSelect}
            allCount={allCount}
            filtered={filtered}
          />
        ) : null}
        {view === 'trend' ? (
          <Trend
            rows={rows}
            kind={kind}
            onPick={(seq) => {
              onSelect(seq)
              go('table')
            }}
          />
        ) : null}
        {view === 'agg' ? (
          <Aggregate
            snap={snap}
            onPick={(c) => {
              onPickCode(c)
              go('table')
            }}
          />
        ) : null}
      </div>
    </div>
  )
}
