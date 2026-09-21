import { describe, expect, it } from 'vitest'
import type { CompressionSuggestion, ItemLevels, ItemSpec, Trend } from '../types'
import {
  beadsHelp,
  byCodeChip,
  byCodeRows,
  fullAt,
  levelsState,
  measuredChip,
  measuredRows,
  profileChip,
  shortAt,
  sourceChip,
} from './panelInfo'

const spec = (over: Partial<ItemSpec> = {}): ItemSpec => ({
  stack_max: 6,
  pallet_max: 0,
  weight_kg: null,
  profiles: [],
  bead_source: '',
  pick_bead_offset: null,
  compression: 1.3,
  compression_source: '',
  auto_apply_measured: null,
  ...over,
})

const measured: NonNullable<ItemLevels['measured']> = {
  plc: 'GR2',
  seq: 41,
  at: '2026-09-18 13:59:30.834',
  count: 3,
  each_height: 236.767,
  stack_height: 710.301,
}

const trend = (over: Partial<Trend> = {}): Trend => ({
  Count: 14,
  Last: 699.57,
  Avg: 699.9053,
  Ema: 699.8398,
  Min: 698.9,
  Max: 700.5,
  ...over,
})

const summary: NonNullable<ItemLevels['measured_summary']> = {
  plc: 'GR2',
  count: 15,
  last_time: '2026-09-18 13:59:30.834',
  upper_bead: trend(),
  tire_height: null,
  each_height: null,
  stack_height: null,
  pick_bead_pos: null,
}

describe('시각 줄이기', () => {
  it('칩에는 월-일 시:분만 남긴다', () => {
    expect(shortAt('2026-09-18 13:59:30.834')).toBe('09-18 13:59')
    expect(shortAt('2026-09-18T13:59:30Z')).toBe('09-18 13:59')
  })
  it('팝오버에는 초까지', () => {
    expect(fullAt('2026-09-18T13:59:30.834')).toBe('2026-09-18 13:59:30')
    expect(fullAt('')).toBe('—')
  })
})

describe('띠에 서는 칩', () => {
  it('최신 측정은 한 조각으로 줄고 전체 값은 title 로 간다', () => {
    const c = measuredChip(measured)
    expect(c).not.toBeNull()
    expect(c?.label).toBe('GR2')
    expect(c?.value).toBe('#41 · 09-18 13:59 · n=3')
    expect(c?.title).toContain('EachHeight 236.8')
    expect(c?.title).toContain('TotalHeight 710.3')
  })
  it('측정이 없으면 칩을 세우지 않는다', () => {
    expect(measuredChip(null)).toBeNull()
    expect(measuredRows(null)).toEqual([])
  })
  it('최신 측정 표는 라벨+값 다섯 줄', () => {
    expect(measuredRows(measured).map((r) => r.label)).toEqual([
      'Seq',
      'At',
      'TotalCount',
      'EachHeight',
      'TotalHeight',
    ])
  })
  it('ByCode 는 개수만 칩으로, 추세는 표로', () => {
    expect(byCodeChip(summary)).toEqual({
      label: 'ByCode',
      value: 'GR2 n=15',
      title: 'MEASLOG.ByCode GR2 · 표본 15 · 마지막 2026-09-18 13:59:30',
    })
    const rows = byCodeRows(summary)
    expect(rows[0]).toEqual({ label: 'UpperBead', value: '699.8 (avg 699.9, n=14)' })
    expect(rows[1].value).toBe('—')
    expect(byCodeChip(null)).toBeNull()
    expect(byCodeRows(null)).toEqual([])
  })
  it('잰 값과 환산값을 가른다', () => {
    expect(profileChip(3, true)).toMatchObject({ label: 'n=3', value: '잰 값' })
    expect(profileChip(6, false).value).toBe('환산')
    expect(profileChip(6, false).title).toContain('manual')
  })
  it('표본이 없으면 Sample 칩이 없다', () => {
    expect(sourceChip('GR2', 0)).toBeNull()
    expect(sourceChip('GR2', 41, '2026-09-18 13:59:30.834')).toMatchObject({
      label: 'Sample',
      value: 'GR2 #41',
    })
  })
})

describe('`?` 팝오버', () => {
  const suggestion: CompressionSuggestion = {
    value: 1.3,
    method: 'beads',
    from_beads: 1.3,
    from_each_height: 3.2,
    samples: 3,
    measured_count: 3,
  }
  it('공식에 지금 값이 박힌다', () => {
    const s = beadsHelp(spec({ pick_bead_offset: 25 }), suggestion)
    const pick = s.find((x) => x.title === 'PickZ')
    expect(pick?.body).toContain('PickBeadOffset 25')
    const comp = s.find((x) => x.title === 'Compression')
    expect(comp?.body).toContain('1.3 mm/개')
    expect(comp?.body).toContain('측정 제안 1.3 mm/개 (beads, n=3, 잰 스택 3단)')
  })
  it('제안이 없으면 제안 문장을 붙이지 않는다', () => {
    const comp = beadsHelp(spec(), null).find((x) => x.title === 'Compression')
    expect(comp?.body).not.toContain('측정 제안')
  })
  it('빈칸 PickBeadOffset 은 기본값으로 읽는다', () => {
    const pick = beadsHelp(spec(), null).find((x) => x.title === 'PickZ')
    expect(pick?.body).toContain('PickBeadOffset 30')
  })
})

describe('측정값 상태 한 줄', () => {
  it('값이 있으면 아무 줄도 없다', () => {
    expect(levelsState(null, false, true, 'GR2')).toBeNull()
  })
  it('없음 · 받는 중 · 실패를 가른다 — 띠에는 짧게, 사정은 title 로', () => {
    expect(levelsState(null, true, false, 'GR2')?.text).toBe('받는 중…')
    expect(levelsState(null, false, false, 'GR2')?.text).toBe('측정 없음')
    expect(levelsState(null, false, false, 'GR2')?.title).toContain('SKU 측정 기록이 없습니다')
    expect(levelsState('503', false, false, 'GR2')).toEqual({
      text: '측정값 실패',
      title: '측정값 조회 실패 — 503',
    })
  })
})
