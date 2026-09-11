// 조작 인스펙터의 계약 — `OpsPanel.tsx`. 컴포넌트 밖에 둔다(`table.ts`와 같은 관용구).

import type * as React from 'react'

/** lucide 아이콘 컴포넌트(`import { Square } from 'lucide-react'`). */
type IconComponent = React.ComponentType<{ size?: number | string; className?: string }>

export interface SegmentOption {
  label: string
  /** 눌린 쪽 — 그린 링으로 갈린다(포커스 파랑과 뜻이 다르다). */
  active?: boolean
  disabled?: boolean
  /** 비활성 사유 — 잠겼으면 **왜**를 말한다. */
  title?: string
  run?: () => void
  /** e2e 손잡이 — 화면 밖에서 이 한 칸을 지목해야 하는 조작에만 단다(축 인가 등) */
  testid?: string
}

/** 상호배타 상태 전환만 세그먼트로 낸다(MANUAL/AUTO · 재생/정지 · 그림/표). */
export interface SegmentGroup {
  label: string
  /** 설명은 제목 뒤가 아니라 `HelpTip`으로 내린다(UX 규칙 ③). */
  hint?: string
  options: SegmentOption[]
}

/** 절반 폭 2열에 서는 나머지 조작. */
export interface OpsAction {
  label: string
  icon?: IconComponent
  intent?: 'neutral' | 'primary' | 'outline' | 'ghost' | 'danger'
  disabled?: boolean
  title?: string
  run?: () => void
  /** e2e 손잡이 — 화면 밖에서 이 한 칸을 지목해야 하는 조작에만 단다(축 인가 등) */
  testid?: string
}

/** 맨 아래 전폭 정지 한 칸. */
export interface StopAction {
  label: string
  icon?: IconComponent
  disabled?: boolean
  title?: string
  /** 확인을 끼우지 않는다(UX 규칙 ⑧) — 급할 때 한 번 더 누르게 만드는 UI는 사고 원인이다.
   *  `false`면 호출자가 `run` 안에서 확인을 책임진다. */
  immediate?: boolean
  run?: () => void
}
