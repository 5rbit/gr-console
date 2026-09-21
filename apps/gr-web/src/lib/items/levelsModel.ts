// 화물 규격 확장(단수 Max · 하중 기준 비드 곡선)의 순수 로직.
//
// 백엔드 `registry/spec.rs` 와 같은 규칙이다 — 검증(`specErrors`)·곡선 풀이(`curveAt`)·보기(`buildGrid`)가
// 바뀌면 두 곳을 함께 고친다. 편집 중인(저장 전) 규격으로 표·그림을 즉시 다시 그리려고 서버의
// `GET /api/items/{code}/levels` 계산을 여기서도 한다. 측정 표본은 서버가 준 것만 쓴다.
//
// # 정본은 스택 크기별 **절대** 프로파일이다
//
// PLC 의 measureSKU 는 스택 하나를 통째로 잰다. 그 결과(셀 바닥 기준 절대 LowerBead/UpperBead/StackHeight)를
// 스택 크기 `n` 별로 그대로 보관한다(`ItemSpec.profiles`). 같은 크기의 스택을 집을 때는 환산도 합산도 없이
// 그 절대값을 그대로 쓴다.
//
// 하중(`Above` = 위에 얹힌 개수) 곡선은 그 프로파일에서 **파생**된다(`derivedCurve`) — 잰 적 없는 스택
// 크기를 위한 대체 경로다. 5 개 스택의 3 단(위 2 개)과 8 개 스택의 3 단(위 5 개)이 다른 값인 이유도 같다.
//
// Z 를 푸는 차례: ① 그 크기의 프로파일 → ② 파생 곡선(환산·보간) → ③ mid(Height/2).
import type {
  AboveRow,
  BeadProfile,
  BeadSource,
  CurveValue,
  GripRef,
  Item,
  ItemSpec,
  LevelValues,
  LevelView,
  ProfileRow,
  ResolvedSource,
  Scenario,
  StockEntry,
} from '../types'

export const STACK_MAX_LIMIT = 20
export const LEVEL_MAX = 20
/** PickBeadOffset 기본값(mm) — 상부 비드에서 이만큼 아래를 잡는다(백엔드 DEFAULT_PICK_BEAD_OFFSET). */
export const DEFAULT_PICK_BEAD_OFFSET = 30
/** 눌림을 먹인 뒤에도 남겨 두는 최소 단 높이·비드(mm). */
export const MIN_PITCH = 1
/** 측정 대비 편차 표시 기준(mm) — 이하는 정상, 두 배 이하는 주의, 넘으면 이상. */
export const DEVIATION_TOL = 3
/** 하중이 커지는데 값이 커지는 경고의 문턱(mm) — 0.1 mm 반올림 잡음은 넘긴다. */
export const MONOTONIC_TOL = 0.5

export const SOURCE_MEASURED: BeadSource = 'measured'
export const SOURCE_MANUAL: BeadSource = 'manual'
export const SOURCE_COMPUTED: BeadSource = 'computed'

/** 프로파일 한 줄에서 사람이 고칠 수 있는 칸(모두 셀 바닥 기준 절대값). */
export type ProfileKey = 'lower_bead' | 'upper_bead' | 'stack_height'
export const PROFILE_KEYS: readonly ProfileKey[] = ['lower_bead', 'upper_bead', 'stack_height']
export const PROFILE_LABEL: Record<ProfileKey, string> = {
  lower_bead: 'AbsLowerBead',
  upper_bead: 'AbsUpperBead',
  stack_height: 'StackHeight',
}

export const DEFAULT_SPEC: ItemSpec = {
  stack_max: 0,
  pallet_max: 0,
  weight_kg: null,
  profiles: [],
  bead_source: '',
  pick_bead_offset: null,
  compression: null,
  compression_source: '',
  auto_apply_measured: null,
}

type Dims = Pick<Item, 'height' | 'lower_bead_height' | 'upper_bead_height'>

/** 서버가 spec 을 안 줬거나(옛 응답·테스트 픽스처) 일부만 줬을 때 기본값으로 채운다. */
export function specOf(i: { spec?: Partial<ItemSpec> | null }): ItemSpec {
  const s = i.spec ?? {}
  return { ...DEFAULT_SPEC, ...s, profiles: (s.profiles ?? []).map((p) => ({ ...p, rows: [...p.rows] })) }
}

/** 스택 크기 `count` 를 통째로 잰 프로파일. */
export function profileOf(s: ItemSpec, count: number): BeadProfile | null {
  return s.profiles.find((p) => p.count === count && p.rows.length > 0) ?? null
}

const rowEmpty = (r: ProfileRow) => r.lower_bead === null && r.upper_bead === null && r.stack_height === null

export function profileRow(p: BeadProfile, level: number): ProfileRow | null {
  return p.rows.find((r) => r.level === level && !rowEmpty(r)) ?? null
}

/** 이 단을 집을 때 쓰는 **절대** 상부 비드(셀 바닥 기준 mm). */
export function absUpperBead(p: BeadProfile, level: number): number | null {
  return profileRow(p, level)?.upper_bead ?? null
}

