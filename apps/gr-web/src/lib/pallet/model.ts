// 팔렛 패턴 순수 모델 — 백엔드 `apps/gr-console/src/pallet` 와 같은 방향 코드·변환, 그리고 화면용 좌표·드래그
// 화살표·적합 표시·"계획에 추가" 스텝·CSV.
//
// 좌표는 전부 기계 축(X, Y, mm)이다. 그릴 때만 보기 방향으로 바꾼다: `spec` = 사양서 슬라이드의 화면 방향
// (흐름마다 `screen_axes`), `machine` = X+ 오른쪽 · Y+ 위. 드래그 방향 코드 1..8 은 PLC FC `DragDelta` 의
// 기계 축 코드이고 PLC 바이트는 `1 << (n-1)` 이다.
//
// 계산(패턴 선택·오프셋·Z)은 백엔드 `/api/pallet/plan` 이 한다 — 작업 명령 compose 와 같은 생성기라 화면과
// 제출 값이 갈리지 않는다. 여기의 변환 함수는 그 결과를 검증·표시하는 데 쓰고, Rust 테스트와 같은 표로 검사한다.
import type { PalletRef, TaskType } from '../types'
import { stepId, type PlanStep } from '../task/plan'

export type DragKind = 'in' | 'out'
export type AxisName = 'X+' | 'X-' | 'Y+' | 'Y-'

export interface Transform {
  rotation: number
  mirror_x: boolean
  mirror_y: boolean
}

export interface Drawn {
  d_mm: number
  min_dist_mm: number
  gap_mm: number
  centroid_mm: [number, number]
  centered: boolean
}
export interface SpecSlot {
  slot: string
  seq: number
  u: [number, number]
  drag_dir: number
}
export interface SpecPattern {
  pattern: number
  od_min: number
  od_max: number
  order_text: string
  drawn: Drawn
  slots: SpecSlot[]
  /** 사양서 전사 메모(읽기 전용). */
  notes: string[]
  /** 운전자 메모(편집 저장소). */
  note?: string
  updated_at?: string
}
export interface ScreenAxes {
  right: AxisName
  down: AxisName
}
export interface Flow {
  id: string
  name: string
  source_slide: number
  also_slides: number[]
  drag_kind: DragKind
  reference: boolean
  robot: string
  zone: string
  screen_axes: ScreenAxes
  notes: string[]
  patterns: SpecPattern[]
  /** 편집 저장소 — `spec_r4` · `custom` · `import`. */
  source?: string
  based_on?: string | null
  note?: string
  updated_at?: string
}

/** 편집 저장소 흐름 상태 — 사양서와 같음 · 고침 · 사양서 기준 없음. */
export type FlowStatus = 'spec' | 'modified' | 'custom'

/** `GET /api/pallet/flows` 의 패턴 한 줄. */
export interface EditPatternView extends SpecPattern {
  note: string
  updated_at: string
  /** 정규화 최소 중심거리(슬롯 1개면 null). */
  min_distance: number | null
  status: 'same' | 'changed' | 'added' | 'custom'
}

/** `GET /api/pallet/flows` 한 줄. */
export interface EditFlowView extends Flow {
  source: string
  based_on: string | null
  note: string
  updated_at: string
  status: FlowStatus
  modified: boolean
  removed_spec_patterns: number[]
  used_by: number[]
  warnings: string[]
  max_od: number
  patterns: EditPatternView[]
}

export interface FieldChange {
  field: string
  spec: unknown
  current: unknown
}
export interface SlotDiffRow {
  slot: string
  status: 'same' | 'changed' | 'added' | 'removed'
  fields: FieldChange[]
}
export interface PatternDiffRow {
  pattern: number
  status: 'same' | 'changed' | 'added' | 'removed'
  fields: FieldChange[]
  slots: SlotDiffRow[]
  spec: SpecPattern | null
  current: SpecPattern | null
}
/** `GET /api/pallet/flows/{id}/diff` */
export interface FlowDiff {
  id: string
  based_on: string | null
  status: FlowStatus
  modified: boolean
  fields: FieldChange[]
  patterns: PatternDiffRow[]
}

