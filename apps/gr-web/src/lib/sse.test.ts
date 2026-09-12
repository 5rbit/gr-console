// SseFeed 가 백엔드의 이름 붙은 SSE 이벤트(`event: status` 등)를 받는지 — 실제 EventSource 처럼
// 이름 붙은 이벤트는 `onmessage` 로 주지 않고 해당 이름의 리스너에게만 준다.
import { afterEach, describe, expect, it } from 'vitest'
import { SseFeed } from './sse'

type Listener = (e: { data: string }) => void

class FakeEventSource {
  static last: FakeEventSource | null = null
  onopen: (() => void) | null = null
  onerror: (() => void) | null = null
  onmessage: Listener | null = null
  listeners = new Map<string, Listener[]>()
  closed = false
  constructor(public url: string) {
    FakeEventSource.last = this
  }
  addEventListener(name: string, fn: Listener) {
    this.listeners.set(name, [...(this.listeners.get(name) ?? []), fn])
  }
  close() {
    this.closed = true
  }
  /** 서버가 `event: <name>` 으로 보낸 프레임. name 이 없으면 이름 없는 message. */
  emit(data: unknown, name?: string) {
    const e = { data: JSON.stringify(data) }
    if (!name || name === 'message') this.onmessage?.(e)
    for (const fn of this.listeners.get(name ?? 'message') ?? []) fn(e)
  }
}

const g = globalThis as Record<string, unknown>
const saved = g.EventSource

afterEach(() => {
  g.EventSource = saved
})

describe('SseFeed', () => {
  it('receives named events when constructed with the event name', () => {
    g.EventSource = FakeEventSource
    const feed = new SseFeed<{ n: number }>('/api/status/stream', 'status')
    const got: number[] = []
    feed.onMessage((m) => got.push(m.n))
    const release = feed.start()
    FakeEventSource.last!.emit({ n: 1 }, 'status')
    expect(feed.data).toEqual({ n: 1 })
    expect(got).toEqual([1])
    // 이름 없는 message 도 여전히 받는다
    FakeEventSource.last!.emit({ n: 2 })
    expect(got).toEqual([1, 2])
    release()
    expect(FakeEventSource.last!.closed).toBe(true)
  })

  it('without an event name, named events are not delivered (the bug this guards)', () => {
    g.EventSource = FakeEventSource
    const feed = new SseFeed<{ n: number }>('/api/x')
    const release = feed.start()
    FakeEventSource.last!.emit({ n: 1 }, 'status')
    expect(feed.data).toBeNull()
    release()
  })

  it('app feeds are wired to the backend event names', async () => {
    const { statusFeed, tasksFeed, runsFeed, stockFeed } = await import('./feeds')
    expect([statusFeed.event, tasksFeed.event, runsFeed.event, stockFeed.event]).toEqual([
      'status',
      'tasks',
      'run',
      'stock',
    ])
  })
})
