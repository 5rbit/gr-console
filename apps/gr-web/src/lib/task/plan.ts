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
  MeasureMode,
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
  /** MEASURE 측정 종류. 없으면 자동 — 셀 재고 1개 이하 = Item, 2개 이상 = SKU(`autoMeasureMode`). */
  measure?: MeasureMode | null
  /** 바닥 측정(Cell Teaching) 명령 Z = 베이스 라인 + 이만큼(mm). 없으면 `TEACH_CLEARANCE`. */
  measureClearance?: number | null
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
  /** Multi-Picking — 다음 스텝이 같은 로봇·같은 스테이션 그룹의 스테이션 PICK/DROP(`sameStationGroup`). */
  multiPick: boolean
  /** MEASURE 측정 종류 — 고정값, auto 면 그 시점 계획 재고로 본 **예상**(실제 판정은 서버). MEASURE 가 아니면 없음. */
  measureMode?: MeasureMode
}

/**
 * 자동 측정 종류 — 셀에 1개(이하) 있으면 Item, 2개 이상이면 SKU(스택 전체의 단별 비드).
 * **판정은 서버가 한다**(`issue::compose_from` 의 MEASURE auto, 작성 시점 재고). 이 함수는 표에 "auto(SKU)" 처럼
 * 예상 값을 보이려고 같은 규칙을 계획 시뮬레이션 재고에 적용할 뿐이다.
 */
export function autoMeasureMode(stock: number | null | undefined): MeasureMode {
  return (stock ?? 0) >= 2 ? 'sku' : 'item'
}

/** 측정 종류 → 요청 플래그. 둘 다 명시해 기본값 층의 플래그가 섞이지 않게 한다. */
export function measureParams(mode: MeasureMode): {
  measure_item: boolean
  measure_sku: boolean
  measure_floor: boolean
} {
  return {
    measure_item: mode === 'item',
    measure_sku: mode === 'sku',
    measure_floor: mode === 'floor',
  }
}

/**
 * 맵 클릭이 만드는 스텝 — `pickdrop` = PICK → DROP 교대(기본), `teach` = Cell Teaching(셀마다 MEASURE Floor).
 * Teaching 은 GRM 이 Teach 모드일 때 PLC 가 잰 바닥(Z − FLD 거리)을 그 셀 Z 로 CELL 표에 넣는다(`PL_Task_V2` 500).
 */
export type PlanKind = 'pickdrop' | 'teach'

export const PLAN_KINDS: readonly { id: PlanKind; label: string; title: string }[] = [
  { id: 'pickdrop', label: 'PICK/DROP', title: '좌클릭마다 PICK → DROP 순으로 계획에 쌓인다' },
  {
    id: 'teach',
    label: 'Cell Teaching',
    title:
      '좌클릭한 셀·스테이션마다 MEASURE Floor(바닥 측정)를 쌓는다 — Z = 등록된 베이스 + N(기본 500). GRM 이 Teach 모드여야 잰 바닥이 그 대상 Z 로 저장된다',
  },
]

/**
 * 바닥 측정(Cell Teaching) 명령 Z 의 기본 높이 — 등록된 베이스 라인 위 이만큼. 백엔드 `TEACH_CLEARANCE`.
 * 재는 값은 **절대 바닥 좌표**(Z 축 위치 − 바닥 레이저 거리)라 어느 높이에서 재든 같다 — 그래서 재고와도 무관하다.
 */
export const TEACH_CLEARANCE_DEFAULT = 500
export const TEACH_CLEARANCE_MAX = 5000

/** Cell Teaching 스텝 — 품목·재고 없이 바닥 측정(MEASURE Floor) 하나. */
export function teachStep(
  target: Target,
  robot: number | null = null,
  clearance: number | null = null,
): PlanStep {
  return {
    id: stepId(),
    type: 'MEASURE',
    target,
    item_code: null,
    count: 1,
    note: '',
    robot,
    measure: 'floor',
    ...(clearance === null ? {} : { measureClearance: clearance }),
  }
}

