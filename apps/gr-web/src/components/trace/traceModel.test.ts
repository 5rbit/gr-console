import { describe, expect, it } from 'vitest'
import {
  CHAN_MAX,
  FLUSH_MIN_MS,
  RowArray,
  TraceRing,
  addChannel,
  fmtT,
  fmtValue,
  groupByDb,
  health,
  nearestIndex,
  parseSelection,
  removeChannel,
  shortPath,
  startDisabledReason,
  stripCycle,
} from './traceModel'
import type { TraceChannel } from '../../lib/trace/api'

/** `[t_ms, v0, v1]` 행 n 개 — t 는 ms 간격 */
function rows(n: number, from = 0, step = 1): number[][] {
  return Array.from({ length: n }, (_, i) => [from + i * step, i, -i])
}

describe('TraceRing', () => {
  it('행 수 상한을 넘으면 가장 오래된 행부터 버린다', () => {
    const r = new TraceRing(4, 0)
    r.push('s1', rows(6))
    expect(r.length).toBe(4)
    expect(r.time(0)).toBe(2)
    expect(r.time(3)).toBe(5)
    expect(r.value(0, 0)).toBe(2)
  })

  it('시간 창보다 오래된 행을 떨군다', () => {
    const r = new TraceRing(1000, 30)
    r.push('s1', rows(101)) // 0..100 ms
    expect(r.length).toBe(31)
    const [t0, t1] = r.bounds()
    expect(t1).toBe(100)
    expect(t0).toBe(70)
  })

  it('링을 한 바퀴 돌아도 push 순서대로 읽힌다', () => {
    const r = new TraceRing(4, 0)
    r.push('s1', rows(3))
    r.push('s1', rows(3, 3))
    const s = r.series(0)
    expect(s.t).toEqual([2, 3, 4, 5])
    expect(s.v).toEqual([2, 0, 1, 2])
  })

  it('세션 id 가 바뀌면 통째로 비운다', () => {
    const r = new TraceRing(100, 0)
    r.push('s1', rows(5))
    r.push('s2', rows(2))
    expect(r.length).toBe(2)
    expect(r.sessionId).toBe('s2')
  })

  it('채널 수가 달라진 청크도 앞 파형에 잇지 않는다', () => {
    const r = new TraceRing(100, 0)
    r.push('s1', rows(5))
    expect(r.chan).toBe(2)
    r.push('s1', [[10, 1, 2, 3]])
    expect(r.chan).toBe(3)
    expect(r.length).toBe(1)
  })

  it('빈 링은 0 길이·0 범위이고 마지막 행이 없다', () => {
    const r = new TraceRing(8, 0)
    expect(r.length).toBe(0)
    expect(r.bounds()).toEqual([0, 0])
    expect(r.lastRow()).toBeNull()
    expect(r.indexAtTime(5)).toBe(-1)
    expect(r.series(0)).toEqual({ t: [], v: [] })
  })

  it('마지막 행은 [t, 값…] 모양이다', () => {
    const r = new TraceRing(8, 0)
    r.push('s1', rows(3))
    expect(r.lastRow()).toEqual([2, 2, -2])
  })

  it('가장 가까운 행을 시각으로 찾는다', () => {
    const r = new TraceRing(100, 0)
    r.push('s1', rows(5, 0, 10)) // 0,10,20,30,40
    expect(r.indexAtTime(0)).toBe(0)
    expect(r.indexAtTime(14)).toBe(1)
    expect(r.indexAtTime(16)).toBe(2)
    expect(r.indexAtTime(999)).toBe(4)
    expect(r.indexAtTime(-5)).toBe(0)
  })
})

describe('RowArray', () => {
  it('저장 행에서 cycle 칸을 떼고 읽는다', () => {
    const stored = [
      [100, 0, 1.5, 2],
      [101, 5, 2.5, 3],
    ]
    const src = new RowArray(stripCycle(stored), 2)
    expect(src.length).toBe(2)
    expect(src.chan).toBe(2)
    expect(src.time(1)).toBe(5)
    expect(src.value(1, 0)).toBe(2.5)
    expect(src.bounds()).toEqual([0, 5])
    expect(src.lastRow()).toEqual([5, 2.5, 3])
    expect(src.series(1)).toEqual({ t: [0, 5], v: [2, 3] })
  })

  it('비어 있어도 안전하다', () => {
    const src = new RowArray([], 0)
    expect(src.length).toBe(0)
    expect(src.bounds()).toEqual([0, 0])
    expect(src.lastRow()).toBeNull()
    expect(src.indexAtTime(1)).toBe(-1)
  })
})

describe('nearestIndex', () => {
  it('오름차순 값에서 동률이면 앞쪽을 고른다', () => {
    const at = (i: number) => [0, 10, 20][i]
    expect(nearestIndex(at, 3, 5)).toBe(0)
    expect(nearestIndex(at, 3, 15)).toBe(1)
    expect(nearestIndex(at, 0, 1)).toBe(-1)
  })
})

