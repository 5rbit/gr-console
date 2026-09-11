// 메인스레드 혼잡도 — 폴([`poll`](./poll.ts))과 렌더([`render`](./render.ts))가 함께 보는 하나의 신호.

/** 심장박동 주기 — 이 간격의 지연이 곧 혼잡도다. */
export const BEAT_MS = 250
const LAG_BUSY = 120
const BLOCKED_RATIO = 0.4
/** 압력 상한 — 폴 주기 ×8 · 그림 정밀도 1/8. */
export const MAX_PRESSURE = 8

let level = 1
let blocked = 0
let beat: ReturnType<typeof setInterval> | null = null
let expected = 0

/**
 * 한 박동의 지연·긴 작업 시간으로 다음 압력을 낸다.
 *
 * 오를 때(×1.6)보다 내릴 때(×0.85)를 느리게 둔다 — 즉시 복귀하면 감속↔복귀를 왕복한다.
 */
export function nextPressure(current: number, lag: number, blockedMs: number): number {
  const busy = lag > LAG_BUSY || blockedMs > BEAT_MS * BLOCKED_RATIO
  return busy ? Math.min(MAX_PRESSURE, current * 1.6) : Math.max(1, current * 0.85)
}

function observeLongTasks(): void {
  if (typeof PerformanceObserver === 'undefined') return
  try {
    new PerformanceObserver((list) => {
      for (const e of list.getEntries()) blocked += e.duration
    }).observe({ entryTypes: ['longtask'] })
  } catch {
    /* 미지원 — 지연만으로 판정 */
  }
}

/** 박동 시작(첫 조회 시 한 번). */
export function startCongestion(): void {
  if (beat !== null || typeof setInterval === 'undefined') return
  observeLongTasks()
  expected = Date.now() + BEAT_MS
  beat = setInterval(() => {
    const now = Date.now()
    const lag = now - expected
    expected = now + BEAT_MS
    level = nextPressure(level, lag, blocked)
    blocked = 0
  }, BEAT_MS)
}

/** 지금 압력(1=한가, 최대 [`MAX_PRESSURE`]). */
export function pressure(): number {
  startCongestion()
  return level
}

/** 테스트 전용. */
export function resetPressure(): void {
  level = 1
  blocked = 0
}
