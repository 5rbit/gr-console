// 토스트 알림 스토어 — 인라인 에러 텍스트를 대체하는 전역 알림. 컴포넌트는 toast.ok/error/... 로
// 발행하고 Toaster가 렌더한다. 성공/실패 피드백(write·배포·명령 결과)에 공용.
//
// 프레임워크 밖 모듈이라 컴포넌트가 아닌 곳(fetch 래퍼·이벤트 핸들러)에서도 그대로 부른다.
// 화면은 `subscribe`/`getSnapshot`을 `useSyncExternalStore`로 읽는다.

export type ToastKind = 'info' | 'ok' | 'warn' | 'error' | 'pending'

export interface ToastItem {
  id: number
  kind: ToastKind
  msg: string
}

/** 화면에 쌓아 둘 상한 — fan-out은 대수만큼 발행하므로(N대 배포 = N건) 상한이 없으면 화면을 덮는다.
 *  넘치면 **오래된 쪽**을 버린다(최신이 지금 벌어지는 일이다). `pending`은 세지 않는다 — 진행 중인
 *  작업이 새 결과에 밀려 사라지면 그 작업의 끝을 아무도 못 본다. */
const VISIBLE_MAX = 6

let items: readonly ToastItem[] = []
let seq = 0

const listeners = new Set<() => void>()

function emit(next: readonly ToastItem[]): void {
  // 스냅샷은 참조가 바뀔 때만 바뀐다 — 매번 새 배열을 내면 useSyncExternalStore가 무한히 돈다.
  if (next === items) return
  items = next
  for (const fn of listeners) fn()
}

/** 상한 초과분(완료된 것 중 오래된 쪽)을 버린다 */
function trim(list: readonly ToastItem[]): readonly ToastItem[] {
  const done = list.filter((t) => t.kind !== 'pending')
  const over = done.length - VISIBLE_MAX
  if (over <= 0) return list
  const drop = new Set(done.slice(0, over).map((t) => t.id))
  return list.filter((t) => !drop.has(t.id))
}

function dismiss(id: number): void {
  const next = items.filter((t) => t.id !== id)
  if (next.length !== items.length) emit(next)
}

function push(kind: ToastKind, msg: string, ms = 4500): number {
  const id = ++seq
  emit(trim([...items, { id, kind, msg }]))
  if (ms > 0) setTimeout(() => dismiss(id), ms)
  return id
}

/** 이미 뜬 토스트를 **갈아 끼운다** — 없으면(사용자가 닫았으면) 새로 발행한다 */
function update(id: number, kind: ToastKind, msg: string, ms = 4500): number {
  const i = items.findIndex((t) => t.id === id)
  if (i < 0) return push(kind, msg, ms)
  const next = items.slice()
  next[i] = { id, kind, msg }
  emit(trim(next))
  if (ms > 0) setTimeout(() => dismiss(id), ms)
  return id
}

export const toasts = {
  get items(): readonly ToastItem[] {
    return items
  },
  push,
  update,
  dismiss,
  /** 변경 구독 — 해제 함수를 돌려준다 */
  subscribe(fn: () => void): () => void {
    listeners.add(fn)
    return () => {
      listeners.delete(fn)
    }
  },
  /** 현재 스냅샷 — 변경 전까지 참조가 고정된다 */
  getSnapshot(): readonly ToastItem[] {
    return items
  },
}

/** 간편 발행기 — `toast.ok("저장됨")` */
export const toast = {
  info: (m: string) => toasts.push('info', m),
  ok: (m: string) => toasts.push('ok', m),
  warn: (m: string) => toasts.push('warn', m),
  error: (m: string) => toasts.push('error', m, 7000),
  /** 진행 중 토스트 — 스스로 사라지지 않는다. 반환한 id를 [`resolve`]에 넘겨 끝을 알린다.
   *  왕복이 수 초인 명령(배포 activate 등)에서 화면이 침묵하지 않게 한다. */
  pending: (m: string) => toasts.push('pending', m, 0),
  /** 진행 중 토스트를 결과로 갈아 끼운다 */
  resolve: (id: number, kind: Exclude<ToastKind, 'pending'>, m: string) =>
    toasts.update(id, kind, m, kind === 'error' ? 7000 : 4500),
}