/** 잰 스택 크기 목록(오름차순). */
export function measuredCounts(s: ItemSpec): number[] {
  return s.profiles.filter((p) => p.rows.length > 0).map((p) => p.count).sort((a, b) => a - b)
}

/** 1 단부터의 절대값(`stackProfilePoints` 입력 모양). */
function profileLevelValues(p: BeadProfile): LevelValues[] {
  const out: LevelValues[] = []
  for (let k = 1; k <= p.count; k++) {
    const r = profileRow(p, k)
    out.push(r ? { lower_bead: r.lower_bead, upper_bead: r.upper_bead, stack_height: r.stack_height } : { lower_bead: null, upper_bead: null, stack_height: null })
  }
  return out
}

export const round1 = (v: number): number => Math.round(v * 10) / 10

/** `k` 단(1-based) **공칭** 계산값(눌림 없음, 셀 바닥 기준 절대값). */
export function computedLevel(i: Dims, k: number): LevelValues {
  const h = Math.max(0, i.height)
  const below = Math.max(0, k - 1) * h
  return {
    lower_bead: below + i.lower_bead_height,
    upper_bead: below + i.upper_bead_height,
    stack_height: k * h,
  }
}

/** 보기 행 수: max(stack_max, 잰 가장 큰 스택, 1). */
export function viewCount(s: ItemSpec): number {
  const deepest = s.profiles.reduce((m, p) => (p.rows.length ? Math.max(m, p.count) : m), 0)
  return Math.min(Math.max(s.stack_max, deepest, 1), LEVEL_MAX)
}

/** 미리보기 스택 크기 — 고른 값이 없으면 StackMax, 그것도 없으면 곡선이 덮는 범위. */
export function previewCount(s: ItemSpec, preview?: number | null): number {
  if (preview !== null && preview !== undefined && preview > 0) return Math.min(preview, LEVEL_MAX)
  return Math.min(Math.max(s.stack_max, viewCount(s), 1), LEVEL_MAX)
}

/** 실제로 쓰는 PickBeadOffset(비었거나 이상하면 30). */
export function pickBeadOffset(s: ItemSpec): number {
  const v = s.pick_bead_offset
  return v !== null && Number.isFinite(v) && v >= 0 ? v : DEFAULT_PICK_BEAD_OFFSET
}

/** 곡선에 점이 없는 하중을 위한 대체 눌림양(비었거나 이상하면 0 = 보정 없음). */
export function compressionOf(s: ItemSpec): number {
  const v = s.compression
  return v !== null && Number.isFinite(v) && v >= 0 ? v : 0
}

/** SKU 측정 자동 반영 스위치(비면 켬). */
export function autoApplyMeasured(s: ItemSpec): boolean {
  return s.auto_apply_measured ?? true
}

/** 단별 보기가 "가득 쌓았다"고 보는 개수 — StackMax, 없으면 보기 행 수. */
export function referenceStack(s: ItemSpec): number {
  return s.stack_max > 0 ? s.stack_max : viewCount(s)
}

/** 위에 `above` 개가 얹힌 타이어의 눌린 높이(대체 모형). */
export function pressedHeight(h: number, compression: number, above: number): number {
  const base = Math.max(h, 0)
  if (compression <= 0 || above <= 0) return base
  return Math.max(base - compression * above, MIN_PITCH)
}

/**
 * `n` 개 스택에서 아래 `below` 개가 차지하는 높이 — 곡선에 점이 없을 때의 **대체** 모형(각자 자기 위 개수
 * 만큼 눌린다). 곡선을 아는 자리에서는 `pickPoint` 가 하중별 PressedHeight 를 대신 쓴다.
 */
export function stackBase(h: number, compression: number, n: number, below: number): number {
  if (compression <= 0) return Math.max(h, 0) * below
  let sum = 0
  for (let j = 1; j <= below; j++) sum += pressedHeight(h, compression, Math.max(n - j, 0))
  return sum
}

export function curveRow(curve: readonly AboveRow[], above: number): AboveRow | undefined {
  return curve.find((r) => r.above === above)
}

/** 프로파일이 덮는 하중(오름차순) — 파생 곡선이 값을 가지는 `Above`. */
export function measuredAboves(s: ItemSpec): number[] {
  const out = new Set<number>()
  for (const p of s.profiles)
    for (const r of p.rows) {
      if (rowEmpty(r)) continue
      const above = Math.max(p.count - r.level, 0)
      if (s.stack_max === 0 || above < s.stack_max) out.add(above)
    }
  return [...out].sort((a, b) => a - b)
}

const pos = (v: number | null | undefined): number | null => (v !== null && v !== undefined && Number.isFinite(v) && v > 0 ? v : null)

/** 잰 스택 하나에서 되짚은 단별 값 — 백엔드 `spec::stack_profile`. 비드·높이는 타이어 자기 바닥 기준. */
export interface ProfilePointCalc {
  level: number
  above: number
  bottom: number
  pressed_height: number | null
  lower_bead: number | null
  upper_bead: number | null
}

