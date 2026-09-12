// **페이지 이동** — 서버가 `total`/`limit`/`offset`을 이미 다 주는데 화면에 수단이 없었다.
//
// 이력 화면은 "{total}건 중 {items}건"이라고 적어 놓고 **2페이지로 갈 방법이 없었다**. 총계를
// 보여 주면서 나머지에 못 가게 하는 것은, 없는 것보다 나쁘다 — 사람이 데이터가 없다고 오해한다.
import { ChevronLeft, ChevronRight } from 'lucide-react'
import { Button } from './Button'

export interface PaginationProps {
  total: number
  limit: number
  offset: number
  /** 새 offset으로 이동 — 조회는 호출자가 한다(서버 질의 방식이 화면마다 다르다) */
  onMove: (offset: number) => void
  testid?: string
}

export function Pagination({ total, limit, offset, onMove, testid = 'pager' }: PaginationProps) {
  const pages = Math.max(1, Math.ceil(total / Math.max(1, limit)))
  const at = Math.floor(offset / Math.max(1, limit)) + 1
  const from = total === 0 ? 0 : offset + 1
  const to = Math.min(total, offset + limit)

  return (
    <div
      className="flex items-center gap-2 text-xs text-content-tertiary"
      data-testid={testid}
    >
      {/* **어디를 보고 있는지 먼저 말한다.** 페이지 번호만 있으면 "지금 몇 번째 줄"을 사람이 센다. */}
      <span className="tabular-nums" data-testid={`${testid}-range`}>
        {total === 0 ? '0건' : `${from}–${to} / ${total}건`}
      </span>
      <div className="ml-auto flex items-center gap-1">
        <Button
          size="icon-sm"
          intent="ghost"
          disabled={offset <= 0}
          onClick={() => onMove(Math.max(0, offset - limit))}
          aria-label="이전 페이지"
          data-testid={`${testid}-prev`}
        >
          <ChevronLeft size={14} />
        </Button>
        <span className="tabular-nums" data-testid={`${testid}-at`}>
          {at} / {pages}
        </span>
        <Button
          size="icon-sm"
          intent="ghost"
          disabled={offset + limit >= total}
          onClick={() => onMove(offset + limit)}
          aria-label="다음 페이지"
          data-testid={`${testid}-next`}
        >
          <ChevronRight size={14} />
        </Button>
      </div>
    </div>
  )
}
