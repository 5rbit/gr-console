// 라벨+값 짝 — `docs/DESIGN.md` 4절 ②("요약은 문장이 아니라 라벨+값 짝이다")의 **한 구현**.
//
// 화면마다 같은 `<dl>` 을 손으로 짜고 있었다(팔렛 넷 · 화물 규격 하나 · 표의 접힌 열 하나). 짝이
// 같은 모양으로 보이려면 라벨 색·값의 고정폭·간격이 한 곳에 있어야 한다 — 다섯 벌이면 다섯 자리가
// 조금씩 어긋나고, 실제로 어긋나 있었다(3xs 와 2xs, `gap-x-3` 과 `gap-x-4`).
//
// 두 모양뿐이다:
//   `Pairs`    — 한 줄에 늘어놓는 띠(요약 · 대화상자 머리 · 캔버스 아래 눈금).
//   `InfoRows` — 두 열 표(팝오버 안 · 좁은 칸). 값이 오른쪽에 서서 자릿수가 맞는다.
//
// 조작은 두지 않는다. 값 옆에 버튼이 필요해지면 그 자리는 짝이 아니라 표다.
import { Fragment, type ReactNode } from 'react'
import { cn } from '../utils'
import { statusTone, type Status } from './status'

/** 짝 하나. `title` 은 잘린 값의 전체(마우스를 얹었을 때). */
export interface PairItem {
  label: ReactNode
  value: ReactNode
  title?: string
  /** 값을 고정폭·자릿수 정렬로 — 숫자면 켠다(기본 켜짐). */
  mono?: boolean
  /** 값의 상태색 — `neutral`(기본)은 색을 쓰지 않는다. 정상을 칠하면 붉은 하나가 사라진다. */
  tone?: Status
  testid?: string
}

export interface PairsProps {
  items: readonly PairItem[]
  /**
   * 값이 라벨 **오른쪽**(`inline`, 기본)인가 **아래**(`stacked`)인가. 값이 길거나 라벨이 길면
   * stacked 가 줄바꿈 없이 선다.
   */
  layout?: 'inline' | 'stacked'
  /** 글자 크기 — 띠는 `3xs`(기본), 대화상자 본문은 `2xs`. 그 위는 쓰지 않는다. */
  size?: '3xs' | '2xs'
  className?: string
  testid?: string
}

/** 한 줄로 늘어놓는 라벨+값 띠. 좁아지면 줄바꿈하되 짝은 붙어 다닌다. */
export function Pairs({
  items,
  layout = 'inline',
  size = '3xs',
  className = '',
  testid,
}: PairsProps) {
  return (
    <dl
      className={cn(
        'flex flex-wrap gap-x-4 gap-y-1',
        size === '3xs' ? 'text-3xs' : 'text-2xs',
        className,
      )}
      data-testid={testid}
    >
      {items.map((p, i) => (
        <div
          key={typeof p.label === 'string' ? p.label : i}
          title={p.title}
          data-testid={p.testid}
          className={cn('min-w-0', layout === 'inline' ? 'flex gap-1' : 'flex flex-col')}
        >
          <dt className="whitespace-nowrap text-content-faint">{p.label}</dt>
          <dd
            className={cn(
              'min-w-0',
              p.tone && p.tone !== 'neutral' ? statusTone(p.tone).text : 'text-content-secondary',
              (p.mono ?? true) && 'font-mono tabular-nums',
            )}
          >
            {p.value}
          </dd>
        </div>
      ))}
    </dl>
  )
}

/** 두 열 표의 한 줄. */
export interface InfoRow {
  label: ReactNode
  value: ReactNode
  title?: string
}

export interface InfoRowsProps {
  rows: readonly InfoRow[]
  /** 값을 오른쪽 끝에 세운다(기본) — 자릿수가 맞아 세로로 훑힌다. */
  alignRight?: boolean
  className?: string
  testid?: string
}

/** 라벨+값 두 열 — 팝오버·좁은 칸 안. 라벨은 왼쪽, 값은 고정폭. */
export function InfoRows({ rows, alignRight = true, className = '', testid }: InfoRowsProps) {
  return (
    <dl
      className={cn('grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5', className)}
      data-testid={testid}
    >
      {rows.map((r, i) => (
        <Fragment key={typeof r.label === 'string' ? r.label : i}>
          <dt className="font-mono text-2xs text-content-muted">{r.label}</dt>
          <dd
            title={r.title}
            className={cn(
              'min-w-0 font-mono text-2xs tabular-nums text-content-tertiary',
              alignRight && 'text-right',
            )}
          >
            {r.value}
          </dd>
        </Fragment>
      ))}
    </dl>
  )
}
