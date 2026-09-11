// 모달 포커스 트랩 — React 훅. 반환한 ref를 모달 상자에 건다. `aria-modal`은 스크린리더에만
// 작용하므로 키보드 포커스는 이렇게 따로 가둔다.
//
// 열 때 첫 포커스 대상은 `[data-autofocus]`가 있으면 그것, 없으면 첫 포커스 가능 요소다. 닫히면
// 열기 전 포커스로 되돌린다.
import { useEffect, useRef } from 'react'
import type { RefObject } from 'react'

/** 포커스 가능 요소 선택자 — 숨김·비활성은 아래에서 걸러 낸다 */
const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'

function focusable(root: HTMLElement): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (el) => el.offsetParent !== null || el === document.activeElement,
  )
}

/** 모달 상자에 건다: 초기 포커스 · Tab 순환 · 닫을 때 복귀 */
export function useFocusTrap<T extends HTMLElement>(active: boolean): RefObject<T | null> {
  const ref = useRef<T | null>(null)

  useEffect(() => {
    const node = ref.current
    if (!active || !node) return

    const prev = document.activeElement as HTMLElement | null

    queueMicrotask(() => {
      const target =
        node.querySelector<HTMLElement>('[data-autofocus]') ?? focusable(node)[0] ?? node
      target.focus()
    })

    function onKey(e: KeyboardEvent): void {
      if (e.key !== 'Tab' || !node) return
      const items = focusable(node)
      if (items.length === 0) {
        e.preventDefault()
        return
      }
      const first = items[0]
      const last = items[items.length - 1]
      const activeEl = document.activeElement as HTMLElement | null
      // 밖으로 나가려는 순간에만 되돌린다 — 안에서 도는 Tab은 브라우저 기본 동작이 자연스럽다.
      if (!e.shiftKey && (activeEl === last || !node.contains(activeEl))) {
        e.preventDefault()
        first.focus()
      } else if (e.shiftKey && (activeEl === first || !node.contains(activeEl))) {
        e.preventDefault()
        last.focus()
      }
    }

    node.addEventListener('keydown', onKey)
    document.addEventListener('keydown', onKey, true) // 포커스가 이미 밖에 있어도 되돌린다.

    return () => {
      node.removeEventListener('keydown', onKey)
      document.removeEventListener('keydown', onKey, true)
      prev?.focus?.()
    }
  }, [active])

  return ref
}
