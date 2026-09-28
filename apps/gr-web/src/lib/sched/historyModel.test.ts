import { describe, expect, it } from 'vitest'
import { fmtAt, fmtSeconds, kindLabel, kindTone, kpiTiles, normalizeKpi, sortLog } from './historyModel'

describe('history helpers', () => {
  it('formats seconds', () => {
    expect(fmtSeconds(null)).toBe('—')
    expect(fmtSeconds(undefined)).toBe('—')
    expect(fmtSeconds(12.34)).toBe('12.3')
    expect(fmtSeconds(123.4)).toBe('123')
  })

  it('formats times relative to today', () => {
    const now = new Date(2026, 8, 28, 15, 0, 0)
    expect(fmtAt(new Date(2026, 8, 28, 9, 5, 7).toISOString(), now)).toBe('09:05:07')
    expect(fmtAt(new Date(2026, 8, 27, 23, 1, 2).toISOString(), now)).toBe('09-27 23:01:02')
    expect(fmtAt('nope', now)).toBe('nope')
    expect(fmtAt(null, now)).toBe('—')
  })

  it('labels kinds', () => {
    expect(kindLabel('aborted')).toBe('중단')
    expect(kindLabel('other')).toBe('other')
    expect(kindTone('aborted')).toBe('fault')
    expect(kindTone('wait_alert')).toBe('warn')
    expect(kindTone('generated')).toBe('neutral')
  })

  it('normalizes partial KPI and builds tiles', () => {
    const k = normalizeKpi({ generated: 10, completed: 9, aborted: 2 }, 8)
    expect(k.hours).toBe(8)
    expect(k.stations).toEqual([])
    const t = kpiTiles(k)
    expect(t.map((x) => x.value)).toEqual(['10', '9', '2', '— / —', '0 / 0 / 0'])
    expect(t[2].tone).toBe('fault')
    expect(kpiTiles(normalizeKpi({ completed: 100, aborted: 1, cycle_avg_s: 42.5, cycle_p90_s: 120 }, 1))[2].tone).toBe(
      'warn',
    )
    expect(kpiTiles(null).every((x) => x.value === '—')).toBe(true)
  })

  it('sorts log newest first', () => {
    const rows = [1, 3, 2].map((id) => ({ id, at: '', kind: 'generated', key: '', robot: null, detail: '' }))
    expect(sortLog(rows).map((r) => r.id)).toEqual([3, 2, 1])
    expect(sortLog(null)).toEqual([])
  })
})
