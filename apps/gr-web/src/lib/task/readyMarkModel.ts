// 레이아웃 맵의 PICK/DROP 준비 표시(▲ ▼) — 스케줄러 준비 상태(`sched.ts` `TargetReady`)를 그릴 것으로 바꾼다.
// 판정은 `readyMark`(sched.ts) 하나를 쓴다 — 수요 표와 지도가 같은 뜻을 보인다. 계약: docs/scheduler.md 준비 상태.
import { STATE_LABEL } from '../gr/const'
import { robotColor } from '../robotContext'
import { readyMark, type Hold, type ReadyMark, type ReadySnapshot, type TargetReady } from '../sched'
import type { TaskState } from '../types'

export type ReadyOp = 'pick' | 'drop'
/** 색 — ok = 초록, warn = 주황(품목 미정), robot = 예약한 로봇 색, muted = 막힘(속 빈 회색). */
export type ReadyTone = 'ok' | 'warn' | 'robot' | 'muted'

export interface ReadyGlyph {
  op: ReadyOp
  mark: Exclude<ReadyMark, 'none'>
  glyph: '▲' | '▼'
  tone: ReadyTone
  /** `tone = robot` 일 때 로봇 색(그 밖 null). */
  color: string | null
  /** 한 줄 설명(`PICK 가능` · `PICK 불가 — 이유` · `GR2 PICK 예약`). */
  title: string
}

const GLYPH: Record<ReadyOp, '▲' | '▼'> = { pick: '▲', drop: '▼' }
const OP_NAME: Record<ReadyOp, string> = { pick: 'PICK', drop: 'DROP' }
const TONE: Record<Exclude<ReadyMark, 'none'>, ReadyTone> = {
  ready: 'ok',
  need_item: 'warn',
  held: 'robot',
  blocked: 'muted',
}

/** 이 일(PICK 은 MEASURE 포함)을 예약한 것 — 실행 중인 것을 먼저. */
export function holdsFor(t: TargetReady, op: ReadyOp): Hold[] {
  const hs = t.holds.filter((h) =>
    op === 'pick' ? h.op === 'PICK' || h.op === 'MEASURE' : h.op === 'DROP',
  )
  return hs.sort((a, b) => Number(b.state === 'running') - Number(a.state === 'running'))
}

/** 예약 상태 — 원장에 없는 `queued` 는 예정 큐, 나머지는 Task 상태 라벨. */
export function holdStateLabel(h: Hold): string {
  if (h.state === 'queued' && !h.task) return '예정'
  return STATE_LABEL[h.state as TaskState] ?? h.state
}

/** 예약 한 줄: `GR2 PICK TO-260928-0003 (실행 중)`. */
export function holdLine(h: Hold): string {
  const ref = h.order ?? h.task
  return `${h.robot_name || `GR${h.robot}`} ${h.op}${ref ? ` ${ref}` : ''} (${holdStateLabel(h)})`
}

/** 한 일의 한 줄 판정 — 표시 여부와 상관없이(호버 · 정보 카드). */
export function opLine(t: TargetReady, op: ReadyOp): string {
  const name = OP_NAME[op]
  const r = t[op]
  if (!r.ok) return `${name} 불가${r.why ? ` — ${r.why}` : ''}`
  if (op === 'pick' && t.kind === 'station' && t.need_item) {
    const hint = t.hints.length ? ` · 후보 ${t.hints.slice(0, 3).join(', ')}` : ''
    return `${name} 가능 — 품목 미정${hint}`
  }
  return `${name} 가능${r.why ? ` — ${r.why}` : ''}`
}

/** 그릴 표식 — `none` 은 빼고 PICK, DROP 순. */
export function readyGlyphs(t: TargetReady | undefined): ReadyGlyph[] {
  if (!t) return []
  const out: ReadyGlyph[] = []
  for (const op of ['pick', 'drop'] as const) {
    const mark = readyMark(t, op)
    if (mark === 'none') continue
    const h = mark === 'held' ? holdsFor(t, op)[0] : undefined
    out.push({
      op,
      mark,
      glyph: GLYPH[op],
      tone: TONE[mark],
      color: h ? robotColor(h.robot) : null,
      title: h ? `${OP_NAME[op]} 예약 — ${holdLine(h)}` : opLine(t, op),
    })
  }
  return out
}

/** 미래 재고 한 줄: `ItemCode 1203 × 4 · Room 2`(Room 모름 = 제한 없음). */
export function stockLine(t: TargetReady): string {
  return `ItemCode ${t.item_code || '-'} × ${t.count} · Room ${t.room ?? '∞'}`
}

/** 호버 카드 줄 — PICK · DROP 판정, 예약, 미래 재고. */
export function readyLines(t: TargetReady | undefined): string[] {
  if (!t) return []
  return [opLine(t, 'pick'), opLine(t, 'drop'), ...t.holds.map(holdLine), stockLine(t)]
}

/** 정보 카드 한 칸: `PICK ✓` · `PICK ✓ 품목 미정`(warn) · `DROP ✗ 이유`(bad). */
export function readyShort(
  t: TargetReady,
  op: ReadyOp,
): { tone: 'ok' | 'warn' | 'bad'; text: string } {
  const r = t[op]
  const need = op === 'pick' && t.kind === 'station' && t.need_item && r.ok
  const why = need ? '품목 미정' : r.why
  return {
    tone: !r.ok ? 'bad' : need ? 'warn' : 'ok',
    // design-lint-allow: no-decoration — ✓ ✗ 는 장식이 아니라 가능/불가 값 글자다(한 줄에 두 판정을 좁게)
    text: `${OP_NAME[op]} ${r.ok ? '✓' : '✗'}${why ? ` ${why}` : ''}`,
  }
}

/** 받은 값을 스냅샷으로 — 모양이 다르면(백엔드 미구현 · 옛 버전) 빈 값. */
export function toSnapshot(d: unknown): ReadySnapshot {
  if (!d || typeof d !== 'object') return { at: '', targets: [] }
  const o = d as { at?: unknown; targets?: unknown }
  if (!Array.isArray(o.targets)) return { at: '', targets: [] }
  const targets = o.targets.filter(
    (t): t is TargetReady =>
      !!t &&
      typeof t === 'object' &&
      (t.kind === 'cell' || t.kind === 'station') &&
      typeof t.id === 'number' &&
      !!t.pick &&
      !!t.drop &&
      Array.isArray(t.holds),
  )
  return { at: typeof o.at === 'string' ? o.at : '', targets }
}