/** 이 스텝의 Teaching 높이 — 정한 값이 없으면 기본값. */
export function teachClearance(s: Pick<PlanStep, 'measureClearance'>): number {
  const c = s.measureClearance
  return c === null || c === undefined || !Number.isFinite(c) || c < 0 || c > TEACH_CLEARANCE_MAX
    ? TEACH_CLEARANCE_DEFAULT
    : c
}

/** 스텝이 바닥 측정(Cell Teaching)인가. */
export function isTeach(s: Pick<PlanStep, 'type' | 'measure'>): boolean {
  return s.type === 'MEASURE' && s.measure === 'floor'
}

/**
 * 모든 셀을 도는 Teaching 경로의 차례.
 *   `id`         — 셀 번호(Id) 오름차순. 번호를 매긴 순서가 곧 순서다.
 *   `row`        — 구간 → 행(X) → 열(Y), 행마다 같은 방향.
 *   `serpentine` — 행 순서에 지그재그(앞 행과 반대 방향) — 행 끝에서 되돌아가지 않아 이동이 짧다.
 */
export type TeachOrder = 'id' | 'row' | 'serpentine'

export const TEACH_ORDERS: readonly { id: TeachOrder; label: string; title: string }[] = [
  {
    id: 'serpentine',
    label: '행 지그재그',
    title: '행(X) 순서, 행마다 방향을 번갈아 — 이동이 가장 짧다',
  },
  { id: 'row', label: '행 순서', title: '구간 → 행(X) → 열(Y), 행마다 같은 방향' },
  { id: 'id', label: '셀 번호 순', title: '셀 Id 오름차순 — 번호를 매긴 순서 그대로' },
]

/**
 * 모든 셀을 한 번씩 도는 Teaching 경로. 축 규약은 `layoutGen` 과 같다(행 = X, 열 = Y).
 * 행·열이 없는(0) 셀은 좌표로 줄을 세운다. `skip` 에 든 셀 id 는 건너뛴다(이미 계획에 있는 셀).
 */
export function teachRoute(
  cells: readonly Cell[],
  robot: number | null = null,
  skip: ReadonlySet<number> = new Set(),
  order: TeachOrder = 'serpentine',
  clearance: number | null = null,
): PlanStep[] {
  const use = cells.filter((c) => c.use !== false && !skip.has(c.id))
  const step = (c: Cell) => teachStep({ kind: 'cell', id: c.id }, robot, clearance)
  if (order === 'id') return [...use].sort((a, b) => a.id - b.id).map(step)
  // 행 = 같은 구간의 같은 행 번호(없으면 X 좌표) — 이 묶음 단위로 방향이 번갈아 바뀐다.
  const rowKey = (c: Cell) => `${c.section}/${c.row || Math.round(c.position[0])}`
  const rows = new Map<string, Cell[]>()
  for (const c of use) {
    const k = rowKey(c)
    const list = rows.get(k) ?? []
    list.push(c)
    rows.set(k, list)
  }
  const keys = [...rows.keys()].sort((a, b) => {
    const [ca, cb] = [rows.get(a)![0], rows.get(b)![0]]
    return ca.section - cb.section || ca.row - cb.row || ca.position[0] - cb.position[0]
  })
  const out: PlanStep[] = []
  keys.forEach((k, i) => {
    const line = rows
      .get(k)!
      .sort((a, b) => a.col - b.col || a.position[1] - b.position[1] || a.id - b.id)
    for (const c of order === 'serpentine' && i % 2 === 1 ? [...line].reverse() : line)
      out.push(step(c))
  })
  return out
}

/** 스텝 요청 `params` — MOVE 방식 · MEASURE 고정 종류. auto(비움)는 플래그를 싣지 않는다 → 서버가 그때 재고로 고른다. */
function stepParams(s: PlanStep): TaskRequest['params'] {
  if (s.type === 'MOVE') return moveParams(moveOf(s))
  if (s.type === 'MEASURE' && s.measure)
    return {
      ...measureParams(s.measure),
      ...(isTeach(s) && s.measureClearance !== null && s.measureClearance !== undefined
        ? { measure_clearance: teachClearance(s) }
        : {}),
    }
  return {}
}

