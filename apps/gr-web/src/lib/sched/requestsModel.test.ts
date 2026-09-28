import { describe, expect, it } from 'vitest'
import {
  EMPTY_DRAFT,
  draftError,
  draftToRequest,
  endpointKinds,
  shortTime,
  sortRequests,
  targetText,
} from './requestsModel'
import type { TransferRequest } from '../sched'

const req = (p: Partial<TransferRequest>): TransferRequest => ({
  id: 'RQ-1',
  seq: 1,
  source: 'user',
  ext_ref: null,
  kind: 'inbound',
  item_code: null,
  qty: 1,
  done: 0,
  active: 0,
  failed: 0,
  from: null,
  to: null,
  priority: 0,
  state: 'open',
  note: '',
  created_at: '',
  updated_at: '',
  ended_at: null,
  reason: null,
  ...p,
})

describe('requestsModel', () => {
  it('endpoint kinds follow the request kind', () => {
    expect(endpointKinds('inbound')).toEqual({ from: 'station', to: 'cell' })
    expect(endpointKinds('outbound')).toEqual({ from: 'cell', to: 'station' })
    expect(endpointKinds('move')).toEqual({ from: 'cell', to: 'cell' })
  })

  it('target text', () => {
    expect(targetText(null)).toBe('Auto')
    expect(targetText({ kind: 'station', id: 2101 })).toBe('Station 2101')
    expect(targetText({ kind: 'cell', id: 401 })).toBe('Cell 401')
  })

  it('sorts live first, then priority desc, then seq', () => {
    const rows = [
      req({ id: 'a', seq: 1, state: 'done', priority: 9 }),
      req({ id: 'b', seq: 2, priority: 1 }),
      req({ id: 'c', seq: 3, priority: 5, state: 'active' }),
      req({ id: 'd', seq: 4, priority: 1 }),
    ]
    expect(sortRequests(rows).map((r) => r.id)).toEqual(['c', 'b', 'd', 'a'])
  })

  it('validates the draft', () => {
    expect(draftError(EMPTY_DRAFT)).toBeNull()
    expect(draftError({ ...EMPTY_DRAFT, qty: '0' })).toMatch(/Qty/)
    expect(draftError({ ...EMPTY_DRAFT, qty: '1.5' })).toMatch(/Qty/)
    expect(draftError({ ...EMPTY_DRAFT, from: '-3' })).toMatch(/From/)
    expect(draftError({ ...EMPTY_DRAFT, to: '0' })).toMatch(/To/)
    expect(draftError({ ...EMPTY_DRAFT, item: 'x' })).toMatch(/ItemCode/)
    expect(draftError({ ...EMPTY_DRAFT, kind: 'move', from: '401' })).toMatch(/이동/)
    expect(draftError({ ...EMPTY_DRAFT, kind: 'move', from: '401', to: '401' })).toMatch(/같습니다/)
    expect(draftError({ ...EMPTY_DRAFT, kind: 'move', from: '401', to: '402' })).toBeNull()
    expect(draftError({ ...EMPTY_DRAFT, priority: '-2' })).toBeNull()
    expect(draftError({ ...EMPTY_DRAFT, priority: 'a' })).toMatch(/Priority/)
  })

  it('draft → request body', () => {
    expect(
      draftToRequest({
        ...EMPTY_DRAFT,
        kind: 'outbound',
        item: '12',
        qty: '3',
        to: '2101',
        note: ' x ',
      }),
    ).toEqual({
      kind: 'outbound',
      source: 'user',
      item_code: 12,
      qty: 3,
      from: null,
      to: { kind: 'station', id: 2101 },
      priority: 0,
      note: 'x',
    })
    expect(draftToRequest({ ...EMPTY_DRAFT, from: '2102' }).from).toEqual({
      kind: 'station',
      id: 2102,
    })
  })

  it('short time', () => {
    const now = new Date(2026, 8, 28, 12, 0, 0)
    expect(shortTime(new Date(2026, 8, 28, 9, 5, 7).toISOString(), now)).toBe('09:05:07')
    expect(shortTime(new Date(2026, 8, 27, 9, 5, 7).toISOString(), now)).toBe('09-27 09:05')
    expect(shortTime('bad', now)).toBe('bad')
    expect(shortTime(null, now)).toBe('')
  })
})
