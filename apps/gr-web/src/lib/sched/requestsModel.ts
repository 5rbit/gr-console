// 스케줄러 요청 탭의 순수 도우미 — 대상 문구 · 상태 색 · 정렬 · 추가 폼 검증.
import type { Status } from '../ui/status'
import type { Target, TargetKind } from '../types'
import type { NewRequest, RequestKind, RequestState, TransferRequest } from '../sched'

export type RequestFilter = 'open' | 'all'

export const REQUEST_STATE_TONE: Record<RequestState, Status> = {
  open: 'neutral',
  active: 'info',
  done: 'ok',
  canceled: 'neutral',
  failed: 'fault',
}

/** 요청 종류별 From · To 대상 종류 — 입고 스테이션→셀, 출고 셀→스테이션, 이동 셀→셀. */
export function endpointKinds(kind: RequestKind): { from: TargetKind; to: TargetKind } {
  switch (kind) {
    case 'inbound':
      return { from: 'station', to: 'cell' }
    case 'outbound':
      return { from: 'cell', to: 'station' }
    case 'move':
      return { from: 'cell', to: 'cell' }
  }
}

/** `Station 2101` · `Cell 401` · 빈 값 = `Auto`. */
export function targetText(t: Target | null | undefined): string {
  if (!t || !t.id) return 'Auto'
  return `${t.kind === 'station' ? 'Station' : 'Cell'} ${t.id}`
}

export function isLive(r: Pick<TransferRequest, 'state'>): boolean {
  return r.state === 'open' || r.state === 'active'
}

/** 살아 있는 요청 먼저 → 우선순위 큰 것 → 먼저 들어온 것(seq). 엔진이 도는 순서와 같다. */
export function sortRequests(rows: readonly TransferRequest[]): TransferRequest[] {
  return [...rows].sort(
    (a, b) => Number(isLive(b)) - Number(isLive(a)) || b.priority - a.priority || a.seq - b.seq,
  )
}

/** 시각 한 칸 — 오늘이면 `HH:MM:SS`, 아니면 `MM-DD HH:MM`. 못 읽으면 받은 그대로. */
export function shortTime(iso: string | null | undefined, now = new Date()): string {
  if (!iso) return ''
  const t = new Date(iso)
  if (!Number.isFinite(t.getTime())) return iso
  const p = (n: number) => String(n).padStart(2, '0')
  const hm = `${p(t.getHours())}:${p(t.getMinutes())}`
  const sameDay =
    t.getFullYear() === now.getFullYear() &&
    t.getMonth() === now.getMonth() &&
    t.getDate() === now.getDate()
  return sameDay ? `${hm}:${p(t.getSeconds())}` : `${p(t.getMonth() + 1)}-${p(t.getDate())} ${hm}`
}

// ── 추가 폼 ──────────────────────────────────────────────────────────

/** 입력 그대로의 문자열 — 빈 From/To = Auto, 빈 ItemCode = 자동(입고: 스테이션 품목, 출고: 가장 오래된 재고). */
export interface RequestDraft {
  kind: RequestKind
  item: string
  qty: string
  from: string
  to: string
  priority: string
  note: string
}

export const EMPTY_DRAFT: RequestDraft = {
  kind: 'inbound',
  item: '',
  qty: '1',
  from: '',
  to: '',
  priority: '0',
  note: '',
}

const blank = (s: string) => s.trim() === ''
const posInt = (s: string) => /^\d+$/.test(s.trim()) && Number(s) > 0

/** 막는 사유(없으면 null). */
export function draftError(d: RequestDraft): string | null {
  if (!posInt(d.qty)) return 'Qty 는 1 이상의 정수'
  if (!blank(d.item) && !posInt(d.item)) return 'ItemCode 는 양의 정수(비우면 자동)'
  if (!blank(d.from) && !posInt(d.from)) return 'From 은 양의 정수 Id(비우면 Auto)'
  if (!blank(d.to) && !posInt(d.to)) return 'To 는 양의 정수 Id(비우면 Auto)'
  if (d.kind === 'move' && (blank(d.from) || blank(d.to))) return '이동은 From · To 셀이 모두 필요'
  if (d.kind === 'move' && d.from.trim() === d.to.trim()) return 'From 과 To 가 같습니다'
  if (!/^-?\d+$/.test(d.priority.trim())) return 'Priority 는 정수'
  return null
}

/** 검증된 초안 → API 본문(검증은 `draftError` 가 먼저). */
export function draftToRequest(d: RequestDraft): NewRequest {
  const k = endpointKinds(d.kind)
  const tgt = (s: string, kind: TargetKind): Target | null =>
    blank(s) ? null : { kind, id: Number(s) }
  return {
    kind: d.kind,
    source: 'user',
    item_code: blank(d.item) ? null : Number(d.item),
    qty: Number(d.qty),
    from: tgt(d.from, k.from),
    to: tgt(d.to, k.to),
    priority: Number(d.priority),
    note: d.note.trim(),
  }
}