/** PLC `isAllowDoublePicking` 의 스테이션 그룹 — `(id / 100) MOD 10`, 0 = 그룹 없음. */
export function stationGroup(id: number): number {
  return Math.floor(id / 100) % 10
}

/**
 * 두 스텝이 Multi-Picking 으로 이어지는가 — 둘 다 스테이션 PICK/DROP, 같은 그룹(≠0), 다음 스텝이 다른 로봇을
 * 지정하지 않음. 백엔드 `issue::same_station_group`(시나리오 실행기)과 같은 규칙이다.
 */
export function sameStationGroup(a: PlanStep, b: PlanStep | undefined): boolean {
  if (!b) return false
  const pd = (t: TaskType) => t === 'PICK' || t === 'DROP'
  if (a.target.kind !== 'station' || b.target.kind !== 'station' || !pd(a.type) || !pd(b.type))
    return false
  if (b.robot != null && b.robot !== a.robot) return false
  const g = stationGroup(a.target.id)
  return g !== 0 && g === stationGroup(b.target.id)
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
 * 클릭 한 번을 스텝으로 만든다. `type` 을 주면 자동 교대 대신 그 종류로.
 *
 * 품목(2026-09-21 수정 — 셀과 맞지 않는 품목이 붙던 문제):
 *  - PICK·MEASURE = 누른 대상(셀·스테이션)의 **계획 반영 재고** 품목. 비었으면 `fallbackItem`(레일에서 고른 품목),
 *    그것도 없으면 `null`(서버가 재고로 채우거나 "품목 없음"을 경고한다). 들고 있는 화물 품목은 쓰지 않는다 —
 *    PICK 은 그 대상에 있는 것을 집는다.
 *  - DROP = 들고 있는 품목 → 대상 재고 품목 → `fallbackItem`.
 * 예전에는 재고가 비면 품목 목록의 **첫 품목**을, 스테이션은 재고를 아예 보지 않고 들고 있던 품목을 붙였다.
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
  // 재고는 셀·스테이션이 같은 id 공간을 쓴다(백엔드 `stock`) — 스테이션도 본다.
  const st = sim.get(target.id)
  const stockItem = st && st.count > 0 && st.item_code ? st.item_code : null
  const carry = carried(steps)
  let item_code: number | null = null
  let count = 1
  if (t === 'DROP') {
    item_code = carry?.item_code || stockItem || fallbackItem
    count = carry?.count ?? 1
  } else if (t === 'PICK' || t === 'MEASURE') {
    item_code = stockItem || fallbackItem
  }
  if (item_code === 0) item_code = null
  return { id: stepId(), type: t, target, item_code, count, note: '', robot }
}

/** DROP 을 놓을 자리에 **다른 품목**이 이미 있을 때의 양쪽 — 확인 창이 보인다(`dropMismatch`). */
export interface DropMismatch {
  /** 들고 있는 화물 — 가져온 자리(마지막 PICK 대상)와 품목·개수. */
  from: { target: Target | null; item_code: number; count: number }
  /** 놓을 자리의 계획 반영 재고. */
  to: { target: Target; item_code: number; count: number }
}

/**
 * PICK/DROP 짝에서 놓을 자리(셀·스테이션)의 **계획 반영 재고** 품목이 들고 있는 품목과 다르면 그 내용.
 * 비었거나 같은 품목이면(또는 어느 한쪽 품목을 모르면) `null` — 묻지 않고 넣는다.
 */
