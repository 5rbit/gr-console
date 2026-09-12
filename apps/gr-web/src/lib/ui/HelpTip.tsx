// `?` 도움말 — 호버·포커스로 설명 말풍선을 띄운다. 네이티브 `title`은 지연이 길고 줄바꿈이 안 돼
// 쓰지 않는다.
//
// `<button>`이라 DataGrid의 셀 포인터 핸들러가 자동으로 비켜 간다(`closest('button')` 제외 규칙).
import { useState } from 'react'

export interface HelpTipProps {
  /** 설명 본문(비면 아이콘 자체를 그리지 않는다 — 빈 말풍선은 노이즈다) */
  text?: string
  /** 말풍선 제목(보통 키·이름) */
  title?: string
  /** 말풍선이 뜨는 쪽 — 표 오른쪽 끝 열에서는 왼쪽으로 연다 */
  align?: 'left' | 'right'
}

export function HelpTip({ text = '', title = '', align = 'left' }: HelpTipProps) {
  // 호버와 **고정**을 나눈다 — 하나로 두면 마우스로 다가가 여는 순간(hover) 클릭이 곧바로 닫는다.
  // 터치에는 호버가 없어 탭으로 고정해야 하고, 마우스는 벗어나면 닫히는 편이 자연스럽다.
  const [hovered, setHovered] = useState(false)
  const [pinned, setPinned] = useState(false)
  const open = hovered || pinned

  if (!text) return null

  return (
    <span className="relative inline-flex">
      <button
        type="button"
        aria-label={`${title || '항목'} 설명`}
        aria-expanded={open}
        className="grid h-4 w-4 shrink-0 place-items-center rounded-full border border-slate-300 text-3xs leading-none font-semibold text-slate-400 transition-colors hover:border-accent hover:text-accent focus-visible:ring-2 focus-visible:ring-focus focus-visible:ring-offset-1 focus-visible:outline-none dark:border-slate-600 dark:text-slate-500"
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        onFocus={() => setHovered(true)}
        onBlur={() => {
          setHovered(false)
          setPinned(false)
        }}
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
          className={`absolute top-5 z-50 w-64 rounded-md border border-slate-200 bg-white p-2 text-left text-2xs leading-relaxed font-normal whitespace-normal text-slate-600 shadow-lg dark:border-slate-700 dark:bg-slate-800 dark:text-slate-300 ${
            align === 'right' ? 'right-0' : 'left-0'
          }`}
        >
          {title && (
            <span className="mb-0.5 block font-mono text-3xs text-slate-400">{title}</span>
          )}
          {text}
        </span>
      )}
    </span>
  )
}
