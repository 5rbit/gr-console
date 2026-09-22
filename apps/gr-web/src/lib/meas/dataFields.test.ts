import { describe, expect, it } from 'vitest'
import { dataLines, skuDiagLabels } from './dataFields'

/** #610 (2026-09-22) — 5 단 명령에 6 단으로 센 SKU 기록. */
const SKU_610 = [
  10, 43.62, 236.36, 324.16, 516.9, 600.42, 799.03, 881.79, 1081.6, 1163.72, 1366.83, 1367.0,
  1367.86, 0, 0, 257, 0, 6, 1.03, 1405.22,
]

describe('dataLines', () => {
  it('SKU : 상태 이름 · 진단 비트 · 단수 정수 · 길이만 mm', () => {
    const l = dataLines(2, SKU_610)
    const by = (label: string) => l.find((x) => x.label === label)!
    expect(by('Status').text).toBe('SkuCmdCount — 명령 단수와 측정 단수 다름 (무효)')
    expect(by('Status').bad).toBe(true)
    expect(by('DiagFlags').text).toBe('X0 방향별 단수 불일치 · X8 명령 단수 불일치')
    expect(by('DiagFlags').unit).toBe('')
    expect(by('StackCount')).toMatchObject({ text: '6', unit: '단' })
    expect(by('Stack[1].LowerBead')).toMatchObject({ unit: 'mm' })
    // 잰 단수(6)를 넘는 0 칸은 뺀다
    expect(l.some((x) => x.label === 'Stack[7].LowerBead')).toBe(false)
  })

  it('Pick : Valid 는 예/아니오, FitError 는 코드', () => {
    const l = dataLines(4, [2, 1149.84, 271.1, 1, 0, 502.84, 7.26, 13.52, 690.84, 1161.55])
    expect(l[0].text).toBe('Done — 정상')
    expect(l[0].bad).toBe(false)
    expect(l.find((x) => x.label === 'Valid')!.text).toBe('예')
    expect(l.find((x) => x.label === 'FitError')).toMatchObject({ text: '0', bad: false })
  })

  it('Item : PLC 예비 칸(4..6, 9) 은 값이 0 이면 숨긴다', () => {
    const data = Array.from({ length: 20 }, () => 0)
    data[0] = 2
    data[8] = 1.234
    const l = dataLines(1, data)
    expect(l.map((x) => x.i)).not.toContain(4)
    expect(l.find((x) => x.label === 'Torq_Factor')!.text).toBe('1.234')
  })

  it('경고 비트는 (경고) 로 표시하고 무효로 치지 않는다', () => {
    expect(skuDiagLabels(0b100)).toEqual(['X2 단별 비드 높이 편차 (경고)'])
    expect(dataLines(2, [2, ...Array(14).fill(0), 4, 0, 0, 0, 0])[1]).toMatchObject({
      label: 'DiagFlags',
      bad: false,
    })
  })
})
