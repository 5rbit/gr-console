// 팔렛 패턴 편집 순수 모델 — 단위 변환(정규화 ↔ mm), 끌기 → 오프셋, Seq 재정렬, 슬롯 추가·삭제, 회전·미러,
// 검증(백엔드 `pallet/edit.rs` 와 같은 규칙), 비교(저장 전 변경 표시).
//
// 편집 좌표 `u` 는 기계 축 정규화 오프셋이고 mm 보기는 `u × (OuterDiameter + Gap)` 이다(미리보기 OD·Gap).
// 생성기는 저장된 u 를 그대로 두고 오프셋을 `(OuterDiameter + Gap) / MinDistance(u)` 로 맞춘다 — 그래서 겹침은
// 편집 좌표로, 팔렛 밖은 생성 결과(배율 적용)로 미리 본다.
import {
  applyDir,
  applyTransform,
  dirGrid,
  overhang,
  toScreen,
  type DragKind,
  type ScreenAxes,
  type Transform,
} from './model'

export interface DraftSlot {
  slot: string
  seq: number
  u: [number, number]
  drag_dir: number
}

export interface DraftPattern {
  pattern: number
  od_min: number
  od_max: number
  note: string
  slots: DraftSlot[]
}

export const FLOWS_ID_RE = /^[A-Z0-9_]{2,32}$/
export const SLOTS_MAX = 20
export const NAME_MAX = 80
export const NOTE_MAX = 500
export const SLOT_NAME_MAX = 16
export const U_MAX = 20
export const OD_LIMIT = 3000
export const COINCIDENT = 0.01
export const NORM_EPS = 1e-3
export const OD_BOUNDARY_TOL = 1
const U_EPS = 5e-5
const OD_EPS = 1e-3

export type Units = 'norm' | 'mm'
export type SnapMode = 'off' | 'mm10' | 'norm005'

const z0 = (v: number): number => (v === 0 ? 0 : v)
export const round4 = (v: number): number => z0(Math.round(v * 10_000) / 10_000)
const round1 = (v: number): number => z0(Math.round(v * 10) / 10)

// ── 흐름 id ─────────────────────────────────────────────────────────────────

/** 앞뒤 공백을 떼고 대문자로 — 백엔드 `normalize_flow_id` 와 같다. */
export function normalizeFlowId(raw: string): { id: string; error: string | null } {
  const id = raw.trim().toUpperCase()
  return FLOWS_ID_RE.test(id)
    ? { id, error: null }
    : { id, error: `Flow id "${raw.trim()}" 는 [A-Z0-9_] 2..32 자여야 합니다` }
}

// ── 복사 · 단위 ─────────────────────────────────────────────────────────────

export function toDraft(p: {
  pattern: number
  od_min: number
  od_max: number
  note?: string
  slots: readonly { slot: string; seq: number; u: readonly [number, number] | number[]; drag_dir: number }[]
}): DraftPattern {
  return {
    pattern: p.pattern,
    od_min: p.od_min,
    od_max: p.od_max,
    note: p.note ?? '',
    slots: p.slots.map((s) => ({ slot: s.slot, seq: s.seq, u: [s.u[0], s.u[1]], drag_dir: s.drag_dir })),
  }
}

export function pitchOf(od: number, gap: number): number {
  const p = od + gap
  return Number.isFinite(p) && p > 0 ? p : 0
}

export function normToMm(v: number, pitch: number): number {
  return round1(v * pitch)
}

export function mmToNorm(mm: number, pitch: number): number {
  return pitch > 0 && Number.isFinite(mm) ? round4(mm / pitch) : 0
}

export function snapU(u: readonly [number, number], mode: SnapMode, pitch: number): [number, number] {
  if (mode === 'norm005') return [round4(Math.round(u[0] / 0.05) * 0.05), round4(Math.round(u[1] / 0.05) * 0.05)]
  if (mode === 'mm10' && pitch > 0)
    return [mmToNorm(Math.round((u[0] * pitch) / 10) * 10, pitch), mmToNorm(Math.round((u[1] * pitch) / 10) * 10, pitch)]
  return [round4(u[0]), round4(u[1])]
}

/**
 * 끌기 — 그림 좌표(보기 방향 mm, 오른쪽·아래 +)의 이동량을 기계 축으로 돌려 시작 u 에 더한다.
 * 보기 변환은 축 부호만 바꾸므로 `toScreen` 이 자기 역변환이다.
 */