/** `POST /api/pallet/import?dry_run=` */
export interface ImportReport {
  dry_run: boolean
  ok: boolean
  errors: string[]
  created: number
  updated: number
  unchanged: number
  flows: {
    id: string
    action: 'created' | 'updated' | 'unchanged' | 'invalid'
    patterns: { pattern: number; action: 'added' | 'changed' | 'unchanged' }[]
    warnings: string[]
    errors: string[]
  }[]
}

export interface SizeClass {
  pattern: number
  od_min: number
  od_max: number
}
export interface PalletSpec {
  version: string
  source: string
  pallet_size_mm: number
  normalization: string
  size_classes: SizeClass[]
  size_class_notes: string[]
  flows: Flow[]
  /** `true` = 내장 사양서 R4, `false` = 편집 저장소. */
  seed?: boolean
}

/** 품목 팔렛 설정 — 패턴의 주인(`/api/pallet/items`). 좌표는 GR1·GR2 공통. */
export interface ItemPallet {
  code: number
  /** PICK/MEASURE 에 쓰는 흐름. 없으면 flow_out. */
  flow_in: string | null
  /** DROP 에 쓰는 흐름. 없으면 flow_in. */
  flow_out: string | null
  /** null = OuterDiameter 로 자동. */
  pattern: number | null
  gap: number
  rotation: number
  mirror_x: boolean
  mirror_y: boolean
  pallet_size: number
  note: string
  updated_at: string
}
export type ItemPalletUpsert = Omit<ItemPallet, 'updated_at'>

/** 로봇별 드래그 방향 보정(`/api/pallet/robots`) — 헤드 방향이 달라 방향 코드에만 건다. */
export interface RobotDir {
  robot: number
  rotation: number
  mirror_x: boolean
  mirror_y: boolean
  note: string
  updated_at: string
}
export type RobotDirView = RobotDir & { name: string; plc: string }
export type RobotDirUpsert = Omit<RobotDir, 'updated_at'>

/** 팔렛 스테이션 여부(`/api/pallet/stations`). */
export interface PalletStation {
  station_id: number
  enabled: boolean
  note: string
  updated_at: string
}
export type PalletStationUpsert = Omit<PalletStation, 'updated_at'>

export interface PlanSlot {
  seq: number
  slot: string
  u: [number, number]
  offset_x: number
  offset_y: number
  x: number
  y: number
  spec_drag_dir: number
  /** 품목 배치 변환만 건 방향(로봇 보정 전). */
  layout_drag_dir?: number
  drag_dir: number
  drag_type: number
  /** 팔렛 밖으로 나간 길이(mm, X·Y) — 0 이하면 안. */
  overhang: [number, number]
}
export interface LevelZ {
  level: number
  n: number
  z: number | null
  z_source: string | null
}
export interface AreaReject {
  seq: number
  slot: string
  axis: string
  code: number
  delta: number
}
export interface AreaCheck {
  station_id: number
  task_type: number
  rotate_type: number
  margin: number
  expected_xy: [number, number]
  from_registry: boolean
  rejects: AreaReject[]
}

/** `GET /api/pallet/plan` */
export interface PalletPlan {
  flow: string
  flow_name: string
  drag_kind: DragKind
  reference: boolean
  pattern: number
  pattern_auto: boolean
  od_range: [number, number]
  od: number
  gap: number
  pitch: number
  transform: Transform
  /** 로봇 헤드 방향 보정(방향 코드에만). */
  dir_transform?: Transform
  center: [number, number]
  pallet_size: number
  min_distance: number
  slots: PlanSlot[]
  warnings: string[]
  errors: string[]
  drawn: Drawn
  notes: string[]
  /** 편집 좌표 최소 중심거리(정규화) · 오프셋 배율 = 1/그것. */
  min_distance_norm?: number
  scale?: number
  flow_source?: string
  based_on?: string | null
  flow_updated_at?: string
  pattern_updated_at?: string
  pattern_note?: string
  type: TaskType
  levels: number
  z_levels: LevelZ[]
  floor_z: number | null
  grip_ref: string
  center_source: 'station' | 'manual'
  station: {
    id: number
    task_type: number
    rotate_type: number
    position: [number, number, number]
    width: number
    length: number
  } | null
  pallet_station: PalletStation | null
  item_pallet: ItemPallet | null
  robot_dir: RobotDir
  item: {
    code: number
    name: string
    outer_diameter: number
    inner_diameter: number
    height: number
  } | null
  area: AreaCheck | null
  sources: Record<string, string | boolean | null>
}

