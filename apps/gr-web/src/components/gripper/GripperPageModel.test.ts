import { describe, expect, it } from 'vitest'
import type { GripperBins, GripperTune } from '../../lib/types'
import {
  anyValid,
  binCenters,
  codeName,
  codeTone,
  gRule,
  learnDisabledReason,
  limitEchoMatches,
  ownerName,
  mechSeries,
  paraRows,
  parseScale,
  scaleRows,
} from './GripperPageModel'

const bins: GripperBins = { range_min: 295, range_max: 630, count: 34, width: (630 - 295) / 34 }

function tune(valid: [boolean, boolean]): GripperTune {
  return {
    Valid: valid,
    LearnedSpd: [3, 1],
    Mech: [Array.from({ length: 34 }, (_, k) => 8 + k * 0.2), Array.from({ length: 34 }, () => 6)],
    Accel: [3.5, 2],
    TorqSign: 1,
    ScaleByInch: Array.from({ length: 13 }, () => 100),
    DriftCount: 0,
    LearnDone: true,
    LearnError: 0,
    LearnErrorPos: 0,
  }
}

describe('gripper model', () => {
  it('names codes and keeps unknown values as numbers', () => {
    expect(codeName(50)).toBe('HOLDING')
    expect(codeName(65)).toBe('LEARNING')
    expect(codeName(77)).toBe('77')
    expect(codeName(null)).toBe('-')
    expect(codeTone(50)).toBe('ok')
    expect(codeTone(95)).toBe('fault')
    expect(codeTone(10)).toBe('neutral')
  })

  it('owner names and torque-limit echo', () => {
    expect(ownerName(5)).toBe('LEARN')
    expect(ownerName(9)).toBe('9')
    expect(limitEchoMatches(13.0, 13.04)).toBe(true)
    expect(limitEchoMatches(13.0, 12.0)).toBe(false)
    expect(limitEchoMatches(13.0, undefined)).toBeNull()
  })

  it('bin centers span RangeMin..RangeMax', () => {
    const xs = binCenters(bins)
    expect(xs).toHaveLength(34)
    expect(xs[0]).toBeCloseTo(295 + bins.width / 2, 6)
    expect(xs[33]).toBeCloseTo(630 - bins.width / 2, 6)
  })

  it('only learned curves become series', () => {
    expect(mechSeries(tune([true, false]), bins)).toHaveLength(1)
    expect(mechSeries(tune([true, true]), bins).map((s) => s.tone)).toEqual([0, 2])
    expect(mechSeries(null, bins)).toEqual([])
    expect(anyValid(tune([false, false]))).toBe(false)
    expect(anyValid(tune([false, true]))).toBe(true)
  })

  it('G marker only with a finite position', () => {
    expect(gRule(412.5)).toEqual([{ axis: 'x', value: 412.5, label: 'G', tone: 4 }])
    expect(gRule(null)).toEqual([])
    expect(gRule(Number.NaN)).toEqual([])
  })

  it('LEARN gate mirrors the PLC conditions', () => {
    const live = { ItemPresent: false, ItemDetect: false, Busy: false, Code: 10 }
    expect(learnDisabledReason('MANUAL', live, true)).toBeUndefined()
    expect(learnDisabledReason('MAINT', live, true)).toBeUndefined()
    expect(learnDisabledReason('AUTO', live, true)).toContain('AUTO')
    expect(learnDisabledReason('MANUAL', { ...live, ItemPresent: true }, true)).toContain('화물')
    expect(learnDisabledReason('MANUAL', { ...live, Code: 65 }, true)).toBe('학습 중')
    expect(learnDisabledReason('MANUAL', live, false)).toContain('명령 경로')
    // 모드를 아직 모르면 막지 않는다(백엔드가 스냅샷으로 판정한다) · 데모는 모드를 안 본다
    expect(learnDisabledReason(null, live, true)).toBeUndefined()
    expect(learnDisabledReason('AUTO', live, true, true)).toBeUndefined()
  })

  it('scale rows are 12..24 inch and parse back', () => {
    const rows = scaleRows(tune([true, true]))
    expect(rows[0]).toEqual({ inch: 12, value: 100 })
    expect(rows[12]).toEqual({ inch: 24, value: 100 })
    expect(parseScale(['', '105', '96.5'])).toEqual({ values: [0, 105, 96.5] })
    expect(parseScale(['1', 'x'])).toEqual({ error: '13 인치 값이 0~1000 % 사이의 숫자가 아닙니다' })
  })

  it('para rows keep p-number order and null for missing', () => {
    const rows = paraRows({ G_TorqRefDia: 508, G_HoldFactor: 100 })
    expect(rows[0]).toEqual({ name: 'G_TorqRefDia', param: 37, group: 'Machine', value: 508 })
    expect(rows.find((r) => r.name === 'G_OpenTorq_Nm')?.value).toBeNull()
    expect(rows).toHaveLength(43)
    expect(rows[23]).toEqual({ name: 'G_Inch1_Min', param: 1040, group: 'Timeout', value: null })
    expect(rows[42]).toEqual({ name: 'G_Inch5_MeasPct', param: 1059, group: 'Timeout', value: null })
  })
})
