import { describe, expect, it } from 'vitest'
import type { Defaults, TaskParams } from '../types'
import {
  EMPTY_DRAFT,
  applyDefaultsEdit,
  buildRequest,
  defaultsChanged,
  defaultsRows,
  effectiveParams,
  previewFields,
  targetKindsFor,
  validateDraft,
} from './compose'
import { ackText, rejectBitsText, rejectText } from '../gr/rejectCodes'

const base: TaskParams = {
  lift_up_height: 2500,
  grip_height: 40,
  pre_grip_delta: 10,
  grip_back_delta: 5,
  blend_up_distance: 300,
  blend_down_distance: 300,
  lift_up_creep_distance: 30,
  lift_down_creep_distance: 30,
  lift_up_after_complete: true,
  lift_up_partial: false,
  measure_floor: false,
  measure_item: false,
  measure_sku: false,
  adjust_center: false,
  find_station_item: false,
  avoid: false,
  outbound: false,
  use_drag_out: false,
  drag_out_height: 0,
  drag_out_dist: 0,
  drag_out_dir: 0,
  use_drag_in: false,
  drag_in_height: 0,
  drag_in_dist: 0,
  drag_in_dir: 0,
}

const defaults: Defaults = {
  version: 1,
  updated_at: '',
  base,
  by: {
    PICK: { cell: { grip_height: 50, avoid: true }, station: { find_station_item: true } },
    DROP: { cell: {}, station: {} },
  },
}

describe('effectiveParams precedence', () => {
  it('base ← by[type][kind] ← overrides', () => {
    expect(effectiveParams(defaults, 'DROP', 'cell', {})?.grip_height).toBe(40)
    expect(effectiveParams(defaults, 'PICK', 'cell', {})?.grip_height).toBe(50)
    expect(effectiveParams(defaults, 'PICK', 'cell', {})?.avoid).toBe(true)
    expect(effectiveParams(defaults, 'PICK', 'station', {})?.find_station_item).toBe(true)
    expect(effectiveParams(defaults, 'PICK', 'station', {})?.grip_height).toBe(40)
    const p = effectiveParams(defaults, 'PICK', 'cell', { grip_height: 60, avoid: undefined })
    expect(p?.grip_height).toBe(60)
    expect(p?.avoid).toBe(true)
  })
  it('MEASURE/UP fall through to base when by has no entry', () => {
    expect(effectiveParams(defaults, 'MEASURE', 'cell', {})?.grip_height).toBe(40)
    expect(effectiveParams(null, 'PICK', 'cell', {})).toBeNull()
  })
})

describe('draft → request + validation', () => {
  it('builds a request with sane count and no overrides leaking', () => {
    const r = buildRequest({ ...EMPTY_DRAFT, count: 0, target: { kind: 'cell', id: 101 }, item_code: 1 })
    expect(r.count).toBe(1)
    expect(r.type).toBe('PICK')
    expect(r.position_override).toBeNull()
    expect(r.source).toBeNull()
    expect(r.params).toEqual({})
  })
  it('validates target / item / count / params by type', () => {
    expect(validateDraft({ ...EMPTY_DRAFT, type: 'UP' })).toEqual([])
    expect(validateDraft({ ...EMPTY_DRAFT, type: 'PICK' })).toEqual([
      '대상(셀/스테이션)을 고르세요',
      '품목을 고르세요',
    ])
    expect(
      validateDraft({
        ...EMPTY_DRAFT,
        type: 'MEASURE',
        target: { kind: 'station', id: 2101 },
        item_code: 1,
      }),
    ).toEqual(['MEASURE에는 스테이션을 쓸 수 없습니다'])
    expect(
      validateDraft({
        ...EMPTY_DRAFT,
        type: 'MOVE',
        target: { kind: 'cell', id: 1 },
        count: 999,
        params: { grip_height: -1, avoid: true },
      }),
    ).toEqual(['수량은 1..255', '그립 높이: 0 이상의 수여야 합니다'])
    expect(targetKindsFor('MEASURE')).toEqual(['cell'])
    expect(targetKindsFor('DROP')).toEqual(['cell', 'station'])
  })
})

