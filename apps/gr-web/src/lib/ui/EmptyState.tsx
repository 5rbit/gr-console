// 중앙 정렬 빈 상태 — "좌측에서 …를 선택하세요" 류를 일관된 형태로. 선택 아이콘은 선택.
import type * as React from 'react'

export interface EmptyStateProps {
  title: string
  hint?: string
  icon?: React.ReactNode
  /** 다음 행동(선택). 빈 화면이 곧 온보딩이라, "없음"만 말하고 끝나면 처음 쓰는 사람은 어디서
   *  시작할지 찾아 다녀야 한다 */
  action?: React.ReactNode
}

export function EmptyState({ title, hint, icon, action }: EmptyStateProps) {
  return (
    <div className="flex h-full min-h-60 flex-col items-center justify-center gap-2 p-8 text-center">
      {icon && <div className="mb-1 text-content-disabled">{icon}</div>}
      <p className="text-sm font-medium text-content-muted">{title}</p>
      {hint && (
        <p className="max-w-sm text-xs leading-relaxed text-content-faint">
          {hint}
        </p>
      )}
      {action && (
        <div className="mt-2 flex flex-wrap items-center justify-center gap-2">{action}</div>
      )}
    </div>
  )
}
