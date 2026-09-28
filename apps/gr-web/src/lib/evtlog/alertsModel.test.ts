import { describe, expect, it } from 'vitest'
import type { AlertRec, EvtCatalog } from './api'
import {
  EMPTY_RULE_FORM,
  alertBody,
  applyAck,
  formToBody,
  matchSummary,
  mergeAlerts,
  ruleFormWhy,
  ruleToForm,
} from './alertsModel'

const rec = (id: number, text = 'x'): AlertRec => ({
  id,
  ts: '',
  ts_ms: id,
  rule_id: 1,
  rule_name: 'EMS',
  acked: false,
  event: {
    id: 100 + id,
    plc: 'GRM',
    origin: 'plc',
    epoch: 1,
    seq: 1,
    ts: '',
    ts_ms: id,
    rx_ts: '',
    cat: 9,
    cat_name: 'SAFETY',
    lvl: 4,
    lvl_name: 'ERROR',
    src: 0,
    code: 905,
    name: 'SAFE_GRM_EMS',
    a: 1,
    b: 0,
    ctx: 0,
    detail: null,
    text,
  },
})

const catalog = {
  events: [
    {
      code: 604,
      name: 'ALM_TO_FAULT',
      cat: 6,
      cat_name: 'ALARM',
      lvl: 4,
      text: '',
      console: false,
    },
  ],
} as unknown as EvtCatalog

describe('rule form', () => {
  it('round trips a rule and drops empty conditions', () => {
    const body = formToBody({
      ...EMPTY_RULE_FORM,
      name: ' FAULT ',
      codes: 'alm_to_fault, 601',
      minLvl: 'ERROR',
      cooldown: '30',
    })
    expect(body).toEqual({
      name: 'FAULT',
      enabled: true,
      match: { codes: ['ALM_TO_FAULT', '601'], min_lvl: 'ERROR' },
      cooldown_s: 30,
    })
    const f = ruleToForm({ id: 3, updated_at: '', ...body })
    expect(formToBody(f)).toEqual(body)
  })
  it('refuses what the server would refuse', () => {
    expect(ruleFormWhy(EMPTY_RULE_FORM)).toBe('이름을 적으세요')
    expect(ruleFormWhy({ ...EMPTY_RULE_FORM, name: 'a', cooldown: '-1' })).toMatch('cooldown')
    expect(ruleFormWhy({ ...EMPTY_RULE_FORM, name: 'a', codes: 'NOPE' }, catalog)).toBe(
      '모르는 이벤트: NOPE',
    )
    expect(
      ruleFormWhy({ ...EMPTY_RULE_FORM, name: 'a', codes: 'ALM_TO_FAULT, 9' }, catalog),
    ).toBeUndefined()
  })
  it('carries ErrorList types, the transition and codes', () => {
    const body = formToBody({
      ...EMPTY_RULE_FORM,
      name: 'Alarm 발생',
      types: ['Alarm'],
      trans: 'raise',
      codes: 'f0202',
    })
    expect(body.match).toEqual({ types: ['Alarm'], trans: 'raise', codes: ['F0202'] })
    expect(formToBody(ruleToForm({ id: 1, updated_at: '', ...body }))).toEqual(body)
    expect(
      ruleFormWhy({ ...EMPTY_RULE_FORM, name: 'a', codes: 'F0202, W1101' }, catalog),
    ).toBeUndefined()
    expect(matchSummary(body.match)).toBe('Alarm · 발생 · F0202')
  })
  it('summarises the match', () => {
    expect(matchSummary({})).toBe('전체')
    expect(
      matchSummary({ plc: 'GR2', min_lvl: 'ERROR', codes: ['CMD_EMS'], text_contains: 'EMS' }),
    ).toBe('GR2 · ≥ ERROR · CMD_EMS · "EMS"')
  })
})

describe('alert list', () => {
  it('merges newest first without duplicates', () => {
    const m = mergeAlerts([rec(1), rec(3)], [rec(3), rec(4)])
    expect(m.map((r) => r.id)).toEqual([4, 3, 1])
    expect(mergeAlerts([rec(1), rec(2)], [rec(3)], 2).map((r) => r.id)).toEqual([3, 2])
  })
  it('ack removes ids, empty = all', () => {
    expect(applyAck([rec(1), rec(2)], [1]).map((r) => r.id)).toEqual([2])
    expect(applyAck([rec(1), rec(2)], [])).toEqual([])
  })
  it('notification body', () => {
    expect(alertBody(rec(1, 'EMS ON'))).toBe('GRM EMS ON')
    expect(alertBody({ ...rec(1), event: null })).toBe('EMS')
    const r = rec(2, 'Emergency Stop 발생')
    expect(alertBody({ ...r, event: r.event && { ...r.event, ecode: 'F0202' } })).toBe(
      'GRM F0202 Emergency Stop 발생',
    )
  })
})
