// 맵 더블 클릭 재고 팝업의 순수 규칙 — 개수 범위, 최근 쓴 품목(브라우저 저장), 저장할 동작.
import type { StockEntry } from '../types'

export const RECENT_KEY = 'gr-quick-stock-recent'
export const RECENT_MAX = 5

/** 개수 범위 — 입력 모드는 1 부터(0 개 입력은 뜻이 없다), 수정 모드는 0 = 비우기. 상한 = StackMax(없으면 99). */
export function clampCount(n: number, mode: 'add' | 'edit', stackMax = 0): number {
  const max = stackMax > 0 ? stackMax : 99
  const min = mode === 'add' ? 1 : 0
  if (!Number.isFinite(n)) return min
  return Math.min(Math.max(Math.round(n), min), max)
}

/** 최근 품목 앞에 넣기(중복 제거, 최대 `RECENT_MAX`). */
export function pushRecent(list: readonly number[], code: number): number[] {
  return [code, ...list.filter((c) => c !== code)].slice(0, RECENT_MAX)
}

export function loadRecent(): number[] {
  try {
    const v = JSON.parse(localStorage.getItem(RECENT_KEY) ?? '[]')
    return Array.isArray(v)
      ? v.filter((c) => Number.isInteger(c) && c > 0).slice(0, RECENT_MAX)
      : []
  } catch {
    return []
  }
}

export function saveRecent(list: readonly number[]): void {
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(list))
  } catch {
    /* 저장 못 해도 동작 */
  }
}

export type QuickAction =
  { kind: 'set'; item_code: number; count: number } | { kind: 'delete' } | { kind: 'none' }

/** 저장 버튼이 할 일 — 입력(품목·개수) / 수량 변경 / 0 이면 삭제 / 그대로면 없음. */
export function quickAction(
  cur: StockEntry | null,
  item: number | null,
  count: number,
): QuickAction {
  const has = !!cur && cur.count > 0
  if (!has) return item ? { kind: 'set', item_code: item, count } : { kind: 'none' }
  if (count <= 0) return { kind: 'delete' }
  if (count === cur.count) return { kind: 'none' }
  return { kind: 'set', item_code: cur.item_code, count }
}