describe('preview fields', () => {
  it('formats position and flags', () => {
    const f = previewFields({
      task: {
        WorkId: 0,
        TaskId: 0,
        TaskType: 0x41,
        Position: [12000, 3000, 1980.04, 351],
        Item: {
          Code: 1001,
          Count: 3,
          InnerDiameter: 381,
          OuterDiameter: 780,
          LowerBidHeight: 20,
          UpperBidHeight: 220,
          Height: 240,
          DeflectionFactor: 0,
        },
        Cell: {
          Use: true,
          BlendUse: false,
          Id: 101,
          Section: 2,
          Row: 1,
          Col: 1,
          Lenth: 0,
          Width: 0,
          Position: [12000, 3000, 1500],
        },
        BlendUpDistance: 300,
        BlendDownDistance: 300,
        DragOutHeight: 0,
        DragOutDist: 0,
        DragOutDir: 0,
        DragInHeight: 0,
        DragInDist: 0,
        DragInDir: 0,
        LiftUpCreepDistance: 30,
        LiftDownCreepDistance: 30,
        LiftUpHeight: 2500,
        PreGripDelta: 10,
        GripBackDelta: 5,
        GripHeight: 50,
        UseDragOut: false,
        UseDragIn: false,
        LiftUpAfterComplete: true,
        LiftUpPartial: false,
        MeasureFloor: false,
        MeasureItem: false,
        MeasureSku: false,
        AdjustCenter: false,
        FindStationItem: false,
        Avoid: true,
        Outbound: false,
      },
      params: { ...base, grip_height: 50, avoid: true },
      warnings: [],
    })
    const by = Object.fromEntries(f.map((x) => [x.label, x.value]))
    expect(by.Z).toBe('1980')
    expect(by.G).toBe('351')
    expect(by.TaskType).toBe('0x41')
    expect(by['품목']).toBe('1001 × 3')
    expect(by['켜진 플래그']).toBe('완료 후 상승, 회피')
    expect(previewFields(null)).toEqual([])
  })
})

describe('defaults grid model', () => {
  it('rows carry base and partial columns; edits update / clear keys', () => {
    const rows = defaultsRows(defaults)
    const grip = rows.find((r) => r.key === 'grip_height')!
    expect(grip.values.base).toBe(40)
    expect(grip.values['PICK.cell']).toBe(50)
    expect(grip.values['PICK.station']).toBeUndefined()
    expect(rows.find((r) => r.key === 'avoid')!.bool).toBe(true)

    const a = applyDefaultsEdit(defaults, 'grip_height', 'PICK.station', '70')!
    expect(a.by.PICK.station.grip_height).toBe(70)
    expect(defaults.by.PICK.station.grip_height).toBeUndefined() // 원본 불변
    const b = applyDefaultsEdit(a, 'grip_height', 'PICK.cell', '')!
    expect('grip_height' in b.by.PICK.cell).toBe(false)
    const c = applyDefaultsEdit(defaults, 'avoid', 'base', 'true')!
    expect(c.base.avoid).toBe(true)
    expect(applyDefaultsEdit(defaults, 'avoid', 'base', 'maybe')).toBeNull()
    expect(applyDefaultsEdit(defaults, 'grip_height', 'base', '')).toBeNull()
    expect(applyDefaultsEdit(defaults, 'grip_height', 'base', '-5')).toBeNull()
    expect(defaultsChanged(defaults, a)).toBe(true)
    expect(defaultsChanged(defaults, { ...defaults, version: 9 })).toBe(false)
  })
})

describe('reject codes', () => {
  it('maps codes and bits', () => {
    expect(rejectText(429)).toContain('셀이 PLC에 등록되지 않음')
    expect(rejectText(12345)).toContain('12345')
    expect(rejectBitsText(0x84)).toEqual(['작업 데이터 무효', '로봇 오프라인'])
    expect(ackText({ accepted: true, code: 1, reason: '', reject_bits: 0, at: '' })).toBe(
      '수락됨 (코드 1)',
    )
    expect(
      ackText({ accepted: false, code: 419, reason: 'x', reject_bits: 4, at: '' }),
    ).toContain('스테이션이 PLC에 등록되지 않음')
  })
})