export function dragToU(
  startU: readonly [number, number],
  startScreen: readonly [number, number],
  curScreen: readonly [number, number],
  axes: ScreenAxes,
  pitch: number,
  snap: SnapMode,
): [number, number] {
  if (!(pitch > 0)) return [startU[0], startU[1]]
  const [mx, my] = toScreen(axes, [curScreen[0] - startScreen[0], curScreen[1] - startScreen[1]])
  return snapU([startU[0] + mx / pitch, startU[1] + my / pitch], snap, pitch)
}

// ── 슬롯 목록 ───────────────────────────────────────────────────────────────

export function slotsBySeq(slots: readonly DraftSlot[]): DraftSlot[] {
  return [...slots].sort((a, b) => a.seq - b.seq)
}

const renumber = (ordered: readonly DraftSlot[]): DraftSlot[] => ordered.map((s, i) => ({ ...s, seq: i + 1 }))

/** 표에서 끌어 놓기 — `fromSeq` 슬롯을 `toSeq` 자리로 옮기고 나머지를 차례로 당긴다. */
export function reorderSeq(slots: readonly DraftSlot[], fromSeq: number, toSeq: number): DraftSlot[] {
  const order = slotsBySeq(slots)
  const from = order.findIndex((s) => s.seq === fromSeq)
  if (from < 0) return slots.map((s) => ({ ...s }))
  const to = Math.min(order.length - 1, Math.max(0, toSeq - 1))
  const [moved] = order.splice(from, 1)
  order.splice(to, 0, moved)
  return renumber(order)
}

/** "Seq 재정렬" — 누른 순서대로 1, 2, … 나머지는 기존 순서로 뒤에. */
export function seqByClicks(slots: readonly DraftSlot[], clicked: readonly string[]): DraftSlot[] {
  const seen = new Set<string>()
  const first: DraftSlot[] = []
  for (const name of clicked) {
    const s = slots.find((x) => x.slot === name)
    if (s && !seen.has(name)) {
      seen.add(name)
      first.push(s)
    }
  }
  return renumber([...first, ...slotsBySeq(slots).filter((s) => !seen.has(s.slot))])
}

export function nextSlotName(slots: readonly DraftSlot[]): string {
  const used = new Set(slots.map((s) => s.slot))
  let k = 1
  while (used.has(`C#${k}`)) k++
  return `C#${k}`
}

export function normMinDistance(slots: readonly { u: readonly [number, number] | number[] }[]): number | null {
  let m = Infinity
  for (let i = 0; i < slots.length; i++)
    for (let j = i + 1; j < slots.length; j++)
      m = Math.min(m, Math.hypot(slots[i].u[0] - slots[j].u[0], slots[i].u[1] - slots[j].u[1]))
  return Number.isFinite(m) ? m : null
}

/** 생성기 배율 = 1 / MinDistance (슬롯 1개면 1). */
export function scaleOf(slots: readonly DraftSlot[]): number {
  const d = normMinDistance(slots)
  return d !== null && d > 1e-6 ? 1 / d : 1
}

/** 새 슬롯 — 다른 슬롯과 1.0 이상 떨어진 가장 가운데 격자점(0.5 간격)에 둔다. 마지막 Seq, DragDir 0. */
export function addSlot(slots: readonly DraftSlot[]): DraftSlot[] {
  const cands: [number, number][] = []
  for (let i = -8; i <= 8; i++) for (let j = -8; j <= 8; j++) cands.push([i * 0.5, j * 0.5])
  cands.sort((a, b) => Math.hypot(a[0], a[1]) - Math.hypot(b[0], b[1]) || a[0] - b[0] || a[1] - b[1])
  const free =
    cands.find((c) => slots.every((s) => Math.hypot(s.u[0] - c[0], s.u[1] - c[1]) >= 1 - 1e-9)) ?? [0, 0]
  return [
    ...slots.map((s) => ({ ...s })),
    { slot: nextSlotName(slots), seq: slots.length + 1, u: [free[0], free[1]], drag_dir: 0 },
  ]
}

export function removeSlot(slots: readonly DraftSlot[], name: string): DraftSlot[] {
  return renumber(slotsBySeq(slots).filter((s) => s.slot !== name))
}

/** 패턴 전체 회전·미러 — 오프셋과 DragDir 를 같은 변환으로(`model.ts` 변환 그대로). */
export function transformSlots(slots: readonly DraftSlot[], t: Transform): DraftSlot[] {
  return slots.map((s) => {
    const u = applyTransform(t, s.u)
    return { ...s, u: [round4(u[0]), round4(u[1])], drag_dir: applyDir(t, s.drag_dir) }
  })
}

