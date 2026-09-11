// 토스트 렌더러 — App 루트에 한 번 마운트. lib/toast 스토어를 구독해 우하단에 쌓는다(상태색 + lucide 아이콘).
import { useSyncExternalStore } from 'react'
import { Info, CircleCheck, TriangleAlert, CircleX, LoaderCircle, X } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { toasts } from './toast'
import type { ToastKind } from './toast'
import { statusTone } from './status'
import type { Status } from './status'
import { cn } from '../utils'

const iconOf: Record<ToastKind, LucideIcon> = {
  info: Info,
  ok: CircleCheck,
  warn: TriangleAlert,
  error: CircleX,
  pending: LoaderCircle,
}
const statusOf: Record<ToastKind, Status> = {
  info: 'info',
  ok: 'ok',
  warn: 'warn',
  error: 'fault',
  pending: 'info',
}

export interface ToasterProps {
  /** 컨테이너 추가 클래스(선택) */
  className?: string
}

export function Toaster({ className }: ToasterProps = {}) {
  const items = useSyncExternalStore(toasts.subscribe, toasts.getSnapshot, toasts.getSnapshot)

  return (
    // bottom-8 — 하단 상태바(h-6) 위에 뜬다. bottom-4면 토스트가 상태바를 덮었다.
    <div
      className={cn(
        'fixed right-4 bottom-8 z-[60] flex w-80 max-w-[calc(100vw-2rem)] flex-col gap-2',
        className,
      )}
      aria-live="polite"
    >
      {items.map((t) => {
        const Icon = iconOf[t.kind]
        const tone = statusTone(statusOf[t.kind])
        return (
          // **테스트 훅이자 접근성 훅.** 토스트는 조작의 유일한 피드백인 경우가 많은데(백업·스캔·정지),
          // 잡을 방법이 없으면 그 피드백이 실제로 뜨는지 아무도 확인하지 못한다.
          <div
            key={t.id}
            className={`flex items-start gap-2 rounded-lg border border-slate-200 px-3 py-2 shadow-lg dark:border-slate-700 ${tone.soft}`}
            role="status"
            data-testid="toast"
            data-kind={t.kind}
          >
            <Icon
              className={`mt-0.5 h-4 w-4 shrink-0 ${tone.text} ${t.kind === 'pending' ? 'animate-spin' : ''}`}
            />
            <span className="flex-1 text-sm break-words text-slate-700 dark:text-slate-200">
              {t.msg}
            </span>
            <button
              className="text-slate-400 hover:text-slate-700 dark:hover:text-slate-200"
              aria-label="닫기"
              onClick={() => toasts.dismiss(t.id)}
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
        )
      })}
    </div>
  )
}
