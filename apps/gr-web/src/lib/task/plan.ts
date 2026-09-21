// 순차 작업 계획(생성 예정 작업) — 레이아웃 클릭이 PICK → DROP → PICK … 으로 번갈아 쌓이는 순수 모델.
//
// 재고를 시뮬레이션하며 각 스텝의 Z(백엔드 `registry::spec::stack_z_with` 와 같은 차례)와 경고를 만든다:
//   ① 잡는 타이어가 설 스택 크기(PICK 은 n, DROP 은 n+1)를 통째로 잰 프로파일이 있으면
//      Z = floor + AbsUpperBead(size, level) − PickBeadOffset — 환산도 아래 타이어 합산도 없다.
//   ② 없으면 프로파일에서 파생한 하중 곡선으로: floor + 아래 타이어들의 PressedHeight 합 + grip.
//   ③ 잰 게 하나도 없으면 grip = mid(H/2). 스칼라 Compression 은 ②·③ 의 아래 타이어 높이에만 쓴다.
// 되돌리기/다시실행은 계획 배열의 스냅샷 스택이다.
import {
  MIN_PITCH,
  absUpperBead,
  compressionOf,
  measuredBead,
  pickBeadOffset,
  profileOf,
  specOf,
  stackBase,
} from '../items/levelsModel'
import type {
  Cell,
  GripRef,
  Item,
  ItemSpec,
  PalletRef,
  ScenarioStep,
  ScenarioUpsert,
  Station,
  StockEntry,
  TaskType,
  Target,
  TaskRequest,
} from '../types'
import { validateDraft } from './compose'
import { moveOf, moveParams, sentItem, moveZ, type MoveOpts } from './moveMode'

export interface PlanStep {
  id: string
  type: TaskType
  target: Target
  item_code: number | null
  count: number
  note: string
  /** 보낼 로봇(없으면 기본 로봇). */
  robot?: number | null
  /** 팔렛 슬롯(팔렛 패턴 화면이 싣는다) — compose 가 슬롯 XY·드래그를 정한다. */
  pallet?: PalletRef | null
  /** MOVE 방식(없으면 화면 기본 `top`) — `toRequest` 가 `params.move_mode` 로 싣는다. */
  move?: MoveOpts | null
}

export interface PlanRow extends PlanStep {
  /** 1부터. */
  no: number
  /** 스텝 시작 시점의 셀 재고 (스테이션이면 null). */
  stockBefore: number | null
  stockAfter: number | null
  /** 계산된 Z (품목 높이를 모르면 null). */
  z: number | null
  floor: number | null
  height: number | null
  warnings: string[]
}

export interface Carry {
  item_code: number | null
  count: number
}

let seq = 0
export function stepId(): string {
  seq++
  return `p${Date.now().toString(36)}${seq}`
}

/** 다음 클릭에 붙을 종류 — 마지막 PICK 뒤엔 DROP, 그 외엔 PICK. */
export function nextType(steps: readonly PlanStep[]): TaskType {
  for (let i = steps.length - 1; i >= 0; i--) {
    const t = steps[i].type
    if (t === 'PICK') return 'DROP'
    if (t === 'DROP') return 'PICK'
  }
  return 'PICK'
}

/** 마지막 PICK 이후 아직 내려놓지 않은 화물. */
export function carried(steps: readonly PlanStep[]): Carry | null {
  for (let i = steps.length - 1; i >= 0; i--) {
    const s = steps[i]
    if (s.type === 'DROP') return null
    if (s.type === 'PICK') return { item_code: s.item_code, count: s.count }
  }
  return null
}

/**
 * 화면이 고를 수 있는 그립 기준. 맨 비드(`bead`)는 없어졌다 — 비드로 잡는 길은 `bead+offset` 하나뿐이다.
 * 저장되는 값(`pick_bead`)은 그대로 두고 이름만 뜻이 드러나게 붙인다.
 */
export const GRIP_REFS: readonly { id: GripRef; label: string; title: string }[] = [
  { id: 'mid', label: '타이어 중간 (H/2)', title: 'Z 의 그립 위치 = 타이어 높이 / 2 — 기본값' },
  {
    id: 'pick_bead',
    label: 'bead+offset (− PickBeadOffset)',
    title:
      'Z 의 그립 위치 = 잰 상부 비드 − PickBeadOffset(화물 규격, 기본 30 mm). 아직 한 번도 안 잰 품목은 타이어 중간(H/2)으로 잡습니다.',
  },
]