export function stackProfilePoints(
  i: Dims,
  measured: readonly LevelValues[],
  count: number,
  eachHeight: number | null,
  totalHeight: number | null,
): ProfilePointCalc[] {
  const h = Math.max(i.height, 0)
  const len = Math.min(count, measured.length, LEVEL_MAX)
  if (len <= 0 || h <= 0) return []
  const ok = (v: number | null): number | null => (v !== null && Number.isFinite(v) && v > 0 && v <= h * 2 ? round1(v) : null)
  const at = (k: number): LevelValues => measured[k - 1] ?? { lower_bead: null, upper_bead: null, stack_height: null }
  const pitches: (number | null)[] = []
  for (let k = 1; k < len; k++) {
    const a = at(k)
    const b = at(k + 1)
    const lo = pos(b.lower_bead) !== null && pos(a.lower_bead) !== null ? (b.lower_bead as number) - (a.lower_bead as number) : null
    const up = pos(b.upper_bead) !== null && pos(a.upper_bead) !== null ? (b.upper_bead as number) - (a.upper_bead as number) : null
    pitches.push(ok(lo ?? up))
  }
  const belowSum = pitches.reduce((acc: number, v) => acc + (v ?? 0), 0)
  const allSome = pitches.every((v) => v !== null)
  const top = allSome && pos(totalHeight) !== null ? ok((totalHeight as number) - belowSum) : null
  pitches.push(top ?? ok(eachHeight))
  const out: ProfilePointCalc[] = []
  let bottom = 0
  for (let k = 1; k <= len; k++) {
    const pitch = pitches[k - 1]
    const m = at(k)
    const rel = (v: number | null): number | null => {
      const x = pos(v)
      if (x === null) return null
      const d = round1(x - bottom)
      return d >= 0 ? d : null
    }
    out.push({
      level: k,
      above: Math.max(count - k, 0),
      bottom: round1(bottom),
      pressed_height: pitch,
      lower_bead: rel(m.lower_bead),
      upper_bead: rel(m.upper_bead),
    })
    bottom += pitch ?? 0
  }
  return out
}

/**
 * 정본 프로파일에서 **파생한** 하중 곡선 — 저장하지 않는다(백엔드 `spec::curve_of`).
 * 크기가 다른 프로파일은 곡선의 다른 구간을 채우고, 겹치면 새 표본이 이긴다(손으로 고친 행은 지지 않는다).
 */
export function derivedCurve(i: Dims, s: ItemSpec): AboveRow[] {
  const by = new Map<number, AboveRow>()
  const profs = s.profiles
    .filter((p) => p.count > 0 && p.rows.length > 0)
    .slice()
    .sort((a, b) => (a.at !== b.at ? (a.at < b.at ? -1 : 1) : a.sample_seq !== b.sample_seq ? a.sample_seq - b.sample_seq : a.count - b.count))
  for (const p of profs) {
    for (const pt of stackProfilePoints(i, profileLevelValues(p), p.count, p.each_height, p.total_height)) {
      if (pt.above > LEVEL_MAX || (s.stack_max > 0 && pt.above >= s.stack_max)) continue
      if (pt.lower_bead === null && pt.upper_bead === null && pt.pressed_height === null) continue
      const manual = profileRow(p, pt.level)?.source === SOURCE_MANUAL
      const prev = by.get(pt.above)
      if (prev && prev.source === SOURCE_MANUAL && !manual) continue
      by.set(pt.above, {
        above: pt.above,
        lower_bead: pt.lower_bead,
        upper_bead: pt.upper_bead,
        pressed_height: pt.pressed_height,
        source: manual ? SOURCE_MANUAL : SOURCE_MEASURED,
        sample_plc: manual ? '' : p.sample_plc,
        sample_seq: manual ? 0 : p.sample_seq,
        updated_at: p.at,
      })
    }
  }
  return [...by.values()].sort((a, b) => a.above - b.above)
}

type CurveKey = 'lower_bead' | 'upper_bead' | 'pressed_height'
const pick = (r: AboveRow, key: CurveKey): number | null => r[key]

/** 두 측정 점 사이 선형 보간 — 양쪽이 다 있어야 한다. */
function interp(rows: readonly AboveRow[], above: number, key: CurveKey): number | null {
  let lo: AboveRow | null = null
  let hi: AboveRow | null = null
  for (const r of rows) {
    if (pick(r, key) === null) continue
    if (r.above < above && (lo === null || r.above > lo.above)) lo = r
    if (r.above > above && (hi === null || r.above < hi.above)) hi = r
  }
  if (!lo || !hi || hi.above <= lo.above) return null
  const va = pick(lo, key) as number
  const vb = pick(hi, key) as number
  return round1(va + ((vb - va) * (above - lo.above)) / (hi.above - lo.above))
}

/** 하중 `above` 에서 쓰는 값 — 파생 곡선의 점 → 보간 → 스칼라 모형(백엔드 `spec::curve_at_in`). */
export function curveAt(i: Dims, s: ItemSpec, above: number): CurveValue {
  return curveAtIn(i, s, derivedCurve(i, s), above)
}

