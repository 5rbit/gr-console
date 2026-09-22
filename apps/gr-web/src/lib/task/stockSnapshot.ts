// 재고 스냅샷(되돌리기) 순수 규칙 — 전체 비우기의 추가 확인 입력과 스냅샷 사유 표기.
import type { StockSnapshotInfo } from '../types'

/** 전체 비우기 확인 단어 — 이것 또는 **합계 개수**를 그대로 쳐야 확인 버튼이 풀린다. */
export const CLEAR_WORD = '비우기'

/** 입력이 확인 단어 또는 합계 개수와 같은가(앞뒤 공백 무시). 빈 입력·다른 수는 거절. */
export function clearArmed(text: string, total: number): boolean {
  const t = text.trim()
  return t === CLEAR_WORD || (t !== '' && t === String(total))
}

const REASON: Record<string, string> = {
  clear: '전체 비우기 전',
  'import-merge': 'Excel 병합 전',
  'import-replace': 'Excel 교체 전',
  'restore-before': '되돌리기 전',
}

/** 사유 표기 — 되돌리기 전 스냅샷은 무엇으로 되돌렸는지(`#id`)까지. 모르는 사유는 그대로. */
export function reasonLabel(s: Pick<StockSnapshotInfo, 'reason' | 'restored_from'>): string {
  const base = REASON[s.reason] ?? s.reason
  return s.reason === 'restore-before' && s.restored_from != null
    ? `${base} (#${s.restored_from})`
    : base
}

/** `2026-09-22T10:11:12…` → `09-22 10:11:12` (표의 다른 시각 열과 같은 모양). */
export function shortTime(at: string | null | undefined): string {
  return at ? at.slice(5, 19).replace('T', ' ') : ''
}
