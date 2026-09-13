// 요소의 실제 폭을 px로 읽는다(ResizeObserver) — 표가 **자기 컨테이너 폭**에 반응하기 위해.
//
// CSS 컨테이너 쿼리로는 안 되는 이유: 열을 CSS로 숨기면 **무엇이 숨었는지 JS가 모른다.** 접힌 열을
// 행 펼치기로 되살려야 하니(`priority+` 패턴) 어느 열이 접혔는지 알아야 하고, 그건 폭을 숫자로
// 아는 쪽만 할 수 있다. 머리띠(`ScreenHeader`)처럼 **접고 끝나는** 자리는 CSS 컨테이너 쿼리가 맞다.
//
// **`RefObject`가 아니라 콜백 ref를 돌려준다.** `useRef`로 받으면 효과가 마운트 때 한 번 돌고,
// 그 시점에 요소가 없으면(표가 로딩 스켈레톤이나 빈 상태를 그리는 중이면) 다시 관찰하지 않는다 —
// 행이 도착해 표가 뒤늦게 마운트되면 폭을 영원히 `null`로 보고 열이 하나도 접히지 않았다.
// 요소를 **상태로** 들면 요소가 생기는 순간 효과가 다시 돈다.
//
// 정수 px로 줄여 비교한다 — 소수점 폭(레티나·zoom)이 흔들리면 상태가 매 프레임 갱신되고, 그러면
// 폴링으로 이미 바쁜 표가 리사이즈만으로 다시 그려진다.
import { useEffect, useState } from 'react'

/**
 * `[ref, width]` — `ref`를 재려는 요소에 걸면 `width`가 그 요소의 폭(px)이 된다.
 * 아직 재지 못했으면 `null`(호출부가 "모른다"를 구분할 수 있게 — 0으로 내면 첫 페인트에서
 * "가장 좁다"로 읽혀 열이 깜빡인다).
 */
export function useElementWidth<T extends HTMLElement>(): [(el: T | null) => void, number | null] {
  const [el, setEl] = useState<T | null>(null)
  const [width, setWidth] = useState<number | null>(null)

  useEffect(() => {
    if (!el) return
    // 구형·비브라우저 환경(테스트) — 폭을 모르는 채로 둔다(전부 보인다).
    if (typeof ResizeObserver === 'undefined') return

    const ro = new ResizeObserver((entries) => {
      const w = Math.round(entries[0]?.contentRect.width ?? el.clientWidth)
      setWidth((prev) => (prev === w ? prev : w))
    })
    ro.observe(el)
    setWidth(Math.round(el.clientWidth))
    return () => ro.disconnect()
  }, [el])

  return [setEl, width]
}
