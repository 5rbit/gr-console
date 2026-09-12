// 로봇 스토어 — `/api/robots` 를 2초마다 폴링(GRM 뒤 최대 2대) + 명령을 보낼 로봇 선택(브라우저에 기억).
//
// 선택된 로봇 id 는 작업 명령(`TaskRequest.robot`)·순차 계획 스텝·시나리오 스텝에 실린다.
import { api } from './api'
import { visibleInterval } from './poll'
import { Store } from './store'
import type { Robot } from './types'

const POLL_MS = 2000
const SEL_KEY = 'gr-robot'

class Robots extends Store {
  #list: Robot[] = []
  #error: string | null = null
  #loaded = false
  #selected: number | null = null
  #stop: ReturnType<typeof setInterval> | null = null
  #refs = 0

  constructor() {
    super()
    try {
      const v = Number(localStorage.getItem(SEL_KEY))
      this.#selected = v > 0 ? v : null
    } catch {
      this.#selected = null
    }
  }

  get list(): readonly Robot[] {
    return this.#list
  }
  get error(): string | null {
    return this.#error
  }
  get loaded(): boolean {
    return this.#loaded
  }
  /** 선택된 로봇 id — 목록에 없으면 기본 로봇. 목록이 비면 null. */
  get selected(): number | null {
    if (this.#selected !== null && this.#list.some((r) => r.id === this.#selected))
      return this.#selected
    return this.#list.find((r) => r.default)?.id ?? this.#list[0]?.id ?? this.#selected
  }
  get current(): Robot | null {
    const id = this.selected
    return this.#list.find((r) => r.id === id) ?? null
  }
  /** 로봇이 둘 이상인가(하나뿐이면 선택 UI 를 숨긴다). */
  get multi(): boolean {
    return this.#list.length > 1
  }
  byId(id: number | null | undefined): Robot | null {
    return this.#list.find((r) => r.id === id) ?? null
  }
  /** 표시용 이름. */
  nameOf(id: number | null | undefined): string {
    return this.byId(id)?.name ?? (id === null || id === undefined ? '기본' : `GR${id}`)
  }

  select(id: number): void {
    this.#selected = id
    try {
      localStorage.setItem(SEL_KEY, String(id))
    } catch {
      /* 저장 못 해도 동작 */
    }
    this.notify()
  }

  async refresh(): Promise<void> {
    try {
      this.#list = await api.robots()
      this.#error = null
      this.#loaded = true
    } catch (e) {
      this.#error = e instanceof Error ? e.message : String(e)
    }
    this.notify()
  }

  /** 폴 수요 등록 — 해제 함수를 돌려준다. */
  start(): () => void {
    this.#refs++
    if (this.#refs === 1) {
      void this.refresh()
      this.#stop = visibleInterval(() => void this.refresh(), POLL_MS)
    }
    let released = false
    return () => {
      if (released) return
      released = true
      this.#refs--
      if (this.#refs === 0) {
        if (this.#stop !== null) clearInterval(this.#stop)
        this.#stop = null
      }
    }
  }
}

export const robots = new Robots()

/** 로봇에 할당된 색 — 맵 작업 테두리·로봇 십자·사이드바 견본이 같은 색을 쓴다. */
export const ROBOT_COLORS: Readonly<Record<number, string>> = { 1: '#0284c7', 2: '#ea580c' }
const FALLBACK_COLORS = ['#7c3aed', '#db2777', '#0d9488']

export function robotColor(id: number | null | undefined): string {
  if (id === null || id === undefined) return FALLBACK_COLORS[0]
  return ROBOT_COLORS[id] ?? FALLBACK_COLORS[Math.abs(id) % FALLBACK_COLORS.length]
}
