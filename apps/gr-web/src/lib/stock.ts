// 셀 재고 스토어 — `stockFeed`(SSE snapshot|upsert|remove)를 Map 에 적용한다. 화면은 `useStore(stock)` 로 읽는다.
import { api } from './api'
import { stockFeed } from './feeds'
import { Store } from './store'
import type { HandEntry, StockEntry, StockEvent, SyncIssue } from './types'

class StockStore extends Store {
  #map = new Map<number, StockEntry>()
  #hands = new Map<string, HandEntry>()
  #sync = new Map<string, SyncIssue[]>()
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
  /** 로봇(상태 PLC 이름)의 Hand — 기록이 없으면 빈 손. */
  hand(plc: string): HandEntry | null {
    return this.#hands.get(plc) ?? null
  }
  get hands(): ReadonlyMap<string, HandEntry> {
    return this.#hands
  }
  /** 로봇(상태 PLC 이름)의 동기화 경고 — PLC 실제 상태와 Hand · 이송 지시가 어긋남. */
  syncIssues(plc: string): readonly SyncIssue[] {
    return this.#sync.get(plc) ?? []
  }
  /** 첫 스냅샷을 받았는가. */
  get ready(): boolean {
    return this.#ready
  }

  apply(ev: StockEvent): void {
    if (ev.kind === 'snapshot') {
      this.#map = new Map(ev.stock.map((s) => [s.cell_id, s]))
      this.#ready = true
    } else if (ev.kind === 'sync') {
      this.#sync = new Map(this.#sync)
      this.#sync.set(ev.plc, ev.issues)
    } else if (ev.kind === 'hand') {
      this.#hands = new Map(this.#hands)
      this.#hands.set(ev.hand.plc, ev.hand)
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
      // 스트림 첫 스냅샷에는 Hand 가 없다 — 한 번 읽어 채우고 뒤로는 'hand' 이벤트로 따라간다.
      void api
        .stockHands()
        .then((hs) => {
          const m = new Map(this.#hands)
          for (const h of hs) if (!m.has(h.plc)) m.set(h.plc, h)
          this.#hands = m
          this.notify()
        })
        .catch(() => {})
      void api
        .stockSync()
        .then((rs) => {
          const m = new Map(this.#sync)
          for (const r of rs) if (!m.has(r.plc)) m.set(r.plc, r.issues)
          this.#sync = m
          this.notify()
        })
        .catch(() => {})
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
