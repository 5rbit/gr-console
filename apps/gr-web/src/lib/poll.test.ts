import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest'
import { visibleInterval, resetPollFactor } from './poll'

describe('visibleInterval', () => {
  beforeEach(() => {
    resetPollFactor()
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('한가하면 제 주기로 그대로 돈다', () => {
    const fn = vi.fn()
    const t = visibleInterval(fn, 1000)
    vi.advanceTimersByTime(3000)
    expect(fn).toHaveBeenCalledTimes(3)
    clearInterval(t)
  })

  it('clearInterval로 멈춘다 — 핸들 계약을 그대로 둔다(호출부 22곳을 안 바꾸는 근거)', () => {
    const fn = vi.fn()
    const t = visibleInterval(fn, 500)
    vi.advanceTimersByTime(1000)
    const n = fn.mock.calls.length
    clearInterval(t)
    vi.advanceTimersByTime(5000)
    expect(fn).toHaveBeenCalledTimes(n)
  })

  // `document.hidden` 스킵은 이 스위트(vitest environment=node)에서 잴 수 없다 — DOM이 없어 가드가
  // 통째로 비활성이다. 브라우저 계측(숨은 탭 요청 0건)으로 확인한다.
})
