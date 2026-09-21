// 라이브 트레이스 스트림 — `/api/trace/stream` 의 청크를 링 버퍼에 쌓는 스토어.
//
// **`SseFeed` 를 쓰지 않는 이유**가 둘이다.
//  ① 이 스트림은 이름 붙은 이벤트가 **둘**이다(`trace` 와 혼잡 시 `lag`). `SseFeed` 는 이름 하나를
//     쥐는 계약이고, 같은 URL 로 피드를 둘 열면 EventSource 도 둘이 되어 백엔드 구독이 두 벌 된다.
//  ② 여기서 필요한 것은 "마지막 메시지"가 아니라 **모든 메시지의 누적**이다(한 프레임에 청크가
//     여러 개 와도 한 줄도 버리면 안 된다). 그 점만 빼고 관용구는 그대로다 — 참조 세기로 열고
//     닫고, 상태 알림은 `frameThrottle` 로 프레임에 묶는다(초당 수십 청크에 매번 렌더할 수 없다).
import { Store } from '../../lib/store'
import { frameThrottle } from '../../lib/sse'
import { TRACE_STREAM_URL, type TraceEvent, type TraceMark, type TraceMeta } from '../../lib/trace/api'
import { RING_MS, RING_ROWS, TraceRing } from './traceModel'

/** 화면에 남기는 마크 수 — 세로줄이 이보다 많으면 파형이 안 보인다. */
const MARK_MAX = 200

class TraceLive extends Store {
  readonly ring = new TraceRing(RING_ROWS, RING_MS)
  #marks: TraceMark[] = []
  #id = ''
  #rows = 0
  #chunks = 0
  #overrun = 0
  #lag = 0
  #connected = false
  #error: string | null = null
  #stopped: string | null = null
  /** 그린 뒤 바뀐 것이 있는지 차트가 보는 표 — 링은 제자리에서 바뀌므로 참조 비교가 안 된다. */
  #version = 0
  #es: EventSource | null = null
  #refs = 0
  #flush = frameThrottle(() => this.notify())

  get marks(): readonly TraceMark[] {
    return this.#marks
  }
  get sessionId(): string {
    return this.#id
  }
  get rows(): number {
    return this.#rows
  }
  get chunks(): number {
    return this.#chunks
  }
  get overrun(): number {
    return this.#overrun
  }
  get lag(): number {
    return this.#lag
  }
  get connected(): boolean {
    return this.#connected
  }
  get error(): string | null {
    return this.#error
  }
  /** 마지막 `stopped` 이벤트의 사유(정상 정지면 빈 문자열, 받은 적 없으면 `null`). */
  get stopped(): string | null {
    return this.#stopped
  }
  get version(): number {
    return this.#version
  }

  /** 새 세션을 시작했을 때 — 앞 세션의 파형·집계를 버린다. */
  reset(id = ''): void {
    this.ring.reset(id, 0)
    this.#marks = []
    this.#id = id
    this.#rows = 0
    this.#chunks = 0
    this.#overrun = 0
    this.#lag = 0
    this.#stopped = null
    this.#version++
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

  #apply(e: TraceEvent): void {
    if (e.event === 'chunk') {
      if (e.id !== this.#id) {
        // 브라우저가 늦게 붙었거나 다른 창이 세션을 갈았다 — 새 세션의 첫 청크가 기준이다.
        this.ring.reset(e.id, 0)
        this.#marks = []
        this.#id = e.id
        this.#chunks = 0
        this.#overrun = 0
        this.#stopped = null
      }
      this.ring.push(e.id, e.rows)
      this.#rows = e.total
      this.#chunks++
      this.#overrun += e.overrun
      this.#version++
    } else if (e.event === 'mark') {
      this.#marks = [...this.#marks, e.mark].slice(-MARK_MAX)
      this.#version++
    } else if (e.event === 'stopped') {
      this.#stopped = e.reason ?? ''
      this.#version++
    }
  }

  /** 백엔드 `Meta` 로 집계를 맞춘다 — 폴이 세션을 물어 올 때(브라우저가 늦게 붙은 경우) 쓴다. */
  adopt(meta: TraceMeta | null): void {
    if (!meta) return
    if (meta.id !== this.#id) {
      this.ring.reset(meta.id, 0)
      this.#marks = []
      this.#id = meta.id
      this.#stopped = null
    }
    this.#rows = Math.max(this.#rows, meta.rows)
    this.#chunks = Math.max(this.#chunks, meta.chunks)
    this.#overrun = Math.max(this.#overrun, meta.overrun)
    this.#version++
    this.#flush()
  }

  #open(): void {
    if (typeof EventSource === 'undefined') return
    const es = new EventSource(TRACE_STREAM_URL)
    es.onopen = () => {
      this.#connected = true
      this.#error = null
      this.#flush()
    }
    es.onerror = () => {
      // EventSource 가 스스로 재연결한다 — 그동안 "끊김"만 알린다. 끊긴 구간의 행은
      // 세션 파일에는 남아 있으므로 재생으로 볼 수 있다.
      this.#connected = false
      this.#error = '스트림 끊김 — 재연결 대기'
      this.#flush()
    }
    // 이름 붙은 이벤트는 `onmessage` 로 오지 않는다.
    es.addEventListener('trace', (ev) => {
      let msg: TraceEvent
      try {
        msg = JSON.parse((ev as MessageEvent<string>).data) as TraceEvent
      } catch {
        return
      }
      this.#connected = true
      this.#apply(msg)
      this.#flush()
    })
    // 혼잡으로 브로드캐스트가 밀리면 백엔드가 건너뛴 개수를 보낸다(본문은 숫자 하나).
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

/** 앱에 하나 — 화면이 마운트된 동안만 스트림이 열린다. */
export const traceLive = new TraceLive()
