// 측정 로그 스토어 — 스냅샷·최근 항목·축 위치 이력. **사이드바에서 고른 로봇** 것만 든다.
//
// 새 측정이 들어왔는지는 그 로봇 상태 피드의 `webmon.MeasLog.Total`로 안다 — 그 값이 바뀔 때만 스냅샷을
// 다시 받는다(폴링 없음). 축 위치 이력(`axisHist`)은 상태 메시지마다 링에 쌓는다(축 4개 × 300점).
// 로봇 선택이 바뀌면 모두 비우고 새 로봇 피드로 갈아탄 뒤 다시 받는다 — 옛 로봇 응답이 늦게 도착해도
// 세대(`#gen`)가 달라 버린다(GR1 표에 GR2 값이 섞이지 않게).
//
// 항목은 **화면의 Kind·Code 필터를 걸어서** 받는다(`setFilter`). 전에는 필터 없이 최근 200 건만 받아
// 화면에서 걸렀는데, PICK 기록이 대부분이라(최근 200 건 중 Item 43 · Sku 9 — 전체는 378 · 16) Item·SKU
// 이력과 추세가 거의 비어 보였다.
import { api } from './api'
import { statusFeedFor } from './feeds'
import { robots } from './robots'
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
  /** 지금 따라가는 로봇(`undefined` = 아직 붙지 않음, `null` = 기본 로봇). */
  #robot: number | null | undefined = undefined
  /** 항목 필터(0 = 전체) — 백엔드에서 거른 뒤 최근 `ENTRIES_LIMIT` 건을 받는다. */
  #kind = 0
  #code = 0
  #gen = 0
  #release: (() => void) | null = null
  #off: (() => void) | null = null
  #offRobots: (() => void) | null = null
  #releaseRobots: (() => void) | null = null
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
  /** 이 스토어가 보이는 로봇 id(null = 기본 로봇). */
  get robot(): number | null {
    return this.#robot ?? null
  }

  /** 스냅샷·최근 항목을 다시 받는다. */
  async refetch(): Promise<void> {
    const gen = this.#gen
    const robot = this.robot
    this.#loading = true
    this.notify()
    try {
      const [snap, ent] = await Promise.all([
        api.measlogSnapshot(robot),
        api.measlogEntries(
          undefined,
          this.#kind || undefined,
          this.#code || undefined,
          ENTRIES_LIMIT,
          robot,
        ),
      ])
      if (gen !== this.#gen) return
      this.#snapshot = snap
      this.#entries = ent.entries
      this.#lastTotal = snap.total
      this.#error = null
    } catch (e) {
      if (gen !== this.#gen) return
      this.#error = e instanceof Error ? e.message : String(e)
    }
    this.#loading = false
    this.notify()
  }

  /** Kind·Code 필터(0 = 전체)를 바꾸고 다시 받는다 — 같으면 아무것도 안 함. 늦게 온 옛 필터 응답은 세대로 버린다. */
  setFilter(kind: number, code: number): void {
    if (kind === this.#kind && code === this.#code) return
    this.#kind = kind
    this.#code = code
    if (this.#robot === undefined) return // 아직 붙지 않음 — 붙을 때 이 필터로 받는다
    this.#gen++
    void this.refetch()
  }

  /** 다시 읽기 — `full`이면 백엔드가 PLC에서 로그 전체를 다시 긁은 뒤 받는다. */
  async reload(full = false): Promise<void> {
    if (!full) return this.refetch()
    const name = robots.nameOf(this.robot)
    const tid = toast.pending(`${name} 측정 로그를 PLC에서 다시 읽는 중…`)
    try {
      await api.measlogReload(this.robot)
      await this.refetch()
      toast.resolve(tid, 'ok', `${name} 측정 로그 ${this.#snapshot?.total ?? 0}건`)
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

  /** 선택된 로봇으로 갈아탄다(같으면 아무것도 안 함). */
  #follow(): void {
    const next = robots.selected
    if (next === this.#robot) return
    this.#off?.()
    this.#release?.()
    this.#robot = next
    this.#gen++
    this.#snapshot = null
    this.#entries = []
    this.#axisHist = Array.from({ length: AXES }, () => [])
    this.#lastTotal = null
    this.#error = null
    this.#loading = false
    const feed = statusFeedFor(next)
    this.#off = feed.onMessage((ev) => this.#onStatus(ev))
    this.#release = feed.start()
    this.notify()
    void this.refetch()
  }

  /** 구독 수요 등록 — 해제 함수를 돌려준다. */
  start(): () => void {
    this.#refs++
    if (this.#refs === 1) {
      this.#releaseRobots = robots.start()
      this.#offRobots = robots.subscribe(() => this.#follow())
      this.#follow()
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
      this.#offRobots?.()
      this.#releaseRobots?.()
      this.#offRobots = null
      this.#releaseRobots = null
      this.#off?.()
      this.#off = null
      this.#release?.()
      this.#release = null
      this.#robot = undefined
      this.#flush.cancel()
    }
  }
}

export const measlog = new MeasLog()
