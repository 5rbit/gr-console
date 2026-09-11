// 확인 모달 — 파괴적/위험 액션(엔지니어링 write_sdo·플릿 배포) 전 명시 확인. 현 ⚠ 글리프+title만 대체.
// a11y: role=dialog·aria-modal, Escape/백드롭 닫기, 확인/취소 버튼. before→after diff 등은 children으로.
import { useEffect } from 'react'
import type { ReactNode } from 'react'
import { Button } from './Button'
import { useFocusTrap } from './focusTrap'

/** 이 확인이 몇 대에 영향을 주는가 — 화면(배지)과 DOM(`data-scope`) 양쪽에 드러낸다. 필수 prop.
 *
 *  1건짜리가 셋인 것은 되돌릴 수 있는 sim 삭제와 물리적 결과가 있는 실장비 조작을 같은 말로
 *  부르지 않기 위해서다. */
export type ConfirmScope =
  | 'single'
  | 'single-sim'
  | 'single-robot'
  | 'selection'
  | 'fleet'
  | 'firmware'
  | 'surface-cmd'

const SCOPE_LABEL: Record<ConfirmScope, string> = {
  single: '대상 1건',
  'single-sim': '이 Instance 1개',
  'single-robot': '실 로봇 1대',
  selection: '선택한 대상 전체',
  fleet: '플릿 전체',
  firmware: '실장비 시스템',
  'surface-cmd': '이 Instance surface 명령',
}
const SCOPE_TONE: Record<ConfirmScope, string> = {
  single: 'bg-slate-100 text-slate-600 dark:bg-slate-700 dark:text-slate-300',
  'single-sim': 'bg-slate-100 text-slate-600 dark:bg-slate-700 dark:text-slate-300',
  // 실 로봇 1대는 sim 1개와 같은 무게가 아니다 — 되돌릴 수 없는 물리적 결과가 붙는다.
  'single-robot': 'bg-red-100 text-red-700 dark:bg-red-500/15 dark:text-red-300',
  selection: 'bg-amber-100 text-amber-700 dark:bg-amber-500/15 dark:text-amber-300',
  fleet: 'bg-orange-100 text-orange-700 dark:bg-orange-500/15 dark:text-orange-300',
  firmware: 'bg-red-100 text-red-700 dark:bg-red-500/15 dark:text-red-300',
  'surface-cmd': 'bg-amber-100 text-amber-700 dark:bg-amber-500/15 dark:text-amber-300',
}

export interface ConfirmDialogProps {
  open: boolean
  /** 열림 상태 변경 요청 — 백드롭·Escape·취소·확인이 `false`로 부른다 */
  onOpenChange: (open: boolean) => void
  /** 영향 범위 — 필수 */
  scope: ConfirmScope
  title?: string
  danger?: boolean
  confirmLabel?: string
  cancelLabel?: string
  onConfirm?: () => void
  children?: ReactNode
}

export function ConfirmDialog({
  open,
  onOpenChange,
  scope,
  title = '확인',
  danger = false,
  confirmLabel = '확인',
  cancelLabel = '취소',
  onConfirm,
  children,
}: ConfirmDialogProps) {
  const box = useFocusTrap<HTMLDivElement>(open)

  const close = () => onOpenChange(false)
  const confirm = () => {
    onConfirm?.()
    onOpenChange(false)
  }

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
      <div className="absolute inset-0 bg-black/40 backdrop-blur-sm" onClick={close}></div>
      {/* 포커스 트랩 — 첫 포커스는 **취소**(안전한 쪽)다. 확인에 주면 Enter 연타로 파괴적 조작이 통과한다. */}
      <div
        ref={box}
        className="relative w-full max-w-md rounded-lg border border-slate-200 bg-white shadow-xl dark:border-slate-700 dark:bg-slate-800"
        role="dialog"
        aria-modal="true"
        aria-label={title}
      >
        <div className="flex items-center gap-2 border-b border-slate-100 px-4 py-3 font-semibold dark:border-slate-700">
          {title}
          {/* 영향 범위 배지 — "무엇을 확인하는가"만큼 "몇 대에 적용되는가"가 중요하다. */}
          <span
            className={`ml-auto rounded px-1.5 py-0.5 text-[11px] font-normal ${SCOPE_TONE[scope]}`}
            data-testid="confirm-scope"
          >
            {SCOPE_LABEL[scope]}
          </span>
        </div>
        <div className="px-4 py-3 text-sm text-slate-600 dark:text-slate-300">{children}</div>
        <div className="flex justify-end gap-2 border-t border-slate-100 px-4 py-3 dark:border-slate-700">
          <Button data-autofocus intent="ghost" onClick={close}>
            {cancelLabel}
          </Button>
          <Button
            intent={danger ? 'danger' : 'primary'}
            onClick={confirm}
            data-testid="confirm-execute"
            data-scope={scope}
          >
            {confirmLabel}
          </Button>
        </div>
      </div>
    </div>
  )
}
