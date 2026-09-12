// 일반 모달 — 확인이 아니라 **작업**을 담는 팝업(파일 관리·경로 편집 따위)
//
// `ConfirmDialog`는 「예/아니오」 한 번을 묻는 물건이라 안에서 여러 조작을 하는 자리에는
// 안 맞는다(확인 버튼이 무엇을 확인하는지 흐려진다). 껍데기·a11y는 그것과 같은 것을 쓴다.
import { useEffect } from 'react'
import type { ReactNode } from 'react'
import { Button } from './Button'
import { useFocusTrap } from './focusTrap'

export interface ModalProps {
  open: boolean
  /** 열림 상태 변경 요청 — 백드롭·Escape·닫기 버튼이 `false`로 부른다 */
  onOpenChange: (open: boolean) => void
  title?: string
  closeLabel?: string
  /** 표·목록이 드는 자리 — 좁으면 가로 스크롤이 생긴다 */
  wide?: boolean
  children?: ReactNode
}

export function Modal({
  open,
  onOpenChange,
  title = '',
  closeLabel = '닫기',
  wide = false,
  children,
}: ModalProps) {
  const box = useFocusTrap<HTMLDivElement>(open)

  const close = () => onOpenChange(false)

  useEffect(() => {
    if (!open) return
    const onkey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onOpenChange(false)
    }
    window.addEventListener('keydown', onkey)
    return () => window.removeEventListener('keydown', onkey)
  }, [open, onOpenChange])

  if (!open) return null

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <div className="absolute inset-0 bg-black/40" onClick={close}></div>
      <div
        ref={box}
        className={`relative w-full ${
          wide ? 'max-w-3xl' : 'max-w-md'
        } rounded-lg border border-line-default bg-surface-panel shadow-xl`}
        role="dialog"
        aria-modal="true"
        aria-label={title}
      >
        <div className="flex items-center gap-2 border-b border-line-subtle px-4 py-3 font-semibold">
          {title}
        </div>
        <div className="max-h-[70vh] overflow-y-auto px-4 py-3 text-sm text-content-tertiary">
          {children}
        </div>
        <div className="flex justify-end border-t border-line-subtle px-4 py-3">
          <Button data-autofocus intent="ghost" onClick={close}>
            {closeLabel}
          </Button>
        </div>
      </div>
    </div>
  )
}
