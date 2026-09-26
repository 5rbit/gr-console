// 이벤트 라이브 스트림 — `/api/events/stream` 의 이름 붙은 이벤트 `evt`(행 하나) + `lag`(건너뛴 수).
//
// `SseFeed` 가 아닌 이유는 `traceStream` 과 같다: 이름이 둘이고, 필요한 것은 마지막 메시지가 아니라
// **모든 행**이다. 이름 붙은 이벤트는 `onmessage` 로 오지 않으므로 `addEventListener('evt')` 로 받는다.
import { frameThrottle } from '../sse'
import { Store } from '../store'
import { EVT_STREAM_URL, type EventRow } from './api'

class EvtStream extends Store {
  #es: EventSource | null = null
  #refs = 0
  #connected = false
  #error: string | null = null
  #lag = 0
  #handlers = new Set<(r: EventRow) => void>()
  #flush = frameThrottle(() => this.notify())

  get connected(): boolean {
    return this.#connected
  }
  get error(): string | null {
    return this.#error
  }
  /** 혼잡으로 서버가 건너뛴 행 수(누적) — 0 이 아니면 라이브 목록에 빠진 행이 있다. */
  get lag(): number {
    return this.#lag
  }
  get active(): boolean {
    return this.#refs > 0
  }

  /** 행 단위 리스너(코얼레싱 없음). */
  onRow(fn: (r: EventRow) => void): () => void {
    this.#handlers.add(fn)
    return () => {
      this.#handlers.delete(fn)
    }
  }

  resetLag(): void {
    this.#lag = 0
    this.notify()
  }

  start(): () => void {
    this.#refs++
    if (this.#refs === 1) this.#open()
    let released = false
    return () => {
      if (released) return
      released = true
      this.#refs--
      if (this.#refs === 0) this.#close()
    }
  }

  #open(): void {
    if (typeof EventSource === 'undefined') return
    const es = new EventSource(EVT_STREAM_URL)
    es.onopen = () => {
      this.#connected = true
      this.#error = null
      this.#flush()
    }
    es.onerror = () => {
      this.#connected = false
      this.#error = '스트림 끊김 — 재연결 대기'
      this.#flush()
    }
    es.addEventListener('evt', (ev) => {
      let r: EventRow
      try {
        r = JSON.parse((ev as MessageEvent<string>).data) as EventRow
      } catch {
        return
      }
      this.#connected = true
      for (const h of this.#handlers) h(r)
      this.#flush()
    })
    es.addEventListener('lag', (ev) => {
      const n = Number((ev as MessageEvent<string>).data)
      this.#lag += Number.isFinite(n) ? n : 0
      this.#flush()
    })
    this.#es = es
  }

  #close(): void {
    this.#flush.cancel()
    this.#es?.close()
    this.#es = null
    this.#connected = false
    this.notify()
  }
}

export const evtStream = new EvtStream()
