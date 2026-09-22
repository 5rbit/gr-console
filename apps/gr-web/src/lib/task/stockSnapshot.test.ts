import { describe, expect, it } from 'vitest'
import { CLEAR_WORD, clearArmed, reasonLabel, shortTime } from './stockSnapshot'

describe('clearArmed', () => {
  it('opens only for the word or the exact total', () => {
    expect(clearArmed(CLEAR_WORD, 12)).toBe(true)
    expect(clearArmed(' 12 ', 12)).toBe(true)
    expect(clearArmed('', 0)).toBe(false)
    expect(clearArmed('11', 12)).toBe(false)
    expect(clearArmed('비우', 12)).toBe(false)
    expect(clearArmed('0', 0)).toBe(true)
  })
})

describe('reasonLabel', () => {
  it('names each reason and points restore-before at its target', () => {
    expect(reasonLabel({ reason: 'clear', restored_from: null })).toBe('전체 비우기 전')
    expect(reasonLabel({ reason: 'import-replace', restored_from: null })).toBe('Excel 교체 전')
    expect(reasonLabel({ reason: 'restore-before', restored_from: 7 })).toBe('되돌리기 전 (#7)')
    expect(reasonLabel({ reason: 'future', restored_from: null })).toBe('future')
  })
  it('shortTime trims to month-day time', () => {
    expect(shortTime('2026-09-22T10:11:12.345+09:00')).toBe('09-22 10:11:12')
    expect(shortTime(null)).toBe('')
  })
})
