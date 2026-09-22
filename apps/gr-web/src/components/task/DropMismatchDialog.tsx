// PICK/DROP 짝에서 놓을 자리에 다른 품목이 있을 때의 확인 창 — 들고 있는 화물과 놓을 자리 재고를 나란히(셀 Id ·
// 품목 코드 · 화물 규격 · 개수). "그래도 옮기기" 를 눌러야 계획에 들어간다(`TaskIssue.planAdd`, `plan.dropMismatch`).
import type { ReactNode } from 'react'
import { itemLabel } from '../../lib/items/model'
import type { DropMismatch } from '../../lib/task/plan'
import type { Item, Target } from '../../lib/types'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'

function placeName(t: Target | null): string {
  if (!t) return '어디서 집었는지 모름'
  return `${t.kind === 'station' ? '스테이션' : '셀'} #${t.id}`
}

function Side({
  title,
  place,
  code,
  count,
  items,
  testid,
}: {
  title: string
  place: string
  code: number
  count: number
  items: readonly Item[]
  testid: string
}): ReactNode {
  const it = items.find((i) => i.code === code)
  return (
    <div
      className="flex min-w-0 flex-col gap-1 rounded-md border border-line-default bg-surface-inset p-2.5"
      data-testid={testid}
    >
      <span className="text-3xs font-semibold tracking-wide text-content-muted">{title}</span>
      <span className="text-sm font-semibold text-content-primary">{place}</span>
      <span className="font-semibold text-content-primary tabular-nums">
        {code} <span className="font-normal text-content-secondary">× {count}</span>
      </span>
      <span className="text-2xs break-words text-content-muted">
        {it ? itemLabel(it) : '등록되지 않은 품목'}
      </span>
    </div>
  )
}

export function DropMismatchDialog({
  mismatch,
  items,
  onCancel,
  onConfirm,
}: {
  mismatch: DropMismatch | null
  items: readonly Item[]
  onCancel: () => void
  onConfirm: () => void
}) {
  return (
    <ConfirmDialog
      open={!!mismatch}
      onOpenChange={(o) => !o && onCancel()}
      scope="single"
      danger
      title="다른 품목이 있는 자리에 놓기"
      confirmLabel="그래도 옮기기"
      onConfirm={onConfirm}
    >
      {mismatch ? (
        <div className="flex flex-col gap-2 text-xs" data-testid="drop-mismatch">
          <div className="grid grid-cols-2 gap-2">
            <Side
              title="들고 있는 화물"
              place={placeName(mismatch.from.target)}
              code={mismatch.from.item_code}
              count={mismatch.from.count}
              items={items}
              testid="drop-mismatch-from"
            />
            <Side
              title="놓을 자리 재고"
              place={placeName(mismatch.to.target)}
              code={mismatch.to.item_code}
              count={mismatch.to.count}
              items={items}
              testid="drop-mismatch-to"
            />
          </div>
          <span className="block text-warn-fg">
            품목이 다른 스택 위에 놓습니다. 정말 옮길까요? 옮기면 그 자리 재고의 품목 코드는 들고
            있는 화물로 바뀝니다.
          </span>
        </div>
      ) : null}
    </ConfirmDialog>
  )
}
