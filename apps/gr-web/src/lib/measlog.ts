// 측정 로그 스토어 — 스냅샷·최근 항목·축 위치 이력.
//
// 새 측정이 들어왔는지는 `statusFeed`의 `webmon.MeasLog.Total`로 안다 — 그 값이 바뀔 때만 스냅샷을
// 다시 받는다(폴링 없음). 축 위치 이력(`axisHist`)은 상태 메시지마다 링에 쌓는다(축 4개 × 300점).
import { api } from './api'
import { statusFeed } from './feeds'
import { frameThrottle } from './sse'
import { Store } from './store'
import { toast } from './ui/toast'
import type { MeasLogEntry, MeasLogSnapshot, StatusEvent } from './types'

/** 축 위치 이력 길이(점). */
export const AXIS_HIST = 300
const AXES = 4
const ENTRIES_LIMIT = 200

class MeasLog extends Store {
  #snapshot: MeasLogSnapshot | null = null
  #entries: MeasLogEntry[] = []
  #axisHist: number[][] = Array.from({ length: AXES }, () => [])
  #lastTotal: number | null = null
  #error: string | null = null
  #loading = false
  #refs = 0
  #release: (() => void) | null = null
  #off: (() => void) | null = null
  #flush = frameThrottle(() => this.notify())

  get snapshot(): MeasLogSnapshot | null {
    return this.#snapshot
  }
  /** 최근 항목(Seq 내림차순 — 백엔드 순서 그대로). */
  get entries(): readonly MeasLogEntry[] {
    return this.#entries
  }
  /** 축별 위치 이력 — `axisHist[axis]`가 오래된 것부터 최신 순. */
  get axisHist(): readonly (readonly number[])[] {
    return this.#axisHist
  }
  get error(): string | null {
    return this.#error
  }
  get loading(): boolean {
    return this.#loading
  }

  /** 스냅샷·최근 항목을 다시 받는다. */
  async refetch(): Promise<void> {
    this.#loading = true
    this.notify()
    try {
      const [snap, ent] = await Promise.all([
        api.measlogSnapshot(),
        api.measlogEntries(undefined, undefined, undefined, ENTRIES_LIMIT),
      ])
      this.#snapshot = snap
      this.#entries = ent.entries
      this.#lastTotal = snap.total
      this.#error = null
    } catch (e) {
      this.#error = e instanceof Error ? e.message : String(e)
    }
    this.#loading = false
    this.notify()
  }

  /** 다시 읽기 — `full`이면 백엔드가 PLC에서 로그 전체를 다시 긁은 뒤 받는다. */
  async reload(full = false): Promise<void> {
    if (!full) return this.refetch()
    const tid = toast.pending('측정 로그를 PLC에서 다시 읽는 중…')
    try {
      await api.measlogReload()
      await this.refetch()
      toast.resolve(tid, 'ok', `측정 로그 ${this.#snapshot?.total ?? 0}건`)
    } catch (e) {
      toast.resolve(tid, 'error', e instanceof Error ? e.message : String(e))
    }
  }

  #onStatus(ev: StatusEvent): void {
    const wm = ev.webmon
    // 축 위치 링.
    for (let i = 0; i < AXES; i++) {
      const pos = wm.Axis[i]?.Position
      if (pos === undefined) continue
      const ring = this.#axisHist[i]
      ring.push(pos)
      if (ring.length > AXIS_HIST) ring.splice(0, ring.length - AXIS_HIST)
    }
    // 새 측정이 쌓였으면 다시 받는다(첫 메시지는 초기 로드가 맡는다).
    const total = wm.MeasLog.Total
    if (this.#lastTotal !== null && total !== this.#lastTotal && !this.#loading) {
      this.#lastTotal = total
      void this.refetch()
    }
    this.#flush()
  }

  /** 구독 수요 등록 — 해제 함수를 돌려준다. */
  start(): () => void {
    this.#refs++
    if (this.#refs === 1) {
      this.#off = statusFeed.onMessage((ev) => this.#onStatus(ev))
      this.#release = statusFeed.start()
      void this.refetch()
    }
    let released = false
    return () => {
      if (released) return
      released = true
      this.stop()
    }
  }

  stop(): void {
    if (this.#refs === 0) return
    this.#refs--
    if (this.#refs === 0) {
      this.#off?.()
      this.#off = null
      this.#release?.()
      this.#release = null
      this.#flush.cancel()
    }
  }
}

export const measlog = new MeasLog()