export function dropMismatch(
  steps: readonly PlanStep[],
  step: PlanStep,
  stockNow: ReadonlyMap<number, StockEntry>,
): DropMismatch | null {
  if (step.type !== 'DROP' || !step.item_code) return null
  const st = simulateStock(steps, stockNow).get(step.target.id)
  if (!st || st.count <= 0 || !st.item_code || st.item_code === step.item_code) return null
  let pick: PlanStep | null = null
  for (let i = steps.length - 1; i >= 0 && !pick; i--) {
    if (steps[i].type === 'DROP') break
    if (steps[i].type === 'PICK') pick = steps[i]
  }
  return {
    from: { target: pick?.target ?? null, item_code: step.item_code, count: step.count },
    to: { target: step.target, item_code: st.item_code, count: st.count },
  }
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
    // 셀·스테이션 모두 — 재고는 대상 id 하나로 쌓인다.
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
  /** 로봇 없는 스텝이 갈 로봇(짝 검사의 "같은 로봇"). */
  robot?: number | null
  /** 계획 시작 때 로봇이 들고 있는 화물(예상 Hand) — 첫 스텝의 "들고 있음" 판단에 쓴다. */
  hand?: { item_code: number; count: number } | null
  /** 두 로봇 영역 간격(mm, 백엔드 `/api/anticol`) — 있으면 이웃한 다른 로봇 스텝의 X 거리를 본다. */
  anticolSep?: number | null
  /** 로봇 이름(경고 문구). */
  robotName?: (id: number | null) => string
}

/** 계획 스텝의 대상 X(레지스트리). */
function targetX(s: PlanStep, ctx: PlanContext): number | null {
  if (s.target.kind === 'cell')
    return ctx.cells.find((c) => c.id === s.target.id)?.position[0] ?? null
  return ctx.stations.find((c) => c.id === s.target.id)?.info.position[0] ?? null
}

/**
 * 짝과 함께 지우기 — PICK 을 지우면 같은 로봇의 짝 DROP 도, DROP 을 지우면 그 짝 PICK 도(계획은 아직 콘솔에만 있다).
 */
export function removeWithPair(
  steps: readonly PlanStep[],
  id: string,
  runRobot: number | null = null,
): { next: PlanStep[]; removed: PlanStep[] } {
  const i = steps.findIndex((s) => s.id === id)
  if (i < 0) return { next: [...steps], removed: [] }
  const robotOf = (k: number) => steps[k].robot ?? runRobot
  const ids = new Set([id])
  if (steps[i].type === 'PICK') {
    const j = steps.findIndex((_, k) => k > i && robotOf(k) === robotOf(i))
    if (j >= 0 && steps[j].type === 'DROP') ids.add(steps[j].id)
  } else if (steps[i].type === 'DROP') {
    let p = i - 1
    while (p >= 0 && robotOf(p) !== robotOf(i)) p--
    if (p >= 0 && steps[p].type === 'PICK') ids.add(steps[p].id)
  }
  return { next: steps.filter((s) => !ids.has(s.id)), removed: steps.filter((s) => ids.has(s.id)) }
}

/** PICK/DROP 짝 위반 한 줄(백엔드 `scenario::io::validate_pairs` 와 같은 규칙). */
export interface PairIssue {
  id: string
  no: number
  message: string
}

/**
 * PICK/DROP 은 늘 한 짝 — 한 로봇의 PICK 다음 **그 로봇의 다음 스텝**은 같은 품목·수량의 DROP 이고, DROP 앞의 그 로봇
 * 스텝은 짝 PICK 이다. 다른 로봇의 스텝(충돌 회피 MOVE · 대기)은 짝 사이에 와도 된다. 위반이 있으면 실행하지 않는다
 * (백엔드 `scenario::io::validate_pairs` 도 시작 때 거부).
 */
export function pairIssues(
  steps: readonly PlanStep[],
  runRobot: number | null = null,
): PairIssue[] {
  return pairIssuesOf(
    steps.map((s) => ({
      type: s.type,
      robot: s.robot ?? runRobot,
      item_code: s.item_code,
      count: s.count,
    })),
  ).map((p) => ({ id: steps[p.index].id, no: p.index + 1, message: p.message }))
}

