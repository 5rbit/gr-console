import { describe, expect, it } from 'vitest'
import type { BeadProfile, Item, ItemSpec, LevelView, ProfileRow, Scenario, StockEntry } from '../types'
import {
  DEFAULT_PICK_BEAD_OFFSET,
  DEFAULT_SPEC,
  MIN_PITCH,
  SOURCE_COMPUTED,
  SOURCE_MANUAL,
  SOURCE_MEASURED,
  absUpperBead,
  autoApplyMeasured,
  buildGrid,
  bulkStackMaxErrors,
  clearManual,
  computedLevel,
  curveAt,
  derivedCurve,
  deviationTone,
  fillComputed,
  measuredAboves,
  measuredBead,
  measuredByLevel,
  measuredCounts,
  overLimit,
  parseCell,
  pickPoint,
  pressedHeight,
  previewCount,
  profileOf,
  removeProfile,
  removeProfileRow,
  scenarioRefs,
  setProfileValue,
  stackBase,
  signed,
  specEquals,
  specErrors,
  specOf,
  specWarnings,
  stackBoxes,
  stockCellsFor,
  viewCount,
} from './levelsModel'

const DIMS = { height: 240, lower_bead_height: 20, upper_bead_height: 220 }
const spec = (patch: Partial<ItemSpec> = {}): ItemSpec => ({ ...DEFAULT_SPEC, ...patch })

/** 위 `above` 개를 인 타이어의 실제 모양(비선형: 아래로 갈수록 더 눌린다). */
const SQUASH = [0, 4, 9, 15, 22, 30, 39, 49]
const squash = (above: number): { pressed: number; upper: number } => {
  const c = SQUASH[Math.min(above, SQUASH.length - 1)]
  return { pressed: 240 - c, upper: 220 - c * 0.8 }
}
const r1 = (v: number) => Math.round(v * 10) / 10

/** `n` 단 스택을 통째로 잰 **절대** 프로파일(셀 바닥 기준) — 정본. */
const profileFrom = (n: number, seq = n, at = 't'): BeadProfile => {
  const rows: ProfileRow[] = []
  let bottom = 0
  for (let k = 1; k <= n; k++) {
    const { pressed, upper } = squash(n - k)
    rows.push({ level: k, lower_bead: r1(bottom + 20), upper_bead: r1(bottom + upper), stack_height: null, source: SOURCE_MEASURED })
    bottom += pressed
  }
  rows[n - 1].stack_height = r1(bottom)
  return { count: n, rows, total_height: r1(bottom), each_height: r1(bottom / n), sample_plc: 'GR2', sample_seq: seq, at }
}
/** `n` 단 스택의 `k` 단을 잡을 때의 절대 상부 비드. */
const absUpper = (n: number, k: number): number => {
  let bottom = 0
  for (let j = 1; j < k; j++) bottom += squash(n - j).pressed
  return r1(bottom + squash(n - k).upper)
}
const measuredSpec = (counts: number[], patch: Partial<ItemSpec> = {}): ItemSpec =>
  spec({ ...patch, profiles: counts.map((c, i) => profileFrom(c, c, `t${i}`)) })