/** [`curveAt`] 이되 곡선을 미리 만들어 둔 자리용(한 번 파생해 돌려 쓴다). */
export function curveAtIn(i: Dims, s: ItemSpec, curve: readonly AboveRow[], above: number): CurveValue {
  const h = Math.max(i.height, 0)
  const c = compressionOf(s)
  const fallbackPressed = pressedHeight(h, c, above)
  const fallbackUpper = i.upper_bead_height > 0 ? i.upper_bead_height - c * above : null
  const fallbackLower = i.lower_bead_height > 0 ? i.lower_bead_height : null
  const exact = curveRow(curve, above)
  if (exact && (exact.upper_bead !== null || exact.pressed_height !== null)) {
    const source: ResolvedSource =
      exact.source === SOURCE_MANUAL ? SOURCE_MANUAL : exact.source === SOURCE_MEASURED ? SOURCE_MEASURED : SOURCE_COMPUTED
    return {
      above,
      upper_bead: exact.upper_bead ?? fallbackUpper,
      lower_bead: exact.lower_bead ?? fallbackLower,
      pressed_height: exact.pressed_height ?? fallbackPressed,
      source,
      // 점은 있어도 UpperBead 칸이 비어 있으면 비드는 여전히 지어낸 값이다.
      upper_bead_source:
        exact.upper_bead !== null
          ? source
          : interp(curve, above, 'upper_bead') !== null
            ? 'interpolated'
            : SOURCE_COMPUTED,
      sample: exact.sample_seq === 0 ? null : { plc: exact.sample_plc, seq: exact.sample_seq },
    }
  }
  const up = interp(curve, above, 'upper_bead')
  const lo = interp(curve, above, 'lower_bead')
  const pr = interp(curve, above, 'pressed_height')
  const source: ResolvedSource = up !== null || pr !== null ? 'interpolated' : SOURCE_COMPUTED
  return {
    above,
    upper_bead: up ?? fallbackUpper,
    lower_bead: lo ?? fallbackLower,
    pressed_height: pr ?? fallbackPressed,
    source,
    upper_bead_source: up !== null ? 'interpolated' : SOURCE_COMPUTED,
    sample: null,
  }
}

/**
 * 그 하중에서 **실제로 잡을** 비드 — 잰(측정·손입력·보간) 값만 돌려준다.
 * 곡선에 점이 없어 공칭값으로 지어낸 비드는 쓰지 않는다(그러면 그립은 mid 로 내려간다).
 */
export function measuredBead(i: Dims, s: ItemSpec, above: number, curve = derivedCurve(i, s)): number | null {
  const v = curveAtIn(i, s, curve, above)
  if (v.upper_bead === null || v.upper_bead_source === SOURCE_COMPUTED) return null
  return Math.max(v.upper_bead, MIN_PITCH)
}

export interface PickPoint {
  /** 셀 바닥 기준 그립 Z. */
  z: number
  /** 아래 스택 윗면(셀 바닥 기준). */
  base: number
  /** 타이어 바닥에서 그립점까지. */
  grip: number
  /** 잡는 타이어 바닥 기준 상부 비드 — 잰 적이 없으면 null. */
  upperBead: number | null
  /** 실제로 쓴 기준 — `pick_bead`(잰 비드 − PickBeadOffset) | `mid`(측정 없음 → Height/2). */
  ref: GripRef
  above: number
  /** 곡선 값을 썼다(측정·보간). */
  fromTable: boolean
  /** 이 크기의 스택을 통째로 잰 프로파일의 절대값을 그대로 썼다(환산 없음). */
  fromProfile: boolean
}

/**
 * `total` 개 스택에서 `k` 단 타이어를 bead+offset 으로 집을 때 — 백엔드 `spec::pick_z_at` 과 같은 식.
 * **아래 타이어는 각자 자기 하중(`total − j`)의 PressedHeight 로** 쌓이고, 잡는 타이어의 비드는 자기 하중
 * (`total − k`)의 곡선 값이다. 그 하중을 **잰 적이 없으면 그립점만 mid(Height/2)** 로 내려간다.
 */
export function pickPoint(i: Dims, s: ItemSpec, total: number, k: number, curve = derivedCurve(i, s)): PickPoint {
  const h = Math.max(i.height, 0)
  const below = Math.max(k - 1, 0)
  const n = Math.max(total, k)
  const above = Math.max(n - k, 0)
  // ① 그 크기를 통째로 잰 프로파일이 있으면 절대값을 그대로 — 환산도 아래 타이어 합산도 없다.
  const p = profileOf(s, k + above)
  const abs = p ? absUpperBead(p, k) : null
  if (abs !== null) {
    const bead = Math.max(abs, MIN_PITCH)
    const grip = Math.max(round1(bead - pickBeadOffset(s)), 0)
    return { z: grip, base: 0, grip, upperBead: bead, ref: 'pick_bead', above, fromTable: true, fromProfile: true }
  }
  // ② 파생 곡선 ③ mid
  let base = 0
  let fromTable = false
  for (let j = 1; j <= below; j++) {
    const v = curveAtIn(i, s, curve, Math.max(n - j, 0))
    if (v.source !== SOURCE_COMPUTED) fromTable = true
    base += v.pressed_height
  }
  base = round1(base)
  const bead = measuredBead(i, s, above, curve)
  if (bead !== null) fromTable = true
  const grip = bead === null ? h / 2 : Math.max(bead - pickBeadOffset(s), 0)
  return { z: round1(base + grip), base, grip, upperBead: bead, ref: bead === null ? 'mid' : 'pick_bead', above, fromTable, fromProfile: false }
}

