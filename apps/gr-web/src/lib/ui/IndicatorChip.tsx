// 인디케이터 칩 — 상태를 **글자로** 말하는 작은 캡슐(`lib/indicators`의 `{label, tone, tooltip}`).
//
// 맨 사각 점은 뜻을 못 싣는다: 사이드바에 글자 없는 빨강·초록 사각이 서 있었을 때 운전자는 무엇이 빨간지
// 몰랐다. 칩은 앞에 작은 표식 + 짧은 글자 + 은은한 면이고, 사유는 `title` 툴팁에 산다.
// 라운딩은 4px(`rounded`) — 알약은 점의 모양이다(`no-pill`).
import type * as React from 'react'
import { toneStatus, type Indicator } from '../indicators'
import { statusTone } from './status'

export interface IndicatorChipProps extends Omit<React.HTMLAttributes<HTMLSpanElement>, 'title'> {
  ind: Indicator
  /** 앞 표식(작은 사각)을 뺀다 — 글자만으로 충분한 자리(모드 칩). */
  bare?: boolean
}

export function IndicatorChip({ ind, bare = false, className = '', ...rest }: IndicatorChipProps) {
  const t = statusTone(toneStatus(ind.tone))
  return (
    <span
      className={`inline-flex shrink-0 items-center gap-1 rounded px-1 text-2xs leading-4 font-medium whitespace-nowrap tabular-nums ${t.soft} ${t.text} ${className}`}
      title={ind.tooltip || undefined}
      data-tone={ind.tone}
      {...rest}
    >
      {bare ? null : <span className={`h-1.5 w-1.5 shrink-0 rounded-sm ${t.dot}`} aria-hidden="true" />}
      {ind.label}
    </span>
  )
}
