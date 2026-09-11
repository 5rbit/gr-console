import { describe, expect, it } from 'vitest'
import { CSV_COLUMNS, fromCsv, fromJson, paramsFromKv, paramsToKv, parseCsv, toCsv, toJson } from './io'
import { newStep } from './model'
import type { ScenarioUpsert } from '../types'

function sample(): ScenarioUpsert {
  return {
    id: 'sc-1',
    name: '삼단 테스트',
    description: 'PICK/DROP/MEASURE',
    repeat: 2,
    steps: [
      newStep({ id: 'a', label: 'pick', type: 'PICK', target: { kind: 'cell', id: 101 }, item_code: 1001, count: 2, params: { grip_height: 55, avoid: true }, wait_after_ms: 500, on_failure: 'retry', note: 'a, "quoted"\nnote' }),
      newStep({ id: 'b', label: 'drop', type: 'DROP', target: { kind: 'station', id: 2101 }, item_code: 1001, on_failure: 'skip' }),
      newStep({ id: 'c', label: 'measure', type: 'MEASURE', target: { kind: 'cell', id: 103 }, item_code: 1002, params: { measure_item: true }, wait_for: 'accepted' }),
      newStep({ id: 'd', label: 'up', type: 'UP' }),
    ],
  }
}

/** id를 지운 비교용 사본(가져오기는 새 id를 준다). */
function strip(s: ScenarioUpsert) {
  return { ...s, id: undefined, steps: s.steps.map((st) => ({ ...st, id: undefined })) }
}

describe('JSON', () => {
  it('round trip (schema 1, id 유지)', () => {
    const s = sample()
    const text = toJson(s)
    const doc = JSON.parse(text) as { schema: number; id: string }
    expect(doc.schema).toBe(1)
    expect(doc.id).toBe('sc-1')
    const back = fromJson(text)
    expect(back.id).toBeUndefined()
    expect(back.steps.map((st) => st.id)).toEqual(['a', 'b', 'c', 'd'])
    expect(strip(back)).toEqual(strip(s))
  })
  it('모르는 필드는 버리고 빠진 필드는 기본값, BOM 허용', () => {
    const s = fromJson('﻿{"schema":1,"name":"x","extra":1,"steps":[{"type":"pick","target":{"kind":"cell","id":"5"},"item_code":7,"zzz":true}]}')
    expect(s.name).toBe('x')
    expect(s.repeat).toBe(1)
    expect(s.steps[0]).toMatchObject({ type: 'PICK', target: { kind: 'cell', id: 5 }, item_code: 7, count: 1, wait_for: 'completed', on_failure: 'stop', params: {} })
    expect(s.steps[0].id).toBeTruthy()
  })
  it('오류: schema 초과·객체 아님·잘못된 종류', () => {
    expect(() => fromJson('{"schema":9,"name":"x"}')).toThrow(/schema/)
    expect(() => fromJson('[1]')).toThrow()
    expect(() => fromJson('{')).toThrow(/JSON/)
    expect(() => fromJson('{"steps":[{"type":"FLY"}]}')).toThrow(/스텝 1/)
    expect(() => fromJson('{"steps":[{"type":"UP","target":{"kind":"moon","id":1}}]}')).toThrow(/moon/)
  })
})

describe('params k=v', () => {
  it('encode는 키 정렬, decode는 타입 복원', () => {
    expect(paramsToKv({ measure_item: true, grip_height: 12 })).toBe('grip_height=12;measure_item=true')
    expect(paramsFromKv('grip_height=12; measure_item=TRUE ;x=abc;f=1.5')).toEqual({ grip_height: 12, measure_item: true, x: 'abc', f: 1.5 })
    expect(paramsFromKv('')).toEqual({})
    expect(() => paramsFromKv('novalue')).toThrow()
  })
})

describe('CSV', () => {
  it('헤더는 백엔드 열 순서 그대로', () => {
    expect(toCsv({ name: 'e', description: '', repeat: 1, steps: [] })).toBe(CSV_COLUMNS.join(',') + '\n')
  })
  it('round trip (따옴표·줄바꿈·빈 대상 포함)', () => {
    const s = sample()
    const text = toCsv(s)
    const back = fromCsv(text, s.name)
    expect(strip(back).steps).toEqual(strip(s).steps)
    expect(back.name).toBe(s.name)
  })
  it('열 순서 자유·BOM·CRLF·빈 줄', () => {
    const s = fromCsv('﻿type,target_id,target_kind,item_code,params\r\nPICK,12,cell,1001,avoid=true\r\n\r\n', 'n')
    expect(s.steps).toHaveLength(1)
    expect(s.steps[0]).toMatchObject({ type: 'PICK', target: { kind: 'cell', id: 12 }, item_code: 1001, params: { avoid: true } })
  })
  it('오류 행 번호를 말한다', () => {
    expect(() => fromCsv('type\nPICK\nFLY\n', 'n')).toThrow(/3행/)
    expect(() => fromCsv('type,target_kind,target_id\nPICK,cell,abc\n', 'n')).toThrow(/2행/)
    expect(() => fromCsv('label\nx\n', 'n')).toThrow(/type/)
    expect(() => fromCsv('type,params\nUP,broken\n', 'n')).toThrow(/2행/)
  })
  it('parseCsv — 겹따옴표와 따옴표 안 콤마', () => {
    expect(parseCsv('a,"b, ""c""",d\n')).toEqual([['a', 'b, "c"', 'd']])
  })
})