export interface GridRow {
  /** 미리보기 스택에서의 단 번호(파생값). */
  level: number
  /** 이 행의 **키** — 위에 얹힌 개수. */
  above: number
  /** 곡선에 저장된 그대로(점이 없으면 null). */
  configured: AboveRow | null
  /** 실제로 쓰는 값. */
  effective: CurveValue
  /** 그 하중에서 먹은 총 눌림(mm) = Height − PressedHeight. */
  compressionAt: number | null
  /** 미리보기 스택에서 이 타이어 바닥이 서는 높이(셀 바닥 기준). */
  bottom: number
  absLowerBead: number | null
  absUpperBead: number | null
  /** 이 단 타이어 윗면까지(셀 바닥 기준). */
  absStackHeight: number
  /** 눌림 없는 공칭 절대값. */
  computed: LevelValues
  /** 같은 크기의 스택을 잰 표본 값(절대). */
  measured: LevelValues | null
  /** measured − abs */
  deviation: LevelValues | null
  /** 이 단을 집을 때 실제로 쓸 그립 Z(셀 바닥 기준) — 높이를 모르면 null. */
  pickZ: number | null
  /** 그 Z 가 쓴 기준 — `pick_bead` | `mid`(측정 없음). */
  pickZRef: GripRef
  source: ResolvedSource
  /** 이 크기의 스택을 통째로 잰 프로파일의 값인가(아니면 환산·보간값 — 화면은 흐리게). */
  fromProfile: boolean
  /** 프로파일에 저장된 그대로(없으면 null) — 편집 칸이 보여 줄 값. */
  stored: ProfileRow | null
}

const sub = (a: number | null, b: number | null) => (a === null || b === null ? null : a - b)
const isEmpty = (v: LevelValues) => v.lower_bead === null && v.upper_bead === null && v.stack_height === null

/** 서버 행에서 단 → 측정값(절대). 측정이 없는 단은 빠진다. */
export function measuredByLevel(rows: readonly LevelView[] | null | undefined): Map<number, LevelValues> {
  const out = new Map<number, LevelValues>()
  for (const r of rows ?? []) if (r.measured && !isEmpty(r.measured)) out.set(r.level, r.measured)
  return out
}

/**
 * 편집 중인 규격으로 만든 표 — 서버 `level_views` 와 같은 모양. `preview` 개를 쌓았다고 보고 단마다
 * 하중을 매긴다(`above = preview − level`).
 */
export function buildGrid(
  i: Dims,
  s: ItemSpec,
  preview?: number | null,
  measured?: ReadonlyMap<number, LevelValues>,
): GridRow[] {
  const n = previewCount(s, preview)
  const h = Math.max(i.height, 0)
  const curve = derivedCurve(i, s)
  const prof = profileOf(s, n)
  const out: GridRow[] = []
  let bottom = 0
  for (let k = 1; k <= n; k++) {
    const above = n - k
    const v = curveAtIn(i, s, curve, above)
    const stored = prof ? profileRow(prof, k) : null
    const abs = (x: number | null) => (x === null ? null : round1(bottom + x))
    // 잰 크기면 절대값을 그대로, 아니면 파생 곡선을 쌓아 만든 값.
    const absLower = stored?.lower_bead ?? abs(v.lower_bead)
    const absUpper = stored?.upper_bead ?? abs(v.upper_bead)
    const absStack = stored?.stack_height ?? round1(bottom + v.pressed_height)
    const m = measured?.get(k) ?? null
    const deviation = m
      ? {
          lower_bead: sub(m.lower_bead, absLower),
          upper_bead: sub(m.upper_bead, absUpper),
          stack_height: sub(m.stack_height, absStack),
        }
      : null
    const p = pickPoint(i, s, n, k, curve)
    out.push({
      level: k,
      above,
      configured: curveRow(curve, above) ?? null,
      effective: v,
      compressionAt: h > 0 ? round1(Math.max(h - v.pressed_height, 0)) : null,
      bottom: round1(bottom),
      absLowerBead: absLower,
      absUpperBead: absUpper,
      absStackHeight: absStack,
      computed: computedLevel(i, k),
      measured: m,
      deviation,
      pickZ: h > 0 || p.upperBead !== null ? p.z : null,
      pickZRef: p.ref,
      source: stored ? (stored.source === SOURCE_MANUAL ? SOURCE_MANUAL : SOURCE_MEASURED) : v.source,
      fromProfile: stored !== null,
      stored,
    })
    bottom += v.pressed_height
  }
  return out
}