/** MinDistance 를 1 로 맞춘다(선택 도구 — 저장은 배율과 무관하게 같은 결과를 낸다). */
export function rescaleToUnit(slots: readonly DraftSlot[]): DraftSlot[] {
  const d = normMinDistance(slots)
  if (d === null || d < COINCIDENT) return slots.map((s) => ({ ...s }))
  return slots.map((s) => ({ ...s, u: [round4(s.u[0] / d), round4(s.u[1] / d)] }))
}

export function cycleDir(d: number): number {
  return Number.isInteger(d) && d >= 0 && d < 8 ? d + 1 : 0
}

/** DragDir 고르기 격자(보기 방향) — 가운데 칸은 0(드래그 없음). */
export function dirPickerGrid(axes: ScreenAxes): number[][] {
  return dirGrid(axes).map((row, i) => row.map((d, j) => (i === 1 && j === 1 ? 0 : (d ?? 0))))
}

// ── 패턴 목록 ───────────────────────────────────────────────────────────────

export function nextPatternNumber(existing: readonly number[], preferred: number): number {
  const used = new Set(existing)
  if (Number.isInteger(preferred) && preferred >= 1 && preferred <= 255 && !used.has(preferred)) return preferred
  for (let n = 1; n <= 255; n++) if (!used.has(n)) return n
  return 0
}

/** 새·복제 패턴의 OD 범위 — 기존 최대 OdMax 위(겹치지 않게). 운전자가 고쳐 넣는다. */
export function odRangeAbove(existing: readonly DraftPattern[]): [number, number] {
  const top = existing.reduce((m, p) => Math.max(m, Number.isFinite(p.od_max) ? p.od_max : 0), 0)
  const lo = existing.length === 0 ? 0 : Math.floor(top) + 1
  return [lo, lo + 50]
}

export function newPattern(existing: readonly DraftPattern[]): DraftPattern {
  const [od_min, od_max] = odRangeAbove(existing)
  return {
    pattern: nextPatternNumber(existing.map((p) => p.pattern), 1),
    od_min,
    od_max,
    note: '',
    slots: [{ slot: 'C#1', seq: 1, u: [0, 0], drag_dir: 0 }],
  }
}

export function duplicatePattern(src: DraftPattern, existing: readonly DraftPattern[]): DraftPattern {
  const [od_min, od_max] = odRangeAbove(existing)
  return {
    ...toDraft(src),
    pattern: nextPatternNumber(existing.map((p) => p.pattern), src.slots.length),
    od_min,
    od_max,
  }
}

// ── 검증(백엔드 edit.rs 와 같은 규칙) ───────────────────────────────────────

export interface Validation {
  errors: string[]
  warnings: string[]
}

export function odOverlaps(ps: readonly Pick<DraftPattern, 'pattern' | 'od_min' | 'od_max'>[]): string[] {
  const out: string[] = []
  for (let i = 0; i < ps.length; i++)
    for (let j = i + 1; j < ps.length; j++) {
      const a = ps[i]
      const b = ps[j]
      if (a.od_min <= b.od_max && b.od_min <= a.od_max)
        out.push(`OD 범위가 겹칩니다: Pattern ${a.pattern} (${a.od_min}~${a.od_max}) 와 Pattern ${b.pattern} (${b.od_min}~${b.od_max})`)
    }
  return out
}

export function odGaps(ps: readonly Pick<DraftPattern, 'pattern' | 'od_min' | 'od_max'>[]): string[] {
  const v = ps.filter((p) => p.od_min < p.od_max).sort((a, b) => a.od_min - b.od_min)
  const out: string[] = []
  for (let i = 1; i < v.length; i++)
    if (v[i].od_min - v[i - 1].od_max > OD_BOUNDARY_TOL)
      out.push(`no pattern for OD ${v[i - 1].od_max}~${v[i].od_min} (Pattern ${v[i - 1].pattern} 과 Pattern ${v[i].pattern} 사이)`)
  return out
}

/**
 * 패턴 한 개 + 같은 흐름의 다른 패턴(`others`, 편집 중인 패턴의 원래 번호는 뺀 것)과의 번호·OD 겹침.
 * 오류가 있으면 저장하지 못한다. 경고는 저장해도 된다.
 */