/** 짝 검사 본체 — 계획 표와 시나리오 편집이 같이 쓴다. `robot` 은 이미 실행 로봇으로 채운 값. */
export function pairIssuesOf(
  steps: readonly {
    type: TaskType
    robot?: number | null
    item_code: number | null
    count: number
  }[],
): { index: number; message: string }[] {
  const out: { index: number; message: string }[] = []
  const same = (a: number, b: number) => (steps[a].robot ?? null) === (steps[b].robot ?? null)
  steps.forEach((s, i) => {
    const push = (message: string) => out.push({ index: i, message })
    if (s.type === 'PICK') {
      let j = i + 1
      while (j < steps.length && !same(i, j)) j++
      const n = steps[j]
      if (!n) push('PICK 뒤에 같은 로봇의 짝 DROP 이 없음')
      else if (n.type !== 'DROP')
        push(`PICK 다음 같은 로봇 스텝은 짝 DROP — 스텝 ${j + 1} 이 ${n.type}`)
      else if (s.item_code !== null && n.item_code !== null && s.item_code !== n.item_code)
        push(`짝 DROP(스텝 ${j + 1}) 품목 ${n.item_code} ≠ ${s.item_code}`)
      else if (n.count !== s.count) push(`짝 DROP(스텝 ${j + 1}) 수량 ${n.count} ≠ ${s.count}`)
    } else if (s.type === 'DROP') {
      let p = i - 1
      while (p >= 0 && !same(i, p)) p--
      if (p < 0 || steps[p].type !== 'PICK') push('DROP 앞의 같은 로봇 스텝이 짝 PICK 이 아님')
    }
  })
  return out
}