/** 스택 크기·단 오름차순 + 앞뒤 공백 정리 — 비교·저장 전에. */
export function normalizeSpec(s: ItemSpec): ItemSpec {
  return {
    ...s,
    bead_source: (s.bead_source ?? '').trim(),
    compression_source: (s.compression_source ?? '').trim(),
    profiles: s.profiles
      .map((p) => ({ ...p, rows: p.rows.filter((r) => !rowEmpty(r)).sort((a, b) => a.level - b.level) }))
      .filter((p) => p.count > 0 && p.rows.length > 0)
      .sort((a, b) => a.count - b.count),
  }
}

export function specEquals(a: ItemSpec, b: ItemSpec): boolean {
  return JSON.stringify(normalizeSpec(a)) === JSON.stringify(normalizeSpec(b))
}

const emptyProfile = (count: number): BeadProfile => ({
  count,
  rows: [],
  total_height: null,
  each_height: null,
  sample_plc: '',
  sample_seq: 0,
  at: '',
})

/**
 * 프로파일의 한 칸(절대값)을 바꾼다. 그 크기의 프로파일이 없으면 만들고, 한 줄의 세 값이 모두 비면 줄을 지운다.
 * 손으로 고친 줄은 `source = manual` 이 되어 SKU 자동 반영이 덮지 않는다(서버도 같은 규칙).
 */
export function setProfileValue(s: ItemSpec, count: number, level: number, key: ProfileKey, value: number | null): ItemSpec {
  const cur = profileOf(s, count) ?? s.profiles.find((p) => p.count === count) ?? emptyProfile(count)
  const row = cur.rows.find((r) => r.level === level) ?? { level, lower_bead: null, upper_bead: null, stack_height: null, source: SOURCE_MANUAL as BeadSource }
  const next: ProfileRow = { ...row, [key]: value, source: SOURCE_MANUAL }
  const rows = cur.rows.filter((r) => r.level !== level)
  const profile: BeadProfile = { ...cur, rows: rowEmpty(next) ? rows : [...rows, next] }
  const rest = s.profiles.filter((p) => p.count !== count)
  return normalizeSpec({ ...s, profiles: [...rest, profile] })
}

/** 손 표시를 지운다 — 다음 SKU 측정이 이 줄을 다시 덮게 된다. */
export function clearManual(s: ItemSpec, count: number, level: number): ItemSpec {
  return normalizeSpec({
    ...s,
    profiles: s.profiles.map((p) =>
      p.count === count ? { ...p, rows: p.rows.map((r) => (r.level === level ? { ...r, source: SOURCE_MEASURED as BeadSource } : r)) } : p,
    ),
  })
}

/** 한 줄 지우기 — 그 단은 파생 곡선(환산·보간)으로 돌아간다. */
export function removeProfileRow(s: ItemSpec, count: number, level: number): ItemSpec {
  return normalizeSpec({ ...s, profiles: s.profiles.map((p) => (p.count === count ? { ...p, rows: p.rows.filter((r) => r.level !== level) } : p)) })
}

/** 그 스택 크기의 프로파일을 통째로 지운다. */
export function removeProfile(s: ItemSpec, count: number): ItemSpec {
  return { ...s, profiles: s.profiles.filter((p) => p.count !== count) }
}

/** 스택 크기 `n` 의 프로파일을 계산값(대체 모형의 절대값)으로 채운다 — 손입력으로 들어간다. */
export function fillComputed(i: Dims, s: ItemSpec, n = previewCount(s)): ItemSpec {
  const h = Math.max(i.height, 0)
  const c = compressionOf(s)
  const rows: ProfileRow[] = []
  let bottom = 0
  for (let k = 1; k <= Math.min(n, LEVEL_MAX); k++) {
    const above = n - k
    const pressed = pressedHeight(h, c, above)
    rows.push({
      level: k,
      lower_bead: i.lower_bead_height > 0 ? round1(bottom + i.lower_bead_height) : null,
      upper_bead: i.upper_bead_height > 0 ? round1(bottom + Math.max(i.upper_bead_height - c * above, MIN_PITCH)) : null,
      stack_height: round1(bottom + pressed),
      source: SOURCE_MANUAL,
    })
    bottom += pressed
  }
  const cur = s.profiles.find((p) => p.count === n) ?? emptyProfile(n)
  const rest = s.profiles.filter((p) => p.count !== n)
  return normalizeSpec({ ...s, profiles: [...rest, { ...cur, rows, total_height: round1(bottom), each_height: n > 0 ? round1(bottom / n) : null }] })
}

const finiteNonNeg = (v: number | null) => v === null || (Number.isFinite(v) && v >= 0)