export const DEFAULT_GAP = 50
export const DEFAULT_PALLET_SIZE = 1600
export const DEFAULT_FLOW = 'HP_IN'
export const ROTATIONS = [0, 90, 180, 270] as const
export const LEVELS_MAX = 20

// ── 방향 코드 ───────────────────────────────────────────────────────────────

const DIR_VEC: Record<number, readonly [number, number]> = {
  1: [1, -1],
  2: [1, 0],
  3: [1, 1],
  4: [0, -1],
  5: [0, 1],
  6: [-1, -1],
  7: [-1, 0],
  8: [-1, 1],
}

/** 방향 코드 → 기계 축 부호(X, Y). 0·범위 밖 = null. */
export function dirVec(d: number): [number, number] | null {
  const v = DIR_VEC[d]
  return v ? [v[0], v[1]] : null
}

export function dirFromVec(x: number, y: number): number {
  for (let d = 1; d <= 8; d++) {
    const v = DIR_VEC[d]
    if (v[0] === x && v[1] === y) return d
  }
  return 0
}

/** PLC `DragInDir`/`DragOutDir` 바이트 — 0 = 드래그 없음. */
export function dragTypeByte(d: number): number {
  return Number.isInteger(d) && d >= 1 && d <= 8 ? 1 << (d - 1) : 0
}

export function hexByte(b: number): string {
  return `0x${b.toString(16).toUpperCase().padStart(2, '0')}`
}

/** PLC FC `DragDelta` 그대로 — 바이트 하나가 한 축에 주는 변위(대각은 Dist/√2). */
export function dragDelta(axis: 'X' | 'Y', byte: number, dist: number): number {
  const r2 = 1.414214
  if (axis === 'X') {
    switch (byte) {
      case 0b1:
      case 0b100:
        return dist / r2
      case 0b10:
        return dist
      case 0b100000:
      case 0b10000000:
        return -dist / r2
      case 0b1000000:
        return -dist
      default:
        return 0
    }
  }
  switch (byte) {
    case 0b1:
    case 0b100000:
      return -dist / r2
    case 0b100:
    case 0b10000000:
      return dist / r2
    case 0b1000:
      return -dist
    case 0b10000:
      return dist
    default:
      return 0
  }
}

// ── 변환 ────────────────────────────────────────────────────────────────────

/** −0 을 0 으로(표에 `-0` 이 보이지 않게). */
const z0 = (v: number): number => (v === 0 ? 0 : v)

/** MirrorX(X → −X) · MirrorY(Y → −Y) 먼저, 그다음 Rotation(X+ 에서 Y+ 쪽으로). 백엔드 `Transform::apply` 와 같다. */
export function applyTransform(t: Transform, p: readonly [number, number]): [number, number] {
  const x = t.mirror_x ? -p[0] : p[0]
  const y = t.mirror_y ? -p[1] : p[1]
  switch ((((t.rotation / 90) % 4) + 4) % 4) {
    case 1:
      return [z0(-y), z0(x)]
    case 2:
      return [z0(-x), z0(-y)]
    case 3:
      return [z0(y), z0(-x)]
    default:
      return [z0(x), z0(y)]
  }
}

export function applyDir(t: Transform, d: number): number {
  const v = dirVec(d)
  if (!v) return 0
  const [x, y] = applyTransform(t, v)
  return dirFromVec(x, y)
}

/** 외경으로 패턴 — od_min 이상인 가장 큰 클래스, 맨 위 클래스의 od_max 만 상한(백엔드와 같다). */
export function patternForOd(classes: readonly SizeClass[], od: number): number | null {
  const max = classes.reduce((m, c) => Math.max(m, c.od_max), 0)
  if (!Number.isFinite(od) || od <= 0 || od > max) return null
  let best: SizeClass | null = null
  for (const c of classes) if (od >= c.od_min && (!best || c.od_min > best.od_min)) best = c
  return best ? best.pattern : null
}

