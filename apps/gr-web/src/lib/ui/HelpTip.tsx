// `?` 도움말 — 호버·포커스로 설명 말풍선을 띄운다. 네이티브 `title`은 지연이 길고 줄바꿈이 안 돼
// 쓰지 않는다.
//
// **설명이 사는 자리가 여기다.** 화면 본문에 문단으로 풀어 쓰면 처음 한 번 읽고 다시는 안 읽는 글이
// 값과 같은 자리를 상시로 먹는다(`docs/DESIGN.md` 4절 ②). 그래서 규칙·공식·용어 풀이는 전부 이
// `?` 뒤로 접는다.
//
// 본문은 세 모양을 받는다:
//   `text`     평문 한 덩이 — 대부분의 자리.
//   `sections` 소제목+본문 여러 칸 — 공식·키 풀이처럼 **갈래가 있는** 설명.
//   `children` 임의 노드 — 표(`InfoRows`)나 목록을 넣어야 할 때.
// 예전에는 `text` 뿐이라 화면들이 제 `HelpDot` 을 따로 만들었다(화물 규격 패널). 셋을 다 받으면서
// 그 사본은 사라졌다.
//
// `<button>`이라 DataGrid의 셀 포인터 핸들러가 자동으로 비켜 간다(`closest('button')` 제외 규칙).
import { useState, type ReactNode } from 'react'
import { cn } from '../utils'
import { useDismiss } from './useDismiss'

/** 말풍선 한 칸 — 소제목 + 본문. */
export interface HelpSection {
  title: string
  body: ReactNode
}

export interface HelpTipProps {
  /** 설명 본문(평문). `sections`·`children` 이 없고 이것도 비면 아이콘 자체를 그리지 않는다 */
  text?: string
  /** 소제목이 있는 여러 칸 — 갈래가 있는 설명(공식 · 키 풀이 · 한계) */
  sections?: readonly HelpSection[]
  /** 말풍선 제목(보통 키·이름) */
  title?: string
  /** 말풍선이 뜨는 쪽 — 표 오른쪽 끝 열에서는 왼쪽으로 연다 */
  align?: 'left' | 'right'
  testid?: string
  /** 임의 본문 — 표·목록을 넣을 때. `text`/`sections` 보다 아래에 붙는다 */
  children?: ReactNode
}

export function HelpTip({
  text = '',
  sections,
  title = '',
  align = 'left',
  testid,
  children,
}: HelpTipProps) {
  // 호버와 **고정**을 나눈다 — 하나로 두면 마우스로 다가가 여는 순간(hover) 클릭이 곧바로 닫는다.
  // 터치에는 호버가 없어 탭으로 고정해야 하고, 마우스는 벗어나면 닫히는 편이 자연스럽다.
  const [hovered, setHovered] = useState(false)
  const [pinned, setPinned] = useState(false)
  const open = hovered || pinned
  // 고정한 말풍선은 바깥 클릭·Escape 로 닫는다 — 킷의 다른 팝오버와 같은 규칙.
  const root = useDismiss<HTMLSpanElement>(pinned, () => setPinned(false))

  const has = !!text || (sections?.length ?? 0) > 0 || !!children
  if (!has) return null

  return (
    <span className="relative inline-flex" ref={root}>
      <button
        type="button"
        aria-label={`${title || '항목'} 설명`}
        aria-expanded={open}
        data-testid={testid}
        className="grid h-4 w-4 shrink-0 place-items-center rounded-full border border-line-strong text-3xs leading-none font-semibold text-content-faint transition-colors hover:border-accent hover:text-accent focus-visible:ring-2 focus-visible:ring-focus focus-visible:ring-offset-1 focus-visible:outline-none"
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        onFocus={() => setHovered(true)}
        onBlur={() => setHovered(false)}
        onClick={(e) => {
          e.stopPropagation() // 셀 선택·편집으로 새지 않게.
          setPinned((p) => !p)
        }}
      >
        ?
      </button>
      {open && (
        <span
          role="tooltip"
          data-testid={testid ? `${testid}-panel` : undefined}
          className={cn(
            'absolute top-5 z-50 w-64 rounded-md border border-line-default bg-surface-panel p-2 text-left text-2xs leading-relaxed font-normal whitespace-normal text-content-tertiary shadow-lg',
            align === 'right' ? 'right-0' : 'left-0',
          )}
        >
          {title && (
            <span className="mb-0.5 block font-mono text-3xs text-content-faint">{title}</span>
          )}
          {text}
          {sections?.map((s) => (
            <span key={s.title} className="mt-1.5 block first:mt-0">
              <span className="block font-mono text-3xs text-content-muted">{s.title}</span>
              <span className="block leading-relaxed whitespace-normal text-content-tertiary">
                {s.body}
              </span>
            </span>
          ))}
          {children ? <span className="mt-1.5 block first:mt-0">{children}</span> : null}
        </span>
      )}
    </span>
  )
}