/** 옛 값(`bead`)·오타를 정본으로 — 백엔드 `spec::normalize_grip_ref` 과 같다. */
export function normalizeGripRef(ref: string | null | undefined): GripRef {
  const v = (ref ?? '').trim().toLowerCase().replace(/[-+ ]/g, '_')
  return v === 'bead' || v === 'pick_bead' || v === 'pickbead' || v === 'bead_offset'
    ? 'pick_bead'
    : 'mid'
}

/**
 * 타이어 바닥에서 그리퍼가 잡는 높이 — `mid` = H/2, `pick_bead` = 상부 비드 − PickBeadOffset.
 * 비드는 위에 얹힌 `above` 개만큼 눌린다. **잰 비드가 없으면 mid 로 내려간다.**
 * 백엔드 `spec::grip_point` 과 같다.
 */
export function gripOffset(
  ref: GripRef | string | undefined,
  item: { height: number; upper_bead_height?: number; spec?: Partial<ItemSpec> | null } | undefined,
  above = 0,
): number {
  const h = item?.height ?? 0
  if (!item || normalizeGripRef(ref) !== 'pick_bead') return h / 2
  const bead = measuredBead(
    { height: h, upper_bead_height: item.upper_bead_height ?? 0, lower_bead_height: 0 },
    specOf(item),
    Math.max(above, 0),
  )
  return bead === null ? h / 2 : Math.max(bead - pickBeadOffset(specOf(item)), 0)
}

/** 미리보기가 보일 한 줄 — 어느 갈래로 잡았는지(백엔드 `StackZ.used` 의 `grip=…` 과 같은 뜻). */
export function gripLabel(ref: GripRef | string | null | undefined, usedRef: GripRef): string {
  if (usedRef === 'pick_bead') return 'grip=bead+offset'
  return normalizeGripRef(ref) === 'pick_bead' ? 'grip=mid (측정 없음)' : 'grip=mid'
}

/** 잡는 타이어 위에 얹힌 개수 — PICK/MEASURE 은 같이 가져갈 c−1 개, DROP 은 0. */
export function aboveCount(type: TaskType, n: number, c: number): number {
  if (type === 'PICK' || type === 'MEASURE') return Math.max(Math.min(n, Math.max(c, 1)) - 1, 0)
  return 0
}

/** 잡는 타이어 아래에 깔리는 개수 — PICK/MEASURE: n − c, DROP: n, 그 밖: 0. */
export function belowCount(type: TaskType, n: number, c: number): number {
  if (type === 'PICK' || type === 'MEASURE') return Math.max(n - Math.max(c, 1), 0)
  return type === 'DROP' ? n : 0
}

/**
 * 한 스텝의 Z — 백엔드 `spec::stack_z_with` 과 같은 차례(프로파일 → 곡선 → mid).
 * `ref` 는 **실제로 쓴** 그립 기준이다.
 */
export function planZ(
  type: TaskType,
  floor: number,
  item: { height: number; upper_bead_height?: number; spec?: Partial<ItemSpec> | null } | undefined,
  gripRef: GripRef | string | undefined,
  n: number,
  c: number,
): { z: number; ref: GripRef; source: 'profile' | 'curve' | 'computed' } {
  const h = item?.height ?? 0
  if (type !== 'PICK' && type !== 'MEASURE' && type !== 'DROP')
    return { z: floor, ref: 'mid', source: 'computed' }
  const spec = item ? specOf(item) : null
  const level = belowCount(type, n, c) + 1
  const above = aboveCount(type, n, c)
  if (spec && normalizeGripRef(gripRef) === 'pick_bead') {
    const prof = profileOf(spec, level + above)
    const abs = prof ? absUpperBead(prof, level) : null
    if (abs !== null)
      return {
        z: floor + Math.max(Math.max(abs, MIN_PITCH) - pickBeadOffset(spec), 0),
        ref: 'pick_bead',
        source: 'profile',
      }
  }
  const grip = gripOffset(gripRef ?? 'mid', item, above)
  const bead =
    spec && item
      ? measuredBead(
          { height: h, upper_bead_height: item.upper_bead_height ?? 0, lower_bead_height: 0 },
          spec,
          above,
        )
      : null
  return {
    z: stackZ(type, floor, h, n, c, grip, spec ? compressionOf(spec) : 0),
    ref: bead === null ? 'mid' : 'pick_bead',
    source: bead === null ? 'computed' : 'curve',
  }
}