export function flowOf(spec: PalletSpec | null, id: string): Flow | null {
  return spec?.flows.find((f) => f.id.toUpperCase() === id.toUpperCase()) ?? null
}

// ── 보기 ────────────────────────────────────────────────────────────────────

export type ViewMode = 'spec' | 'machine'
export const MACHINE_AXES: ScreenAxes = { right: 'X+', down: 'Y-' }

export function viewAxes(mode: ViewMode, flow: Flow | null): ScreenAxes {
  return mode === 'spec' && flow ? flow.screen_axes : MACHINE_AXES
}

/** 기계 좌표 → 화면 좌표(오른쪽 +, 아래 +). */
export function toScreen(axes: ScreenAxes, p: readonly [number, number]): [number, number] {
  return [z0(axes.right === 'X+' ? p[0] : -p[0]), z0(axes.down === 'Y+' ? p[1] : -p[1])]
}

const opposite = (a: AxisName): AxisName => (a.endsWith('+') ? `${a[0]}-` : `${a[0]}+`) as AxisName

export function axisLabels(axes: ScreenAxes): {
  left: AxisName
  right: AxisName
  up: AxisName
  down: AxisName
} {
  return { right: axes.right, left: opposite(axes.right), down: axes.down, up: opposite(axes.down) }
}

/** 방향 코드 3×3 격자 — 코드를 그 변위가 가리키는 칸에 둔다(사양서 범례와 같은 모양). [행][열]. */
export function dirGrid(axes: ScreenAxes): (number | null)[][] {
  const g: (number | null)[][] = [
    [null, null, null],
    [null, null, null],
    [null, null, null],
  ]
  for (let d = 1; d <= 8; d++) {
    const [sx, sy] = toScreen(axes, DIR_VEC[d])
    g[sy + 1][sx + 1] = d
  }
  return g
}

/**
 * 드래그 화살표(기계 좌표) — `in` 은 슬롯 가장자리 바깥 `dist` 에서 가장자리로(슬롯 쪽으로 들어온다),
 * `out` 은 가장자리에서 바깥으로(놓은 뒤 빠진다). 길이 = Dist(대각 성분은 DragDelta 처럼 /√2).
 */
export function dragArrow(
  offset: readonly [number, number],
  dir: number,
  radius: number,
  dist: number,
  kind: DragKind,
): { from: [number, number]; to: [number, number] } | null {
  const v = dirVec(dir)
  if (!v || !(dist > 0)) return null
  const k = v[0] !== 0 && v[1] !== 0 ? Math.SQRT1_2 : 1
  const ux = v[0] * k
  const uy = v[1] * k
  const rim: [number, number] = [offset[0] + ux * radius, offset[1] + uy * radius]
  const far: [number, number] = [offset[0] + ux * (radius + dist), offset[1] + uy * (radius + dist)]
  return kind === 'in' ? { from: far, to: rim } : { from: rim, to: far }
}

// ── 적합 ────────────────────────────────────────────────────────────────────

const r1 = (v: number): number => z0(Math.round(v * 10) / 10)

/** 팔렛 밖으로 나간 길이(X, Y) — `|offset| + OD/2 − size/2`. */
export function overhang(offset: readonly [number, number], od: number, size: number): [number, number] {
  return [r1(Math.abs(offset[0]) + od / 2 - size / 2), r1(Math.abs(offset[1]) + od / 2 - size / 2)]
}

export function minDistance(points: readonly (readonly [number, number])[]): number {
  let m = Infinity
  for (let i = 0; i < points.length; i++)
    for (let j = i + 1; j < points.length; j++)
      m = Math.min(m, Math.hypot(points[i][0] - points[j][0], points[i][1] - points[j][1]))
  return Number.isFinite(m) ? m : 0
}

export type SlotTone = 'ok' | 'warn' | 'error'

