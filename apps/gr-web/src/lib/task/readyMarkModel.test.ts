import { describe, expect, it } from 'vitest'
import { robotColor } from '../robotContext'
import type { Hold, TargetReady } from '../sched'
import {
  holdLine,
  opLine,
  readyGlyphs,
  readyLines,
  readyShort,
  stockLine,
  toSnapshot,
} from './readyMarkModel'

const OK = { ok: true, why: null }
const no = (why: string) => ({ ok: false, why })

function station(p: Partial<TargetReady> = {}): TargetReady {
  return {
    kind: 'station',
    id: 2101,
    pick: OK,
    drop: OK,
    measure: OK,
    item_code: 0,
    count: 0,
    room: null,
    holds: [],
    need_item: false,
    hints: [],
    since: null,
    role: 'both',
    drop_mode: 'single',
    ...p,
  }
}
function cell(p: Partial<TargetReady> = {}): TargetReady {
  return station({ kind: 'cell', id: 401, role: null, drop_mode: null, ...p })
}
const hold = (p: Partial<Hold> = {}): Hold => ({
  robot: 2,
  robot_name: 'GR2',
  op: 'PICK',
  state: 'running',
  order: 'TO-260928-0003',
  task: 'T-1',
  ...p,
})

describe('readyGlyphs', () => {
  it('정보 없으면 아무것도 안 그린다', () => {
    expect(readyGlyphs(undefined)).toEqual([])
  })
  it('가능 = 초록 ▲ ▼', () => {
    const g = readyGlyphs(station())
    expect(g.map((x) => [x.glyph, x.tone])).toEqual([
      ['▲', 'ok'],
      ['▼', 'ok'],
    ])
    expect(g[0].title).toBe('PICK 가능')
  })
  it('품목 미정 = 주황 ▲ (스테이션 PICK 만)', () => {
    const g = readyGlyphs(station({ need_item: true, hints: [1203, 1204] }))
    expect(g[0]).toMatchObject({ mark: 'need_item', tone: 'warn' })
    expect(g[0].title).toBe('PICK 가능 — 품목 미정 · 후보 1203, 1204')
    expect(g[1].tone).toBe('ok')
  })
  it('예약 = 로봇 색, 실행 중인 예약을 먼저', () => {
    const g = readyGlyphs(
      station({
        pick: no('예약됨'),
        holds: [hold({ robot: 1, robot_name: 'GR1', state: 'queued', task: null }), hold()],
      }),
    )
    expect(g[0]).toMatchObject({ mark: 'held', tone: 'robot', color: robotColor(2) })
    expect(g[0].title).toBe('PICK 예약 — GR2 PICK TO-260928-0003 (실행 중)')
  })
  it('MEASURE 예약은 PICK 쪽, DROP 예약은 DROP 쪽', () => {
    const g = readyGlyphs(station({ holds: [hold({ op: 'MEASURE' })] }))
    expect(g.map((x) => x.mark)).toEqual(['held', 'ready'])
    const d = readyGlyphs(station({ holds: [hold({ op: 'DROP' })] }))
    expect(d.map((x) => x.mark)).toEqual(['ready', 'held'])
  })
  it('막힘 = 역할이 그 일을 하는 스테이션만', () => {
    const g = readyGlyphs(station({ role: 'pick', pick: no('Req 없음'), drop: no('역할 아님') }))
    expect(g).toHaveLength(1)
    expect(g[0]).toMatchObject({ op: 'pick', mark: 'blocked', tone: 'muted' })
    expect(g[0].title).toBe('PICK 불가 — Req 없음')
    expect(readyGlyphs(station({ role: null, pick: no('x'), drop: no('x') }))).toEqual([])
  })
  it('셀은 불가면 그리지 않는다', () => {
    const g = readyGlyphs(cell({ pick: no('재고 없음') }))
    expect(g.map((x) => x.op)).toEqual(['drop'])
    expect(readyGlyphs(cell({ pick: no('a'), drop: no('b') }))).toEqual([])
  })
  it('셀의 need_item 은 PICK 가능으로 본다', () => {
    expect(readyGlyphs(cell({ need_item: true }))[0].mark).toBe('ready')
  })
})

describe('lines', () => {
  it('예약 한 줄 — 예정 큐 · 이름 없음', () => {
    expect(holdLine(hold({ state: 'queued', task: null, order: null }))).toBe('GR2 PICK (예정)')
    expect(holdLine(hold({ robot_name: '', robot: 1, op: 'DROP', state: 'submitted' }))).toBe(
      'GR1 DROP TO-260928-0003 (제출됨)',
    )
  })
  it('미래 재고', () => {
    expect(stockLine(cell({ item_code: 1203, count: 4, room: 2 }))).toBe(
      'ItemCode 1203 × 4 · Room 2',
    )
    expect(stockLine(cell())).toBe('ItemCode - × 0 · Room ∞')
  })
  it('호버 줄 — 판정 둘 + 예약 + 재고', () => {
    const t = cell({ drop: no('가득'), holds: [hold()], item_code: 7, count: 1, room: 0 })
    expect(readyLines(t)).toEqual([
      'PICK 가능',
      'DROP 불가 — 가득',
      'GR2 PICK TO-260928-0003 (실행 중)',
      'ItemCode 7 × 1 · Room 0',
    ])
    expect(readyLines(undefined)).toEqual([])
  })
  it('opLine 은 이유 없는 불가도 적는다', () => {
    expect(opLine(station({ drop: { ok: false, why: null } }), 'drop')).toBe('DROP 불가')
  })
  it('정보 카드 칸', () => {
    expect(readyShort(station(), 'pick')).toEqual({ tone: 'ok', text: 'PICK ✓' })
    expect(readyShort(station({ need_item: true }), 'pick')).toEqual({
      tone: 'warn',
      text: 'PICK ✓ 품목 미정',
    })
    expect(readyShort(station({ drop: no('비어 있지 않음') }), 'drop')).toEqual({
      tone: 'bad',
      text: 'DROP ✗ 비어 있지 않음',
    })
  })
})

describe('toSnapshot', () => {
  it('모양이 다르면 빈 값', () => {
    expect(toSnapshot(null).targets).toEqual([])
    expect(toSnapshot('x').targets).toEqual([])
    expect(toSnapshot({ at: 'a' }).targets).toEqual([])
  })
  it('잘못된 항목은 버린다', () => {
    const s = toSnapshot({ at: 'now', targets: [station(), { kind: 'x' }, null, cell()] })
    expect(s.at).toBe('now')
    expect(s.targets.map((t) => t.id)).toEqual([2101, 401])
  })
})
