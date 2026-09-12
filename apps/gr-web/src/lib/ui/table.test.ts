import { describe, expect, it } from 'vitest'
import { columnLevel, COLUMN_WIDTH_FOR, splitColumns, type Column } from './table'

interface Row {
  id: string
}

const cols = (...spec: [string, 1 | 2 | 3 | undefined][]): Column<Row>[] =>
  spec.map(([key, priority]) => ({ key, label: key, priority }))

describe('columnLevel', () => {
  it('폭을 모르면 전부 보인다 — 첫 페인트에서 열이 깜빡이지 않게', () => {
    expect(columnLevel(null)).toBe(3)
  })

  it('좁으면 필수 열만', () => {
    expect(columnLevel(0)).toBe(1)
    expect(columnLevel(300)).toBe(1)
    expect(columnLevel(COLUMN_WIDTH_FOR[2] - 1)).toBe(1)
  })

  it('경계값에서 바로 올라간다', () => {
    expect(columnLevel(COLUMN_WIDTH_FOR[2])).toBe(2)
    expect(columnLevel(COLUMN_WIDTH_FOR[3])).toBe(3)
  })

  it('아주 넓으면 3', () => {
    expect(columnLevel(4000)).toBe(3)
  })
})

describe('splitColumns', () => {
  it('우선순위를 안 적은 열은 항상 보인다 — 기존 표의 동작이 바뀌지 않는다', () => {
    const { visible, hidden } = splitColumns(cols(['a', undefined], ['b', undefined]), 1)
    expect(visible.map((c) => c.key)).toEqual(['a', 'b'])
    expect(hidden).toEqual([])
  })

  it('레벨보다 큰 우선순위만 접힌다', () => {
    const c = cols(['id', 1], ['state', 1], ['robot', 2], ['origin', 3])
    expect(splitColumns(c, 1).visible.map((x) => x.key)).toEqual(['id', 'state'])
    expect(splitColumns(c, 1).hidden.map((x) => x.key)).toEqual(['robot', 'origin'])
    expect(splitColumns(c, 2).visible.map((x) => x.key)).toEqual(['id', 'state', 'robot'])
    expect(splitColumns(c, 2).hidden.map((x) => x.key)).toEqual(['origin'])
    expect(splitColumns(c, 3).hidden).toEqual([])
  })

  it('원본 순서를 지킨다 — 열이 자리를 바꾸면 눈이 다시 찾는다', () => {
    const c = cols(['a', 3], ['b', 1], ['c', 2], ['d', 1])
    expect(splitColumns(c, 2).visible.map((x) => x.key)).toEqual(['b', 'c', 'd'])
    expect(splitColumns(c, 2).hidden.map((x) => x.key)).toEqual(['a'])
  })

  it('원본 배열을 갈지 않는다', () => {
    const c = cols(['a', 1], ['b', 3])
    splitColumns(c, 1)
    expect(c.map((x) => x.key)).toEqual(['a', 'b'])
  })

  it('빈 열 목록도 받는다', () => {
    expect(splitColumns<Row>([], 1)).toEqual({ visible: [], hidden: [] })
  })
})
