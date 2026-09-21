// 중앙 정렬 빈 상태 — "좌측에서 …를 선택하세요" 류를 일관된 형태로. 선택 아이콘은 선택.
//
// `compact` 는 **좁은 자리용 한 줄**이다. 기본 모양은 `min-h-60`(240px)이라 반 높이 카드나
// `<td colSpan>` 안에 들어가면 표가 제 내용보다 빈 상태 때문에 길어진다 — "없음" 하나를 말하려고
// 화면의 절반을 쓰는 셈이다. 말하는 것(제목+다음 행동)은 같고 자리만 줄인다.
import type * as React from 'react'
import { cn } from '../utils'

export interface EmptyStateProps {
  title: string
  hint?: string
  icon?: React.ReactNode
  /** 다음 행동(선택). 빈 화면이 곧 온보딩이라, "없음"만 말하고 끝나면 처음 쓰는 사람은 어디서
   *  시작할지 찾아 다녀야 한다 */
  action?: React.ReactNode
  /** 좁은 자리(반 높이 카드 · 표의 빈 행 · 사이드 패널) — 최소 높이를 두지 않고 한 줄로 앉는다. */
  compact?: boolean
  testid?: string
}

export function EmptyState({
  title,
  hint,
  icon,
  action,
  compact = false,
  testid,
}: EmptyStateProps) {
  return (
    <div
      className={cn(
        'flex flex-col items-center justify-center text-center',
        compact ? 'gap-1 px-3 py-4' : 'h-full min-h-60 gap-2 p-8',
      )}
      data-testid={testid}
    >
      {icon && <div className={cn('text-content-disabled', !compact && 'mb-1')}>{icon}</div>}
      <p className={cn('font-medium text-content-muted', compact ? 'text-xs' : 'text-sm')}>
        {title}
      </p>
      {hint && (
        <p
          className={cn(
            'max-w-sm leading-relaxed text-content-faint',
            compact ? 'text-2xs' : 'text-xs',
          )}
        >
          {hint}
        </p>
      )}
      {action && (
        <div
          className={cn(
            'flex flex-wrap items-center justify-center gap-2',
            compact ? 'mt-1' : 'mt-2',
          )}
        >
          {action}
        </div>
      )}
    </div>
  )
}
