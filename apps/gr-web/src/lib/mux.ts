// 스트림 하나로 — 브라우저는 한 호스트에 HTTP/1.1 연결을 6 개까지만 연다. 화면마다 SSE 를 따로 열면
// 그 수를 넘겨 새 조회(GET)가 영원히 줄을 선다("읽는 중…" 에서 멈춘다). 그래서 이름 붙은 이벤트를
// `/api/stream` 한 연결로 받아 이름으로 나눠 준다.
//
// 콘솔이 이 주소를 모르면(옛 버전) 각 피드가 제 스트림을 연다 — `SseFeed` 가 되돌아간다.

type Handler = (data: unknown) => void

const MUX_URL = '/api/stream'
/** 이 연결이 실어 나르는 이벤트 — 여기 없는 이름(trace · 이벤트 로그)은 제 스트림을 연다. */
export const MUX_EVENTS = new Set(['tasks', 'stock', 'run', 'stations'])

/** 로봇별 상태는 한 연결에 섞이므로 이름에 id 가 붙는다. */
export function muxStatusEvent(robot: number | null | undefined): string {
  return robot === null || robot === undefined ? 'status' : `status:${robot}`
}

class Mux {
  #es: EventSource | null = null
  #refs = 0
  #byEvent = new Map<string, Set<Handler>>()
  #listening = new Set<string>()
  /** 이 연결로 못 받는다고 판정됐다(콘솔이 옛 버전 · 연결 실패 반복) — 피드가 제 스트림을 연다. */
  #unavailable = false
  #fails = 0

  get unavailable(): boolean {
    return this.#unavailable
  }

  /** 이 연결을 쓰지 않는다 — 시험에서 낱개 스트림 경로를 볼 때. */
  disable(): void {
    this.#unavailable = true
    this.#close()
  }

  /** 다시 쓰게 한다(시험 뒷정리). */
  enable(): void {
    this.#unavailable = false
    this.#fails = 0
  }

  /** 이 이벤트를 이 연결로 받을 수 있나. */
  carries(event: string | null): boolean {
    if (this.#unavailable || !event) return false
    return MUX_EVENTS.has(event) || event.startsWith('status')
  }

  /** 구독 — 해제 함수를 돌려준다. 첫 구독자가 연결을 열고 마지막이 닫는다. */
  on(event: string, fn: Handler): () => void {
    let set = this.#byEvent.get(event)
    if (!set) {
      set = new Set()
      this.#byEvent.set(event, set)
    }
    set.add(fn)
    this.#refs++
    this.#open()
    this.#listen(event)
    let released = false
    return () => {
      if (released) return
      released = true
      set.delete(fn)
      this.#refs--
      if (this.#refs === 0) this.#close()
    }
  }

  #open(): void {
    if (this.#es || this.#unavailable || typeof EventSource === 'undefined') return
    const es = new EventSource(MUX_URL)
    es.onerror = () => {
      // 한 번도 못 열렸으면 옛 콘솔로 본다 — 피드가 제 스트림으로 돌아간다.
      if (es.readyState === EventSource.CLOSED && ++this.#fails >= 2) {
        this.#unavailable = true
        this.#close()
      }
    }
    es.onopen = () => {
      this.#fails = 0
    }
    this.#es = es
    for (const e of this.#byEvent.keys()) this.#listen(e)
  }

  #listen(event: string): void {
    const es = this.#es
    if (!es || this.#listening.has(event)) return
    this.#listening.add(event)
    es.addEventListener(event, (m: MessageEvent<string>) => {
      let data: unknown
      try {
        data = JSON.parse(m.data)
      } catch {
        return
      }
      for (const h of this.#byEvent.get(event) ?? []) h(data)
    })
  }

  #close(): void {
    this.#es?.close()
    this.#es = null
    this.#listening.clear()
  }
}

export const mux = new Mux()