/** 백엔드 `validate_spec`(+ `validate_spec_for`) 과 같은 규칙(엄격한 모순만). 빈 배열이면 통과. */
export function specErrors(s: ItemSpec, height?: number): string[] {
  const out: string[] = []
  if (!Number.isInteger(s.stack_max) || s.stack_max < 0 || s.stack_max > STACK_MAX_LIMIT)
    out.push(`StackMax 는 0..${STACK_MAX_LIMIT} 정수(0 = 제한 없음)`)
  if (!Number.isInteger(s.pallet_max) || s.pallet_max < 0 || s.pallet_max > 255)
    out.push('PalletMax 는 0..255 정수(0 = 제한 없음)')
  if (!finiteNonNeg(s.weight_kg)) out.push('WeightKg 는 0 이상의 숫자')
  if (!finiteNonNeg(s.pick_bead_offset)) out.push('PickBeadOffset 은 0 이상의 숫자')
  if (!finiteNonNeg(s.compression)) out.push('Compression 은 0 이상의 숫자')
  if (height !== undefined && height > 0) {
    if (s.pick_bead_offset !== null && s.pick_bead_offset > height)
      out.push(`PickBeadOffset 은 Height ${round1(height)} 이하여야 합니다`)
    if (s.compression !== null && s.compression >= height)
      out.push(`Compression 은 Height ${round1(height)} 보다 작아야 합니다`)
  }
  const counts = new Set<number>()
  for (const p of s.profiles) {
    if (!Number.isInteger(p.count) || p.count < 1 || p.count > LEVEL_MAX) {
      out.push(`스택 ${p.count}: 1..${LEVEL_MAX}`)
      continue
    }
    if (counts.has(p.count)) out.push(`스택 ${p.count}: 중복`)
    counts.add(p.count)
    const seen = new Set<number>()
    for (const r of p.rows) {
      const at = `스택 ${p.count} 의 ${r.level}단`
      if (!Number.isInteger(r.level) || r.level < 1 || r.level > p.count) {
        out.push(`${at}: 1..${p.count}`)
        continue
      }
      if (seen.has(r.level)) out.push(`${at}: 중복`)
      seen.add(r.level)
      for (const k of PROFILE_KEYS) if (!finiteNonNeg(r[k])) out.push(`${at} ${PROFILE_LABEL[k]}: 0 이상의 숫자`)
      if (r.lower_bead !== null && r.upper_bead !== null && r.upper_bead <= r.lower_bead)
        out.push(`${at}: AbsUpperBead 는 AbsLowerBead 보다 커야 합니다`)
      if (height !== undefined && height > 0) {
        const ceiling = height * 1.5 * r.level
        for (const v of [r.upper_bead, r.stack_height])
          if (v !== null && v > ceiling) out.push(`${at}: ${round1(v)} 는 Height ${round1(height)} × 1.5 × ${r.level} 이하여야 합니다`)
      }
    }
  }
  return out
}

/** 모순은 아니지만 의심스러운 값(백엔드 `spec_warnings`). */
export function specWarnings(s: ItemSpec, dims?: Dims): string[] {
  const out: string[] = []
  // 정본(절대 프로파일)은 품목 치수를 몰라도 본다 — 위 단의 비드가 아래 단보다 낮을 수 없다.
  for (const p of s.profiles) {
    const rows = [...p.rows].sort((a, b) => a.level - b.level)
    for (const k of PROFILE_KEYS) {
      let prev: { level: number; v: number } | null = null
      for (const r of rows) {
        const v = r[k]
        if (v === null) continue
        if (prev && v < prev.v)
          out.push(`${PROFILE_LABEL[k]}: 스택 ${p.count} 의 ${r.level}단 ${round1(v)} 이 ${prev.level}단 ${round1(prev.v)} 보다 낮습니다`)
        prev = { level: r.level, v }
      }
    }
    if (p.total_height !== null && p.each_height !== null && Math.abs(p.total_height - p.each_height * p.count) > 5)
      out.push(
        `스택 ${p.count}: 측정 TotalHeight ${round1(p.total_height)} 와 EachHeight ${round1(p.each_height)} × ${p.count} 가 어긋납니다 — 측정을 확인하세요`,
      )
  }
  const h = dims ? Math.max(dims.height, 0) : 0
  if (dims) {
    for (const k of PROFILE_KEYS) {
      if (k === 'stack_height') continue
      let prev: { above: number; v: number } | null = null
      for (const r of derivedCurve(dims, s)) {
        const v = k === 'lower_bead' ? r.lower_bead : r.upper_bead
        if (v === null) continue
        if (prev && v > prev.v + MONOTONIC_TOL)
          out.push(`${PROFILE_LABEL[k]}: Above ${r.above} ${round1(v)} > Above ${prev.above} ${round1(prev.v)} — 위가 무거운데 값이 커집니다`)
        prev = { above: r.above, v }
      }
    }
  }
  if (dims && h > 0) {
    const c = compressionOf(s)
    const above = Math.max(referenceStack(s) - 1, 0)
    if (s.pick_bead_offset !== null && s.pick_bead_offset > h / 2)
      out.push(`PickBeadOffset ${round1(s.pick_bead_offset)} > Height/2 ${round1(h / 2)} — 그립점이 타이어 아래쪽입니다`)
    if (c > 0 && above > 0 && h - c * above < MIN_PITCH)
      out.push(`Compression ${round1(c)} × 위 ${above}개가 Height ${round1(h)} 를 다 먹습니다 — 단 높이를 ${MIN_PITCH} 로 제한합니다`)
    if (c > 0 && above > 0 && dims.upper_bead_height > 0 && dims.upper_bead_height - c * above - pickBeadOffset(s) < 0)
      out.push(`맨 아래 단(위 ${above}개)에서 UpperBead − Compression − PickBeadOffset < 0 — 그립점을 0 으로 제한합니다`)
  }
  return out
}

