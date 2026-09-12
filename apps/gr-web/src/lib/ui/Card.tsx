// 표준 카드 컨테이너 — 경계·배경·그림자. accent(좌측 상태 보더) 선택. 진단 카드·패널 공용.
import type * as React from 'react'
import { cn } from '../utils'

export interface CardProps {
  accent?: string
  padded?: boolean
  className?: string
  children?: React.ReactNode
}

export function Card({ accent = '', padded = true, className = '', children }: CardProps) {
  return (
    <div
      className={cn(
        'rounded-lg border border-slate-200 bg-white shadow-sm dark:border-slate-700 dark:bg-slate-800',
        accent && 'border-l-4 ' + accent,
        padded && 'p-3',
        className,
      )}
    >
      {children}
    </div>
  )
}
