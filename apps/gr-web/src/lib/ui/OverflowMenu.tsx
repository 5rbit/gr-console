// 넘침 메뉴(`⋯`) — 가끔 쓰는 보조 조작을 띠에서 걷어 내 한 손잡이 뒤로 모은다.
//
// 새 팝업 층을 만들지 않고 **우클릭 도구 상자(`lib/ui/menu.ts`)를 그대로 연다**. 같은 조작이
// 우클릭과 `⋯`에서 같은 모양으로 뜨고(기능 일관성), 그리는 자리는 앱에 하나뿐인
// `ContextMenuHost`라 메뉴가 두 겹으로 겹칠 일이 없다.
//
// 자리 규칙: 이 손잡이는 **도구 띠와 화면 머리띠의 오른쪽 끝**에만 선다. 표의 행에는 두지 않는다 —
// 행 액션은 고정 너비 둘이 항상 같은 자리에 있어야 한다(`docs/DESIGN.md` 5절 자리 표).
import { MoreHorizontal } from 'lucide-react'
import { ctxMenu, type MenuItem } from './menu'
import { cn } from '../utils'

export interface OverflowMenuProps {
  /** 항목은 **열 때 계산해** 넘긴다(체크·비활성 사유가 옛 상태에 굳지 않게). */
  items: MenuItem[]
  title?: string
  testid?: string
  size?: 'sm' | 'md'
  /**
   * 손잡이를 잠그는 **사유** — `docs/DESIGN.md` 4절 ⑥("비활성 컨트롤은 이유를 단다"). 회색으로
   * 침묵하는 `⋯` 는 고장으로 읽힌다. 항목이 하나도 없을 때도 이 문장이 `title` 로 나간다.
   */
  disabledReason?: string
  className?: string
}

export function OverflowMenu({
  items,
  title = '더 보기',
  testid,
  size = 'sm',
  disabledReason,
  className = '',
}: OverflowMenuProps) {
  const off = !!disabledReason || items.length === 0
  return (
    <button
      type="button"
      title={disabledReason ?? title}
      aria-label={title}
      aria-haspopup="menu"
      aria-disabled={off || undefined}
      data-testid={testid}
      disabled={off}
      className={cn(
        'inline-flex items-center justify-center rounded-md text-content-muted transition-colors hover:bg-surface-inset hover:text-content-primary focus-visible:ring-2 focus-visible:ring-focus focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50',
        size === 'sm' ? 'h-control-sm w-control-sm' : 'h-control-md w-control-md',
        className,
      )}
      onClick={(e) => {
        if (off) return
        // 손잡이 **아래 왼쪽**에 연다 — 마우스 좌표에 열면 같은 버튼이 누를 때마다 다른 자리에 뜬다.
        const r = e.currentTarget.getBoundingClientRect()
        ctxMenu.show(
          {
            clientX: r.left,
            clientY: r.bottom + 2,
            preventDefault: () => e.preventDefault(),
            stopPropagation: () => e.stopPropagation(),
          },
          items,
        )
      }}
    >
      <MoreHorizontal className="h-4 w-4" />
    </button>
  )
}