describe('정본은 스택 크기별 절대 프로파일이다', () => {
  it('같은 단 번호라도 스택 크기가 다르면 자기 프로파일의 절대값을 그대로 쓴다', () => {
    const s = measuredSpec([5, 8], { stack_max: 8 })
    expect(measuredCounts(s)).toEqual([5, 8])
    const p5 = pickPoint(DIMS, s, 5, 3)
    const p8 = pickPoint(DIMS, s, 8, 3)
    expect(p5.fromProfile && p8.fromProfile).toBe(true)
    // 환산도 아래 타이어 합산도 없다 — 잰 절대값 − PickBeadOffset
    expect(p5).toMatchObject({ base: 0, above: 2, ref: 'pick_bead' })
    expect(p8).toMatchObject({ base: 0, above: 5, ref: 'pick_bead' })
    expect(p5.z).toBeCloseTo(absUpper(5, 3) - 30, 3)
    expect(p8.z).toBeCloseTo(absUpper(8, 3) - 30, 3)
    expect(p5.z).not.toBe(p8.z)
    expect(absUpperBead(profileOf(s, 8)!, 3)).toBeCloseTo(absUpper(8, 3), 3)
  })

  it('잰 적 없는 크기는 파생 곡선으로 환산하고, 잰 게 없으면 mid 로 내려간다', () => {
    const s = measuredSpec([5, 8], { stack_max: 8 })
    // 6 단은 잰 적이 없다 — 아래 1 개는 자기 하중(위 5 개)의 눌린 높이, 비드는 위 4 개의 곡선 값
    const p = pickPoint(DIMS, s, 6, 2)
    expect(p.fromProfile).toBe(false)
    expect(p.ref).toBe('pick_bead')
    expect(p.base).toBeCloseTo(squash(5).pressed, 1)
    expect(p.z).toBeCloseTo(squash(5).pressed + squash(4).upper - 30, 1)
    // 한 번도 안 잰 품목은 공칭 비드가 있어도 mid(H/2)
    const never = spec({ stack_max: 5, compression: 6 })
    expect(pickPoint(DIMS, never, 5, 1)).toMatchObject({ ref: 'mid', grip: 120, upperBead: null, fromProfile: false })
    expect(measuredBead(DIMS, never, 0)).toBeNull()
  })

  it('파생 곡선 — 정확히 맞는 점 → 보간 → 대체 모형', () => {
    const s = measuredSpec([8], { stack_max: 8 })
    for (let a = 0; a < 8; a++) {
      expect(curveAt(DIMS, s, a).upper_bead).toBeCloseTo(squash(a).upper, 1)
      expect(curveAt(DIMS, s, a).pressed_height).toBeCloseTo(squash(a).pressed, 1)
      expect(curveAt(DIMS, s, a).source).toBe(SOURCE_MEASURED)
    }
    expect(measuredAboves(s)).toEqual([0, 1, 2, 3, 4, 5, 6, 7])
    expect(derivedCurve(DIMS, s)).toHaveLength(8)
    // 프로파일이 없으면 전부 스칼라(대체 모형)
    const empty = spec({ stack_max: 6, compression: 5 })
    expect(curveAt(DIMS, empty, 2)).toMatchObject({ source: SOURCE_COMPUTED, upper_bead: 210, pressed_height: 230 })
    // 곡선에 구멍이 있으면 두 점 사이를 보간 — 1·5 단 스택만 재면 하중 0 과 4 만 찬다
    const holes = measuredSpec([1, 5], { stack_max: 6 })
    expect(measuredAboves(holes)).toEqual([0, 1, 2, 3, 4])
    // 손으로 고친 줄은 파생 곡선에서도 manual
    const man = setProfileValue(measuredSpec([3]), 3, 2, 'upper_bead', 400)
    expect(curveAt(DIMS, man, 1).source).toBe(SOURCE_MANUAL)
  })

  it('처음 재는 품목의 아래 타이어는 스칼라 눌림 모형 그대로', () => {
    expect(pressedHeight(240, 0, 4)).toBe(240)
    expect(pressedHeight(240, 8, 3)).toBe(216)
    expect(pressedHeight(240, 300, 2)).toBe(MIN_PITCH)
    expect(stackBase(240, 0, 3, 2)).toBe(480)
    expect(stackBase(240, 8, 5, 5)).toBe(208 + 216 + 224 + 232 + 240)
    const c8 = spec({ stack_max: 5, compression: 8 })
    expect(pickPoint(DIMS, c8, 5, 5)).toMatchObject({ base: 880, grip: 120, z: 1000, ref: 'mid' })
    expect(pickPoint({ ...DIMS, upper_bead_height: 0 }, spec({ stack_max: 5 }), 1, 1)).toMatchObject({ grip: 120, upperBead: null })
  })

  it('PickBeadOffset 은 프로파일 갈래에도 그대로 먹고 0 에서 멈춘다', () => {
    expect(DEFAULT_PICK_BEAD_OFFSET).toBe(30)
    const s = measuredSpec([5], { stack_max: 5 })
    expect(pickPoint(DIMS, { ...s, pick_bead_offset: 45 }, 5, 5).z).toBeCloseTo(absUpper(5, 5) - 45, 3)
    expect(pickPoint(DIMS, { ...s, pick_bead_offset: 210 }, 5, 1).z).toBe(0)
  })
})

