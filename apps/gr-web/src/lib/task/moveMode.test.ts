import { describe, expect, it } from 'vitest'
import type { Cell, Item, StockEntry } from '../types'
import { buildRequest, validateDraft, EMPTY_DRAFT } from './compose'
import { MOVE_TOP_Z, moveErrors, moveOf, moveParams, moveZ, sentItem } from './moveMode'
import { patch, planRows, retype, stepErrors, toRequest, toScenario, type PlanStep } from './plan'

const cell = (id: number, z = 1500): Cell => ({
  id,
  use: true,
  blend_use: false,
  section: 1,
  row: 1,
  col: 1,
  length: 1200,
  width: 1100,
  position: [id * 100, 3000, z],
  source: 'plc',
  dirty: false,
  updated_at: '',
})
const item = (code: number, height: number): Item =>
  ({
    code,
    name: '',
    count: 1,
    inner_diameter: 381,
    outer_diameter: 780,
    lower_bead_height: 0,
    upper_bead_height: 0,
    height,
    deflection_factor: 0,
  }) as Item
const ctx = {
  cells: [cell(101), cell(102)],
  stations: [],
  items: [item(1001, 240)],
  stockNow: new Map<number, StockEntry>([
    [101, { cell_id: 101, item_code: 1001, count: 3, note: '', updated_at: '' }],
  ]),
}
const step = (p: Partial<PlanStep> = {}): PlanStep => ({
  id: 's1',
  type: 'MOVE',
  target: { kind: 'cell', id: 101 },
  item_code: null,
  count: 1,
  note: '',
  ...p,
})

describe('MOVE 방식', () => {
  it('기본은 top — 요청에 늘 모드를 싣고, 감춘 품목은 보내지 않는다', () => {
    expect(moveOf({})).toEqual({ mode: 'top' })
    expect(moveParams({ mode: 'top', clearance: 100 })).toEqual({ move_mode: 'top' })
    expect(moveParams({ mode: 'stack', clearance: 100 })).toEqual({
      move_mode: 'stack',
      move_clearance: 100,
    })
    expect(moveParams({ mode: 'stack' })).toEqual({ move_mode: 'stack' })
    expect(sentItem({ type: 'MOVE', item_code: 1001 })).toBeNull()
    expect(sentItem({ type: 'MOVE', item_code: 1001, move: { mode: 'stack' } })).toBe(1001)
    expect(sentItem({ type: 'PICK', item_code: 1001 })).toBe(1001)
  })

  it('Z — top/avoid 9999, stack 은 바닥 + 스택 + 여유', () => {
    expect(moveZ({ mode: 'avoid' }, 1500, null)).toBe(MOVE_TOP_Z)
    expect(moveZ({ mode: 'stack' }, 1500, 720)).toBe(1500 + 720 + 500)
    expect(moveZ({ mode: 'stack', clearance: 100 }, 1500, 0)).toBe(1600)
    expect(moveZ({ mode: 'stack' }, 1500, null)).toBeNull()
    expect(moveErrors({ mode: 'stack', clearance: -1 })).toHaveLength(1)
    expect(moveErrors({ mode: 'top', clearance: -1 })).toEqual([])
  })

  it('계획 행 — 재고 품목으로 스택 높이를 잡고, 품목·재고 경고를 내지 않는다', () => {
    const rows = planRows(
      [
        step(),
        step({ id: 's2', move: { mode: 'stack', clearance: 100 } }),
        step({ id: 's3', target: { kind: 'cell', id: 102 }, move: { mode: 'stack' } }),
      ],
      ctx,
    )
    expect(rows[0].z).toBe(MOVE_TOP_Z)
    expect(rows[0].warnings).toEqual([])
    expect(rows[1].z).toBe(1500 + 3 * 240 + 100)
    expect(rows[2].z).toBe(1500 + 500)
    expect(rows[1].stockAfter).toBe(3)
  })

  it('toRequest / toScenario 가 모드를 싣고 로봇을 채운다', () => {
    const r = toRequest(step({ item_code: 1001 }), 2)
    expect(r.params).toEqual({ move_mode: 'top' })
    expect(r.item_code).toBeNull()
    expect(r.robot).toBe(2)
    const sc = toScenario([step({ move: { mode: 'avoid' } })], 'x')
    expect(sc.steps[0].params).toEqual({ move_mode: 'avoid' })
    expect(toRequest(step({ type: 'PICK', item_code: 1001 })).params).toEqual({})
  })

  it('종류 바꾸기 · 검증', () => {
    const p = step({ type: 'PICK', item_code: 1001 })
    expect(retype(p, 'MOVE')).toEqual({ type: 'MOVE', move: { mode: 'top' } })
    expect(retype(step({ move: { mode: 'avoid' } }), 'MOVE').move).toEqual({ mode: 'avoid' })
    const steps = patch([p], 's1', retype(p, 'MOVE'))
    expect(steps[0].type).toBe('MOVE')
    expect(stepErrors(step())).toEqual([])
    expect(stepErrors(step({ type: 'PICK' }))).toContain('품목을 고르세요')
    expect(stepErrors(step({ move: { mode: 'stack', clearance: 9000 } }))).toHaveLength(1)
    expect(
      stepErrors(step({ target: { kind: 'cell', id: 101 }, pallet: { seq: 0, level: 1 } })).length,
    ).toBe(2)
  })

  it('작성 카드 초안 — MOVE 는 품목 없이 유효, 모드가 params 로', () => {
    const d = { ...EMPTY_DRAFT, type: 'MOVE' as const, target: { kind: 'cell' as const, id: 101 } }
    expect(validateDraft(d)).toEqual([])
    expect(buildRequest(d).params).toEqual({ move_mode: 'top' })
    expect(buildRequest({ ...d, move: { mode: 'stack', clearance: 50 } }).params).toEqual({
      move_mode: 'stack',
      move_clearance: 50,
    })
  })
})
