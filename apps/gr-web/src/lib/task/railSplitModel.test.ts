import { describe, expect, it } from 'vitest'
import {
  DEFAULT_SPLIT,
  MAX_RATIO,
  MIN_RATIO,
  clampRatio,
  parseRailTab,
  parseSplit,
  pxToRatio,
  ratioToPx,
  serializeSplit,
  splitBounds,
  tableForPick,
} from './railSplitModel'

describe('clampRatio', () => {
  it('범위 밖은 끝으로, 숫자가 아니면 기본값', () => {
    expect(clampRatio(0.5)).toBe(0.5)
    expect(clampRatio(0)).toBe(MIN_RATIO)
    expect(clampRatio(1)).toBe(MAX_RATIO)
    expect(clampRatio(Number.NaN)).toBe(DEFAULT_SPLIT.ratio)
    expect(clampRatio('0.4')).toBe(DEFAULT_SPLIT.ratio)
  })
})

describe('parseRailTab', () => {
  it('옛 탭 값은 그대로 살고 모르는 값은 기본으로', () => {
    expect(parseRailTab('layout')).toBe('layout')
    expect(parseRailTab('station')).toBe('station')
    expect(parseRailTab('split')).toBe('split')
    expect(parseRailTab('bogus')).toBe('split')
    expect(parseRailTab(null, 'layout')).toBe('layout')
  })
})

describe('parseSplit / serializeSplit', () => {
  it('비어 있거나 깨진 JSON 은 기본값', () => {
    expect(parseSplit(null)).toEqual(DEFAULT_SPLIT)
    expect(parseSplit('{oops')).toEqual(DEFAULT_SPLIT)
    expect(parseSplit('42')).toEqual(DEFAULT_SPLIT)
  })

  it('필드마다 따로 검증한다', () => {
    expect(parseSplit('{"ratio":0.95,"orient":"side","table":"nope"}')).toEqual({
      ratio: MAX_RATIO,
      orient: 'side',
      table: 'cell',
    })
    expect(parseSplit('{"ratio":0.3,"orient":"diag","table":"stock"}')).toEqual({
      ratio: 0.3,
      orient: 'stack',
      table: 'stock',
    })
  })

  it('왕복한다', () => {
    const s = { ratio: 0.42, orient: 'side' as const, table: 'station' as const }
    expect(parseSplit(serializeSplit(s))).toEqual(s)
  })
})

describe('splitBounds / ratioToPx / pxToRatio', () => {
  it('큰 컨테이너는 비율 한계, 작은 컨테이너는 최소 px, 아주 작으면 반반', () => {
    expect(splitBounds(1000)).toEqual({ min: 200, max: 800 })
    expect(splitBounds(600)).toEqual({ min: 160, max: 440 })
    expect(splitBounds(200)).toEqual({ min: 100, max: 100 })
    expect(splitBounds(0)).toEqual({ min: 0, max: 0 })
  })

  it('비율 ↔ px', () => {
    expect(ratioToPx(0.5, 1000)).toBe(500)
    expect(ratioToPx(0.2, 600)).toBe(160)
    expect(ratioToPx(0.5, 0)).toBe(0)
    expect(pxToRatio(300, 1000, 0.5)).toBe(0.3)
    expect(pxToRatio(50, 1000, 0.5)).toBe(MIN_RATIO)
    expect(pxToRatio(300, 0, 0.6)).toBe(0.6)
  })
})

describe('tableForPick', () => {
  it('누른 종류의 표로 옮기되 재고 표는 셀에서 지킨다', () => {
    expect(tableForPick('item', 'cell')).toBe('cell')
    expect(tableForPick('station', 'cell')).toBe('cell')
    expect(tableForPick('stock', 'cell')).toBe('stock')
    expect(tableForPick('cell', 'station')).toBe('station')
    expect(tableForPick('stock', 'station')).toBe('station')
  })
})
