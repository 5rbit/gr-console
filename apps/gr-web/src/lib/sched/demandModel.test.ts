import { describe, expect, it } from 'vitest'
import {
  cellSummary,
  demandFor,
  handAlertText,
  holdsText,
  stationRows,
  waitText,
} from './demandModel'
import type { DerivedRuleStatus, TargetReady } from '../sched'

const tr = (p: Partial<TargetReady>): TargetReady => ({
  kind: 'station',
  id: 2101,
  pick: { ok: false, why: 'Req 0' },
  drop: { ok: false, why: 'Req 0' },
  measure: { ok: false, why: null },
  item_code: 0,
  count: 0,
  room: null,
  holds: [],
  need_item: false,
  hints: [],
  since: null,
  role: 'pick',
  drop_mode: 'single',
  ...p,
})

const dr = (p: Partial<DerivedRuleStatus>): DerivedRuleStatus => ({
  rule_id: 'r',
  fires: false,
  inputs: '',
  terms: [],
  state: 'idle',
  reason: null,
  age_min: 0,
  generated: 0,
  last_generated_at: null,
  origin: 'policy',
  name: 'r',
  request_id: null,
  station: 2101,
  ...p,
})

describe('demandModel', () => {
  it('holds text', () => {
    expect(
      holdsText([
        { robot: 1, robot_name: 'GR1', op: 'PICK', state: 'submitted', order: 'TO-12', task: null },
        { robot: 2, robot_name: '', op: 'DROP', state: 'queued', order: null, task: null },
      ]),
    ).toBe('GR1 PICK TO-12 · R2 DROP')
    expect(holdsText(undefined)).toBe('')
  })

  it('demand picks the most actionable derived rule of the station', () => {
    const d = demandFor(
      2101,
      [
        dr({ rule_id: 'a', name: 'A', state: 'idle' }),
        dr({ rule_id: 'b', name: 'B', state: 'waiting', reason: '영역' }),
        dr({ rule_id: 'c', station: 2102, state: 'ready' }),
      ],
      true,
    )
    expect(d?.label).toBe('대기 +1')
    expect(d?.tone).toBe('warn')
    expect(d?.title).toContain('B: 대기 — 영역')
    expect(demandFor(2103, [], true)).toBeNull()
    expect(demandFor(2103, undefined, true)).toBeNull()
  })

  it('station rows: marks, wait, need item, sorted', () => {
    const now = Date.parse('2026-09-28T00:01:00Z')
    const rows = stationRows(
      [
        tr({ id: 2102, role: 'drop', drop: { ok: true, why: null } }),
        tr({
          id: 2101,
          pick: { ok: true, why: null },
          need_item: true,
          hints: [12],
          since: '2026-09-28T00:00:00Z',
        }),
        tr({ kind: 'cell', id: 401 }),
      ],
      [],
      false,
      now,
    )
    expect(rows.map((r) => r.id)).toEqual([2101, 2102])
    expect(rows[0].pick).toBe('need_item')
    expect(rows[0].drop).toBe('none')
    expect(rows[0].wait).toBe(60)
    expect(rows[0].hints).toEqual([12])
    expect(rows[1].drop).toBe('ready')
    expect(rows[1].pick).toBe('none')
  })

  it('tolerates missing fields', () => {
    const t = { kind: 'station', id: 1 } as unknown as TargetReady
    expect(() => stationRows([t], undefined, true)).not.toThrow()
    expect(cellSummary([{ kind: 'cell', id: 2 } as unknown as TargetReady]).total).toBe(1)
  })

  it('cell summary', () => {
    const s = cellSummary([
      tr({ kind: 'cell', id: 1, pick: { ok: true, why: null }, role: null }),
      tr({ kind: 'cell', id: 2, drop: { ok: true, why: null }, role: null }),
      tr({
        kind: 'cell',
        id: 3,
        role: null,
        holds: [
          { robot: 1, robot_name: 'GR1', op: 'DROP', state: 'queued', order: null, task: null },
        ],
      }),
      tr({ id: 2101 }),
    ])
    expect(s).toEqual({ total: 3, pick: 1, drop: 1, held: 1 })
  })

  it('hand alert text and wait text', () => {
    expect(
      handAlertText({
        robot: 2,
        robot_name: 'GR2',
        item_code: 1203,
        count: 1,
        order: null,
        why: '짝 DROP 없음',
      }),
    ).toBe('GR2 Hand 1203 × 1 — 짝 DROP 없음')
    expect(waitText(null)).toBe('')
    expect(waitText(42)).toBe('42')
    expect(waitText(185)).toBe('3:05')
    expect(waitText(3723)).toBe('1:02:03')
  })
})
