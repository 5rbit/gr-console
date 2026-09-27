// 이벤트 화면 상태 — 모듈에 하나라 탭을 옮겨 다녀도 필터·목록·라이브 여부·하위 보기가 남는다.
import { frameThrottle } from '../sse'
import { Store } from '../store'
import {
  evtApi,
  type AlarmStats,
  type EventRow,
  type EvtCatalog,
  type EvtSource,
  type SavedFilter,
  type StepStats,
  type Swimlane,
} from './api'
import { EMPTY_FILTER, buildQuery, matchesFilter, type EvtFilter } from './evtFilterModel'
import { PAGE_SIZE, ROW_CAP, mergeRows } from './evtRowsModel'
import { EMPTY_STATS, statsQuery, type StatsState } from './evtStatsModel'
import { type EvtUrlState, type EvtViewId } from './evtUrlModel'
import { EMPTY_SWIM, swimQuery, type SwimState } from './swimlaneModel'
import { evtStream } from './stream'

/** 하위 보기 하나의 조회 결과. */
export interface Loaded<T> {
  data: T | null
  loading: boolean
  error: string | null
}

const idle = <T>(): Loaded<T> => ({ data: null, loading: false, error: null })

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

class EvtView extends Store {
  #filter: EvtFilter = EMPTY_FILTER
  #rows: EventRow[] = []
  #next: string | null = null
  #trimmed = false
  #loaded = false
  #loading = false
  #loadingMore = false
  #error: string | null = null
  #req = 0
  #live = false
  #liveBuf: EventRow[] = []
  #releaseLive: (() => void) | null = null
  #catalog: EvtCatalog | null = null
  #catalogError: string | null = null
  #sources: EvtSource[] | null = null
  #sourcesError: string | null = null
  #flushLive = frameThrottle(() => this.#drainLive())
  #view: EvtViewId = 'list'
  #stats: StatsState = EMPTY_STATS
  #swim: SwimState = EMPTY_SWIM
  #alarms: Loaded<AlarmStats> = idle()
  #steps: Loaded<StepStats> = idle()
  #lanes: Loaded<Swimlane> = idle()
  #focus: EventRow | null = null
  #saved: SavedFilter[] = []
  #subReq = 0

