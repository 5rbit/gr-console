// 폴링 유틸 — 숨은 탭 스킵 + 혼잡 시 전역 감속.

import { pressure, resetPressure } from './congestion'

/** 지금 폴이 물러난 배수(1=무감속). */
export function pollFactor(): number {
  return pressure()
}

/** 테스트 전용. */
export const resetPollFactor = resetPressure

/**
 * `document.hidden`이면 건너뛰는 interval — 반환 id는 `clearInterval`로 정리(기존 계약 그대로).
 *
 * 혼잡하면 실제 주기가 압력만큼 늘어난다(최대 8배). 타이머는 그대로 두고 발화를 버리므로 회복 즉시
 * 제 주기로 돌아온다.
 */
export function visibleInterval(fn: () => void, ms: number): ReturnType<typeof setInterval> {
  let last = 0
  return setInterval(() => {
    if (typeof document !== 'undefined' && document.hidden) return
    const now = Date.now()
    if (now - last < ms * pressure() - 1) return
    last = now
    fn()
  }, ms)
}
