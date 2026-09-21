import { describe, expect, it } from 'vitest'
import type { BeadSample } from '../types'
import { abovesOf, mm, sampleLabel, sampleRows, sampleTime, samplesStats, toRow } from './samples'

function sample(over: Partial<BeadSample> = {}): BeadSample {
  return {
    plc: 'GR2',
    seq: 41,
    code: 1001,
    at: '2026-09-18 10:11:12.345',
    status: 2,
    sku_status: 2,
    diag_flags: 0,
    layer_offset: 0,
    total_count: 3,
    each_height: 237.0,
    total_height: 711.0,
    levels: [],
    filled: 3,
    applied: true,
    reason: 'Above [0, 1, 2] 반영',
    recorded_at: '2026-09-18T10:11:13Z',
    valid: true,
    ...over,
  }
}

describe('SKU 표본 목록 모델', () => {
  it('표본 한 줄을 화면 모양으로 바꾼다', () => {
    const r = toRow(sample())
    expect(r.id).toBe('GR2#41')
    expect(r.time).toBe('2026-09-18 10:11:12')
    expect(r.totalCount).toBe(3)
    expect(r.eachHeight).toBe(237)
    expect(r.totalHeight).toBe(711)
    expect(r.state).toBe('applied')
    expect(r.label).toBe('반영됨')
    expect(r.canApply).toBe(true)
  })

  it('n 단 스택은 Above 0..n−1 을 채운다', () => {
    expect(toRow(sample({ total_count: 5 })).aboves).toEqual([0, 1, 2, 3, 4])
    expect(abovesOf(1)).toEqual([0])
    expect(abovesOf(0)).toEqual([])
    expect(abovesOf(-2)).toEqual([])
    expect(abovesOf(2.5)).toEqual([])
    expect(abovesOf(50)).toHaveLength(20)
  })

  it('거부된 표본은 사유를 그대로 보인다', () => {
    const r = toRow(sample({ applied: false, valid: false, reason: 'DiagFlags 0x21 — 스택 정렬 진단 경고' }))
    expect(r.state).toBe('rejected')
    expect(r.label).toBe('거부 — DiagFlags 0x21 — 스택 정렬 진단 경고')
    expect(r.canApply).toBe(false)
  })

  it('아직 안 넣은(스위치 끔) 표본은 대기 + Apply 가능', () => {
    const r = toRow(sample({ applied: false, valid: true, reason: 'AutoApplyMeasured 꺼짐 — 손으로 Apply 하세요' }))
    expect(r.state).toBe('pending')
    expect(r.canApply).toBe(true)
    expect(r.label).toContain('대기')
  })

  it('목록은 seq 내림차순이다', () => {
    const rows = sampleRows([sample({ seq: 3 }), sample({ seq: 9 }), sample({ seq: 7 })])
    expect(rows.map((r) => r.seq)).toEqual([9, 7, 3])
    expect(sampleRows(null)).toEqual([])
    expect(sampleRows(undefined)).toEqual([])
  })

  it('머리 개수는 문장이 아니라 숫자 넷', () => {
    expect(samplesStats([])).toEqual({ total: 0, applied: 0, rejected: 0, pending: 0 })
    const rows = sampleRows([
      sample({ seq: 1 }),
      sample({ seq: 2, applied: false, valid: false, reason: 'TotalCount 0' }),
      sample({ seq: 3, applied: false, valid: true, reason: '' }),
    ])
    expect(samplesStats(rows)).toEqual({ total: 3, applied: 1, rejected: 1, pending: 1 })
  })

  it('숫자·시각 서식', () => {
    expect(mm(237.04)).toBe('237')
    expect(mm(236.96)).toBe('237')
    expect(mm(0)).toBe('—')
    expect(mm(null)).toBe('—')
    expect(mm(Number.NaN)).toBe('—')
    expect(sampleTime('')).toBe('—')
    expect(sampleTime('2026-09-18T10:11:12.345Z')).toBe('2026-09-18 10:11:12')
    expect(sampleLabel('GR2', 0)).toBe('—')
    expect(sampleLabel('GR2', 12)).toBe('GR2#12')
    expect(sampleLabel('', 12)).toBe('?#12')
  })
})