/** 원 색 — 겹침(계획 오류)이면 error, 팔렛 밖·GR2 영역 거부 예상이면 warn. */
export function slotTone(plan: Pick<PalletPlan, 'errors' | 'area'>, s: PlanSlot): SlotTone {
  if (plan.errors.length > 0) return 'error'
  if (s.overhang[0] > 0 || s.overhang[1] > 0) return 'warn'
  if (plan.area?.rejects.some((r) => r.seq === s.seq)) return 'warn'
  return 'ok'
}

// ── 계획 스텝 ───────────────────────────────────────────────────────────────

export function clampLevels(v: number | undefined): number {
  if (v === undefined || !Number.isFinite(v)) return 1
  return Math.min(LEVELS_MAX, Math.max(1, Math.round(v)))
}

/** 단 순서 — 출하(놓기)는 아래부터, 입고(집기)는 맨 위부터. */
export function levelOrder(kind: DragKind, levels: number): number[] {
  const n = clampLevels(levels)
  const up = Array.from({ length: n }, (_, i) => i + 1)
  return kind === 'out' ? up : up.reverse()
}

export interface StepOptions {
  station: number
  item_code: number | null
  levels: number
  robot: number | null
  /** 없으면 계획의 종류(흐름 기준 PICK/DROP). */
  type?: TaskType
}

/** 단 순서 × Seq 순서로 스테이션 대상 스텝 — 각 스텝은 `pallet: {seq, level}` 을 싣고 compose 가 위치·드래그를 정한다. */
export function buildSteps(
  plan: Pick<PalletPlan, 'slots' | 'drag_kind' | 'type' | 'flow' | 'pattern'>,
  o: StepOptions,
  makeId: () => string = stepId,
): PlanStep[] {
  const type = o.type ?? plan.type
  const slots = [...plan.slots].sort((a, b) => a.seq - b.seq)
  const out: PlanStep[] = []
  for (const level of levelOrder(plan.drag_kind, o.levels)) {
    for (const s of slots) {
      const pallet: PalletRef = { seq: s.seq, level }
      out.push({
        id: makeId(),
        type,
        target: { kind: 'station', id: o.station },
        item_code: o.item_code,
        count: 1,
        note: `Pallet ${plan.flow} P${plan.pattern} L${level} Seq ${s.seq} ${s.slot}`,
        robot: o.robot,
        pallet,
      })
    }
  }
  return out
}

export function planCsv(plan: Pick<PalletPlan, 'slots' | 'z_levels'>): string {
  const head = ['Seq', 'Slot', 'OffsetX', 'OffsetY', 'X', 'Y', 'DragDir', 'DragType']
  for (const z of plan.z_levels) head.push(`Z L${z.level}`)
  const rows = [...plan.slots]
    .sort((a, b) => a.seq - b.seq)
    .map((s) => [
      s.seq,
      s.slot,
      s.offset_x,
      s.offset_y,
      s.x,
      s.y,
      s.drag_dir,
      hexByte(s.drag_type),
      ...plan.z_levels.map((z) => (z.z === null ? '' : z.z)),
    ])
  return [head, ...rows].map((r) => r.join(',')).join('\n')
}

// ── 쿼리 ────────────────────────────────────────────────────────────────────

export interface PlanParams {
  station?: number
  center_x?: number
  center_y?: number
  floor_z?: number
  item_code?: number
  od?: number
  gap?: number
  flow?: string
  pattern?: number | 'auto'
  rotation?: number
  mirror_x?: boolean
  mirror_y?: boolean
  pallet_size?: number
  levels?: number
  robot?: number
}

/** 빈 값·NaN 은 싣지 않는다(백엔드가 품목 팔렛 설정·기본값으로 채운다). */
export function planQuery(p: PlanParams): string {
  const s = new URLSearchParams()
  for (const [k, v] of Object.entries(p)) {
    if (v === undefined || v === null || v === '') continue
    if (typeof v === 'number' && !Number.isFinite(v)) continue
    s.set(k, typeof v === 'boolean' ? (v ? '1' : '0') : String(v))
  }
  const t = s.toString()
  return t ? `?${t}` : ''
}
