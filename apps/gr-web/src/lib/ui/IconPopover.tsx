// 플롯 위 아이콘 버튼과 거기에 붙어 뜨는 팝업 — 보기 설정·범례처럼 가끔 쓰는 조작을 화면 밖으로 치운다.
// 바깥 클릭·Escape 로 닫힌다. 버튼은 모두 `--spacing-control-md` 한 규격(전역 밀도가 그 토큰을 간다).
import { useEffect, useRef, useState, type ReactNode } from 'react'
import { cn } from '../utils'

export const mapIconBtn =
  'flex h-control-md w-control-md items-center justify-center rounded-md border border-slate-200 bg-white/95 text-slate-600 shadow-sm transition-colors hover:bg-slate-50 hover:text-slate-900 dark:border-slate-700 dark:bg-slate-900/95 dark:text-slate-300 dark:hover:bg-slate-800 dark:hover:text-white'

export interface MapIconButtonProps {
  icon: ReactNode
  title: string
  onClick: () => void
  testid?: string
  disabled?: boolean
}

export function MapIconButton({
  icon,
  title,
  onClick,
  testid,
  disabled = false,
}: MapIconButtonProps) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      data-testid={testid}
      disabled={disabled}
      onClick={onClick}
      className={cn(mapIconBtn, 'disabled:cursor-not-allowed disabled:opacity-40')}
    >
      {icon}
    </button>
  )
}

export interface IconPopoverProps {
  icon: ReactNode
  title: string
  testid?: string
  /** 팝업 위치: 버튼 왼쪽(오른쪽 위 아이콘 열) 또는 아래. */
  side?: 'left' | 'bottom-left' | 'bottom-right'
  width?: number
  children: ReactNode
}

export function IconPopover({
  icon,
  title,
  testid,
  side = 'left',
  width = 260,
  children,
}: IconPopoverProps) {
  const [open, setOpen] = useState(false)
  const root = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const onDown = (e: MouseEvent) => {
      if (root.current && !root.current.contains(e.target as Node)) setOpen(false)
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false)
    }
    document.addEventListener('mousedown', onDown)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('mousedown', onDown)
      document.removeEventListener('keydown', onKey)
    }
  }, [open])

  return (
    <div ref={root} className="relative">
      <button
        type="button"
        title={title}
        aria-label={title}
        aria-expanded={open}
        data-testid={testid}
        className={cn(mapIconBtn, open && 'border-indigo-400 text-indigo-600 dark:text-indigo-300')}
        onClick={() => setOpen((o) => !o)}
      >
        {icon}
      </button>
      {open ? (
        <div
          role="dialog"
          aria-label={title}
          style={{ width }}
          data-testid={testid ? `${testid}-panel` : undefined}
          className={cn(
            'absolute z-20 rounded-lg border border-slate-200 bg-white p-3 text-xs shadow-lg dark:border-slate-700 dark:bg-slate-900',
            side === 'left'
              ? 'top-0 right-10'
              : side === 'bottom-right'
                ? 'top-10 right-0'
                : 'top-10 left-0',
          )}
        >
          <div className="mb-2 text-[11px] font-semibold text-slate-500">{title}</div>
          {children}
        </div>
      ) : null}
    </div>
  )
}
