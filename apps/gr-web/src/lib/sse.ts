// SSE 피드 — 한 스트림을 여러 화면이 나눠 읽는 참조 세기 스토어.
//
// sh4w-web `platform.ts`의 라이브 구독 관용구를 일반화했다: 수신은 모으기만 하고 **상태 쓰기(=렌더)는
// 프레임에 묶는다**(`frameThrottle`). 20Hz로 오는 상태를 매 메시지마다 알리면 사이드바·상태바·화면이
// 초당 스무 번 다시 그린다. 메시지 단위로 반응해야 하는 소비자(작업 이벤트 적용·이력 링)는
// `onMessage`로 **코얼레싱 없이** 받는다.
//
// `start()`는 해제 함수를 돌려준다 — 여러 화면이 겹쳐 열려도 EventSource는 하나이고, 마지막이 놓을 때
// 닫힌다. `EventSource`는 스스로 재연결하므로 여기서 재시도 로직을 두지 않는다.

import { useEffect } from 'react'
import { pressure } from './congestion'
import { Store, useStore } from './store'

/**
 * rAF에 묶은 호출 — 한 프레임에 여러 번 불러도 한 번만 실행된다. 혼잡(`pressure`)하면 프레임을
 * 건너뛰어 더 드물게 그린다. (sh4w-web `lib/render.ts`의 `frameThrottle`을 그대로 옮겼다.)
 */
export function frameThrottle(fn: () => void): { (): void; cancel(): void } {
  let queued = false
  let skips = 0
  let raf = 0
  const raf_ = typeof requestAnimationFrame === 'function' ? requestAnimationFrame : null
  const tick = () => {
    if (skips > 0) {
      skips--
      raf = raf_ ? raf_(tick) : 0
      return
    }
    queued = false
    raf = 0
    fn()
  }
  const run = () => {
    if (queued) return
    queued = true
    skips = Math.max(0, Math.round(pressure()) - 1)
    if (raf_) raf = raf_(tick)
    else {
      queued = false
      fn()
    }
  }
  run.cancel = () => {
    if (raf && typeof cancelAnimationFrame === 'function') cancelAnimationFrame(raf)
    queued = false
    raf = 0
  }
  return run as { (): void; cancel(): void }
}

export class SseFeed<T> extends Store {
  readonly url: string
  /** 백엔드가 `event: <이름>` 으로 보내는 이벤트 이름. 이름 붙은 이벤트는 `onmessage` 로 오지 않으므로
   *  반드시 `addEventListener(이름)` 으로 받아야 한다(없으면 이름 없는 `message` 만 받는다). */
  readonly event: string | null
  #es: EventSource | null = null
  #refs = 0
  #data: T | null = null
  #connected = false
  #lastAt: number | null = null
  #error: string | null = null
  #handlers = new Set<(msg: T) => void>()
  /** 프레임에 묶인 알림 — 메시지가 몰려도 렌더는 프레임당 한 번. */
  #flush = frameThrottle(() => this.notify())

  constructor(url: string, event: string | null = null) {
    super()
    this.url = url
    this.event = event
  }

  /** 마지막으로 받은 메시지(아직 없으면 `null`). */
  get data(): T | null {
    return this.#data
  }
  /** 스트림이 열려 있는가(EventSource가 재연결 중이면 거짓). */
  get connected(): boolean {
    return this.#connected
  }
  /** 마지막 메시지 시각(`Date.now()`), 없으면 `null`. */
  get lastAt(): number | null {
    return this.#lastAt
  }
  /** 마지막 연결 오류(정상이면 `null`). */
  get error(): string | null {
    return this.#error
  }
  /** 구독자 수 — 0이면 닫혀 있다. */
  get active(): boolean {
    return this.#refs > 0
  }

  /** 메시지 단위 리스너(코얼레싱 없음). 해제 함수를 돌려준다. */
  onMessage(fn: (msg: T) => void): () => void {
    this.#handlers.add(fn)
    return () => {
      this.#handlers.delete(fn)
    }
  }

  /** 구독 수요 등록 — 첫 구독자가 스트림을 열고, 반환된 해제 함수의 마지막 호출이 닫는다.
   *  해제 함수는 두 번 불러도 한 번만 센다(effect cleanup이 겹쳐도 안전). */
  start(): () => void {
    this.#refs++
    if (this.#refs === 1) this.#open()
    let released = false
    return () => {
      if (released) return
      released = true
      this.stop()
    }
  }

  /** 구독 하나를 놓는다(`start()`의 해제 함수가 부른다). */
  stop(): void {
    if (this.#refs === 0) return
    this.#refs--
    if (this.#refs === 0) this.#close()
  }

  #open(): void {
    if (typeof EventSource === 'undefined') return
    const es = new EventSource(this.url)
    es.onopen = () => {
      this.#connected = true
      this.#error = null
      this.#flush()
    }
    es.onerror = () => {
      // EventSource가 스스로 재연결한다 — 그동안 "끊김"만 알린다.
      this.#connected = false
      this.#error = '스트림 끊김 — 재연결 대기'
      this.#flush()
    }
    const onData = (e: MessageEvent<string>) => {
      let msg: T
      try {
        msg = JSON.parse(e.data) as T
      } catch {
        return
      }
      this.#data = msg
      this.#lastAt = Date.now()
      this.#connected = true
      for (const h of this.#handlers) h(msg)
      this.#flush()
    }
    es.onmessage = onData
    if (this.event) es.addEventListener(this.event, onData as EventListener)
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

/** 컴포넌트가 피드를 구독한다 — 마운트 동안 스트림을 잡고, 갱신마다 다시 그린다. */
export function useSse<T>(feed: SseFeed<T>): SseFeed<T> {
  useStore(feed)
  useEffect(() => feed.start(), [feed])
  return feed
}
