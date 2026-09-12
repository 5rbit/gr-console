// 화면 헤더 — 이름과 **요약을 항목별로**. 조작 띠(`Toolbar`)와 다른 물건이다:
// 여기에는 버튼이 서지 않는다(규칙 ①: 한 영역은 조작이거나 표시다).
//
// 요약은 문장이 아니라 라벨+값 짝이다. `대상 sim #12 | 축 2 | 모드 MANUAL`처럼 구분선으로
// 나누면 눈이 필요한 항목 하나에 바로 꽂힌다 — 회색 문장으로 풀어 쓰면 매번 다시 읽힌다.
//
// **좁은 면에서는 요약을 접는다.** 화면이 도킹 존에 들어갈 수 있게 되면서(오른쪽 인스펙터 폭
// 200~560px) 이 띠가 300px 안에 서는 일이 생겼고, 그때 요약 항목이 줄바꿈되며 `실 행 0`처럼
// **글자 중간에서 끊겼다**. 끊긴 값은 안 보이는 값보다 나쁘다 — 읽을 수 있다고 착각하게 만든다.
// 그래서 순서를 정했다: 제목과 조작이 먼저고, 요약은 폭이 32rem을 넘을 때만 선다.
// 미디어 쿼리가 아니라 컨테이너 쿼리인 이유: 기준은 **창 폭이 아니라 이 띠가 든 존의 폭**이다.
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
    <div className="@container flex h-screen-header flex-none items-center gap-2 overflow-hidden border-b border-line-default bg-surface-panel px-3">
      {icon ? <span className="flex flex-none text-content-muted">{icon}</span> : null}
      {/* 제목은 줄바꿈하지 않고 넘치면 잘린다 — 줄바꿈되면 띠 높이(36px)가 깨진다. */}
      <span className="min-w-0 truncate text-sm font-semibold whitespace-nowrap">{title}</span>
      {items.map((m) => (
        <span
          key={m.label}
          className="hidden flex-none items-baseline gap-1 border-l border-line-subtle pl-2.5 text-2xs whitespace-nowrap @lg:inline-flex"
        >
          <span className="text-content-faint">{m.label}</span>
          <span className="tabular-nums text-content-secondary">{m.value}</span>
        </span>
      ))}
      <span className="flex-1" />
      {trailing ? <span className="flex flex-none items-center">{trailing}</span> : null}
    </div>
  )
}