  get view(): EvtViewId {
    return this.#view
  }
  get stats(): StatsState {
    return this.#stats
  }
  get swim(): SwimState {
    return this.#swim
  }
  get alarms(): Loaded<AlarmStats> {
    return this.#alarms
  }
  get steps(): Loaded<StepStats> {
    return this.#steps
  }
  get lanes(): Loaded<Swimlane> {
    return this.#lanes
  }
  get saved(): readonly SavedFilter[] {
    return this.#saved
  }
  /** URL · 저장된 필터가 담는 화면 상태. */
  get urlState(): EvtUrlState {
    return { view: this.#view, filter: this.#filter, stats: this.#stats, swim: this.#swim }
  }

  setView(v: EvtViewId): void {
    if (v === this.#view) return
    this.#view = v
    this.notify()
    void this.loadView()
  }

  setStats(s: StatsState): void {
    this.#stats = s
    this.notify()
    void this.loadView()
  }

  setSwim(s: SwimState): void {
    this.#swim = s
    this.notify()
    void this.loadView()
  }

  /** 링크 · 저장된 필터를 한 번에 — 목록과 지금 보기를 다시 받는다. */
  applyUrl(st: EvtUrlState): void {
    this.#view = st.view
    this.#filter = st.filter
    this.#stats = st.stats
    this.#swim = st.swim
    this.notify()
    void this.reload()
    void this.loadView()
  }

  /** 다른 화면(알림 배지)이 고른 행 — 이벤트 화면이 전후 창으로 연다. */
  focus(r: EventRow): void {
    this.#focus = r
    this.#view = 'list'
    this.notify()
  }

  consumeFocus(): EventRow | null {
    const r = this.#focus
    if (r) {
      this.#focus = null
      this.notify()
    }
    return r
  }

  /** 지금 보기의 자료 — 목록은 `reload` 가 맡는다. */
  async loadView(): Promise<void> {
    const req = ++this.#subReq
    const now = Date.now()
    const run = async <T>(
      get: () => Loaded<T>,
      set: (l: Loaded<T>) => void,
      fetch: () => Promise<T>,
    ): Promise<void> => {
      set({ ...get(), loading: true, error: null })
      this.notify()
      try {
        const data = await fetch()
        if (req !== this.#subReq) return
        set({ data, loading: false, error: null })
      } catch (e) {
        if (req !== this.#subReq) return
        set({ ...get(), loading: false, error: errText(e) })
      }
      this.notify()
    }
    switch (this.#view) {
      case 'alarms':
        return run(
          () => this.#alarms,
          (l) => (this.#alarms = l),
          () => evtApi.alarmStats(statsQuery(this.#stats, now)),
        )
      case 'steps':
        return run(
          () => this.#steps,
          (l) => (this.#steps = l),
          () => evtApi.stepStats(statsQuery(this.#stats, now, { split: true })),
        )
      case 'ilock': {
        const q = swimQuery(this.#swim, now)
        if (!q) {
          this.#lanes = idle()
          this.notify()
          return
        }
        return run(
          () => this.#lanes,
          (l) => {
            this.#lanes = l
            // 이벤트 · 슬롯으로 들어왔으면 서버가 찾은 스테이션으로 바꿔 둔다(링크가 그 스테이션을 가리키게)
            const st = l.data?.station
            if (st && this.#swim.station === null)
              this.#swim = { ...this.#swim, station: st, slot: null, event: null }
          },
          () => evtApi.swimlane(q),
        )
      }
      default:
        return
    }
  }

  async loadSaved(): Promise<void> {
    try {
      this.#saved = await evtApi.filters()
    } catch {
      this.#saved = []
    }
    this.notify()
  }

  async saveFilter(name: string, query: string): Promise<SavedFilter> {
    const f = await evtApi.saveFilter(name, query)
    await this.loadSaved()
    return f
  }

  async deleteFilter(id: number): Promise<void> {
    await evtApi.deleteFilter(id)
    await this.loadSaved()
  }

  get filter(): EvtFilter {
    return this.#filter
  }
  get rows(): readonly EventRow[] {
    return this.#rows
  }
  /** 커서로 더 받을 수 있나(상한으로 잘렸으면 틈이 생기므로 더 받지 않는다). */
  get hasMore(): boolean {
    return this.#next !== null && !this.#trimmed && this.#rows.length < ROW_CAP
  }
  get trimmed(): boolean {
    return this.#trimmed
  }
  get loaded(): boolean {
    return this.#loaded
  }
  get loading(): boolean {
    return this.#loading
  }
  get loadingMore(): boolean {
    return this.#loadingMore
  }
  get error(): string | null {
    return this.#error
  }
  get live(): boolean {
    return this.#live
  }
  get catalog(): EvtCatalog | null {
    return this.#catalog
  }
  get catalogError(): string | null {
    return this.#catalogError
  }
  get sources(): readonly EvtSource[] | null {
    return this.#sources
  }
  get sourcesError(): string | null {
    return this.#sourcesError
  }

  setFilter(next: EvtFilter): void {
    this.#filter = next
    this.notify()
    void this.reload()
  }

  async reload(): Promise<void> {
    const req = ++this.#req
    this.#loading = true
    this.#error = null
    this.notify()
    try {
      const page = await evtApi.list(
        buildQuery(this.#filter, { now: Date.now(), limit: PAGE_SIZE }),
      )
      if (req !== this.#req) return
      // 조회 중에 라이브로 들어온(쪽의 최신보다 새) 행만 살린다 — 옛 필터의 오래된 행이 쪽 사이에 끼면 틈이 가려진다.
      const top = page.rows[0]?.ts_ms ?? -Infinity
      const now = Date.now()
      const keep = this.#live
        ? this.#rows.filter((r) => r.ts_ms > top && matchesFilter(r, this.#filter, now))
        : []
      const m = mergeRows(page.rows, keep)
      this.#rows = m.rows
      this.#next = page.next_before
      this.#trimmed = m.trimmed
      this.#loaded = true
    } catch (e) {
      if (req !== this.#req) return
      this.#error = errText(e)
    } finally {
      if (req === this.#req) {
        this.#loading = false
        this.notify()
      }
    }
  }

  async more(): Promise<void> {
    if (!this.hasMore || this.#loadingMore || this.#loading) return
    const req = this.#req
    this.#loadingMore = true
    this.notify()
    try {
      const page = await evtApi.list(
        buildQuery(this.#filter, { now: Date.now(), limit: PAGE_SIZE, before: this.#next }),
      )
      if (req !== this.#req) return
      const m = mergeRows(this.#rows, page.rows)
      this.#rows = m.rows
      this.#next = page.next_before
      this.#trimmed = this.#trimmed || m.trimmed
    } catch (e) {
      if (req === this.#req) this.#error = errText(e)
    } finally {
      this.#loadingMore = false
      this.notify()
    }
  }

  async loadCatalog(force = false): Promise<void> {
    if (this.#catalog && !force) return
    try {
      this.#catalog = await evtApi.catalog()
      this.#catalogError = null
    } catch (e) {
      this.#catalogError = errText(e)
    }
    this.notify()
  }

  async loadSources(): Promise<void> {
    try {
      this.#sources = await evtApi.sources()
      this.#sourcesError = null
    } catch (e) {
      this.#sourcesError = errText(e)
    }
    this.notify()
  }

  /** 설정 PUT 결과를 목록에 반영한다. */
  adoptSource(s: EvtSource): void {
    if (!this.#sources) return
    this.#sources = this.#sources.map((x) => (x.plc === s.plc ? s : x))
    this.notify()
  }

  setLive(on: boolean): void {
    if (on === this.#live) return
    this.#live = on
    if (!on) this.detachLive()
    this.notify()
  }

  /** 화면이 떠 있는 동안만 스트림을 잡는다 — 해제 함수를 돌려준다. */
  attachLive(): () => void {
    if (this.#releaseLive) return () => this.detachLive()
    const release = evtStream.start()
    const off = evtStream.onRow((r) => {
      this.#liveBuf.push(r)
      this.#flushLive()
    })
    this.#releaseLive = () => {
      off()
      release()
    }
    return () => this.detachLive()
  }

  detachLive(): void {
    this.#flushLive.cancel()
    this.#liveBuf = []
    this.#releaseLive?.()
    this.#releaseLive = null
  }

  #drainLive(): void {
    const buf = this.#liveBuf
    this.#liveBuf = []
    if (!this.#live || buf.length === 0) return
    const now = Date.now()
    const hit = buf.filter((r) => matchesFilter(r, this.#filter, now))
    if (hit.length === 0) return
    const m = mergeRows(this.#rows, hit)
    if (m.added === 0) return
    this.#rows = m.rows
    this.#trimmed = this.#trimmed || m.trimmed
    this.notify()
  }
}

export const evtView = new EvtView()
