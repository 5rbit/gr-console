// 순차 작업 계획(생성 예정 작업) — 레이아웃 클릭이 PICK → DROP → PICK … 으로 번갈아 쌓이는 순수 모델.
//
// 재고를 시뮬레이션하며 각 스텝의 Z(백엔드 `stack_z` 와 같은 식)와 경고를 만든다:
//   PICK/MEASURE: floor + H·(n − c) + H/2   (집을 c개 중 맨 위 타이어 중심; n = 지금 셀에 있는 개수)
//   DROP:         floor + H·n + H/2         (n개 위에 첫 타이어가 놓일 중심)
// 되돌리기/다시실행은 계획 배열의 스냅샷 스택이다.
import type {
  Cell,
  Item,
  ScenarioStep,
  ScenarioUpsert,
  Station,
  StockEntry,
  TaskType,
  Target,
  TaskRequest,
} from '../types'

export interface PlanStep {
  id: string
  type: TaskType
  target: Target
  item_code: number | null
  count: number
  note: string
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

export function stackZ(type: TaskType, floor: number, h: number, n: number, c: number): number {
  const mid = h / 2
  if (type === 'PICK' || type === 'MEASURE')
    return floor + h * Math.max(n - Math.max(c, 1), 0) + mid
  if (type === 'DROP') return floor + h * n + mid
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
  return { id: stepId(), type: t, target, item_code, count, note: '' }
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
    if (s.type === 'PICK') {
      const left = Math.max(cur.count - s.count, 0)
      m.set(s.target.id, { item_code: left === 0 ? 0 : cur.item_code, count: left })
    } else if (s.type === 'DROP') {
      m.set(s.target.id, { item_code: s.item_code ?? cur.item_code, count: cur.count + s.count })
    }
  }
  return m
}

export interface PlanContext {
  cells: readonly Cell[]
  stations: readonly Station[]
  items: readonly Item[]
  stockNow: ReadonlyMap<number, StockEntry>
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
    if (floor !== null && h !== null) z = stackZ(s.type, floor, h, n, s.count)
    else if (floor !== null && s.type === 'MOVE') z = floor
    if (cell) {
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
    if (cell && st) {
      if (s.type === 'PICK') {
        const left = Math.max(st.count - s.count, 0)
        sim.set(cell.id, { item_code: left === 0 ? 0 : st.item_code, count: left })
      } else if (s.type === 'DROP') {
        sim.set(cell.id, { item_code: s.item_code ?? st.item_code, count: st.count + s.count })
      }
    }
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

export function toRequest(s: PlanStep): TaskRequest {
  return {
    type: s.type,
    target: s.target,
    item_code: s.item_code,
    count: Math.max(1, s.count),
    params: {},
    position_override: null,
    note: s.note,
    source: null,
  }
}

export function toScenario(
  steps: readonly PlanStep[],
  name: string,
  description = '',
): ScenarioUpsert {
  const st: ScenarioStep[] = steps.map((s, i) => ({
    id: '',
    label: `${i + 1}. ${s.type} ${s.target.kind === 'cell' ? '셀' : '스테이션'} #${s.target.id}`,
    type: s.type,
    target: s.target,
    item_code: s.item_code,
    count: Math.max(1, s.count),
    params: {},
    wait_for: 'completed',
    wait_after_ms: 0,
    on_failure: 'stop',
    note: s.note,
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
