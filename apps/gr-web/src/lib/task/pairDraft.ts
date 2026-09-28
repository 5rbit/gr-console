// 순차 계획의 짝 DROP 초안 — PICK 을 보낼 때 만든 DROP 초안이 어느 계획 줄에 걸렸나. 브라우저마다 기억한다
// (새로 고쳐도 그 줄 차례에 초안을 보낸다). 저장이 막혀도 이번 화면에서는 동작.
const KEY = 'gr.plan.pairDraft'

export interface PairDraft {
  stepId: string
  taskId: string
}

export function readPairDraft(): PairDraft | null {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? 'null') as unknown
    if (v && typeof v === 'object' && 'stepId' in v && 'taskId' in v) {
      const o = v as Record<string, unknown>
      if (typeof o.stepId === 'string' && typeof o.taskId === 'string')
        return { stepId: o.stepId, taskId: o.taskId }
    }
    return null
  } catch {
    return null
  }
}

export function writePairDraft(v: PairDraft | null): void {
  try {
    if (v) localStorage.setItem(KEY, JSON.stringify(v))
    else localStorage.removeItem(KEY)
  } catch {
    /* 기억 못 해도 동작 */
  }
}