export function validatePattern(p: DraftPattern, kind: DragKind, others: readonly DraftPattern[]): Validation {
  const tag = `Pattern ${p.pattern}`
  const errors: string[] = []
  const warnings: string[] = []
  if (!Number.isInteger(p.pattern) || p.pattern < 1 || p.pattern > 255) errors.push(`${tag}: Pattern 번호는 1..255`)
  if (others.some((o) => o.pattern === p.pattern)) errors.push(`${tag}: 같은 번호의 Pattern 이 이미 있습니다`)
  if (!Number.isFinite(p.od_min) || !Number.isFinite(p.od_max) || p.od_min < 0 || p.od_max > OD_LIMIT)
    errors.push(`${tag}: OdMin/OdMax 는 0..=${OD_LIMIT} mm 여야 합니다`)
  else if (p.od_min >= p.od_max) errors.push(`${tag}: OdMin ${p.od_min} 은 OdMax ${p.od_max} 보다 작아야 합니다`)
  const n = p.slots.length
  if (n < 1 || n > SLOTS_MAX) errors.push(`${tag}: 슬롯 수 ${n} — 1..=${SLOTS_MAX} 개여야 합니다`)
  const names = new Set<string>()
  for (const s of p.slots) {
    const nm = s.slot.trim()
    if (nm === '' || [...nm].length > SLOT_NAME_MAX) errors.push(`${tag}: Slot 이름 "${nm}" 은 1..=${SLOT_NAME_MAX} 자여야 합니다`)
    else if (names.has(nm)) errors.push(`${tag}: Slot ${nm} 이 중복입니다`)
    names.add(nm)
    if (!Number.isInteger(s.drag_dir) || s.drag_dir < 0 || s.drag_dir > 8) errors.push(`${tag}: ${nm} DragDir ${s.drag_dir} — 0..8 이어야 합니다`)
    if (!s.u.every((v) => Number.isFinite(v) && Math.abs(v) <= U_MAX))
      errors.push(`${tag}: ${nm} OffsetX/OffsetY(정규화) 는 |u| ≤ ${U_MAX} 여야 합니다`)
  }
  const seqs = p.slots.map((s) => s.seq).sort((a, b) => a - b)
  if (n > 0 && !seqs.every((s, i) => s === i + 1)) errors.push(`${tag}: Seq 는 1..${n} 을 한 번씩 써야 합니다 (받은 값 ${seqs.join(',')})`)
  if ([...p.note].length > NOTE_MAX) errors.push(`${tag}: note 는 ${NOTE_MAX} 자 이하`)
  const d = p.slots.every((s) => s.u.every(Number.isFinite)) ? normMinDistance(p.slots) : null
  if (d !== null) {
    if (d < COINCIDENT) errors.push(`${tag}: 두 슬롯이 같은 자리입니다 (MinDistance ${round4(d)})`)
    else if (Math.abs(d - 1) > NORM_EPS)
      warnings.push(`${tag}: MinDistance ${round4(d)} ≠ 1 — 생성기가 오프셋을 ×${round4(1 / d)} 해서 최소 중심거리를 OuterDiameter + Gap 으로 맞춥니다`)
  }
  errors.push(...odOverlaps([p, ...others]).filter((e) => e.includes(`Pattern ${p.pattern} (`)))
  warnings.push(...odGaps([p, ...others]))
  if (errors.length === 0) {
    const at = kind === 'in' ? 1 : n
    const s = p.slots.find((x) => x.seq === at)
    if (s && s.drag_dir !== 0)
      warnings.push(
        `${tag}: ${kind === 'in' ? '입고' : '출하'} 흐름인데 Seq ${at}(${s.slot}) 에 DragDir ${s.drag_dir} — 사양서는 ${kind === 'in' ? '첫' : '마지막'} 작업을 드래그 없이 둡니다`,
      )
  }
  return { errors, warnings }
}

// ── 실시간 경고 ─────────────────────────────────────────────────────────────

export interface LiveIssue {
  slot: string
  kind: 'overlap' | 'overhang'
  text: string
}

