import { describe, expect, it } from 'vitest'
import {
  applyBlockedReason,
  importOutcome,
  isSheetFile,
  outcomeStats,
  outcomeSummary,
  problemRows,
  sheetCountsText,
} from './importPreview'
import type { FileImportResult } from './types'

const file = (p: Partial<FileImportResult>): FileImportResult => ({
  imported: 0,
  added: 0,
  updated: 0,
  removed: 0,
  skipped: 0,
  unchanged: 0,
  errors: [],
  ...p,
})

/** PLC 읽기 응답 — `added`·`unchanged` 가 없고 `skipped` 가 '동일'을 뜻한다. */
const plcRead = (p: Partial<FileImportResult>): FileImportResult => ({
  imported: 0,
  updated: 0,
  removed: 0,
  skipped: 0,
  errors: [],
  ...p,
})

describe('importOutcome', () => {
  it('파일 응답의 넷을 그대로 센다', () => {
    const o = importOutcome(file({ imported: 12, added: 12, updated: 3, unchanged: 5, skipped: 2 }))
    expect(o).toEqual({
      added: 12,
      updated: 3,
      unchanged: 5,
      skipped: 2,
      removed: 0,
      applicable: 15,
      problems: 0,
    })
  })

  it('재고 교체의 삭제는 적용할 줄로 세고, 있을 때만 칸이 생긴다', () => {
    const o = importOutcome(
      file({ imported: 0, added: 0, updated: 0, unchanged: 3, skipped: 0, removed: 2 }),
    )
    expect(o.removed).toBe(2)
    expect(o.applicable).toBe(2)
    expect(outcomeStats(o).map((x) => x.key)).toEqual([
      'added',
      'updated',
      'unchanged',
      'skipped',
      'removed',
    ])
    expect(outcomeStats(importOutcome(file({ imported: 1 }))).map((x) => x.key)).not.toContain(
      'removed',
    )
  })

  it('added 가 없으면 imported 를 쓴다', () => {
    expect(importOutcome(file({ imported: 4, added: undefined, updated: 1 })).added).toBe(4)
  })

  it('옛 PLC 읽기 응답은 skipped 를 동일로 읽는다', () => {
    const o = importOutcome(plcRead({ imported: 2, updated: 1, skipped: 7 }))
    expect(o.unchanged).toBe(7)
    expect(o.skipped).toBe(0)
    expect(o.applicable).toBe(3)
  })

  it('problems 는 오류 줄 수다', () => {
    const o = importOutcome(file({ errors: [{ row: 3, sheet: 'Items', message: 'x' }] }))
    expect(o.problems).toBe(1)
  })
})

describe('outcomeStats · outcomeSummary', () => {
  it('머리줄은 넷을 모두 보인다(0 도)', () => {
    const s = outcomeStats(importOutcome(file({ added: 2 })))
    expect(s.map((x) => x.key)).toEqual(['added', 'updated', 'unchanged', 'skipped'])
    expect(s.map((x) => x.label)).toEqual(['추가', '갱신', '동일', '건너뜀'])
    expect(s.map((x) => x.value)).toEqual([2, 0, 0, 0])
  })

  it('요약은 0 인 결과를 뺀다', () => {
    const o = importOutcome(file({ imported: 12, added: 12, updated: 3 }))
    expect(outcomeSummary('품목', o)).toBe('품목 12건 추가 · 3건 갱신')
  })

  it('요약에 동일·건너뜀도 붙는다', () => {
    const o = importOutcome(file({ added: 1, unchanged: 2, skipped: 3 }))
    expect(outcomeSummary('품목', o)).toBe('품목 1건 추가 · 2건 동일 · 3건 건너뜀')
  })

  it('전부 0 이면 바뀐 것이 없다고 말한다', () => {
    expect(outcomeSummary('품목', importOutcome(file({})))).toBe('품목 바뀐 것 없음')
  })
})

describe('applyBlockedReason', () => {
  it('쓸 줄이 있으면 막지 않는다', () => {
    expect(applyBlockedReason(file({ added: 2 }))).toBeNull()
    expect(applyBlockedReason(file({ updated: 1, skipped: 3 }))).toBeNull()
  })

  it('파일이 없으면 고르라고 말한다', () => {
    expect(applyBlockedReason(null, { file: false })).toBe('먼저 파일을 고르세요')
  })

  it('미리보기 전에는 기다리라고, 실패했으면 다시 고르라고 말한다', () => {
    expect(applyBlockedReason(null, { file: true })).toMatch(/기다리는/)
    expect(applyBlockedReason(null, { file: true, failed: true })).toMatch(/실패/)
  })

  it('전부 같은 값이면 그렇다고 말한다', () => {
    expect(applyBlockedReason(file({ unchanged: 9 }))).toMatch(/같습니다/)
  })

  it('전부 오류면 고칠 곳을 가리킨다', () => {
    const p = file({
      skipped: 2,
      errors: [
        { row: 3, message: 'a' },
        { row: 4, message: 'b' },
      ],
    })
    expect(applyBlockedReason(p)).toMatch(/오류/)
  })

  it('빈 파일(양식 그대로)은 적용할 행이 없다', () => {
    expect(applyBlockedReason(file({}))).toBe('적용할 행이 없습니다')
  })
})

describe('problemRows', () => {
  it('줄·시트·사유를 그대로 옮기고 키가 겹치지 않는다', () => {
    const rows = problemRows(
      file({
        errors: [
          { row: 3, sheet: 'Items', message: 'Items row 3: count must be >= 1' },
          { row: 3, sheet: 'Items', message: 'Items row 3: another' },
          { row: 7, message: 'row 7: x' },
        ],
      }),
    )
    expect(rows.map((r) => r.row)).toEqual([3, 3, 7])
    expect(rows[2].sheet).toBe('')
    expect(new Set(rows.map((r) => r.key)).size).toBe(3)
  })

  it('미리보기가 없으면 빈 표다', () => {
    expect(problemRows(null)).toEqual([])
  })
})

describe('isSheetFile · sheetCountsText', () => {
  it('표 파일만 받는다', () => {
    expect(isSheetFile('items_template.xlsx')).toBe(true)
    expect(isSheetFile('ITEMS.CSV')).toBe(true)
    expect(isSheetFile('photo.png')).toBe(false)
    expect(isSheetFile('xlsx')).toBe(false)
  })

  it('0 인 표는 빼고 센다', () => {
    expect(sheetCountsText({ cells: 0, stations: 0, items: 2, item_profiles: 6 })).toBe(
      '품목 2 · 비드 6줄',
    )
    expect(sheetCountsText({ cells: 3, stations: 1, items: 0 })).toBe('셀 3 · 스테이션 1')
    expect(sheetCountsText(undefined)).toBe('')
  })
})
