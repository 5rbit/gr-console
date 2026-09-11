import { describe, expect, it, vi } from 'vitest'
import { shareGet, invalidateShared, SHARE_MS } from './share'

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms))

describe('shareGet — 같은 조회를 하나로', () => {
  it('동시에 같은 키를 부르면 run은 한 번(워치표+모니터 팝업 N개가 같은 dump를 폴하던 자리)', async () => {
    const run = vi.fn(async () => {
      await wait(10)
      return 'v'
    })
    const [a, b, c] = await Promise.all([
      shareGet('k1', run),
      shareGet('k1', run),
      shareGet('k1', run),
    ])
    expect(run).toHaveBeenCalledTimes(1)
    expect([a, b, c]).toEqual(['v', 'v', 'v'])
  })

  it('키가 다르면 합치지 않는다', async () => {
    const run = vi.fn(async () => 'v')
    await Promise.all([shareGet('a', run), shareGet('b', run)])
    expect(run).toHaveBeenCalledTimes(2)
  })

  it('창이 지나면 다시 나간다 — 캐시가 아니라 합치기다', async () => {
    const run = vi.fn(async () => 'v')
    await shareGet('k2', run)
    await wait(SHARE_MS + 30)
    await shareGet('k2', run)
    expect(run).toHaveBeenCalledTimes(2)
  })

  it('실패는 즉시 버린다 — 오류를 창 내내 나눠 주면 재시도가 그만큼 늦다', async () => {
    const run = vi.fn(async () => {
      throw new Error('boom')
    })
    await expect(shareGet('k3', run)).rejects.toThrow('boom')
    await expect(shareGet('k3', run)).rejects.toThrow('boom')
    expect(run).toHaveBeenCalledTimes(2)
  })

  it('invalidateShared 후에는 다시 나간다 — 명령 직후 조회가 옛 값을 보지 않게', async () => {
    const run = vi.fn(async () => 'v')
    await shareGet('k4', run)
    invalidateShared()
    await shareGet('k4', run)
    expect(run).toHaveBeenCalledTimes(2)
  })
})
