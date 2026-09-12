// 상태 캡슐 — soft 배경 + 강조 텍스트 + 점. **자리가 고정된 곳 둘**에만 선다(DESIGN.md 4절 ④):
// 표의 상태 열(한 표에 하나) · 상세의 머리줄(제목 옆 하나).
//
// 그 밖의 자리(목록 행의 이름 옆 · 값 자리 · 비트 묶음 · 개수)에는 `StatusDot` + label을 쓴다.
// 캡슐은 글자 길이대로 폭이 바뀌어 행마다 다른 x에 서고, 세로로 훑는 눈은 자리가 맞는 것만
// 비교할 수 있다. 라운딩이 4px(`rounded`)인 이유도 같다 — 알약(`rounded-full`)은 점 하나의 모양이고
// 글자를 담는 면이 알약이면 점과 헷갈린다(`no-pill` 린트가 센다).
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
      className={`inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-2xs font-medium whitespace-nowrap ${t.soft} ${t.text}`}
      {...rest}
    >
      {dot && <span className={`h-1.5 w-1.5 rounded-full ${t.dot}`} />}
      {children}
    </span>
  )
}
