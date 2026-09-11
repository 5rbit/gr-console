import { describe, expect, it } from 'vitest'
import {
  cloneStep,
  duplicateScenario,
  issueMap,
  moveStep,
  newStep,
  paramCount,
  stepSummary,
  validateScenario,
} from './model'
import type { Cell, Item, ScenarioUpsert, Station } from '../types'

const cell = (id: number): Cell =>
  ({ id, use: true, blend_use: false, section: 1, row: 1, col: 1, length: 0, width: 0, position: [0, 0, 0], source: 'plc', dirty: false, updated_at: '' }) as Cell
const station = (id: number): Station => ({ id }) as Station
const item = (code: number): Item => ({ code, name: `i${code}` }) as Item
const reg = { cells: [cell(101), cell(102)], stations: [station(2101)], items: [item(1001)] }

function sample(): ScenarioUpsert {
  return {
    name: '삼단',
    description: '',
    repeat: 2,
    steps: [
      newStep({ id: 'a', label: 'pick', type: 'PICK', target: { kind: 'cell', id: 101 }, item_code: 1001, count: 2 }),
      newStep({ id: 'b', label: 'drop', type: 'DROP', target: { kind: 'station', id: 2101 }, item_code: 1001 }),
      newStep({ id: 'c', label: 'meas', type: 'MEASURE', target: { kind: 'cell', id: 102 }, item_code: 1001, params: { measure_item: true } }),
    ],
  }
}

describe('newStep / cloneStep / duplicateScenario', () => {
  it('기본값과 고유 id', () => {
    const a = newStep()
    const b = newStep()
    expect(a.id).not.toBe(b.id)
    expect(a).toMatchObject({ type: 'PICK', count: 1, wait_for: 'completed', on_failure: 'stop', params: {} })
  })
  it('복제는 id만 새로, 나머지는 깊은 복사', () => {
    const s = sample().steps[0]
    const c = cloneStep(s)
    expect(c.id).not.toBe(s.id)
    expect(c.target).toEqual(s.target)
    expect(c.target).not.toBe(s.target)
    expect(c.params).not.toBe(s.params)
  })
  it('시나리오 복제는 id를 버리고 이름에 (복사)', () => {
    const d = duplicateScenario({ ...sample(), id: 'x' })
    expect(d.id).toBeUndefined()
    expect(d.name).toBe('삼단 (복사)')
    expect(d.steps.map((s) => s.id)).not.toContain('a')
  })
})

describe('moveStep', () => {
  it('위/아래로 옮기고 끝에서는 그대로', () => {
    const steps = sample().steps
    expect(moveStep(steps, 1, -1).map((s) => s.id)).toEqual(['b', 'a', 'c'])
    expect(moveStep(steps, 1, 1).map((s) => s.id)).toEqual(['a', 'c', 'b'])
    expect(moveStep(steps, 0, -1)).toBe(steps)
    expect(moveStep(steps, 2, 1)).toBe(steps)
    expect(moveStep(steps, 5, 1)).toBe(steps)
    expect(steps.map((s) => s.id)).toEqual(['a', 'b', 'c']) // 원본 불변
  })
})

describe('validateScenario', () => {
  it('정상 문서는 이슈 없음', () => {
    expect(validateScenario(sample(), reg)).toEqual([])
  })
  it('필수 필드·레지스트리 참조·params 형식', () => {
    const s = sample()
    s.name = ' '
    s.steps[0].count = 0
    s.steps[0].item_code = null
    s.steps[1].target = null
    s.steps[2].target = { kind: 'cell', id: 999 }
    s.steps[2].item_code = 4242
    s.steps[2].params = { measure_item: 1 as unknown as boolean, bogus: 3 } as never
    const v = validateScenario(s, reg)
    const keys = v.map((i) => `${i.step_index}:${i.field}`)
    expect(keys).toContain('null:name')
    expect(keys).toContain('0:count')
    expect(keys).toContain('0:item_code')
    expect(keys).toContain('1:target')
    expect(keys).toContain('2:target')
    expect(keys).toContain('2:item_code')
    expect(keys.filter((k) => k === '2:params')).toHaveLength(2)
  })
  it('레지스트리가 비어 있으면 참조 검사는 건너뛴다', () => {
    const s = sample()
    s.steps[0].target = { kind: 'cell', id: 999 }
    expect(validateScenario(s)).toEqual([])
    expect(validateScenario(s, { cells: [] })).toEqual([])
  })
  it('UP은 대상·품목 없이 통과, MOVE는 대상만 필요', () => {
    const s: ScenarioUpsert = { name: 'x', description: '', repeat: 1, steps: [newStep({ type: 'UP' }), newStep({ type: 'MOVE' })] }
    const v = validateScenario(s, reg)
    expect(v).toEqual([{ step_index: 1, field: 'target', message: 'MOVE에는 대상이 필요' }])
  })
  it('issueMap은 (스텝,필드)로 모은다', () => {
    const m = issueMap([
      { step_index: 0, field: 'target', message: 'a' },
      { step_index: 0, field: 'target', message: 'b' },
      { step_index: null, field: 'name', message: 'c' },
    ])
    expect(m.get('0:target')).toBe('a; b')
    expect(m.get('-:name')).toBe('c')
  })
})

describe('summary helpers', () => {
  it('paramCount / stepSummary', () => {
    const s = sample().steps[0]
    expect(paramCount(s)).toBe(0)
    expect(stepSummary(s)).toBe('PICK 셀#101 ×2 · 1001')
    expect(stepSummary(newStep({ type: 'UP' }))).toBe('UP')
  })
})
