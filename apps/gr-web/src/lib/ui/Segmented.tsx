// 세그먼트 토글 — 도구 막대 높이 규칙(`--spacing-control-sm`)을 한 곳에서 지킨다.
// 높이를 토큰으로 부르는 이유: 전역 밀도(`lib/density.ts`)가 그 토큰을 갈아 끼운다(`docs/DESIGN.md` 6절).
// 레일 탭·작성 방식·그립 기준·맵 모드·편집 목록이 모두 같은 모양과 높이를 쓴다.
import type { ReactNode } from 'react'
import { cn } from '../utils'

export interface SegmentedOption<T extends string> {
  id: T
  label?: ReactNode
  icon?: ReactNode
  title?: string
  testid?: string
  /** 라벨 오른쪽의 작은 숫자/글자(빈 값이면 숨김). */
  badge?: ReactNode
}

export interface SegmentedProps<T extends string> {
  value: T
  onChange: (v: T) => void
  options: readonly SegmentedOption<T>[]
  ariaLabel: string
  /** 아이콘만 두고 선택된 항목에만 라벨을 붙인다(플롯 위 오버레이). */
  compact?: boolean
  className?: string
}

export function Segmented<T extends string>({
  value,
  onChange,
  options,
  ariaLabel,
  compact = false,
  className = '',
}: SegmentedProps<T>) {
  return (
    <div
      role="tablist"
      aria-label={ariaLabel}
      className={cn(
        'inline-flex h-control-sm flex-none items-stretch rounded-md border border-slate-300 bg-white p-0.5 dark:border-slate-600 dark:bg-slate-900',
        className,
      )}
    >
      {options.map((o) => {
        const on = o.id === value
        const showLabel = o.label !== undefined && (!compact || on || !o.icon)
        const hasBadge = o.badge !== undefined && o.badge !== null && o.badge !== ''
        return (
          <button
            key={o.id}
            type="button"
            role="tab"
            aria-selected={on}
            aria-label={typeof o.label === 'string' ? o.label : o.title}
            title={o.title ?? (typeof o.label === 'string' ? o.label : undefined)}
            data-testid={o.testid}
            className={cn(
              'flex items-center gap-1 rounded px-2 text-xs whitespace-nowrap transition-colors',
              on
                ? 'bg-indigo-600 text-white'
                : 'text-slate-600 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800',
            )}
            onClick={() => onChange(o.id)}
          >
            {o.icon}
            {showLabel ? <span>{o.label}</span> : null}
            {hasBadge ? (
              <span
                className={cn('text-[10px] tabular-nums', on ? 'opacity-80' : 'text-slate-400')}
              >
                {o.badge}
              </span>
            ) : null}
          </button>
        )
      })}
    </div>
  )
}