describe('specOf / computedLevel / viewCount', () => {
  it('spec 이 없거나 일부면 기본값으로 채운다', () => {
    expect(specOf({})).toEqual(DEFAULT_SPEC)
    expect(specOf({ spec: { stack_max: 4 } as ItemSpec })).toEqual({ ...DEFAULT_SPEC, stack_max: 4 })
    expect(autoApplyMeasured(spec())).toBe(true)
    expect(autoApplyMeasured(spec({ auto_apply_measured: false }))).toBe(false)
  })
  it('공칭 계산값은 (k−1)·H + 비드, 적재 = k·H (처짐 계수 미적용)', () => {
    expect(computedLevel(DIMS, 1)).toEqual({ lower_bead: 20, upper_bead: 220, stack_height: 240 })
    expect(computedLevel(DIMS, 3)).toEqual({ lower_bead: 500, upper_bead: 700, stack_height: 720 })
  })
  it('보기 행 수 / 미리보기 스택 크기', () => {
    expect(viewCount(spec())).toBe(1)
    expect(viewCount(spec({ stack_max: 4 }))).toBe(4)
    expect(viewCount(measuredSpec([12]))).toBe(12)
    expect(previewCount(spec({ stack_max: 4 }))).toBe(4)
    expect(previewCount(spec({ stack_max: 4 }), 8)).toBe(8)
    expect(previewCount(spec({ stack_max: 4 }), 0)).toBe(4)
    expect(previewCount(spec({ stack_max: 4 }), 99)).toBe(20)
  })
})

describe('buildGrid', () => {
  it('잰 크기는 절대값 그대로, 잰 적 없는 크기는 곡선에서 지어낸다', () => {
    const s = measuredSpec([8], { stack_max: 8 })
    const g8 = buildGrid(DIMS, s, 8)
    expect(g8).toHaveLength(8)
    expect(g8.map((r) => [r.level, r.above])).toEqual([
      [1, 7],
      [2, 6],
      [3, 5],
      [4, 4],
      [5, 3],
      [6, 2],
      [7, 1],
      [8, 0],
    ])
    for (const row of g8) {
      expect(row.fromProfile).toBe(true)
      expect(row.absUpperBead).toBeCloseTo(absUpper(8, row.level), 1)
      expect(row.source).toBe(SOURCE_MEASURED)
      expect(row.pickZ).toBeCloseTo(absUpper(8, row.level) - 30, 1)
      expect(row.pickZRef).toBe('pick_bead')
    }
    // 잰 적 없는 크기 — 흐리게 보일 환산값
    const g5 = buildGrid(DIMS, s, 5)
    expect(g5.every((r) => !r.fromProfile)).toBe(true)
    expect(g5[2].absUpperBead).not.toBe(g8[2].absUpperBead)
    expect(g8[0].bottom).toBe(0)
  })
  it('편차 = 측정 − 절대값', () => {
    const s = spec({ stack_max: 3 })
    const measured = new Map([[2, { lower_bead: 257, upper_bead: 456, stack_height: 474 }]])
    const g = buildGrid(DIMS, s, 3, measured)
    expect(g[1].deviation).toEqual({ lower_bead: -3, upper_bead: -4, stack_height: -6 })
    expect(g[0].measured).toBeNull()
    expect(g[0].deviation).toBeNull()
    // 잰 적 없는 품목도 표는 실제로 쓸 Z(= mid)를 보인다
    expect(buildGrid({ ...DIMS, upper_bead_height: 0 }, spec({ stack_max: 3 }))[0]).toMatchObject({ pickZ: 120, pickZRef: 'mid' })
  })
  it('서버 행에서 측정값만 뽑는다(빈 측정은 버림)', () => {
    const v = (level: number, measured: LevelView['measured']): LevelView => ({
      level,
      above: 0,
      configured: null,
      effective: { above: 0, upper_bead: null, lower_bead: null, pressed_height: 240, source: SOURCE_COMPUTED, upper_bead_source: SOURCE_COMPUTED, sample: null },
      compression_at: null,
      bottom: 0,
      abs_lower_bead: null,
      abs_upper_bead: null,
      computed: { lower_bead: null, upper_bead: null, stack_height: null },
      measured,
      deviation: null,
      pick_z: null,
      pick_z_ref: 'mid',
      source: SOURCE_COMPUTED,
      sample: null,
      from_profile: false,
    })
    const m = measuredByLevel([
      v(1, { lower_bead: 20, upper_bead: null, stack_height: null }),
      v(2, { lower_bead: null, upper_bead: null, stack_height: null }),
      v(3, null),
    ])
    expect([...m.keys()]).toEqual([1])
  })
})

