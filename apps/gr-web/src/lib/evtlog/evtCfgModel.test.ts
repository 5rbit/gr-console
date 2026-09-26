import { describe, expect, it } from 'vitest'
import { cfgPatch, draftOf, hasBit, maxPerScanError, setBit } from './evtCfgModel'

describe('CatMask bits', () => {
  it('reads and toggles bits', () => {
    expect(hasBit(0b101, 0)).toBe(true)
    expect(hasBit(0b101, 1)).toBe(false)
    expect(setBit(0b101, 1, true)).toBe(0b111)
    expect(setBit(0b111, 0, false)).toBe(0b110)
    expect(setBit(0b110, 1, true)).toBe(0b110)
  })
  it('stays unsigned at bit 31 and ignores out-of-range bits', () => {
    const m = setBit(0, 31, true)
    expect(m).toBe(0x8000_0000)
    expect(hasBit(m, 31)).toBe(true)
    expect(setBit(m, 31, false)).toBe(0)
    expect(setBit(5, 40, true)).toBe(5)
    expect(hasBit(0xffff_ffff, 32)).toBe(false)
  })
})

describe('cfg patch', () => {
  const orig = { min_level: 2, cat_mask: 0xff, max_per_scan: 20 }

  it('is empty when nothing changed', () => {
    expect(cfgPatch(orig, draftOf(orig))).toEqual({})
  })
  it('carries only the changed fields', () => {
    expect(cfgPatch(orig, { minLevel: 3, catMask: 0xff, maxPerScan: '20' })).toEqual({
      min_level: 3,
    })
    expect(cfgPatch(orig, { minLevel: 2, catMask: 0x7f, maxPerScan: ' 50 ' })).toEqual({
      cat_mask: 0x7f,
      max_per_scan: 50,
    })
  })
  it('never sends an invalid MaxPerScan', () => {
    expect(cfgPatch(orig, { minLevel: 2, catMask: 0xff, maxPerScan: '0' })).toEqual({})
    expect(maxPerScanError('0')).toBeDefined()
    expect(maxPerScanError('201')).toBeDefined()
    expect(maxPerScanError('1.5')).toBeDefined()
    expect(maxPerScanError('200')).toBeUndefined()
    expect(maxPerScanError('1')).toBeUndefined()
  })
})
