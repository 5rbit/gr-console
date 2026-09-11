// 표준 버튼 — tailwind-variants로 intent/size 변형(설치된 자산 활성화). 컴포넌트마다 흩어진 btn()/seg() 대체.
import type * as React from 'react'
import { tv } from 'tailwind-variants'
import { LoaderCircle } from 'lucide-react'
import { cn } from '../utils'

const button = tv({
  base: 'inline-flex items-center justify-center gap-1.5 rounded-md font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-1 focus-visible:ring-offset-white dark:focus-visible:ring-offset-slate-900',
  variants: {
    intent: {
      primary: 'bg-accent text-white hover:bg-accent-hover focus-visible:ring-focus',
      neutral:
        'bg-slate-100 text-slate-800 hover:bg-slate-200 focus-visible:ring-focus dark:bg-slate-700 dark:text-slate-100 dark:hover:bg-slate-600',
      ghost:
        'text-slate-500 hover:bg-slate-100 hover:text-slate-900 focus-visible:ring-focus dark:hover:bg-slate-800 dark:hover:text-slate-100',
      outline:
        'border border-slate-300 text-slate-700 hover:bg-slate-100 focus-visible:ring-focus dark:border-slate-600 dark:text-slate-200 dark:hover:bg-slate-800',
      danger: 'bg-danger text-white hover:bg-danger-hover focus-visible:ring-focus',
    },
    size: {
      sm: 'h-7 px-2 text-xs',
      md: 'h-8 px-3 text-sm',
      icon: 'h-8 w-8',
      'icon-sm': 'h-7 w-7',
    },
    active: { true: 'ring-2 ring-accent/40', false: '' },
  },
  defaultVariants: { intent: 'neutral', size: 'md', active: false },
})

// `[k: string]: unknown`을 쓰지 않는다 — 인덱스 시그니처는 초과 속성 검사를 꺼서
// `intnet="danger"` 같은 오타를 조용히 통과시킨다. 실제 HTML 속성 타입으로 받는다.
export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  intent?: 'primary' | 'neutral' | 'ghost' | 'outline' | 'danger'
  size?: 'sm' | 'md' | 'icon' | 'icon-sm'
  active?: boolean
  /** 진행 중 — 스피너로 아이콘을 갈고 스스로 비활성화한다(`disabled`는 rest로 그대로 전달되므로
   *  둘 중 하나만 참이어도 잠긴다). 왕복이 긴 명령의 이중 발사를 막는다 */
  loading?: boolean
  className?: string
  /** 라벨 앞 아이콘(선택) — `EmptyState`와 동일 계약 */
  icon?: React.ReactNode
  children?: React.ReactNode
}

export function Button({
  intent = 'neutral',
  size = 'md',
  active = false,
  loading = false,
  className = '',
  icon,
  children,
  ...rest
}: ButtonProps) {
  return (
    <button
      className={cn(button({ intent, size, active }), className)}
      {...rest}
      disabled={loading || rest.disabled}
      aria-busy={loading || undefined}
      data-loading={loading ? 'true' : undefined}
    >
      {loading ? (
        <LoaderCircle className="h-3.5 w-3.5 shrink-0 animate-spin" aria-hidden="true" />
      ) : (
        icon
      )}
      {children}
    </button>
  )
}
