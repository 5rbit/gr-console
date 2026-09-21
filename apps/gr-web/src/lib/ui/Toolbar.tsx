// 조작 띠 — **조작은 여기에만 있다**. 값·상태는 `FieldList`와 표가 맡는다.
// 이 재설계의 중심 규칙이 "한 영역은 조작이거나 표시다"이고, 그 규칙을 컴포넌트로 강제하는 자리다.
// (원본은 화면마다 `div.flex`로 버튼을 복붙해서, 조작과 값이 같은 줄에 섞여 있었다.)
//
// 왼쪽은 **이 띠가 무엇을 조작하는지**(아이콘 · 제목 · 대상), 오른쪽은 버튼이다.
// 되돌릴 수 없는 조작이 사는 띠는 `tone="danger"`로 면을 붉게 갈라 둔다.
import type * as React from 'react'
import { cn } from '../utils'

export interface ToolbarProps {
  /** 띠 왼쪽 아이콘 — `<Wrench size={14} />`. */
  icon?: React.ReactNode
  /** 이 띠가 무엇을 조작하는지. */
  title?: string
  /**
   * 제목 자리를 대신하는 조작(표 종류 토글 · 검색 등). 있으면 아이콘·제목은 그리지 않는다 —
   * 토글이 곧 "이 띠가 무엇을 조작하는지"를 말한다.
   */
  lead?: React.ReactNode
  /** 대상·범위 같은 부차 정보(제목보다 작고 흐리게). 값을 늘어놓는 자리가 아니다. */
  meta?: React.ReactNode
  /** `danger` — 되돌릴 수 없는 조작이 사는 띠. */
  tone?: 'default' | 'danger'
  /** 조밀 모드(28px) — 파형 도구처럼 띠가 여러 겹 쌓이는 자리. */
  dense?: boolean
  className?: string
  /** 오른쪽 버튼들. */
  children?: React.ReactNode
}

export function Toolbar({
  icon,
  title = '',
  lead,
  meta,
  tone = 'default',
  dense = false,
  className = '',
  children,
}: ToolbarProps) {
  const danger = tone === 'danger'
  return (
    <div
      className={cn(
        'flex flex-wrap items-center gap-2 border-b px-2',
        dense ? 'min-h-control-sm' : 'min-h-screen-header',
        danger ? 'border-fault bg-fault-soft' : 'border-line-default bg-surface-inset',
        className,
      )}
    >
      {lead}
      {icon && !lead ? (
        <span className={danger ? 'flex text-fault-fg' : 'flex text-content-muted'}>{icon}</span>
      ) : null}
      {title && !lead ? (
        <span
          className={cn(
            'text-xs font-semibold whitespace-nowrap',
            danger ? 'text-fault-fg' : 'text-content-primary',
          )}
        >
          {title}
        </span>
      ) : null}
      {meta ? (
        <span className="flex min-w-0 items-center gap-2 text-2xs text-content-muted">{meta}</span>
      ) : null}
      {/* 버튼은 항상 오른쪽 끝에 모인다 — 띠마다 조작의 자리가 같아야 손이 기억한다. */}
      <span className="ml-auto flex items-center gap-1 py-1">{children}</span>
    </div>
  )
}
