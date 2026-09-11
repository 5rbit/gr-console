// 프레임워크 무관 스토어 바닥 — 룬(`$state`)이 하던 일을 React가 읽을 수 있는 모양으로 옮긴다.
//
// 규약은 `lib/ui/toast.ts`가 먼저 쓰던 것과 같다: 모듈이 상태를 소유하고, 구독자에게 알리고,
// 화면은 `useSyncExternalStore`로 읽는다. 컴포넌트 밖(fetch 래퍼·이벤트 핸들러·SSE 콜백)에서도
// 그대로 쓸 수 있는 것이 이 모양의 값이다 — 훅 안에 상태를 가두면 그 자리들이 못 읽는다.
//
// **스냅샷은 버전 숫자다.** 상태 객체 자체를 스냅샷으로 내면 필드를 하나 고칠 때마다 새 객체를
// 만들어야 하고, 안 만들면 React가 변화를 못 본다. 버전을 세면 스토어가 자기 모양을 자유롭게
// 들고 있을 수 있고 `Object.is` 비교도 항상 안정적이다.

import { useSyncExternalStore } from 'react'

export class Store {
  #listeners = new Set<() => void>()
  #version = 0

  /** `useSyncExternalStore`의 구독 — 화살표 함수라 참조가 안정적이다(매 렌더 재구독을 막는다). */
  subscribe = (fn: () => void): (() => void) => {
    this.#listeners.add(fn)
    return () => {
      this.#listeners.delete(fn)
    }
  }

  getSnapshot = (): number => this.#version

  /** 값이 바뀐 뒤에 부른다. 안 부르면 화면이 옛 값을 든 채로 남는다. */
  protected notify(): void {
    this.#version++
    for (const fn of this.#listeners) fn()
  }
}

/**
 * 구독 가능한 것의 구조적 계약 — `Store`를 상속하지 않아도(인터페이스로만 아는 `RobotSource`처럼)
 * 이 모양이면 `useStore`가 읽는다. `Store`는 비공개 필드를 들고 있어 이름 기반으로 좁혀지므로,
 * 인터페이스 쪽에는 이 구조 계약을 세워 둔다.
 */
export interface Subscribable {
  subscribe(fn: () => void): () => void
  getSnapshot(): number
}

/**
 * 스토어 하나를 구독한다 — 그 스토어가 바뀔 때만 이 컴포넌트가 다시 그려진다.
 *
 * 전역 무효화로 접지 않는 이유는 폴링 때문이다: 축 값이 300ms마다 갱신되는데 전역으로 알리면
 * 파형·격자까지 초당 세 번 다시 그린다.
 */
export function useStore(...stores: Subscribable[]): void {
  // 여러 스토어를 한 훅으로 받는다 — 화면 하나가 nav·platform을 함께 읽는 일이 흔하다.
  for (const s of stores) {
    // eslint-disable-next-line react-hooks/rules-of-hooks -- 인자 개수는 호출부마다 고정이다.
    useSyncExternalStore(s.subscribe, s.getSnapshot, s.getSnapshot)
  }
}
