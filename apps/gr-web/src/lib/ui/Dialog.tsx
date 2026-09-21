// 작업 대화상자 — **입력과 편집을 팝업으로** 옮길 때 쓰는 한 벌.
//
// 이미 있는 둘과 자리가 다르다: `ConfirmDialog`는 예/아니오 한 번을 묻는 물건이고, `Modal`은 닫기
// 버튼 하나만 있는 표시용 팝업이라 **제출이 없다**. 화면에서 상시로 자리를 먹던 폼(기록 설정 ·
// 필터 · 검색)을 옮기려면 셋이 필요했다:
//
//  ① 열자마자 **첫 입력에 포커스**가 간다 — 팝업을 연 손이 바로 타이핑으로 이어진다.
//  ② **Enter로 제출**한다(폼의 기본 동작 — 버튼을 찾아 누르지 않는다).
//  ③ **Escape로 닫되, 편집이 남아 있으면 조용히 버리지 않는다**(`dirty`).
//
// ③이 이 파일이 있는 진짜 이유다. 값을 팝업으로 옮기면 화면에서 사라지므로, 실수로 닫았을 때
// 되돌릴 자리가 없다. 확인은 새 모달을 겹쳐 띄우지 않고(오버레이 예산: 두 번째 모달 금지) 같은
// 상자의 바닥 띠를 갈아 끼워 묻는다.
import { useCallback, useEffect, useRef, useState } from 'react'
import type { FormEvent, ReactNode } from 'react'
import { X } from 'lucide-react'
import { Button } from './Button'
import { cn } from '../utils'
import { DIALOG_WIDTH, shouldConfirmDiscard, type CloseIntent, type DialogSize } from './dialogModel'
import { useFocusTrap } from './focusTrap'

export type { CloseIntent, DialogSize }

/** 포커스 가능 요소 — `focusTrap`의 것과 같은 규약(여기서는 **본문의 첫 입력**만 찾는다). */
const FOCUSABLE =
  'input:not([disabled]), select:not([disabled]), textarea:not([disabled]), button:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])'

/**
 * 저장 안 한 편집 지킴이 — 닫기 요청을 받아 그대로 닫거나 되묻는 상태를 돌려준다.
 *
 * 훅으로 뺀 이유: 대화상자 밖(패널 안 인라인 편집기)에서도 같은 규칙이 필요하고, 규칙이 두 벌이면
 * 한쪽만 고쳐진다.
 */
export function useUnsavedGuard(dirty: boolean, close: () => void) {
  const [asking, setAsking] = useState(false)

  const request = useCallback(
    (intent: CloseIntent) => {
      if (shouldConfirmDiscard(dirty, intent)) setAsking(true)
      else {
        setAsking(false)
        close()
      }
    },
    [dirty, close],
  )

  const discard = useCallback(() => {
    setAsking(false)
    close()
  }, [close])

  return { asking, request, discard, keepEditing: () => setAsking(false) }
}

export interface DialogProps {
  open: boolean
  /** 열림 상태 변경 요청 — 백드롭·Escape·닫기·제출이 `false`로 부른다. */
  onOpenChange: (open: boolean) => void
  title: string
  /** 제목 오른쪽의 라벨+값 요약(대상·개수). 조작은 두지 않는다. */
  meta?: ReactNode
  size?: DialogSize
  /** 편집이 남아 있는가 — 참이면 Escape·백드롭·취소가 한 번 되묻는다. */
  dirty?: boolean
  /** 바닥 띠 — 비우면 띠 자체를 그리지 않는다(읽기용 팝업). */
  footer?: ReactNode
  /**
   * **바로 적용되는 설정 팝업**의 바닥 띠 — 닫기 버튼 하나를 이 라벨로 그린다(`footer` 가 있으면
   * 그쪽이 이긴다).
   *
   * 값이 켜는 즉시 반영되는 팝업(보기 설정 · 표시 옵션)에는 "적용/취소"가 거짓말이다 — 이미
   * 적용됐으니 취소할 것이 없다. 그렇다고 바닥 띠를 비우면 닫는 길이 머리줄의 작은 ✕ 하나뿐이라
   * 터치와 좁은 화면에서 찾기 어렵다. 그래서 **닫기 하나**를 킷이 그린다(팔렛이 손으로 짜던 것).
   */
  closeLabel?: string
  testid?: string
  children?: ReactNode
}

