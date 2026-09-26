import { describe, expect, it } from 'vitest'
import type { EventRow, EvtSource } from './api'
import {
  aroundWindow,
  cfgBlockedReason,
  countLevels,
  fmtEvtTime,
  fmtOffset,
  fmtSpan,
  lvlTone,
  mergeRows,
  msToStamp,
} from './evtRowsModel'

function row(id: number, ts_ms: number, lvl_name = 'INFO'): EventRow {
  return {
    id,
    plc: 'GR2',
    origin: 'plc',
    epoch: 1,
    seq: id,
    ts: '',
    ts_ms,
    rx_ts: '',
    cat: 0,
    cat_name: 'SYS',
    lvl: 2,
    lvl_name,
    src: 0,
    code: 1,
    name: null,
    a: 0,
    b: 0,
    ctx: 0,
    detail: null,
    text: '',
  }
}

describe('fmtEvtTime', () => {
  // TZ=Asia/Seoul — 로컬 날짜로 오늘을 가른다.
  const now = new Date(2026, 8, 27, 15, 0, 0).getTime()

  it('shows only the time for today', () => {
    expect(fmtEvtTime('2026-09-27 09:08:07.123', now)).toBe('09:08:07.123')
  })
  it('adds the date for other days', () => {
    expect(fmtEvtTime('2026-09-26 23:59:59.900', now)).toBe('09-26 23:59:59.900')
  })
  it('round-trips epoch ms through the server stamp shape', () => {
    const ms = new Date(2026, 8, 27, 9, 8, 7, 5).getTime()
    expect(msToStamp(ms)).toBe('2026-09-27 09:08:07.005')
    expect(fmtEvtTime(msToStamp(ms), now)).toBe('09:08:07.005')
  })
  it('pads missing milliseconds and keeps unknown input', () => {
    expect(fmtEvtTime('2026-09-27 09:08:07', now)).toBe('09:08:07.000')
    expect(fmtEvtTime('2026-09-27 09:08:07.5', now)).toBe('09:08:07.500')
    expect(fmtEvtTime('garbage', now)).toBe('garbage')
  })
})

describe('tones and counts', () => {
  it('colors only WARN/ERROR', () => {
    expect(lvlTone('ERROR')).toBe('fault')
    expect(lvlTone('WARN')).toBe('warn')
    expect(lvlTone('INFO')).toBe('neutral')
    expect(lvlTone('DEBUG')).toBe('neutral')
  })
  it('counts errors and warnings', () => {
    expect(
      countLevels([row(1, 1, 'ERROR'), row(2, 2, 'WARN'), row(3, 3, 'WARN'), row(4, 4)]),
    ).toEqual({
      error: 1,
      warn: 2,
    })
  })
  it('formats offsets', () => {
    expect(fmtOffset(1234)).toBe('+1.234')
    expect(fmtOffset(-50)).toBe('-0.050')
    expect(fmtOffset(0)).toBe('+0.000')
  })
  it('formats spans', () => {
    expect(fmtSpan(9999)).toBe('9999 ms')
    expect(fmtSpan(5_090_580)).toBe('5090.6 s')
  })
  it('builds a ±30 s window', () => {
    expect(aroundWindow(row(1, 100_000))).toEqual({ from: 70_000, to: 130_000 })
  })
})

describe('mergeRows', () => {
  it('dedupes by id and sorts newest first', () => {
    const page = [row(3, 300), row(2, 200), row(1, 100)]
    const live = [row(4, 400), row(3, 300)]
    const m = mergeRows(page, live)
    expect(m.rows.map((r) => r.id)).toEqual([4, 3, 2, 1])
    expect(m.added).toBe(1)
    expect(m.trimmed).toBe(false)
  })

  it('appends an older page after live rows', () => {
    const m = mergeRows([row(5, 500), row(4, 400)], [row(2, 200), row(1, 100), row(4, 400)])
    expect(m.rows.map((r) => r.id)).toEqual([5, 4, 2, 1])
    expect(m.added).toBe(2)
  })

  it('breaks time ties by id and trims to the cap from the old end', () => {
    const m = mergeRows([row(1, 100), row(2, 100)], [row(3, 50), row(4, 300)], 3)
    expect(m.rows.map((r) => r.id)).toEqual([4, 2, 1])
    expect(m.trimmed).toBe(true)
  })
})

describe('cfgBlockedReason', () => {
  const base: EvtSource = {
    plc: 'GR2',
    available: true,
    layout_ok: true,
    detail: null,
    header: { layout_sig: 1, total: 10, head: 10, boot_id: 3, dropped: 0 },
    stat: { run_us: 5, max_run_us: 12, max_per_scan_hit: 0 },
    cfg: { min_level: 2, cat_mask: 0xff, max_per_scan: 20 },
    collector: { epoch: 1, last_seq: 9, stored: 9, gaps: 0, last_read_at: null, error: null },
  }
  it('allows an available source with cfg', () => {
    expect(cfgBlockedReason(base)).toBeUndefined()
  })
  it('explains why not', () => {
    expect(cfgBlockedReason(null)).toBeDefined()
    expect(cfgBlockedReason({ ...base, available: false })).toContain('EVTLOG')
    expect(cfgBlockedReason({ ...base, layout_ok: false, detail: 'sig' })).toContain('sig')
    expect(cfgBlockedReason({ ...base, cfg: null })).toBeDefined()
  })
})
