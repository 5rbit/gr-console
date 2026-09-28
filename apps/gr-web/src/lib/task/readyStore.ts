// 스케줄러 준비 상태 — 대상마다 PICK/DROP 가능 여부 · 예약(docs/scheduler.md 준비 상태).
// 백엔드는 바뀔 때만 합친 연결(`/api/stream`)의 `ready` 이벤트로 전체 스냅샷을 보낸다. 이벤트는 바뀔 때만
// 오므로 시작할 때 `GET /api/taskgen/ready` 로 지금 값을 한 번 읽고, 연결이 끊겨 놓친 변화는 느린 재조회로
// 메운다. 합친 연결을 못 쓰면(옛 콘솔) 1 초 폴링. 조회가 실패하면(백엔드 미구현) 점점 드물게 — 빈 값 = 안 그림.
import { useEffect } from 'react'
import { mux } from '../mux'
import { visibleInterval } from '../poll'
import { EMPTY_READY, readyIndex, schedApi, type ReadySnapshot, type TargetReady } from '../sched'
import { Store, useStore } from '../store'
import { toSnapshot } from './readyMarkModel'

const EVENT = 'ready'
const POLL_MS = 1000
/** 스트림을 받는 동안의 재조회 주기 — 재연결 사이에 놓친 변화만 메운다. */
const RESYNC_MS = 15_000
const BACKOFF_MAX_MS = 30_000

class ReadyStore extends Store {
  #snap: ReadySnapshot = EMPTY_READY
  #index = new Map<string, TargetReady>()
  #refs = 0
  #offMux: (() => void) | null = null
  #timer: ReturnType<typeof setInterval> | null = null
  #nextAt = 0
  #fails = 0
  #busy = false
  #key = ''

  get snap(): ReadySnapshot {
    return this.#snap
  }
  /** `targetKey` → 대상. */
  get index(): ReadonlyMap<string, TargetReady> {
    return this.#index
  }

  start(): () => void {
    this.#refs++
    if (this.#refs === 1) this.#open()
    let released = false
    return () => {
      if (released) return
      released = true
      if (--this.#refs === 0) this.#close()
    }
  }

  #set(d: unknown): void {
    const s = toSnapshot(d)
    // 폴링은 같은 값을 매초 다시 받는다 — 바뀌었을 때만 알린다(맵이 다시 그리지 않게).
    const key = JSON.stringify(s.targets)
    if (key === this.#key) return
    this.#key = key
    this.#snap = s
    this.#index = readyIndex(s)
    this.notify()
  }

  async #fetch(): Promise<void> {
    if (this.#busy) return
    this.#busy = true
    try {
      this.#set(await schedApi.ready())
      this.#fails = 0
    } catch {
      this.#fails++
    } finally {
      this.#busy = false
    }
    const base = this.#offMux && !mux.unavailable ? RESYNC_MS : POLL_MS
    const wait = this.#fails ? Math.min(BACKOFF_MAX_MS, POLL_MS * 2 ** this.#fails) : base
    this.#nextAt = Date.now() + Math.max(base, wait)
  }

  #open(): void {
    if (mux.carries(EVENT)) this.#offMux = mux.on(EVENT, (d) => this.#set(d))
    void this.#fetch()
    this.#timer = visibleInterval(() => {
      if (this.#refs === 0 || Date.now() < this.#nextAt) return
      void this.#fetch()
    }, POLL_MS)
  }

  #close(): void {
    this.#offMux?.()
    this.#offMux = null
    if (this.#timer) clearInterval(this.#timer)
    this.#timer = null
    this.#nextAt = 0
  }
}

export const readyStore = new ReadyStore()

/** 준비 상태를 구독한다 — 마운트 동안 피드를 잡는다. `enabled = false` 면 잡지 않는다(빈 값). */
export function useReady(enabled = true): {
  snap: ReadySnapshot
  index: ReadonlyMap<string, TargetReady>
} {
  useStore(readyStore)
  useEffect(() => (enabled ? readyStore.start() : undefined), [enabled])
  return enabled
    ? { snap: readyStore.snap, index: readyStore.index }
    : { snap: EMPTY_READY, index: EMPTY_INDEX }
}

const EMPTY_INDEX: ReadonlyMap<string, TargetReady> = new Map()
