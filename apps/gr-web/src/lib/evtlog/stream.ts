// 이벤트 라이브 스트림 — `/api/events/stream` 의 이름 붙은 이벤트 `evt`(행 하나) + `lag`(건너뛴 수).
// 같은 연결에 `alert` · `alert_ack` 도 온다 — 이 스트림이 열려 있는 동안은 알림 배지가 자기 연결을 닫고
// 이것을 빌려 쓴다(`lib/evtlog/alerts`). 브라우저의 호스트당 연결 수(HTTP/1.1 에서 6)를 한 칸 아낀다.
//
// `SseFeed` 가 아닌 이유는 `traceStream` 과 같다: 이름이 둘이고, 필요한 것은 마지막 메시지가 아니라
// **모든 행**이다. 이름 붙은 이벤트는 `onmessage` 로 오지 않으므로 `addEventListener('evt')` 로 받는다.
import { frameThrottle } from '../sse'
import { Store } from '../store'
import { EVT_STREAM_URL, type AlertRec, type EventRow } from './api'

class EvtStream extends Store {
  #es: EventSource | null = null
  #refs = 0
  #connected = false
  #error: string | null = null
  #lag = 0
  #handlers = new Set<(r: EventRow) => void>()
  #alertHandlers = new Set<(a: AlertRec) => void>()
  #ackHandlers = new Set<(ids: number[]) => void>()
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

  /** 알림(`alert`) · 확인(`alert_ack`, 빈 목록 = 전부) 리스너. */
  onAlert(fn: (a: AlertRec) => void, ack: (ids: number[]) => void): () => void {
    this.#alertHandlers.add(fn)
    this.#ackHandlers.add(ack)
    return () => {
      this.#alertHandlers.delete(fn)
      this.#ackHandlers.delete(ack)
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
    es.addEventListener('alert', (ev) => {
      try {
        const a = JSON.parse((ev as MessageEvent<string>).data) as AlertRec
        for (const h of this.#alertHandlers) h(a)
      } catch {
        // 깨진 프레임은 버린다
      }
    })
    es.addEventListener('alert_ack', (ev) => {
      try {
        const ids = JSON.parse((ev as MessageEvent<string>).data) as number[]
        for (const h of this.#ackHandlers) h(ids)
      } catch {
        // 깨진 프레임은 버린다
      }
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