export function stackZ(
  type: TaskType,
  floor: number,
  h: number,
  n: number,
  c: number,
  grip = h / 2,
  compression = 0,
): number {
  if (type === 'PICK' || type === 'MEASURE')
    return floor + stackBase(h, compression, n, Math.max(n - Math.max(c, 1), 0)) + grip
  if (type === 'DROP') return floor + stackBase(h, compression, n, n) + grip
  return floor
}

/**
 * 클릭 한 번을 스텝으로 만든다. PICK 은 셀 재고의 품목을, DROP 은 들고 있는 품목을 잇는다.
 * `type` 을 주면 자동 교대 대신 그 종류로.
 */
export function stepForClick(
  steps: readonly PlanStep[],
  target: Target,
  stockNow: ReadonlyMap<number, StockEntry>,
  type?: TaskType,
  fallbackItem: number | null = null,
  robot: number | null = null,
): PlanStep {
  const t = type ?? nextType(steps)
  const sim = simulateStock(steps, stockNow)
  const st = target.kind === 'cell' ? sim.get(target.id) : undefined
  const carry = carried(steps)
  let item_code: number | null = null
  let count = 1
  if (t === 'DROP') {
    item_code = carry?.item_code ?? st?.item_code ?? fallbackItem
    count = carry?.count ?? 1
  } else if (t === 'PICK' || t === 'MEASURE') {
    item_code = st?.item_code || carry?.item_code || fallbackItem
  }
  if (item_code === 0) item_code = null
  return { id: stepId(), type: t, target, item_code, count, note: '', robot }
}

/**
 * 스텝 하나를 셀 재고에 접는다 — 백엔드 `stock::fold`(완료 시 재고 반영 · 미리 넣기의 예상 재고)와 같은 규칙.
 * DROP: 품목 = 스텝 품목(없으면 셀 품목), 개수 + n · PICK: 개수 − n(0 에서 멈춤), 남으면 셀 품목(없으면 스텝 품목), 다 비면 0.
 */
export function foldStock(
  cur: { item_code: number; count: number },
  type: TaskType,
  item_code: number | null,
  count: number,
): { item_code: number; count: number } {
  if (type === 'PICK') {
    const left = Math.max(cur.count - count, 0)
    return { item_code: left === 0 ? 0 : cur.item_code || (item_code ?? 0), count: left }
  }
  if (type === 'DROP') return { item_code: item_code ?? cur.item_code, count: cur.count + count }
  return cur
}

/** 계획을 순서대로 적용한 뒤의 셀 재고(품목/개수). */
export function simulateStock(
  steps: readonly PlanStep[],
  stockNow: ReadonlyMap<number, StockEntry>,
): Map<number, { item_code: number; count: number }> {
  const m = new Map<number, { item_code: number; count: number }>()
  for (const [k, v] of stockNow) m.set(k, { item_code: v.item_code, count: v.count })
  for (const s of steps) {
    if (s.target.kind !== 'cell') continue
    const cur = m.get(s.target.id) ?? { item_code: 0, count: 0 }
    m.set(s.target.id, foldStock(cur, s.type, s.item_code, s.count))
  }
  return m
}

export interface PlanContext {
  cells: readonly Cell[]
  stations: readonly Station[]
  items: readonly Item[]
  stockNow: ReadonlyMap<number, StockEntry>
  /** 그립 기준(기본 mid). */
  gripRef?: GripRef
}

