import { describe, expect, it } from 'vitest'
import type { Defaults, TaskParams } from '../types'
import {
  EMPTY_DRAFT,
  applyDefaultsEdit,
  buildRequest,
  defaultsChanged,
  defaultsRows,
  effectiveParams,
  inheritedOf,
  inheritedText,
  isRedundant,
  pruneRedundant,
  redundantCells,
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
  grip_ref: 'mid',
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
    const r = buildRequest({
      ...EMPTY_DRAFT,
      count: 0,
      target: { kind: 'cell', id: 101 },
      item_code: 1,
    })
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
    ).toEqual(['수량은 1..255', 'GripHeight: 0 이상의 수여야 합니다'])
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
    expect(by['Item (Code × Count)']).toBe('1001 × 3')
    expect(by.Flags).toBe('LiftUpAfterComplete, Avoid')
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
    expect(ackText({ accepted: false, code: 419, reason: 'x', reject_bits: 4, at: '' })).toContain(
      '스테이션이 PLC에 등록되지 않음',
    )
  })
})

describe('situation layers', () => {
  const sit: Defaults = {
    ...defaults,
    situations: {
      measure_sku: { grip_height: 22 },
      multi_pick: { lift_up_partial: true, grip_height: 33 },
    },
  }
  it('stack between by[type][kind] and overrides, in server order', () => {
    expect(effectiveParams(sit, 'PICK', 'cell', {}, ['measure_sku'])?.grip_height).toBe(22)
    // 뒤 층이 이긴다(measure_sku → multi_pick), 덮어쓰기가 마지막
    expect(
      effectiveParams(sit, 'PICK', 'cell', {}, ['measure_sku', 'multi_pick'])?.grip_height,
    ).toBe(33)
    expect(
      effectiveParams(sit, 'PICK', 'cell', { grip_height: 5 }, ['multi_pick'])?.grip_height,
    ).toBe(5)
    expect(effectiveParams(sit, 'PICK', 'cell', {}, ['multi_pick'])?.lift_up_partial).toBe(true)
  })
  it('edits a situation column without touching base or by', () => {
    const next = applyDefaultsEdit(sit, 'lift_up_height', 'sit.measure_item', '1234')!
    expect(next.situations?.measure_item?.lift_up_height).toBe(1234)
    expect(next.base).toEqual(sit.base)
    expect(defaultsChanged(next, sit)).toBe(true)
    // 빈 값 = 상속(키 제거)
    const back = applyDefaultsEdit(next, 'lift_up_height', 'sit.measure_item', '')!
    expect(back.situations?.measure_item?.lift_up_height).toBeUndefined()
    const row = defaultsRows(next).find((r) => r.key === 'lift_up_height')!
    expect(row.values['sit.measure_item']).toBe(1234)
    expect(row.values['sit.multi_pick']).toBeUndefined()
  })
  it('only station PICK/DROP drafts carry multi_pick', () => {
    const d = { ...EMPTY_DRAFT, item_code: 1001, multi_pick: true }
    expect(
      buildRequest({ ...d, type: 'PICK', target: { kind: 'station', id: 2101 } }).multi_pick,
    ).toBe(true)
    expect(
      buildRequest({ ...d, type: 'PICK', target: { kind: 'cell', id: 101 } }).multi_pick,
    ).toBeUndefined()
  })
})

describe('defaults inheritance display and cleanup', () => {
  const d: Defaults = {
    ...defaults,
    by: {
      PICK: { cell: { grip_height: 50 }, station: { grip_height: 55 } },
      DROP: { cell: { grip_height: 40 }, station: {} },
    },
    situations: { multi_pick: { lift_up_partial: true, grip_height: 40 }, pallet_station: {} },
  }
  it('an empty cell shows what it inherits', () => {
    expect(inheritedOf(d, 'grip_height', 'base')).toEqual([])
    expect(inheritedOf(d, 'grip_height', 'DROP.station')).toEqual([{ from: 'base', value: 40 }])
    // Multi-Pick 은 스테이션 PICK/DROP 에 걸린다 — 두 값이 다르면 둘 다 보인다
    const mp = inheritedOf(d, 'grip_height', 'sit.multi_pick')
    expect(mp).toEqual([
      { from: 'PICK.station', value: 55 },
      { from: 'base', value: 40 },
    ])
    expect(inheritedText(mp)).toBe('55 / 40')
    expect(inheritedText(inheritedOf(d, 'grip_height', 'sit.measure_sku'))).toBe('40')
  })
  it('flags only overrides equal to every inherited value', () => {
    expect(isRedundant(d, 'grip_height', 'DROP.cell')).toBe(true)
    expect(isRedundant(d, 'grip_height', 'PICK.cell')).toBe(false)
    // PICK·Station 55 가 걸려 있어 Multi-Pick 40 은 의미가 있다
    expect(isRedundant(d, 'grip_height', 'sit.multi_pick')).toBe(false)
    const same: Defaults = { ...d, by: { ...d.by, PICK: { ...d.by.PICK, station: {} } } }
    expect(isRedundant(same, 'grip_height', 'sit.multi_pick')).toBe(true)
    // 같이 걸리는 아래 상황(팔렛)이 같은 키를 쥐면 지우면 결과가 바뀐다
    const pal: Defaults = {
      ...same,
      situations: { ...same.situations, pallet_station: { grip_height: 66 } },
    }
    expect(isRedundant(pal, 'grip_height', 'sit.multi_pick')).toBe(false)
    expect(redundantCells(d)).toEqual([{ key: 'grip_height', col: 'DROP.cell' }])
  })
  it('pruning never changes any final value', () => {
    const d2: Defaults = {
      ...d,
      by: { ...d.by, PICK: { ...d.by.PICK, station: { grip_height: 40 } } },
    }
    const { next, removed } = pruneRedundant(d2)
    // DROP·Cell 40, PICK·Station 40 → 그 뒤 Multi-Pick 40 도 같은 값이 된다
    expect(removed).toBe(3)
    expect(redundantCells(next)).toEqual([])
    const combos = [
      ['PICK', 'cell'],
      ['PICK', 'station'],
      ['DROP', 'cell'],
      ['DROP', 'station'],
    ] as const
    for (const [t, k] of combos) {
      // 상황은 스테이션에만 걸린다(서버 `situations_for`)
      const sitSets =
        k === 'station' ? [[], ['multi_pick'], ['pallet_station', 'multi_pick']] : [[]]
      for (const sits of sitSets as (readonly ('multi_pick' | 'pallet_station')[])[]) {
        expect(effectiveParams(next, t, k, {}, [...sits])).toEqual(
          effectiveParams(d2, t, k, {}, [...sits]),
        )
      }
    }
  })
})

describe('applyDefaultsEdit keeps layers the grid does not show', () => {
  it('does not drop by.MOVE when a PICK/DROP cell is edited', () => {
    const withMove = {
      ...defaults,
      by: { ...defaults.by, MOVE: { cell: { move_mode: 'top', move_clearance: 300 } } },
    } as unknown as Defaults
    const next = applyDefaultsEdit(withMove, 'grip_height', 'PICK.cell', '60')!
    expect((next.by as Record<string, unknown>).MOVE).toEqual({
      cell: { move_mode: 'top', move_clearance: 300 },
    })
    expect(next.by.PICK.cell.grip_height).toBe(60)
  })
})
