import { describe, expect, it } from 'vitest'
import { EMPTY_FILTER } from './evtFilterModel'
import { EMPTY_URL_STATE, fromQuery, mergeSearch, toQuery, type EvtUrlState } from './evtUrlModel'

describe('evt url', () => {
  it('the empty screen has no e.* keys', () => {
    expect(toQuery(EMPTY_URL_STATE)).toBe('')
    expect(fromQuery('?tab=events')).toBeNull()
  })

  it('round trips every part', () => {
    const st: EvtUrlState = {
      view: 'alarms',
      filter: {
        ...EMPTY_FILTER,
        plcs: ['GR2', 'GRM'],
        cats: ['ALARM'],
        types: ['Warn', 'Task'],
        minLvl: 3,
        code: 'ALM_RAISED, 602',
        ctx: '777',
        range: 'custom',
        from: 1000,
        to: 2000,
        q: 'W1101 스테이션',
      },
      stats: {
        period: '7d',
        from: null,
        to: null,
        plcs: ['GR1'],
        types: ['Alarm', 'Operator'],
        split: true,
      },
      swim: { station: 2101, slot: null, event: null, from: 5, to: 6 },
    }
    const q = toQuery(st)
    expect(q).toContain('e.view=alarms')
    expect(q).toContain('e.type=Warn%2CTask')
    expect(q).toContain('e.stype=Alarm%2COperator')
    expect(fromQuery(`?tab=events&${q}`)).toEqual(st)
  })

  it('custom times only travel with custom ranges', () => {
    const st = {
      ...EMPTY_URL_STATE,
      filter: { ...EMPTY_FILTER, range: '1h' as const, from: 5, to: 6 },
    }
    expect(toQuery(st)).toBe('e.range=1h')
    expect(fromQuery('?e.range=1h&e.from=5')?.filter.from).toBeNull()
  })

  it('bad values fall back instead of breaking the screen', () => {
    const s = fromQuery('?e.view=nope&e.range=2y&e.lvl=x&e.sp=1y&e.st=abc&e.slot=3')
    expect(s?.view).toBe('list')
    expect(s?.filter.range).toBe('all')
    expect(s?.filter.minLvl).toBeNull()
    expect(s?.stats.period).toBe('today')
    expect(s?.swim).toMatchObject({ station: null, slot: 3 })
  })

  it('an event entry keeps its id until a station is known', () => {
    const st = {
      ...EMPTY_URL_STATE,
      view: 'ilock' as const,
      swim: { station: null, slot: null, event: 77, from: 1, to: 2 },
    }
    expect(fromQuery(`?${toQuery(st)}`)?.swim).toEqual(st.swim)
  })

  it('merge keeps the other keys', () => {
    expect(mergeSearch('?tab=events&e.view=steps&x=1', 'e.view=alarms')).toBe(
      '?tab=events&x=1&e.view=alarms',
    )
    expect(mergeSearch('?e.view=steps', '')).toBe('')
  })
})
