// 우클릭 도구 상자의 **그리는 쪽** — 앱에 하나만 뜬다(`ctxMenu` 스토어가 여는 쪽).
// 본문 흐름 밖(fixed)이라 표 셀의 overflow나 SVG 경계에 잘리지 않는다.
import { useEffect, useSyncExternalStore } from 'react'
import { ctxMenu } from './menu'

export interface ContextMenuHostProps {
  /** 메뉴 상자 추가 클래스(선택) */
  className?: string
}

export function ContextMenuHost({ className }: ContextMenuHostProps = {}) {
  const state = useSyncExternalStore(ctxMenu.subscribe, ctxMenu.getSnapshot, ctxMenu.getSnapshot)

  useEffect(() => {
    const close = () => {
      if (ctxMenu.open) ctxMenu.close()
    }
    const onkey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && ctxMenu.open) ctxMenu.close()
    }
    window.addEventListener('click', close)
    window.addEventListener('keydown', onkey)
    window.addEventListener('resize', close)
    // 스크롤은 캡처로 받는다 — 스크롤은 버블링하지 않아 안쪽 컨테이너가 굴러도 메뉴가 남는다.
    window.addEventListener('scroll', close, { capture: true })
    return () => {
      window.removeEventListener('click', close)
      window.removeEventListener('keydown', onkey)
      window.removeEventListener('resize', close)
      window.removeEventListener('scroll', close, { capture: true })
    }
  }, [])

  if (!state.open) return null

  return (
    <div
      className={
        className
          ? `fixed z-[70] min-w-[190px] rounded-md border border-slate-200 bg-white py-1 text-xs shadow-lg dark:border-slate-700 dark:bg-slate-900 ${className}`
          : 'fixed z-[70] min-w-[190px] rounded-md border border-slate-200 bg-white py-1 text-xs shadow-lg dark:border-slate-700 dark:bg-slate-900'
      }
      style={{ left: `${state.x}px`, top: `${state.y}px` }}
      role="menu"
      tabIndex={-1}
      data-testid="ctx-menu"
    >
      {state.items.map((it, i) =>
        !it.run ? (
          <div
            key={it.label + i}
            className="truncate px-2.5 py-1 font-mono text-3xs text-slate-400"
          >
            {it.label}
          </div>
        ) : (
          <button
            key={it.label + i}
            className={`flex w-full items-center gap-3 px-2.5 py-1 text-left hover:bg-slate-100 disabled:opacity-40 dark:hover:bg-slate-800 ${
              it.danger ? 'text-red-600 dark:text-red-400' : ''
            }`}
            role="menuitem"
            disabled={!!it.disabled}
            title={it.disabled}
            data-testid={`ctx-${it.label}`}
            onClick={() => {
              const run = it.run
              ctxMenu.close()
              run?.()
            }}
          >
            <span className="flex-1">{it.label}</span>
            {it.hint && <kbd className="font-mono text-3xs text-slate-400">{it.hint}</kbd>}
          </button>
        ),
      )}
    </div>
  )
}