/** 일반 작업 대화상자 — 바닥 띠를 호출자가 채운다. 폼이면 `FormDialog`를 쓴다. */
export function Dialog({
  open,
  onOpenChange,
  title,
  meta,
  size = 'md',
  dirty = false,
  footer,
  closeLabel,
  testid,
  children,
}: DialogProps) {
  const box = useFocusTrap<HTMLDivElement>(open)
  const body = useRef<HTMLDivElement>(null)
  const close = useCallback(() => onOpenChange(false), [onOpenChange])
  const guard = useUnsavedGuard(dirty, close)
  const { request } = guard

  // 첫 포커스를 **본문의 첫 입력**으로 옮긴다. `focusTrap`은 상자의 첫 포커스 요소를 잡는데,
  // 그건 머리줄의 닫기(✕)라 팝업을 연 손이 한 번 더 Tab을 눌러야 한다. 표식만 달아 두면
  // `focusTrap`의 초기 포커스가 이것을 고른다(그 마이크로태스크가 이 이펙트 뒤에 돈다).
  useEffect(() => {
    if (!open) return
    const first = body.current?.querySelector<HTMLElement>(FOCUSABLE)
    first?.setAttribute('data-autofocus', '')
  }, [open, children])

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      e.stopPropagation()
      request('esc')
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open, request])

  if (!open) return null

  // 바로 적용되는 설정 팝업의 기본 바닥 띠 — 닫기 하나. `footer` 를 준 쪽이 이긴다.
  const bar =
    footer ??
    (closeLabel ? (
      <Button size="sm" intent="neutral" onClick={() => request('cancel')} data-testid="dialog-close">
        {closeLabel}
      </Button>
    ) : null)

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <div className="absolute inset-0 bg-black/40" onClick={() => request('backdrop')} />
      <div
        ref={box}
        // `text-left`: 대화상자는 어디서 열렸든 제 정렬을 가진다(표의 액션 열 안에서 열릴 수 있다).
        className={cn(
          'relative flex w-full flex-col rounded-lg border border-line-default bg-surface-panel text-left shadow-xl',
          DIALOG_WIDTH[size],
        )}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        data-testid={testid}
      >
        <div className="flex items-center gap-2 border-b border-line-subtle px-4 py-2">
          <span className="text-sm font-semibold">{title}</span>
          {meta ? (
            <span className="flex min-w-0 items-center gap-2 text-2xs text-content-muted">
              {meta}
            </span>
          ) : null}
          <span className="flex-1" />
          <button
            type="button"
            title="닫기"
            aria-label="닫기"
            className="rounded p-1 text-content-faint hover:bg-surface-inset hover:text-content-primary"
            onClick={() => request('cancel')}
          >
            <X className="h-3.5 w-3.5" />
          </button>
        </div>
        <div ref={body} className="max-h-[70vh] min-h-0 overflow-y-auto px-4 py-3 text-xs">
          {children}
        </div>
        {guard.asking ? (
          <DiscardBar onKeep={guard.keepEditing} onDiscard={guard.discard} />
        ) : bar ? (
          <div className="flex items-center justify-end gap-2 border-t border-line-subtle px-4 py-2">
            {bar}
          </div>
        ) : null}
      </div>
    </div>
  )
}

/** 버림 확인 띠 — 두 번째 모달을 겹치지 않고 바닥 띠만 갈아 끼운다. */
function DiscardBar({ onKeep, onDiscard }: { onKeep: () => void; onDiscard: () => void }) {
  return (
    <div
      className="flex items-center gap-2 border-t border-warn bg-warn-soft px-4 py-2"
      data-testid="dialog-discard"
    >
      <span className="text-xs text-warn-fg">저장하지 않은 입력이 있습니다</span>
      <span className="flex-1" />
      <Button size="sm" intent="ghost" onClick={onDiscard}>
        버리고 닫기
      </Button>
      <Button size="sm" intent="primary" data-autofocus onClick={onKeep}>
        계속 편집
      </Button>
    </div>
  )
}

export interface FormDialogProps extends Omit<DialogProps, 'footer' | 'closeLabel'> {
  /** 제출 — Enter 또는 제출 버튼. 닫기는 호출자가 정한다(실패하면 열어 둔다). */
  onSubmit: () => void
  submitLabel?: string
  cancelLabel?: string
  /** 제출을 막는 사유 — 회색으로 침묵하지 않게 `title`로 나간다. */
  disabledReason?: string
  busy?: boolean
  danger?: boolean
  /** 제출 버튼 왼쪽에 서는 보조 조작(초기화 따위). */
  extra?: ReactNode
}

/**
 * 입력 대화상자 — **Enter 제출 · Escape 취소 · 첫 입력 포커스**.
 *
 * `<form>`으로 감싸는 이유가 Enter다. 키 핸들러로 흉내 내면 select·체크박스 안에서 동작이 갈리고,
 * 브라우저가 이미 하는 일을 다시 만드는 셈이다(외부 관례를 거스르지 않는다).
 */
export function FormDialog({
  onSubmit,
  submitLabel = '적용',
  cancelLabel = '취소',
  disabledReason,
  busy = false,
  danger = false,
  extra,
  ...rest
}: FormDialogProps) {
  function submit(e: FormEvent) {
    e.preventDefault()
    if (disabledReason) return
    onSubmit()
  }

  return (
    <Dialog
      {...rest}
      footer={
        <>
          {extra}
          <Button size="sm" intent="ghost" onClick={() => rest.onOpenChange(false)}>
            {cancelLabel}
          </Button>
          <Button
            size="sm"
            type="submit"
            form={`${rest.testid ?? 'form'}-dialog`}
            intent={danger ? 'danger' : 'primary'}
            loading={busy}
            disabled={!!disabledReason}
            title={disabledReason}
            data-testid="dialog-submit"
          >
            {submitLabel}
          </Button>
        </>
      }
    >
      {/* 바닥 띠의 버튼이 `form` 속성으로 이 폼을 가리킨다 — 띠가 본문 밖에 있어도 Enter와 클릭이
          같은 제출을 탄다. */}
      <form id={`${rest.testid ?? 'form'}-dialog`} onSubmit={submit} className="space-y-3">
        {rest.children}
      </form>
    </Dialog>
  )
}