/** 표에 보일 행 — 재고 전/후, Z, 경고. */
export function planRows(steps: readonly PlanStep[], ctx: PlanContext): PlanRow[] {
  const sim = new Map<number, { item_code: number; count: number }>()
  for (const [k, v] of ctx.stockNow) sim.set(k, { item_code: v.item_code, count: v.count })
  const rows: PlanRow[] = []
  // 로봇이 이미 들고 있으면(앞 짝의 DROP 이 취소됨 등) 그 화물을 들고 시작한다.
  let carry: Carry | null =
    ctx.hand && ctx.hand.count > 0
      ? { item_code: ctx.hand.item_code || null, count: ctx.hand.count }
      : null
  const pairs = new Map<string, string[]>()
  for (const p of pairIssues(steps, ctx.robot ?? null))
    pairs.set(p.id, [...(pairs.get(p.id) ?? []), p.message])
  steps.forEach((s, i) => {
    const warnings: string[] = [...(pairs.get(s.id) ?? [])]
    // 두 로봇 영역(정적) — 바로 앞 스텝이 다른 로봇이고 목표 X 가 간격 안이면 실행 때 영역 대기가 된다.
    const prev = i > 0 ? steps[i - 1] : null
    const sep = ctx.anticolSep ?? null
    if (
      prev &&
      sep !== null &&
      (prev.robot ?? ctx.robot ?? null) !== (s.robot ?? ctx.robot ?? null)
    ) {
      const xa = targetX(prev, ctx)
      const xb = targetX(s, ctx)
      if (xa !== null && xb !== null && Math.abs(xa - xb) < sep) {
        const name =
          ctx.robotName ?? ((id: number | null) => (id === null ? '기본 로봇' : `로봇 ${id}`))
        warnings.push(
          `${name(prev.robot ?? ctx.robot ?? null)} X ${xa.toFixed(0)} 와 ${name(s.robot ?? ctx.robot ?? null)} X ${xb.toFixed(0)} 거리 ${Math.abs(xa - xb).toFixed(0)} < ${sep.toFixed(0)} mm — 영역 대기 예정`,
        )
      }
    }
    const cell = s.target.kind === 'cell' ? ctx.cells.find((c) => c.id === s.target.id) : null
    const station =
      s.target.kind === 'station' ? ctx.stations.find((c) => c.id === s.target.id) : null
    const floor = cell ? cell.position[2] : station ? station.info.position[2] : null
    if (!cell && !station) warnings.push('대상이 레지스트리에 없음')
    const item = s.item_code !== null ? ctx.items.find((it) => it.code === s.item_code) : undefined
    if (s.item_code !== null && !item) warnings.push(`품목 ${s.item_code} 미등록`)
    const teach = isTeach(s)
    if (
      s.item_code === null &&
      !teach &&
      (s.type === 'PICK' || s.type === 'DROP' || s.type === 'MEASURE')
    )
      warnings.push('품목 없음')
    const h = item?.height ?? null
    // 셀·스테이션 모두 재고를 본다(같은 id 공간) — 스테이션 위 멀티 PICK/DROP 도 기존 타이어 위로 쌓는다.
    const tgtId = cell ? cell.id : station ? station.id : null
    const st = tgtId !== null ? (sim.get(tgtId) ?? { item_code: 0, count: 0 }) : null
    const n = st ? st.count : 0
    const measureMode =
      s.type === 'MEASURE' ? (s.measure ?? autoMeasureMode(st ? st.count : null)) : undefined
    let z: number | null = null
    if (teach) {
      // 베이스 라인(셀·스테이션 모두 `LGR_Cell_Info.Position[Z]`) + N — 재고를 보지 않는다.
      if (floor !== null) z = floor + teachClearance(s)
    } else if (s.type === 'MOVE') {
      const mv: MoveOpts = moveOf(s)
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
    } else if (floor !== null && h !== null) {
      // MEASURE SKU = 스택 전체 — 1단 중간 높이에서 위로 잰다(개수 = 재고 전체, 그립 mid). 서버 `compose_from` 과 같다.
      const sku = measureMode === 'sku' && n > 0
      z = planZ(s.type, floor, item, sku ? 'mid' : (ctx.gripRef ?? 'mid'), n, sku ? n : s.count).z
    }
    if (st && s.type !== 'MOVE' && !teach) {
      if ((s.type === 'PICK' || s.type === 'MEASURE') && n === 0)
        warnings.push(`${cell ? '셀' : '스테이션'} 재고 없음`)
      else if (s.type === 'PICK' && n < s.count) warnings.push(`재고 ${n} < 수량 ${s.count}`)
      if (n > 0 && st!.item_code && s.item_code !== null && st!.item_code !== s.item_code)
        warnings.push(`${cell ? '셀' : '스테이션'} 품목 ${st!.item_code} ≠ ${s.item_code}`)
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
    if (tgtId !== null && st) sim.set(tgtId, foldStock(st, s.type, s.item_code, s.count))
    if (s.type === 'PICK') carry = { item_code: s.item_code, count: s.count }
    else if (s.type === 'DROP') carry = null
    rows.push({
      ...s,
      multiPick: sameStationGroup(s, steps[i + 1]),
      ...(measureMode ? { measureMode } : {}),
      no: i + 1,
      stockBefore: before,
      stockAfter: tgtId !== null ? (sim.get(tgtId)?.count ?? null) : null,
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
    // Cell Teaching(MEASURE Floor)은 품목 없이 간다 — 품목 필수 검사를 건너뛴다.
    item_code: isTeach(s) ? (s.item_code ?? 0) : s.item_code,
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
export function toRequest(
  s: PlanStep,
  fallback: number | null = null,
  multiPick = false,
): TaskRequest {
  return {
    type: s.type,
    target: s.target,
    item_code: sentItem(s),
    count: Math.max(1, s.count),
    params: stepParams(s),
    position_override: null,
    note: s.note,
    source: null,
    robot: s.robot ?? fallback ?? null,
    ...(s.pallet ? { pallet: s.pallet } : {}),
    ...(multiPick ? { multi_pick: true } : {}),
  }
}

/**
 * 계획 → 시나리오. MEASURE auto 스텝은 플래그 없이 실린다 — 실행 때 서버가 그 시점 재고로 Item/SKU 를 고른다.
 *
 * `preQueue` 면 스텝마다 **접수(accepted)** 까지만 기다리고 다음 스텝을 보낸다 — 백엔드 실행기가
 * PLC 에 실행 중 + 다음 1 건까지만 두므로 앞 Task 가 끝나기 전에 다음 1 건이 들어간다. 백엔드는 진행 중 Task 를 반영한 예상 재고로 Z 를
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
    params: stepParams(s),
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