/** 겹침 = 편집 좌표(u × pitch) 중심거리 < OuterDiameter, 팔렛 밖 = 생성 결과(배율 적용) 기준. */
export function liveIssues(slots: readonly DraftSlot[], od: number, gap: number, palletSize: number): LiveIssue[] {
  const pitch = pitchOf(od, gap)
  if (!(pitch > 0) || !(od > 0)) return []
  const out: LiveIssue[] = []
  for (let i = 0; i < slots.length; i++)
    for (let j = i + 1; j < slots.length; j++) {
      const a = slots[i]
      const b = slots[j]
      const dist = Math.hypot(a.u[0] - b.u[0], a.u[1] - b.u[1]) * pitch
      if (dist + 0.05 < od) {
        const text = `${a.slot} · ${b.slot} 겹침 — 편집 좌표 중심거리 ${round1(dist)} mm < OuterDiameter ${od}`
        out.push({ slot: a.slot, kind: 'overlap', text }, { slot: b.slot, kind: 'overlap', text })
      }
    }
  const k = pitch * scaleOf([...slots])
  for (const s of slots) {
    const oh = overhang([s.u[0] * k, s.u[1] * k], od, palletSize)
    const axes = (['X', 'Y'] as const).filter((_, i) => oh[i] > 0)
    if (axes.length > 0)
      out.push({ slot: s.slot, kind: 'overhang', text: `${s.slot} 팔렛 밖 ${axes.map((a) => `${a} ${oh[a === 'X' ? 0 : 1]} mm`).join(' · ')} (생성 결과)` })
  }
  return out
}

// ── 비교 ────────────────────────────────────────────────────────────────────

export type ChangeStatus = 'same' | 'changed' | 'added' | 'removed'

export interface SlotChange {
  slot: string
  status: ChangeStatus
  fields: string[]
}

export interface PatternChange {
  status: ChangeStatus
  fields: string[]
  slots: SlotChange[]
}

/** `cur` 를 `base` 와 비교(백엔드 `diff_flow` 의 패턴 부분과 같은 필드·허용오차). */
export function diffPattern(cur: DraftPattern | null, base: DraftPattern | null): PatternChange {
  if (cur && !base) return { status: 'added', fields: [], slots: cur.slots.map((s) => ({ slot: s.slot, status: 'added', fields: [] })) }
  if (!cur && base) return { status: 'removed', fields: [], slots: base.slots.map((s) => ({ slot: s.slot, status: 'removed', fields: [] })) }
  if (!cur || !base) return { status: 'same', fields: [], slots: [] }
  const fields: string[] = []
  if (cur.pattern !== base.pattern) fields.push('pattern')
  if (Math.abs(cur.od_min - base.od_min) > OD_EPS || Number.isNaN(cur.od_min)) fields.push('od_min')
  if (Math.abs(cur.od_max - base.od_max) > OD_EPS || Number.isNaN(cur.od_max)) fields.push('od_max')
  if (cur.note.trim() !== base.note.trim()) fields.push('note')
  const slots: SlotChange[] = []
  for (const b of base.slots) {
    const c = cur.slots.find((s) => s.slot.trim() === b.slot)
    if (!c) {
      slots.push({ slot: b.slot, status: 'removed', fields: [] })
      continue
    }
    const f: string[] = []
    if (c.seq !== b.seq) f.push('seq')
    if (!(Math.abs(c.u[0] - b.u[0]) <= U_EPS && Math.abs(c.u[1] - b.u[1]) <= U_EPS)) f.push('u')
    if (c.drag_dir !== b.drag_dir) f.push('drag_dir')
    slots.push({ slot: b.slot, status: f.length ? 'changed' : 'same', fields: f })
  }
  for (const c of cur.slots) if (!base.slots.some((b) => b.slot === c.slot.trim())) slots.push({ slot: c.slot, status: 'added', fields: [] })
  const same = fields.length === 0 && slots.every((s) => s.status === 'same')
  return { status: same ? 'same' : 'changed', fields, slots }
}

export function isDirty(draft: DraftPattern | null, saved: DraftPattern | null): boolean {
  return diffPattern(draft, saved).status !== 'same'
}

/** `PUT /api/pallet/flows/{id}/patterns/{원래 번호}` 본문 — 번호가 바뀌었으면 `pattern` 으로 옮긴다. */
export function patternBody(d: DraftPattern): {
  pattern: number
  od_min: number
  od_max: number
  note: string
  slots: DraftSlot[]
} {
  return {
    pattern: d.pattern,
    od_min: d.od_min,
    od_max: d.od_max,
    note: d.note.trim(),
    slots: d.slots.map((s) => ({ slot: s.slot.trim(), seq: s.seq, u: [round4(s.u[0]), round4(s.u[1])], drag_dir: s.drag_dir })),
  }
}

/** 편집 그림의 반폭(mm) — 팔렛과 모든 슬롯이 들어오게. */
export function viewHalfExtent(slots: readonly DraftSlot[], pitch: number, od: number, palletSize: number): number {
  const far = slots.reduce((m, s) => Math.max(m, Math.abs(s.u[0]) * pitch, Math.abs(s.u[1]) * pitch), 0)
  return Math.max(palletSize / 2, far + od / 2)
}
