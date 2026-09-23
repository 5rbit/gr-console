import { describe, expect, it } from 'vitest'
import {
  actionLabel,
  breakdownText,
  formatMap,
  newRule,
  parseMap,
  ruleState,
  triggerLabel,
} from './taskgen'

describe('taskgen helpers', () => {
  it('weight maps round-trip and reject bad parts', () => {
    const m = parseMap('2101:10, 401 : 2.5')
    expect(m).toEqual({ ok: { '2101': 10, '401': 2.5 } })
    expect(formatMap({ '2101': 10, '401': 2.5 })).toBe('401:2.5, 2101:10') // 숫자 키는 오름차순
    expect(parseMap('')).toEqual({ ok: {} })
    expect(parseMap('abc:1')).toMatchObject({ error: expect.stringContaining('abc:1') })
    expect(parseMap('2101')).toMatchObject({ error: expect.any(String) })
  })

  it('labels and score explanation', () => {
    expect(triggerLabel({ kind: 'station_req', station: 2101 })).toBe('Station 2101 Req + CVOK')
    expect(triggerLabel({ kind: 'station_req', station: 2101, require_cvok: false })).toBe(
      'Station 2101 Req',
    )
    expect(triggerLabel({ kind: 'cell_stock', cell: 401, min: 2, item: 2011 })).toBe(
      'Cell 401 ≥ 2 (Item 2011)',
    )
    expect(
      actionLabel({
        kind: 'transfer',
        from: { kind: 'cell', id: 401 },
        to: { kind: 'station', id: 2101 },
        item: 2011,
        count: 2,
      }),
    ).toBe('PICK Cell 401 → DROP Station 2101 · 2011 ×2')
    expect(
      breakdownText({
        score: 12.5,
        breakdown: [
          ['Rule.Priority', 3],
          ['Target 2101', 10],
          ['Distance', -0.5],
        ],
      }),
    ).toBe('Score 12.5 = Rule.Priority +3 Target 2101 +10 Distance -0.5')
  })

  it('new rules get unique ids and start disabled', () => {
    const a = newRule([])
    const b = newRule([a])
    expect(a.id).not.toBe(b.id)
    expect(a.enabled).toBe(false)
  })
  it('rule state chips say what the operator should do', () => {
    const base = {
      rule_id: 'r1',
      fires: true,
      inputs: 'STATION 2101: Req=1 CVOK=1',
      reason: null,
      age_min: 2.5,
      generated: 3,
      last_generated_at: '2026-09-23 10:00:00',
    } as const

    const off = ruleState({ ...base, state: 'ready' }, false)
    expect(off.label).toBe('만들 수 있음')
    expect(off.tone).toBe('warn')
    expect(off.title).toContain('자동 생성이 꺼져')
    expect(ruleState({ ...base, state: 'ready' }, true).label).toBe('생성')

    expect(ruleState({ ...base, fires: false, state: 'idle' }, true).tone).toBe('muted')
    expect(ruleState({ ...base, state: 'off', fires: false }, true).label).toBe('꺼짐')
    expect(ruleState({ ...base, state: 'queued' }, true).label).toBe('예정')
    expect(ruleState({ ...base, state: 'busy' }, true).label).toBe('진행 중')

    const waiting = ruleState({ ...base, state: 'waiting', reason: '영역 겹침 — GR2 대기' }, true)
    expect(waiting.tone).toBe('warn')
    expect(waiting.title).toContain('영역 겹침')
    expect(waiting.title).toContain('Req=1')
    expect(waiting.title).toContain('2.5분')
    expect(waiting.title).toContain('만든 3건')

    expect(ruleState(undefined, true).label).toBe('—')
  })
})