describe('startDisabledReason', () => {
  const base = {
    linkReady: true,
    running: false,
    plc: 'GR2',
    channels: ['A.b'],
    divider: '1',
    flushMs: '100',
    chanMax: CHAN_MAX,
    flushMin: FLUSH_MIN_MS,
  }

  it('모두 갖춰지면 막지 않는다', () => {
    expect(startDisabledReason(base)).toBeNull()
  })

  it('링크가 없으면 그 이유를 낸다', () => {
    expect(startDisabledReason({ ...base, linkReady: false })).toMatch(/링크/)
  })

  it('이미 도는 세션이 있으면 막는다', () => {
    expect(startDisabledReason({ ...base, running: true })).toMatch(/이미/)
  })

  it('PLC 를 모르면 막는다', () => {
    expect(startDisabledReason({ ...base, plc: '' })).toMatch(/PLC/)
  })

  it('채널이 없거나 상한을 넘으면 막는다', () => {
    expect(startDisabledReason({ ...base, channels: [] })).toMatch(/채널/)
    const many = Array.from({ length: 33 }, (_, i) => `A.b${i}`)
    expect(startDisabledReason({ ...base, channels: many })).toMatch(/32/)
  })

  it('분주와 전송 주기는 정수 범위를 지켜야 한다', () => {
    expect(startDisabledReason({ ...base, divider: '0' })).toMatch(/분주/)
    expect(startDisabledReason({ ...base, divider: '1.5' })).toMatch(/분주/)
    expect(startDisabledReason({ ...base, flushMs: '19' })).toMatch(/20/)
    expect(startDisabledReason({ ...base, flushMs: '' })).toMatch(/20/)
  })
})

describe('채널 선택', () => {
  it('중복은 더하지 않고 상한을 넘기지 않는다', () => {
    expect(addChannel(['a'], 'a')).toEqual(['a'])
    expect(addChannel(['a'], 'b')).toEqual(['a', 'b'])
    expect(addChannel(['a', 'b'], 'c', 2)).toEqual(['a', 'b'])
    expect(removeChannel(['a', 'b'], 'a')).toEqual(['b'])
  })

  it('저장된 선택을 되읽을 때 깨진 값·중복·초과를 걷어 낸다', () => {
    expect(parseSelection(null)).toEqual([])
    expect(parseSelection('nonsense')).toEqual([])
    expect(parseSelection('{"a":1}')).toEqual([])
    expect(parseSelection('["a", "a", 3, "", "b"]')).toEqual(['a', 'b'])
    expect(parseSelection(JSON.stringify(Array.from({ length: 40 }, (_, i) => `c${i}`)))).toHaveLength(32)
  })

  it('검색 결과를 DB 별로 묶고 DB 이름 순으로 낸다', () => {
    const ch = (path: string, db: string): TraceChannel => ({
      path,
      db,
      db_no: 1,
      offset: 0,
      bit: null,
      width: 4,
      kind: 'real',
      bits: 32,
      type_name: 'Real',
    })
    const g = groupByDb([ch('WEBMON.a', 'WEBMON'), ch('PARA.x', 'PARA'), ch('WEBMON.b', 'WEBMON')])
    expect(g.map((x) => x.db)).toEqual(['PARA', 'WEBMON'])
    expect(g[1].channels).toHaveLength(2)
  })
})

describe('표시', () => {
  it('PLC 타입에 맞게 값을 쓴다', () => {
    expect(fmtValue('bool', 0)).toBe('0')
    expect(fmtValue('bool', 1)).toBe('1')
    expect(fmtValue('real', 1.23456)).toBe('1.235')
    expect(fmtValue('int', -12.0)).toBe('-12')
    expect(fmtValue('uint', 7)).toBe('7')
    expect(fmtValue('time_ms', 1500.4)).toBe('1500 ms')
    expect(fmtValue('real', Number.NaN)).toBe('-')
  })

  it('시각은 초 세 자리, 경로는 DB 앞머리를 뗀다', () => {
    expect(fmtT(1234)).toBe('1.234 s')
    expect(shortPath('WEBMON.Axis[1].Position')).toBe('Axis[1].Position')
    expect(shortPath('NoDot')).toBe('NoDot')
  })

})

describe('health', () => {
  it('라이브 집계가 이기되 손실 값은 둘 중 큰 쪽을 남긴다', () => {
    const meta = { rows: 100, chunks: 4, overrun: 3, gaps: 2 }
    expect(health(meta, { rows: 120, chunks: 5, overrun: 1, lag: 7 })).toEqual({
      rows: 120,
      chunks: 5,
      overrun: 3,
      gaps: 2,
      lag: 7,
    })
  })

  it('세션이 없으면 0 으로 떨어진다', () => {
    expect(health(null, {})).toEqual({ rows: 0, chunks: 0, overrun: 0, gaps: 0, lag: 0 })
  })
})
