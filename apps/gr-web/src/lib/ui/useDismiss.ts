// 바깥 클릭 · Escape 로 닫는 껍데기 — 팝오버·드롭다운이 **같은 닫기 규칙**을 쓰게.
//
// 같은 이펙트가 `IconPopover` 와 `components/items/Info.tsx` 에 두 벌로 있었고, 실제로 갈려 있었다
// (한쪽은 `mousedown`, 한쪽은 `click`). 닫히는 방법이 자리마다 다르면 "이 팝업은 어떻게 닫지"를
// 매번 다시 배운다 — 그래서 훅 하나로 모은다.
//
// `mousedown` 으로 받는다(`click` 이 아니라): 팝오버 안의 버튼을 누르면서 팝오버 밖으로 마우스를
// 끌고 놓는 경우에도 열림/닫힘이 뒤집히지 않고, 여는 클릭이 제 리스너에 바로 잡히지 않는다.
import { useEffect, useRef, type RefObject } from 'react'

/**
 * 열린 동안 바깥 클릭·Escape 를 듣는다. 돌려주는 ref 를 **껍데기 요소**에 건다(그 안쪽 클릭은
 * 바깥으로 치지 않는다).
 *
 * `close` 는 매 렌더 새 함수여도 된다 — 이펙트가 다시 걸릴 뿐 동작은 같다.
 */
export function useDismiss<T extends HTMLElement = HTMLElement>(
  open: boolean,
  close: () => void,
): RefObject<T | null> {
  const root = useRef<T>(null)
  useEffect(() => {
    if (!open) return
    const onDown = (e: MouseEvent) => {
      if (root.current && !root.current.contains(e.target as Node)) close()
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') close()
    }
    document.addEventListener('mousedown', onDown)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('mousedown', onDown)
      document.removeEventListener('keydown', onKey)
    }
  }, [open, close])
  return root
}
