// PLC 연결 상태 스토어 — `/api/plcs`를 2초마다 폴링(숨은 탭 스킵·혼잡 감속은 `visibleInterval`).
//
// 참조 세기다 — 사이드바·상태바가 겹쳐 잡아도 타이머는 하나, 마지막이 놓을 때 멈춘다.
import { api } from './api'
import { visibleInterval } from './poll'
import { Store } from './store'
import { toast } from './ui/toast'
import type { PlcId, PlcStatus } from './types'

const POLL_MS = 2000

class Plcs extends Store {
  #list: PlcStatus[] = []
  #error: string | null = null
  #loaded = false
  #timer: ReturnType<typeof setInterval> | null = null
  #refs = 0

  /** PLC 목록(마지막 폴 결과). */
  get list(): readonly PlcStatus[] {
    return this.#list
  }
  /** 마지막 폴 오류(정상이면 `null`). */
  get error(): string | null {
    return this.#error
  }
  /** 한 번이라도 목록을 받았는가. */
  get loaded(): boolean {
    return this.#loaded
  }

  byId(id: PlcId): PlcStatus | null {
    return this.#list.find((p) => p.id === id) ?? null
  }

  /** 전 PLC 연결 여부(목록이 비면 거짓). */
  get allConnected(): boolean {
    return this.#list.length > 0 && this.#list.every((p) => p.connected)
  }

  /** 레이아웃 검사 종합 — 하나라도 불일치면 `false`, 전부 OK면 `true`, 그 밖(미검사·목록 없음)은 `null`. */
  get layoutOk(): boolean | null {
    if (this.#list.length === 0) return null
    if (this.#list.some((p) => p.layout.ok === false)) return false
    if (this.#list.every((p) => p.layout.ok === true)) return true
    return null
  }

  async refresh(): Promise<void> {
    try {
      this.#list = await api.plcs()
      this.#error = null
      this.#loaded = true
    } catch (e) {
      this.#error = e instanceof Error ? e.message : String(e)
    }
    this.notify()
  }

  /** 폴 수요 등록 — 해제 함수를 돌려준다(effect cleanup에서 부른다). */
  start(): () => void {
    this.#refs++
    if (this.#refs === 1) {
      void this.refresh()
      this.#timer = visibleInterval(() => void this.refresh(), POLL_MS)
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
    if (this.#refs === 0 && this.#timer !== null) {
      clearInterval(this.#timer)
      this.#timer = null
    }
  }

  #replace(p: PlcStatus): void {
    this.#list = this.#list.map((x) => (x.id === p.id ? p : x))
    this.notify()
  }

  /** 레이아웃 재검사. */
  async check(id: PlcId): Promise<PlcStatus | null> {
    const tid = toast.pending(`${id} 레이아웃 검사 중…`)
    try {
      const p = await api.plcCheck(id)
      this.#replace(p)
      const ok = p.layout.ok
      toast.resolve(
        tid,
        ok === true ? 'ok' : ok === false ? 'warn' : 'info',
        ok === true
          ? `${p.label} 레이아웃 OK`
          : ok === false
            ? `${p.label} 레이아웃 불일치 ${p.layout.mismatches.length}건`
            : `${p.label} 검사 결과 없음`,
      )
      return p
    } catch (e) {
      toast.resolve(tid, 'error', e instanceof Error ? e.message : String(e))
      return null
    }
  }

  /** 재연결. */
  async reconnect(id: PlcId): Promise<PlcStatus | null> {
    const tid = toast.pending(`${id} 재연결 중…`)
    try {
      const p = await api.plcReconnect(id)
      this.#replace(p)
      toast.resolve(
        tid,
        p.connected ? 'ok' : 'warn',
        `${p.label} ${p.connected ? '연결됨' : '미연결'}`,
      )
      return p
    } catch (e) {
      toast.resolve(tid, 'error', e instanceof Error ? e.message : String(e))
      return null
    }
  }
}

export const plcs = new Plcs()
