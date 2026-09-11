import { describe, expect, it } from 'vitest'
import { nextPressure, MAX_PRESSURE } from './congestion'

// 폴(주기)과 렌더(점 수·행 수·프레임)가 **같은 신호**를 본다 — 그 신호의 정책만 여기서 고정한다.
describe('nextPressure — 혼잡 정책', () => {
  it('지연이 크면 물러난다(상한을 넘지 않는다)', () => {
    let p = 1
    for (let i = 0; i < 20; i++) p = nextPressure(p, 500, 0)
    expect(p).toBeGreaterThan(1)
    expect(p).toBeLessThanOrEqual(MAX_PRESSURE)
  })

  it('긴 작업이 창의 40%를 넘어도 물러난다(지연이 0이어도)', () => {
    expect(nextPressure(1, 0, 200)).toBeGreaterThan(1)
    expect(nextPressure(1, 0, 10)).toBe(1)
  })

  it('한가하면 되돌아오되 **천천히** — 감속↔복귀를 왕복하지 않게', () => {
    const up = nextPressure(1, 500, 0)
    const down = nextPressure(up, 0, 0)
    expect(down).toBeLessThan(up)
    expect(down, '한 박동에 다 돌아오면 진동한다').toBeGreaterThan(1)
    let p = up
    for (let i = 0; i < 50; i++) p = nextPressure(p, 0, 0)
    expect(p).toBe(1)
  })
})
