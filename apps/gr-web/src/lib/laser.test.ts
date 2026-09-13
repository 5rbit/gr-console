import { describe, expect, it } from 'vitest'
import { activeDirs, diagFlagLabels, zcalPhase } from './laser'

describe('diagFlagLabels', () => {
  it('비트가 없으면 빈 목록', () => {
    expect(diagFlagLabels(0)).toEqual([])
  })
  it('방향별 무응답·불안정과 측정 플래그를 순서대로 푼다', () => {
    const flags = (1 << 0) | (1 << 6) | (1 << 8) | (1 << 11)
    expect(diagFlagLabels(flags)).toEqual(['L 무응답', 'R 불안정', '편차 폭 초과', '원 맞춤 실패'])
  })
  it('B 방향과 타이어 미검출', () => {
    expect(diagFlagLabels((1 << 3) | (1 << 7) | (1 << 10))).toEqual(['B 무응답', 'B 불안정', '타이어 미검출'])
  })
})

describe('zcalPhase', () => {
  it('Busy 가 Done/Error 보다 우선', () => {
    expect(zcalPhase({ Busy: true, Done: true, Error: true })).toBe('busy')
  })
  it('Error, Done, idle', () => {
    expect(zcalPhase({ Busy: false, Done: true, Error: true })).toBe('error')
    expect(zcalPhase({ Busy: false, Done: true, Error: false })).toBe('done')
    expect(zcalPhase({ Busy: false, Done: false, Error: false })).toBe('idle')
    expect(zcalPhase(null)).toBe('idle')
  })
})

describe('activeDirs', () => {
  it('B 샘플이 있으면 TBR 4방향', () => {
    expect(activeDirs([{ Samples: [10, 10, 10, 0] }, { Samples: [10, 10, 10, 3] }])).toBe(4)
  })
  it('없으면 PCR 3방향', () => {
    expect(activeDirs([{ Samples: [10, 10, 10, 0] }])).toBe(3)
    expect(activeDirs([])).toBe(3)
  })
})
