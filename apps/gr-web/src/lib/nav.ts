// 셸 내비게이션 상태 — 활성 탭의 단일 진실원.
//
// App은 이 스토어를 렌더하고 어느 컴포넌트든 `nav.go('measure')`로 이동한다. URL 딥링크(`?tab=`)도
// 이 값을 읽고 쓴다(투영 — 중복 진실원 금지).
//
// 탭 외의 필드(`taskId`·`measSeq`·`measCode`·`scenarioId`)는 **한 번만 반응하는 신호**다 — 화면이
// 읽어 그 행을 고른 뒤 `consume*()`로 비운다. 상태가 아니라 신호이므로 URL에 싣지 않는다.

import { Store } from './store'

/** 셸 탭 id — `lib/tabs.ts`의 레지스트리와 같은 집합(백엔드 `console/info.tabs`가 노출 여부를 정한다).
 *  이 유니온의 정의는 여기 하나뿐이다(컴포넌트에서 다시 선언하지 않는다). */
export type Tab = 'task' | 'taskmgr' | 'measure' | 'scenario'

class Nav extends Store {
  /** 활성 탭. */
  #tab: Tab = 'task'
  get tab(): Tab {
    return this.#tab
  }
  set tab(v: Tab) {
    this.#tab = v
    this.notify()
  }

  /** Task 관리 화면이 고를 Task id(빈 문자열 = 없음). */
  #taskId = ''
  get taskId(): string {
    return this.#taskId
  }
  set taskId(v: string) {
    this.#taskId = v
    this.notify()
  }

  /** 측정 모니터가 고를 측정 로그 Seq(`null` = 없음). */
  #measSeq: number | null = null
  get measSeq(): number | null {
    return this.#measSeq
  }
  set measSeq(v: number | null) {
    this.#measSeq = v
    this.notify()
  }

  /** 측정 모니터가 걸 품목 코드 필터(`null` = 없음). */
  #measCode: number | null = null
  get measCode(): number | null {
    return this.#measCode
  }
  set measCode(v: number | null) {
    this.#measCode = v
    this.notify()
  }

  /** 시나리오 화면이 열 시나리오 id(빈 문자열 = 없음). */
  #scenarioId = ''
  get scenarioId(): string {
    return this.#scenarioId
  }
  set scenarioId(v: string) {
    this.#scenarioId = v
    this.notify()
  }

  /** 탭 이동(화면 가로지르는 흐름의 진입점). */
  go(tab: Tab): void {
    this.tab = tab
  }

  /** 지목된 Task id를 가져가며 비운다 — 재렌더마다 다시 반응하지 않게. */
  consumeTaskId(): string {
    const v = this.#taskId
    if (v) this.taskId = ''
    return v
  }

  /** 지목된 측정 Seq를 가져가며 비운다. */
  consumeMeasSeq(): number | null {
    const v = this.#measSeq
    if (v !== null) this.measSeq = null
    return v
  }

  /** 지목된 품목 코드를 가져가며 비운다. */
  consumeMeasCode(): number | null {
    const v = this.#measCode
    if (v !== null) this.measCode = null
    return v
  }

  /** 지목된 시나리오 id를 가져가며 비운다. */
  consumeScenarioId(): string {
    const v = this.#scenarioId
    if (v) this.scenarioId = ''
    return v
  }

  /** Task 관리 화면을 그 Task로 연다 — 배지·행의 단일 목적지. */
  goTask(id: string): void {
    this.#taskId = id
    this.tab = 'taskmgr'
  }

  /** 측정 모니터를 그 측정 건(Seq)으로 연다. */
  goMeasSeq(seq: number): void {
    this.#measSeq = seq
    this.tab = 'measure'
  }

  /** 측정 모니터를 그 품목 코드 필터로 연다. */
  goMeasCode(code: number): void {
    this.#measCode = code
    this.tab = 'measure'
  }

  /** 시나리오 화면을 그 시나리오로 연다. */
  goScenario(id: string): void {
    this.#scenarioId = id
    this.tab = 'scenario'
  }
}

export const nav = new Nav()
