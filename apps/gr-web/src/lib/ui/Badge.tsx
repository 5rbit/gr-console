// 제네릭 소형 뱃지(중립 톤) — 태그·메타·access 표시 등. 상태색이 필요하면 StatusBadge를 쓴다.
import type * as React from 'react'

export interface BadgeProps {
  className?: string
  children?: React.ReactNode
}

export function Badge({ className = '', children }: BadgeProps) {
  return (
    <span
      className={`inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-3xs font-medium text-slate-600 dark:text-slate-300 ${className}`}
    >
      {children}
    </span>
  )
}