/** 표에 보일 행 — 재고 전/후, Z, 경고. */
export function planRows(steps: readonly PlanStep[], ctx: PlanContext): PlanRow[] {
  const sim = new Map<number, { item_code: number; count: number }>()
  for (const [k, v] of ctx.stockNow) sim.set(k, { item_code: v.item_code, count: v.count })
  const rows: PlanRow[] = []
  let carry: Carry | null = null
  steps.forEach((s, i) => {
    const warnings: string[] = []
    const cell = s.target.kind === 'cell' ? ctx.cells.find((c) => c.id === s.target.id) : null
    const station =
      s.target.kind === 'station' ? ctx.stations.find((c) => c.id === s.target.id) : null
    const floor = cell ? cell.position[2] : station ? station.info.position[2] : null
    if (!cell && !station) warnings.push('대상이 레지스트리에 없음')
    const item = s.item_code !== null ? ctx.items.find((it) => it.code === s.item_code) : undefined
    if (s.item_code !== null && !item) warnings.push(`품목 ${s.item_code} 미등록`)
    if (s.item_code === null && (s.type === 'PICK' || s.type === 'DROP' || s.type === 'MEASURE'))
      warnings.push('품목 없음')
    const h = item?.height ?? null
    const st = cell ? (sim.get(cell.id) ?? { item_code: 0, count: 0 }) : null
    const n = st ? st.count : 0
    let z: number | null = null
    if (s.type === 'MOVE') {
      const mv = moveOf(s)
      // 스택 높이 — 스텝 품목, 없으면 셀 재고의 품목(백엔드도 재고 품목으로 채운다)
      const stackItem =
        item ?? (st?.item_code ? ctx.items.find((it) => it.code === st.item_code) : undefined)
      const sh =
        n === 0
          ? 0
          : stackItem && stackItem.height > 0
            ? stackBase(stackItem.height, compressionOf(specOf(stackItem)), n, n)
            : null
      if (mv.mode === 'stack' && sh === null) warnings.push('스택 높이 모름 — Top 권장')
      if (floor !== null) z = moveZ(mv, floor, sh)
    } else if (floor !== null && h !== null)
      z = planZ(s.type, floor, item, ctx.gripRef ?? 'mid', n, s.count).z
    if (cell && s.type !== 'MOVE') {
      if ((s.type === 'PICK' || s.type === 'MEASURE') && n === 0) warnings.push('셀 재고 없음')
      else if (s.type === 'PICK' && n < s.count) warnings.push(`재고 ${n} < 수량 ${s.count}`)
      if (n > 0 && st!.item_code && s.item_code !== null && st!.item_code !== s.item_code)
        warnings.push(`셀 품목 ${st!.item_code} ≠ ${s.item_code}`)
    }
    if (s.type === 'DROP' && !carry) warnings.push('들고 있는 화물 없음 (앞에 PICK 없음)')
    if (s.type === 'PICK' && carry) warnings.push('이미 들고 있음 (앞의 PICK 미완)')
    if (
      s.type === 'DROP' &&
      carry &&
      s.item_code !== null &&
      carry.item_code !== null &&
      carry.item_code !== s.item_code
    )
      warnings.push(`들고 있는 품목 ${carry.item_code} ≠ ${s.item_code}`)
    const before = st ? st.count : null
    // 적용
    if (cell && st) sim.set(cell.id, foldStock(st, s.type, s.item_code, s.count))
    if (s.type === 'PICK') carry = { item_code: s.item_code, count: s.count }
    else if (s.type === 'DROP') carry = null
    rows.push({
      ...s,
      no: i + 1,
      stockBefore: before,
      stockAfter: cell ? (sim.get(cell.id)?.count ?? null) : null,
      z,
      floor,
      height: h,
      warnings,
    })
  })
  return rows
}

// ── 편집 (순수, 새 배열 반환) ─────────────────────────────────────────────────

export function move(steps: readonly PlanStep[], from: number, to: number): PlanStep[] {
  if (from === to || from < 0 || from >= steps.length || to < 0 || to >= steps.length)
    return [...steps]
  const out = [...steps]
  const [s] = out.splice(from, 1)
  out.splice(to, 0, s)
  return out
}

export function remove(steps: readonly PlanStep[], id: string): PlanStep[] {
  return steps.filter((s) => s.id !== id)
}

export function patch(
  steps: readonly PlanStep[],
  id: string,
  p: Partial<Omit<PlanStep, 'id'>>,
): PlanStep[] {
  return steps.map((s) => (s.id === id ? { ...s, ...p } : s))
}

/** PICK ↔ DROP 토글 (그 외 종류는 PICK 으로). */
/**
 * 종류 바꾸기 — MOVE 로 바꿀 때 방식이 없으면 화면 기본(`top`)을 붙인다. 다른 종류로 가도 `move` 는
 * 남겨 둔다(요청에는 MOVE 일 때만 실린다) — 다시 MOVE 로 오면 고른 방식이 그대로다.
 */
export function retype(s: PlanStep, type: TaskType): Partial<Omit<PlanStep, 'id'>> {
  return type === 'MOVE' ? { type, move: moveOf(s) } : { type }
}

/** 편집 팝업 검증 — 작성 카드(`validateDraft`)와 같은 규칙 + MOVE·팔렛. 문제가 없으면 빈 배열. */
export function stepErrors(s: PlanStep): string[] {
  const out = validateDraft({
    type: s.type,
    target: s.target,
    item_code: s.item_code,
    count: s.count,
    params: {},
    note: s.note,
    move: s.move,
  })
  const p = s.pallet
  if (p && !p.auto) {
    if (!Number.isInteger(p.seq) || (p.seq ?? 0) < 1) out.push('Pallet Seq: 1 이상의 정수')
    if (!Number.isInteger(p.level) || (p.level ?? 0) < 1) out.push('Pallet Level: 1 이상의 정수')
  }
  if (p && s.target.kind !== 'station') out.push('Pallet 은 스테이션 대상에만')
  return out
}

