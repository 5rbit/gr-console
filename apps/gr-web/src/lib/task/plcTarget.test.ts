import { describe, expect, it } from 'vitest'
import {
  defaultTarget,
  effectiveTarget,
  readTarget,
  s7Plcs,
  targetLabel,
  targetOptions,
  type PlcRef,
} from './plcTarget'

const s7 = (id: string, name?: string, role?: 'gr' | 'grm') => ({ id, kind: 's7' as const, name, role })

describe('s7Plcs', () => {
  it('OPC UA 항목은 빼고 GR PLC 뒤에 GRM을 둔다', () => {
    const list = [
      s7('gr2_s7', 'GR2', 'gr'),
      s7('grm_s7', 'GRM', 'grm'),
      s7('gr1_s7', 'GR1', 'gr'),
      { id: 'grm_opcua', kind: 'opcua' as const },
    ]
    expect(s7Plcs(list)).toEqual([
      { name: 'GR2', role: 'gr' },
      { name: 'GR1', role: 'gr' },
      { name: 'GRM', role: 'grm' },
    ])
  })

  it('옛 백엔드(name·role 없음)는 id에서 이름과 역할을 짐작한다', () => {
    expect(s7Plcs([s7('grm_s7'), s7('gr2_s7')])).toEqual([
      { name: 'GR2', role: 'gr' },
      { name: 'GRM', role: 'grm' },
    ])
  })
})

const THREE: PlcRef[] = [
  { name: 'GR1', role: 'gr' },
  { name: 'GR2', role: 'gr' },
  { name: 'GRM', role: 'grm' },
]

describe('defaultTarget', () => {
  it('선택된 로봇의 PLC가 먼저다(대소문자 무시)', () => {
    expect(defaultTarget(THREE, 'gr2')).toBe('GR2')
  })
  it('로봇 PLC가 목록에 없으면 첫 GR PLC', () => {
    expect(defaultTarget(THREE, 'GR9')).toBe('GR1')
    expect(defaultTarget(THREE, null)).toBe('GR1')
  })
  it('GR PLC가 없으면 첫 PLC, 목록이 비면 null', () => {
    expect(defaultTarget([{ name: 'GRM', role: 'grm' }], null)).toBe('GRM')
    expect(defaultTarget([], 'GR2')).toBeNull()
  })
})

describe('effectiveTarget / readTarget', () => {
  it('고른 적 없으면 기본값, 목록에서 사라진 값도 기본값', () => {
    expect(effectiveTarget(null, THREE, 'GR2')).toBe('GR2')
    expect(effectiveTarget('GR7', THREE, 'GR2')).toBe('GR2')
    expect(effectiveTarget('grm', THREE, 'GR2')).toBe('GRM')
  })
  it('전체는 PLC가 둘 이상일 때만 남는다', () => {
    expect(effectiveTarget('all', THREE, 'GR2')).toBe('all')
    expect(effectiveTarget('all', [THREE[1]], 'GR2')).toBe('GR2')
  })
  it('읽기·차이는 전체·both면 기본 대상 한 대', () => {
    expect(readTarget('all', 'GR1')).toBe('GR1')
    expect(readTarget('both', 'GR2')).toBe('GR2')
    expect(readTarget('GRM', 'GR2')).toBe('GRM')
  })
})

describe('targetOptions / targetLabel', () => {
  it('PLC 하나씩 + 전체 쓰기', () => {
    expect(targetOptions(THREE).map((o) => o.value)).toEqual(['GR1', 'GR2', 'GRM', 'all'])
    expect(targetOptions([THREE[1]]).map((o) => o.value)).toEqual(['GR2'])
  })
  it('전체는 쓰는 순서대로 이름을 잇는다', () => {
    expect(targetLabel('all', THREE)).toBe('GR1 + GR2 + GRM')
    expect(targetLabel('both', THREE)).toBe('GR2 + GRM')
    expect(targetLabel('GR1', THREE)).toBe('GR1')
  })
})
