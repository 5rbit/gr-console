// 명령 팔레트의 **열림 상태**만 든 작은 스토어.
//
// 컴포넌트 안의 `useState`로 두면 팔레트를 여는 길이 단축키 하나뿐이다. 메뉴의 `명령 팔레트…`,
// 상태바의 힌트, 그리고 "이름을 받아야 하는 명령"(배치 저장)을 메뉴에서 바로 부르는 길까지
// 열려면 밖에서 부를 수 있는 손잡이가 필요하다.
import { Store } from './store'

class Palette extends Store {
  #open = false
  #query = ''
  /** 두 번째 걸음(값 입력)으로 바로 들어갈 명령 id — 메뉴에서 `…` 항목을 고른 경우. */
  #askId: string | null = null

  get open(): boolean {
    return this.#open
  }
  get query(): string {
    return this.#query
  }
  get askId(): string | null {
    return this.#askId
  }

  /** 팔레트를 연다. `query`를 주면 그 글자로 걸러진 상태로 뜬다(메뉴 → 관련 명령만). */
  show(query = ''): void {
    this.#open = true
    this.#query = query
    this.#askId = null
    this.notify()
  }

  /** 값 입력이 필요한 명령을 바로 펼친다 — 메뉴의 `현재 배치 저장…`이 이 길로 온다. */
  ask(id: string): void {
    this.#open = true
    this.#query = ''
    this.#askId = id
    this.notify()
  }

  hide(): void {
    if (!this.#open) return
    this.#open = false
    this.#askId = null
    this.notify()
  }

  toggle(): void {
    if (this.#open) this.hide()
    else this.show()
  }

  setQuery(q: string): void {
    this.#query = q
    this.notify()
  }

  /** 값 입력을 접고 목록으로 돌아간다(Escape 한 번). */
  clearAsk(): void {
    if (this.#askId === null) return
    this.#askId = null
    this.notify()
  }
}

export const palette = new Palette()