export function toggleType(steps: readonly PlanStep[], id: string): PlanStep[] {
  return steps.map((s) => (s.id === id ? { ...s, type: s.type === 'PICK' ? 'DROP' : 'PICK' } : s))
}

/** 되돌리기 스택 — 편집마다 이전 배열을 쌓는다. */
export interface History {
  past: PlanStep[][]
  present: PlanStep[]
  future: PlanStep[][]
}
export const EMPTY_HISTORY: History = { past: [], present: [], future: [] }
const HISTORY_MAX = 100

export function commit(h: History, next: PlanStep[]): History {
  if (next === h.present) return h
  const past = [...h.past, h.present]
  if (past.length > HISTORY_MAX) past.shift()
  return { past, present: next, future: [] }
}
export function undo(h: History): History {
  if (!h.past.length) return h
  const past = h.past.slice(0, -1)
  return { past, present: h.past[h.past.length - 1], future: [h.present, ...h.future] }
}
export function redo(h: History): History {
  if (!h.future.length) return h
  return { past: [...h.past, h.present], present: h.future[0], future: h.future.slice(1) }
}

// ── 변환 ────────────────────────────────────────────────────────────────────

/**
 * 계획 스텝 → 작업 요청. **로봇을 반드시 싣는다** — 스텝에 고정한 로봇이 있으면 그것, 없으면 `fallback`(카드의 선택 로봇).
 * 전에는 로봇을 빼먹어 서버가 첫 로봇(GR1)으로 보냈다(2026-09-21 GR2 선택 중 GR1 로 제출된 사고).
 */
export function toRequest(s: PlanStep, fallback: number | null = null): TaskRequest {
  return {
    type: s.type,
    target: s.target,
    item_code: sentItem(s),
    count: Math.max(1, s.count),
    params: s.type === 'MOVE' ? moveParams(moveOf(s)) : {},
    position_override: null,
    note: s.note,
    source: null,
    robot: s.robot ?? fallback ?? null,
    ...(s.pallet ? { pallet: s.pallet } : {}),
  }
}

/**
 * 계획 → 시나리오. `preQueue` 면 스텝마다 **접수(accepted)** 까지만 기다리고 다음 스텝을 보낸다 — GR 버퍼(4 칸)에
 * 자리가 있으면 앞 Task 가 끝나기 전에 다음 Task 가 들어간다. 백엔드는 진행 중 Task 를 반영한 예상 재고로 Z 를
 * 작성하므로(같은 셀 DROP → PICK) 이 표의 재고 시뮬레이션과 같은 값이 나간다. 기본은 완료(completed) 대기.
 */
export function toScenario(
  steps: readonly PlanStep[],
  name: string,
  description = '',
  opts: { preQueue?: boolean } = {},
): ScenarioUpsert {
  const st: ScenarioStep[] = steps.map((s, i) => ({
    id: '',
    label: `${i + 1}. ${s.type} ${s.target.kind === 'cell' ? 'Cell' : 'Station'} #${s.target.id}`,
    type: s.type,
    target: s.target,
    item_code: sentItem(s),
    count: Math.max(1, s.count),
    params: s.type === 'MOVE' ? moveParams(moveOf(s)) : {},
    wait_for: opts.preQueue ? 'accepted' : 'completed',
    wait_after_ms: 0,
    on_failure: 'stop',
    note: s.note,
    robot: s.robot ?? null,
    ...(s.pallet ? { pallet: s.pallet } : {}),
  }))
  return { name, description, steps: st, repeat: 1 }
}

/** 레이아웃 오버레이용: 대상별 순번·종류 배지와 연속 스텝 화살표. */
export function overlay(steps: readonly PlanStep[]): {
  badges: Map<string, { no: number; type: TaskType }[]>
  path: Target[]
} {
  const badges = new Map<string, { no: number; type: TaskType }[]>()
  steps.forEach((s, i) => {
    const k = `${s.target.kind}-${s.target.id}`
    const list = badges.get(k) ?? []
    list.push({ no: i + 1, type: s.type })
    badges.set(k, list)
  })
  return { badges, path: steps.map((s) => s.target) }
}