export type DeviationTone = 'ok' | 'warn' | 'fault'

export function deviationTone(d: number | null, tol = DEVIATION_TOL): DeviationTone | null {
  if (d === null || !Number.isFinite(d)) return null
  const a = Math.abs(d)
  return a <= tol ? 'ok' : a <= tol * 2 ? 'warn' : 'fault'
}

/** `+1.2` / `-0.4` / `0` */
export function signed(d: number): string {
  const v = round1(d)
  return v > 0 ? `+${v}` : String(v)
}

/** 입력 칸 문자열 → 숫자(빈칸 = `null`, 숫자가 아니면 `undefined`). */
export function parseCell(s: string): number | null | undefined {
  const t = s.trim().replace(',', '.')
  if (t === '') return null
  const n = Number(t)
  return Number.isFinite(n) ? n : undefined
}

export interface TireBox {
  level: number
  bottom: number
  top: number
  lower: number
  upper: number
  /** 이 단의 값이 측정(또는 손으로 넣은 곡선 점)에서 왔다. */
  fromTable: boolean
  /** 측정 점에서 곧바로 왔는가 — 그림에서 계산·보간과 구분해 그린다. */
  measured: boolean
  /** pick_bead 로 집을 높이(셀 바닥 기준) — 비드를 모르면 null. */
  pick: number | null
  /** 이 단 위에 얹힌 타이어 수. */
  above: number
}

/** 옆모습 그림용 — 미리보기 스택의 단마다 바닥/윗면/비드(셀 바닥 기준). */
export function stackBoxes(grid: readonly GridRow[], count: number): TireBox[] {
  const out: TireBox[] = []
  const n = Math.min(count, grid.length)
  for (let k = 1; k <= n; k++) {
    const g = grid[k - 1]
    const bottom = g.bottom
    const top = round1(bottom + g.effective.pressed_height)
    out.push({
      level: k,
      bottom,
      top: Math.max(top, bottom),
      lower: g.absLowerBead ?? bottom,
      upper: g.absUpperBead ?? top,
      fromTable: g.source !== SOURCE_COMPUTED,
      measured: g.source === SOURCE_MEASURED || g.source === SOURCE_MANUAL,
      pick: g.pickZ,
      above: g.above,
    })
  }
  return out
}

/** 0 = 제한 없음. */
export function overLimit(count: number, stackMax: number): boolean {
  return stackMax > 0 && count > stackMax
}

export function limitLabel(v: number): string {
  return v > 0 ? String(v) : '제한 없음'
}

export interface CellUse {
  cell_id: number
  count: number
  over: boolean
}

/** 코드를 든 재고 칸(칸 번호 순) — 단수 Max 를 넘은 칸 표시. */
export function stockCellsFor(code: number, stock: Iterable<StockEntry>, stackMax: number): CellUse[] {
  const out: CellUse[] = []
  for (const s of stock)
    if (s.item_code === code && s.count > 0)
      out.push({ cell_id: s.cell_id, count: s.count, over: overLimit(s.count, stackMax) })
  return out.sort((a, b) => a.cell_id - b.cell_id)
}

export interface StepRef {
  scenario_id: string
  scenario: string
  step: number
  label: string
  type: string
  target: string
}

/** 코드를 쓰는 시나리오 스텝(스텝 번호는 1-based). */
export function scenarioRefs(scenarios: readonly Scenario[], code: number): StepRef[] {
  const out: StepRef[] = []
  for (const sc of scenarios)
    sc.steps.forEach((st, i) => {
      if (st.item_code !== code) return
      out.push({
        scenario_id: sc.id,
        scenario: sc.name,
        step: i + 1,
        label: st.label,
        type: st.type,
        target: st.target ? `${st.target.kind === 'cell' ? 'Cell' : 'Station'} ${st.target.id}` : '',
      })
    })
  return out
}

/** 일괄 단수 Max 적용 전 검사 — 값 자체가 말이 되는지만(백엔드 `validate_spec` 과 같은 규칙). */
export function bulkStackMaxErrors(items: readonly Item[], stackMax: number): string[] {
  void items
  if (!Number.isInteger(stackMax) || stackMax < 0 || stackMax > STACK_MAX_LIMIT)
    return [`StackMax 는 0..${STACK_MAX_LIMIT} 정수`]
  return []
}