describe('편집 도우미', () => {
  it('setProfileValue — 프로파일·줄 생성, 세 칸이 비면 줄 삭제, 고치면 manual', () => {
    let s = setProfileValue(spec(), 3, 3, 'upper_bead', 700)
    s = setProfileValue(s, 3, 1, 'lower_bead', 20)
    expect(s.profiles).toHaveLength(1)
    expect(s.profiles[0].rows.map((r) => r.level)).toEqual([1, 3])
    expect(s.profiles[0].rows[1]).toMatchObject({ level: 3, upper_bead: 700, source: SOURCE_MANUAL })
    s = setProfileValue(s, 3, 3, 'upper_bead', null)
    expect(s.profiles[0].rows.map((r) => r.level)).toEqual([1])
    // 측정 줄을 고치면 손 표시가 붙고, 지우면 다시 자동 반영 대상
    const meas = measuredSpec([3])
    const edited = setProfileValue(meas, 3, 2, 'upper_bead', 400)
    expect(profileOf(edited, 3)!.rows[1]).toMatchObject({ source: SOURCE_MANUAL, upper_bead: 400 })
    expect(profileOf(clearManual(edited, 3, 2), 3)!.rows[1].source).toBe(SOURCE_MEASURED)
    // 줄·프로파일 지우기
    expect(profileOf(removeProfileRow(meas, 3, 2), 3)!.rows.map((r) => r.level)).toEqual([1, 3])
    expect(removeProfile(meas, 3).profiles).toHaveLength(0)
  })
  it('계산값으로 채우기 — 그 크기의 프로파일을 손입력 절대값으로 만든다', () => {
    const s = fillComputed(DIMS, spec({ stack_max: 0, compression: 8 }), 2)
    const p = profileOf(s, 2)!
    expect(p.rows.map((r) => [r.level, r.lower_bead, r.upper_bead, r.stack_height])).toEqual([
      [1, 20, 212, 232],
      [2, 252, 452, 472],
    ])
    expect(p.rows.every((r) => r.source === SOURCE_MANUAL)).toBe(true)
    expect(p.total_height).toBe(472)
  })
  it('specEquals 는 순서·공백을 무시한다', () => {
    const a = spec({ profiles: [profileFrom(5), profileFrom(3)], bead_source: ' x ' })
    const b = spec({ profiles: [profileFrom(3), profileFrom(5)], bead_source: 'x' })
    expect(specEquals(a, b)).toBe(true)
    expect(specEquals(a, spec())).toBe(false)
  })
})

describe('검증 — 백엔드 validate_spec 과 같은 규칙', () => {
  it('정상 / 기본값 통과', () => {
    expect(specErrors(spec())).toEqual([])
    expect(specErrors(measuredSpec([5], { stack_max: 8 }), 240)).toEqual([])
  })
  it('엄격한 모순만 오류', () => {
    const row = (patch: Partial<ProfileRow>): ItemSpec =>
      spec({ profiles: [{ ...profileFrom(3), rows: [{ level: 1, lower_bead: 20, upper_bead: 220, stack_height: null, source: SOURCE_MEASURED, ...patch }] }] })
    expect(specErrors(spec({ stack_max: 21 }))).toHaveLength(1)
    expect(specErrors(spec({ weight_kg: -1 }))).toHaveLength(1)
    expect(specErrors(spec({ profiles: [profileFrom(3), profileFrom(3)] }))).toContain('스택 3: 중복')
    expect(specErrors(spec({ profiles: [{ ...profileFrom(3), count: 21 }] }))[0]).toContain('1..20')
    expect(specErrors(row({ level: 9 }))[0]).toContain('1..3')
    expect(specErrors(row({ lower_bead: 300 }))[0]).toContain('AbsUpperBead')
    expect(specErrors(row({ lower_bead: Number.NaN }))).toHaveLength(1)
    expect(specErrors(row({ upper_bead: 5000 }), 240)[0]).toContain('Height 240')
    expect(specErrors(spec({ compression: 240 }), 240)).toContain('Compression 은 Height 240 보다 작아야 합니다')
    expect(specErrors(spec({ pick_bead_offset: 300 }), 240)).toHaveLength(1)
    expect(specErrors(spec({ pick_bead_offset: 150 }), 240)).toHaveLength(0)
  })
  it('단이 올라가는데 절대 비드가 내려가면 경고만', () => {
    const p = profileFrom(3)
    p.rows[1].upper_bead = 1
    p.rows[1].lower_bead = 0.5
    const s = spec({ profiles: [p] })
    expect(specErrors(s)).toEqual([])
    expect(specWarnings(s).some((w) => w.includes('AbsUpperBead'))).toBe(true)
    expect(specWarnings(spec({ pick_bead_offset: 150 }), DIMS)[0]).toContain('PickBeadOffset')
    expect(specWarnings(spec({ pick_bead_offset: 150 }))).toHaveLength(0)
  })
  it('잰 TotalHeight 와 EachHeight × 단수가 어긋나면 경고', () => {
    const s = measuredSpec([3])
    expect(specWarnings(s, DIMS)).toEqual([])
    const bad = { ...s, profiles: [{ ...s.profiles[0], each_height: s.profiles[0].each_height! + 20 }] }
    expect(specWarnings(bad, DIMS)[0]).toContain('TotalHeight')
  })
})

