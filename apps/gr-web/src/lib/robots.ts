// 로봇 스토어 — `/api/robots` 를 2초마다 폴링(GRM 뒤 최대 2대) + 명령을 보낼 로봇 선택(브라우저에 기억).
//
// 선택된 로봇 id 는 작업 명령(`TaskRequest.robot`)·순차 계획 스텝·시나리오 스텝에 실린다.
import { api } from './api'
import { visibleInterval } from './poll'
import { pickDefaultRobot, robotChip, type RobotChipModel } from './robotContext'
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
  /** 선택된 로봇 id — 규칙은 `robotContext.pickDefaultRobot`(지난 선택 → 쓸 수 있는 첫 호기 → 첫 호기). */
  get selected(): number | null {
    return pickDefaultRobot(this.#list, this.#selected) ?? this.#selected
  }
  get current(): Robot | null {
    const id = this.selected
    return this.#list.find((r) => r.id === id) ?? null
  }
  /** 선택된 로봇의 칩 모델 — 이름·PLC·색 한 벌. 목록 전에는 "로봇 미확인". */
  get chip(): RobotChipModel {
    return robotChip(this.current)
  }
  /**
   * Task 의 주인 로봇 칩 — Task 는 로봇 id 대신 원장의 상태 PLC 이름(`plc_name`)을 든다.
   * 선택과 다른 호기의 Task 를 다룰 때 그 행이 **누구 것인지** 이것으로 밝힌다.
   */
  chipOfPlc(plc: string | null | undefined): RobotChipModel {
    const r = plc ? this.#list.find((x) => x.plc === plc || x.name === plc) : null
    return robotChip(r, { name: plc ?? null })
  }
  /** 로봇 하나의 칩 모델(모르면 이름만이라도 남긴다). */
  chipOf(id: number | null | undefined): RobotChipModel {
    return robotChip(this.byId(id), { id: id ?? null, name: id === null || id === undefined ? null : `GR${id}` })
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

// 색과 칩 모델의 정본은 `robotContext.ts`(순수 모듈)다 — 여기서는 이미 쓰고 있는 자리들을 위해 다시 낸다.
export { ROBOT_COLORS, robotColor } from './robotContext'
export type { RobotChipModel } from './robotContext'
