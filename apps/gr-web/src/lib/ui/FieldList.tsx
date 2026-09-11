// 값 목록 — **표시 전용**(조작 없음). 라벨 열이 자리를 지키는 정형 격자라 눈이 한 줄로 내려간다.
//
// 카드를 쓰지 않는다: 값 하나에 카드 하나는 공간을 먹고 산만해진다. 카드는 독립 개체에
// 자체 액션 바가 있을 때만이다.
//
// 없는 값은 **글로 적지 않는다**:
//   아직 안 옴(loading) → 스켈레톤 바
//   로봇이 안 냄(null)  → 점선 원 아이콘(툴팁이 이유를 말한다)
// "미상"·"없음"·"—" 같은 낱말은 값처럼 읽혀서 데이터인 줄 오해하게 만든다.
//
// 값 글꼴은 **sans + tabular-nums**가 기본이다(UX 규칙 ⑩) — 숫자는 자리를 지키되 한글 라벨과
// 같은 리듬으로 읽힌다. mono는 식별자에만 `mono: true`로 켠다.
import * as React from 'react'
import { useEffect, useRef, useState } from 'react'
import { CircleDashed, ChevronsUpDown, ChevronsDownUp } from 'lucide-react'
import { Skeleton } from './Skeleton'
import { statusTone } from './status'
import { cn } from '../utils'
import type { FieldItem } from './fieldListModel'

export type { FieldItem }

export interface FieldListProps {
  items?: FieldItem[]
  /** 열 수 — 판이 좁으면 1, 보통 2, 아주 넓을 때만 3. */
  columns?: 1 | 2 | 3
  /** 라벨 열 폭(px). 화면 안에서는 같은 값을 써서 여러 목록의 값 열이 한 줄로 선다. */
  labelWidth?: number
  /** 조밀 모드 — 줄 높이를 22px 안팎으로 좁힌다. */
  dense?: boolean
  className?: string
}

const isEmpty = (v: unknown) => v === null || v === undefined || v === ''

function Value({ it }: { it: FieldItem }) {
  const ref = useRef<HTMLSpanElement>(null)
  const [clipped, setClipped] = useState(false)
  const [open, setOpen] = useState(false)

  // 값이 잘렸는지는 **그려 봐야** 안다 — 잘린 줄에만 펼치기 손잡이를 낸다.
  useEffect(() => {
    const el = ref.current
    if (!el) return
    const check = () => setClipped(el.scrollWidth > el.clientWidth + 1)
    check()
    const ro = new ResizeObserver(check)
    ro.observe(el)
    return () => ro.disconnect()
  }, [it.value, open])

  if (it.loading) return <Skeleton className={cn('h-2.5', it.wide ? 'w-3/5' : 'w-20')} />

  if (isEmpty(it.value)) {
    return (
      <span
        className="inline-flex text-content-faint"
        title={
          it.missing || '값 없음 — 이 필드를 내지 않았습니다(모듈 미지원이거나 아직 관측되지 않음).'
        }
        aria-label={`${it.label} 값 없음`}
      >
        <CircleDashed size={13} />
      </span>
    )
  }

  const text =
    typeof it.value === 'string' || typeof it.value === 'number' ? String(it.value) : undefined

  return (
    <span className="flex min-w-0 items-baseline gap-1">
      <span
        ref={ref}
        title={text}
        className={
          open ? 'min-w-0 break-all whitespace-normal' : 'min-w-0 truncate whitespace-nowrap'
        }
      >
        {it.value as React.ReactNode}
      </span>
      {clipped || open ? (
        <button
          type="button"
          className="inline-flex flex-none border-0 bg-transparent p-0 text-accent-text"
          title={open ? '한 줄로 접기' : '전체 보기 — 값이 잘렸습니다'}
          aria-label={open ? '접기' : '전체 보기'}
          onClick={() => setOpen(!open)}
        >
          {open ? <ChevronsDownUp size={12} /> : <ChevronsUpDown size={12} />}
        </button>
      ) : null}
    </span>
  )
}

export function FieldList({
  items = [],
  columns = 2,
  labelWidth = 116,
  dense = false,
  className = '',
}: FieldListProps) {
  /** 첫 줄에는 구분선을 얹지 않는다 — 1열이면 모든 줄이, 2·3열이면 둘째 줄부터. */
  const ruled = (i: number) => columns === 1 || i >= columns
  return (
    <dl
      className={cn('m-0 grid items-baseline gap-x-4', className)}
      style={{ gridTemplateColumns: `repeat(${columns}, ${labelWidth}px minmax(0,1fr))` }}
    >
      {items.map((it, i) => (
        <React.Fragment key={it.label + i}>
          <dt
            className={cn(
              'text-2xs whitespace-nowrap text-content-muted',
              dense ? 'py-0.5 pr-2' : 'py-1 pr-2',
              ruled(i) && 'border-t border-line-subtle',
            )}
            title={it.tooltip || undefined}
          >
            {/* 설명이 붙은 라벨만 점선 밑줄로 표시한다 — 있는 줄에만 손이 가게. */}
            <span
              className={
                it.tooltip ? 'cursor-help underline decoration-dotted underline-offset-2' : ''
              }
            >
              {it.label}
            </span>
          </dt>
          <dd
            className={cn(
              'm-0 min-w-0 text-xs tabular-nums',
              dense ? 'py-0.5 pr-2' : 'py-1 pr-2',
              ruled(i) && 'border-t border-line-subtle',
              it.mono ? 'font-mono' : 'font-sans',
              it.status ? `${statusTone(it.status).text} font-medium` : 'text-content-primary',
            )}
            style={it.wide ? { gridColumn: `span ${columns * 2 - 1}` } : undefined}
          >
            <Value it={it} />
          </dd>
        </React.Fragment>
      ))}
    </dl>
  )
}