describe('표시 도우미', () => {
  it('편차 톤 / 부호 / 입력 파싱', () => {
    expect(deviationTone(null)).toBeNull()
    expect(deviationTone(2.9)).toBe('ok')
    expect(deviationTone(-5)).toBe('warn')
    expect(deviationTone(7)).toBe('fault')
    expect(signed(1.24)).toBe('+1.2')
    expect(signed(-0.44)).toBe('-0.4')
    expect(signed(0)).toBe('0')
    expect(parseCell('')).toBeNull()
    expect(parseCell(' 12,5 ')).toBe(12.5)
    expect(parseCell('abc')).toBeUndefined()
  })
  it('옆모습 상자 — 잰 단과 지어낸 단을 구분한다', () => {
    const s = measuredSpec([3], { stack_max: 3 })
    const b = stackBoxes(buildGrid(DIMS, s, 3), 5)
    expect(b).toHaveLength(3)
    expect(b[0]).toMatchObject({ level: 1, above: 2, bottom: 0, measured: true, fromTable: true })
    // 잰 적 없는 크기는 곡선에서 지어낸다
    const g5 = buildGrid(DIMS, s, 5)
    expect(stackBoxes(g5, 5)[0].measured).toBe(false)
  })
  it('단수 Max 초과·재고 칸·시나리오 참조', () => {
    expect(overLimit(5, 4)).toBe(true)
    expect(overLimit(5, 0)).toBe(false)
    const st = (cell_id: number, item_code: number, count: number): StockEntry => ({
      cell_id,
      item_code,
      count,
      note: '',
      updated_at: '',
    })
    expect(stockCellsFor(1001, [st(3, 1001, 5), st(1, 1001, 2), st(2, 1002, 9), st(4, 1001, 0)], 4)).toEqual([
      { cell_id: 1, count: 2, over: false },
      { cell_id: 3, count: 5, over: true },
    ])
    const sc = {
      id: 's1',
      name: '입고',
      steps: [
        { item_code: 1002, label: 'a', type: 'PICK', target: { kind: 'cell', id: 1 } },
        { item_code: 1001, label: 'b', type: 'DROP', target: { kind: 'cell', id: 101 } },
        { item_code: 1001, label: '', type: 'PICK', target: null },
      ],
    } as unknown as Scenario
    expect(scenarioRefs([sc], 1001)).toEqual([
      { scenario_id: 's1', scenario: '입고', step: 2, label: 'b', type: 'DROP', target: 'Cell 101' },
      { scenario_id: 's1', scenario: '입고', step: 3, label: '', type: 'PICK', target: '' },
    ])
  })
  it('일괄 단수 Max — 값 자체만 검사한다', () => {
    const item = (code: number): Item => ({ code, spec: measuredSpec([8]) }) as Item
    expect(bulkStackMaxErrors([item(1)], 4)).toEqual([])
    expect(bulkStackMaxErrors([item(1)], 0)).toEqual([])
    expect(bulkStackMaxErrors([], 21)).toHaveLength(1)
  })
})
