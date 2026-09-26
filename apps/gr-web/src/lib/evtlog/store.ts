// 이벤트 화면 상태 — 모듈에 하나라 탭을 옮겨 다녀도 필터·목록·라이브 여부가 남는다.
import { frameThrottle } from '../sse'
import { Store } from '../store'
import { evtApi, type EventRow, type EvtCatalog, type EvtSource } from './api'
import { EMPTY_FILTER, buildQuery, matchesFilter, type EvtFilter } from './evtFilterModel'
import { PAGE_SIZE, ROW_CAP, mergeRows } from './evtRowsModel'
import { evtStream } from './stream'

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
