// 상태 pill 뱃지 — soft 배경 + 강조 텍스트 + 점. drive_state·health·이벤트 심각도 등에 공용.
import type * as React from 'react'
import { statusTone, type Status } from './status'

// rest는 **실제 HTML 속성 타입**으로 받는다(`[k: string]: unknown`이 아니라) — 인덱스 시그니처는
// 초과 속성 검사를 꺼서 오타를 조용히 삼킨다. 이 컴포넌트가 정확히 그 사고를 겪었다.
export interface StatusBadgeProps extends React.HTMLAttributes<HTMLSpanElement> {
  status?: Status | string
  dot?: boolean
  children?: React.ReactNode
}

export function StatusBadge({
  status = 'neutral',
  dot = true,
  children,
  ...rest
}: StatusBadgeProps) {
  const t = statusTone(status)

  return (
    <span
      className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-medium whitespace-nowrap ${t.soft} ${t.text}`}
      {...rest}
    >
      {dot && <span className={`h-1.5 w-1.5 rounded-full ${t.dot}`} />}
      {children}
    </span>
  )
}
