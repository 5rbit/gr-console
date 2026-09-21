// 계획 받은편지함 — 다른 화면(팔렛 패턴)이 작업 명령의 순차 계획에 스텝을 보내는 한 방향 신호.
//
// 계획의 진실원은 `TaskIssue` 의 되돌리기 스택이다. 도킹 배치에서는 두 화면이 동시에 떠 있을 수 있어
// localStorage(`gr-plan`)를 직접 고치면 TaskIssue 의 저장 효과가 곧바로 덮어쓴다. 그래서 여기에 쌓아 두고
// TaskIssue 가 마운트·변경 때 가져가 한 번의 편집(되돌리기 한 칸)으로 붙인다.
import { Store } from '../store'
import type { PlanStep } from './plan'

class PlanInbox extends Store {
  #pending: PlanStep[] = []

  get size(): number {
    return this.#pending.length
  }

  push(steps: readonly PlanStep[]): void {
    if (steps.length === 0) return
    this.#pending = [...this.#pending, ...steps]
    this.notify()
  }

  /** 쌓인 스텝을 가져가며 비운다. */
  take(): PlanStep[] {
    const v = this.#pending
    if (v.length > 0) {
      this.#pending = []
      this.notify()
    }
    return v
  }
}

export const planInbox = new PlanInbox()
