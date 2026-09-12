// 셀 재고 스토어 — `stockFeed`(SSE snapshot|upsert|remove)를 Map 에 적용한다. 화면은 `useStore(stock)` 로 읽는다.
import { stockFeed } from './feeds'
import { Store } from './store'
import type { StockEntry, StockEvent } from './types'

class StockStore extends Store {
  #map = new Map<number, StockEntry>()
  #ready = false
  #refs = 0
  #release: (() => void) | null = null
  #off: (() => void) | null = null

  /** 셀 id → 재고. 없는 셀은 비어 있는 것으로 본다. */
  get(cell: number): StockEntry | null {
    return this.#map.get(cell) ?? null
  }
  get all(): readonly StockEntry[] {
    return [...this.#map.values()].sort((a, b) => a.cell_id - b.cell_id)
  }
  get map(): ReadonlyMap<number, StockEntry> {
    return this.#map
  }
  /** 첫 스냅샷을 받았는가. */
  get ready(): boolean {
    return this.#ready
  }

  apply(ev: StockEvent): void {
    if (ev.kind === 'snapshot') {
      this.#map = new Map(ev.stock.map((s) => [s.cell_id, s]))
      this.#ready = true
    } else if (ev.kind === 'upsert') {
      this.#map = new Map(this.#map)
      this.#map.set(ev.entry.cell_id, ev.entry)
    } else {
      this.#map = new Map(this.#map)
      this.#map.delete(ev.cell_id)
    }
    this.notify()
  }

  /** 구독 수요 등록 — 해제 함수를 돌려준다. */
  start(): () => void {
    this.#refs++
    if (this.#refs === 1) {
      this.#off = stockFeed.onMessage((ev) => this.apply(ev))
      this.#release = stockFeed.start()
    }
    let released = false
    return () => {
      if (released) return
      released = true
      this.#refs--
      if (this.#refs === 0) {
        this.#off?.()
        this.#off = null
        this.#release?.()
        this.#release = null
      }
    }
  }
}

export const stock = new StockStore()
