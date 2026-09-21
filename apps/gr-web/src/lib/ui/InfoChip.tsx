// 값 칩 한 조각과, 눌러서 여는 칩 — **설명 문단을 걷어 낸 자리**에 서는 것들.
//
// 화물 규격 패널이 표 위에 문단을 넷까지 쌓고 있었다(공식 · 키 풀이 · 최신 측정 · ByCode). 문단은
// 한 번 읽으면 끝인데 자리는 매번 먹고, 정작 고칠 표를 아래로 밀어 냈다. 규칙을 바꿨다:
// **값은 칩으로 띠에 서고, 전체 표는 칩 뒤에 접힌다.** (규칙·용어 풀이는 `HelpTip` 의 `?` 뒤다.)
//
// 칩은 `h-control-sm` 한 규격이라 도구 띠의 다른 컨트롤과 같은 줄에 선다. 칩은 **면을 만들지
// 않는다** — 1px 보더뿐이다(면 예산: `docs/DESIGN.md` 5절).
import { useState, type ReactNode } from 'react'
import { cn } from '../utils'
import { useDismiss } from './useDismiss'

/** 칩 하나의 내용 — 라벨(보통 PLC/데이터 이름)과 값. `title` 은 잘린 값의 전체. */
export interface ChipContent {
  label: ReactNode
  value: ReactNode
  title?: string
}

const CHIP =
  'inline-flex h-control-sm items-center gap-1 rounded border border-line-default px-1.5 text-2xs whitespace-nowrap text-content-muted'

/** 팝오버 상자 — `InfoChip`/`HelpTip` 이 같은 모양으로 연다. */
export const POPOVER_PANEL =
  'absolute top-6 z-50 w-64 rounded-md border border-line-default bg-surface-panel p-2 text-left shadow-lg'

/** 라벨+값 한 조각 — 누를 수 없는 표시용. 전체 값은 `title` 로 간다. */
export function InfoChip({ chip, testid }: { chip: ChipContent; testid?: string }) {
  return (
    <span className={CHIP} title={chip.title} data-testid={testid}>
      <span className="font-mono">{chip.label}</span>
      <span className="font-mono tabular-nums text-content-tertiary">{chip.value}</span>
    </span>
  )
}

export interface ChipPopoverProps {
  chip: ChipContent
  /** 팝오버 제목 — 칩이 무엇의 요약인지. */
  title: string
  /** 팝오버 본문 — 보통 `InfoRows`. */
  children: ReactNode
  /** 오른쪽 끝에 선 칩은 왼쪽으로 연다(화면 밖으로 나가지 않게). */
  align?: 'left' | 'right'
  testid?: string
}

/** 눌러서 여는 칩 — 띠에는 요약 한 조각, 뒤에는 전체 값. 바깥 클릭·Escape 로 닫힌다. */
export function ChipPopover({ chip, title, children, align = 'left', testid }: ChipPopoverProps) {
  const [open, setOpen] = useState(false)
  const root = useDismiss<HTMLSpanElement>(open, () => setOpen(false))
  return (
    <span className="relative inline-flex" ref={root}>
      <button
        type="button"
        title={chip.title}
        aria-expanded={open}
        data-testid={testid}
        className={cn(
          CHIP,
          'transition-colors hover:border-accent hover:text-content-secondary focus-visible:ring-2 focus-visible:ring-focus focus-visible:outline-none',
          open && 'border-accent',
        )}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="font-mono">{chip.label}</span>
        <span className="font-mono tabular-nums text-content-tertiary">{chip.value}</span>
      </button>
      {open ? (
        <span
          role="dialog"
          aria-label={title}
          className={cn(POPOVER_PANEL, align === 'right' ? 'right-0' : 'left-0')}
          data-testid={testid ? `${testid}-panel` : undefined}
        >
          <span className="mb-1 block font-mono text-3xs text-content-muted">{title}</span>
          {children}
        </span>
      ) : null}
    </span>
  )
}
