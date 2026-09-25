import { describe, expect, it } from 'vitest'
import { CHAN_MAX } from './traceModel'
import { GRIPPER_PRESET, applyPreset, tracePreset } from './tracePresets'

describe('trace presets', () => {
  it('gripper preset fits one session and has no duplicates', () => {
    expect(GRIPPER_PRESET.channels.length).toBeLessThanOrEqual(CHAN_MAX)
    expect(new Set(GRIPPER_PRESET.channels).size).toBe(GRIPPER_PRESET.channels.length)
    expect(GRIPPER_PRESET.channels.every((p) => p.startsWith('WEBMON.'))).toBe(true)
    expect(GRIPPER_PRESET.divider).toBe('1')
    expect(GRIPPER_PRESET.flushMs).toBe('20')
    expect(tracePreset('gripper')).toBe(GRIPPER_PRESET)
    expect(tracePreset('nope')).toBeNull()
  })

  it('filters by the catalog and reports what is missing', () => {
    const avail = GRIPPER_PRESET.channels.filter((p) => !p.includes('.Drive.'))
    const r = applyPreset(GRIPPER_PRESET, avail)
    expect(r.missing).toEqual(GRIPPER_PRESET.channels.filter((p) => p.includes('.Drive.')))
    expect(r.channels).toEqual(avail)
  })

  it('keeps everything when the catalog is unknown and respects the cap', () => {
    expect(applyPreset(GRIPPER_PRESET, null).channels).toEqual([...GRIPPER_PRESET.channels])
    expect(applyPreset(GRIPPER_PRESET, null, 3).channels).toHaveLength(3)
  })
})
