// 우클릭 도구 상자(싱글턴 스토어) — 어느 표에서든 같은 방법으로 같은 조작을 꺼낸다.
//
// 감싸는 컴포넌트가 아니라 스토어 + 호스트 한 벌인 이유는 DOM이다: 표의 `<tr>`을 `<div>`로 감쌀 수
// 없고 SVG 안에서도 써야 한다. 호출자는 `onContextMenu`에서 `ctxMenu.show`를 부르고, 그리는 일은
// 앱에 하나만 뜨는 `ContextMenuHost`가 맡는다.

/** 항목 하나 — `run`이 없으면 머리줄(제목)로 그린다 */
export interface MenuItem {
  label: string
  /** 오른쪽 정렬로 붙는 단축키 표기(선택) */
  hint?: string
  /** 비활성 사유(있으면 눌리지 않고 그 이유를 title로 말한다 — 회색으로 침묵하지 않는다) */
  disabled?: string
  /** 위험한 조작은 붉게 */
  danger?: boolean
  /**
   * 이 항목의 `data-testid`. 없으면 호스트가 라벨로 짓는다(`ctx-<label>`) — 라벨은 사람의 말이라
   * 문구를 다듬을 때마다 테스트가 깨진다. 스모크가 짚는 항목에는 이것을 적는다.
   */
  testid?: string
  run?: () => void
}

/** `show`가 받는 최소 이벤트 — 네이티브와 React 합성 이벤트 양쪽이 맞는다 */
export interface CtxMouseEvent {
  clientX: number
  clientY: number
  preventDefault(): void
  stopPropagation(): void
}

/** 호스트가 읽는 스냅샷 — 변경 전까지 참조가 고정된다 */
export interface CtxMenuState {
  items: readonly MenuItem[]
  x: number
  y: number
  open: boolean
}

const EMPTY: CtxMenuState = { items: [], x: 0, y: 0, open: false }

let state: CtxMenuState = EMPTY
const listeners = new Set<() => void>()

function emit(next: CtxMenuState): void {
  if (next === state) return
  state = next
  for (const fn of listeners) fn()
}

export const ctxMenu = {
  /** 열려 있으면 항목들, 아니면 빈 */
  get items(): readonly MenuItem[] {
    return state.items
  },
  get x(): number {
    return state.x
  },
  get y(): number {
    return state.y
  },
  get open(): boolean {
    return state.open
  },

  /** 우클릭 자리에 연다 — 기본 컨텍스트 메뉴는 막는다. 항목은 열 때 계산해 넘긴다 */
  show(e: CtxMouseEvent, items: MenuItem[]): void {
    if (items.length === 0) return
    e.preventDefault()
    e.stopPropagation()
    // 화면 밖으로 나가지 않게 — 오른쪽·아래 끝에서는 안쪽으로 접는다.
    const x = Math.min(e.clientX, Math.max(0, window.innerWidth - 220))
    const y = Math.min(e.clientY, Math.max(0, window.innerHeight - (items.length * 26 + 16)))
    emit({ items, x, y, open: true })
  },

  close(): void {
    if (state !== EMPTY) emit(EMPTY)
  },

  /** 변경 구독 — 해제 함수를 돌려준다 */
  subscribe(fn: () => void): () => void {
    listeners.add(fn)
    return () => {
      listeners.delete(fn)
    }
  },

  /** 현재 스냅샷 — 변경 전까지 참조가 고정된다 */
  getSnapshot(): CtxMenuState {
    return state
  },
}
