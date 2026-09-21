// 순차 계획 실행 방식 — 미리 넣기(Pre-queue) 켬/끔. 브라우저마다 기억한다(저장이 막혀도 동작, 기본 끔).
const KEY = 'gr.plan.preQueue'

export function readPreQueue(): boolean {
  try {
    return localStorage.getItem(KEY) === '1'
  } catch {
    return false
  }
}

export function writePreQueue(on: boolean): void {
  try {
    localStorage.setItem(KEY, on ? '1' : '0')
  } catch {
    /* 저장 못 해도 이번 화면에서는 동작 */
  }
}

/** 확인 창·토스트에 쓰는 한 줄. */
export function preQueueLabel(on: boolean): string {
  return on ? 'Pre-queue — 실행 중 + 다음 1건 미리 넣기' : '완료 대기 — 스텝마다 완료 후 다음'
}
