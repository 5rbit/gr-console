// 머리 숫자 한 줄 — 화면마다 손으로 짜던 **통계 카드 격자**를 대신한다.
//
// 카드 격자가 나빴던 이유는 예쁨이 아니라 자리다: 카드 하나가 보더+면+그림자로 값 하나를 감싸면
// 아홉 개가 화면의 첫 3분의 1을 먹고, 그 아래의 표(진짜 일하는 자리)가 스크롤 밖으로 밀린다.
// 게다가 카드 안의 값은 서로 다른 x에 서서 **세로로 훑을 수 없다**.
//
// 그래서 띠 하나에 라벨+값 짝을 늘어놓는다(화면 머리띠와 같은 관용구, 글자만 한 단계 크다).
// 면을 만들지 않으니 예산도 쓰지 않는다 — 구분은 1px 세로선이 한다.
import type { ReactNode } from 'react'
import { cn } from '../utils'
import { HelpTip } from './HelpTip'
import { statusTone, type Status } from './status'

export interface StatItem {
  label: string
  value: ReactNode
  /** 값 아래 한 줄(단위·출처·부차 값). 문장이 아니라 짧은 꼬리표다. */
  hint?: ReactNode
  /** 상태색 — `neutral`(기본)은 색을 쓰지 않는다. 정상을 칠하면 붉은 하나가 사라진다. */
  tone?: Status
  /**
   * 라벨 옆 `?` — **이 숫자를 어떻게 읽나**(세는 범위 · 계산식 · 갱신 주기). 네이티브 `title`
   * (아래 prop)은 지연이 길고 줄바꿈이 안 돼서 문장 하나를 넘기지 못한다. 설명이 한 문장을 넘으면
   * 이것을 쓴다 — 설명은 `?` 뒤에 산다(`docs/DESIGN.md` 4절 ②).
   */
  help?: string
  /** 짧은 꼬리표 하나(전체 값 따위)를 마우스로 확인시킬 때만. 설명은 `help` 로. */
  title?: string
  testid?: string
}

export interface StatRowProps {
  items: readonly StatItem[]
  /** 아래 경계선 — 띠가 다른 내용 위에 설 때. */
  bordered?: boolean
  className?: string
  testid?: string
}

export function StatRow({ items, bordered = true, className = '', testid }: StatRowProps) {
  return (
    <dl
      className={cn(
        'flex flex-wrap items-stretch gap-x-4 gap-y-1 px-3 py-1.5',
        bordered && 'border-b border-line-default bg-surface-panel',
        className,
      )}
      data-testid={testid}
    >
      {items.map((s, i) => (
        <div
          key={s.label}
          title={s.title}
          data-testid={s.testid}
          className={cn(
            'flex min-w-0 flex-col justify-center',
            i > 0 && 'border-l border-line-subtle pl-4',
          )}
        >
          <dt className="flex items-center gap-1 text-3xs whitespace-nowrap text-content-faint">
            {s.label}
            {s.help ? <HelpTip title={s.label} text={s.help} /> : null}
          </dt>
          <dd
            className={cn(
              'font-mono text-sm leading-tight font-medium tabular-nums',
              s.tone && s.tone !== 'neutral' ? statusTone(s.tone).text : 'text-content-primary',
            )}
          >
            {s.value}
          </dd>
          {s.hint ? (
            <dd className="text-3xs whitespace-nowrap text-content-muted">{s.hint}</dd>
          ) : null}
        </div>
      ))}
    </dl>
  )
}
