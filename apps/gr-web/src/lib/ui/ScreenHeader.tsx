// 화면 헤더 — 이름과 **요약을 항목별로**. 조작 띠(`Toolbar`)와 다른 물건이다:
// 여기에는 버튼이 서지 않는다(규칙 ①: 한 영역은 조작이거나 표시다).
//
// 요약은 문장이 아니라 라벨+값 짝이다. `대상 sim #12 | 축 2 | 모드 MANUAL`처럼 구분선으로
// 나누면 눈이 필요한 항목 하나에 바로 꽂힌다 — 회색 문장으로 풀어 쓰면 매번 다시 읽힌다.
import type * as React from 'react'
import type { MetaItem } from './screenHeaderModel'

export type { MetaItem }

export interface ScreenHeaderProps {
  title: string
  icon?: React.ReactNode
  items?: MetaItem[]
  /** 오른쪽 끝 — 접힌 조작 판넬을 펼치는 손잡이 같은 것. */
  trailing?: React.ReactNode
}

export function ScreenHeader({ title, icon, items = [], trailing }: ScreenHeaderProps) {
  return (
    <div className="flex h-9 flex-none items-center gap-2 border-b border-line-default bg-surface-panel px-3">
      {icon ? <span className="flex text-content-muted">{icon}</span> : null}
      <span className="text-sm font-semibold">{title}</span>
      {items.map((m) => (
        <span
          key={m.label}
          className="inline-flex items-baseline gap-1 border-l border-line-subtle pl-2.5 text-2xs"
        >
          <span className="text-content-faint">{m.label}</span>
          <span className="tabular-nums text-content-secondary">{m.value}</span>
        </span>
      ))}
      <span className="flex-1" />
      {trailing}
    </div>
  )
}
