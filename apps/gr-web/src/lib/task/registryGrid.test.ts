import { describe, expect, it } from 'vitest'
import type { CellUpsert, StationUpsert } from '../types'
import {
  applyCellEdit,
  applyStationEdit,
  CELL_POSZ_WARNING,
  cellErrors,
  cellPositionErrors,
  cellPositionWarnings,
  cellWarnings,
  diffDraft,
  draftFrom,
  newCellRow,
  newStationRow,
  nextCellId,
  nextStationId,
  parseBool,
  pasteInt,
  pasteNum,
  setRow,
  savedKey,
} from './registryGrid'

const cell = (id: number, x = 1000, y = 2000): CellUpsert => ({
  id,
  use: true,
  blend_use: false,
  section: 2,
  row: 1,
  col: 1,
  length: 1200,
  width: 1100,
  position: [x, y, 1500],
})
const station = (id: number): StationUpsert =>
  ({
    id,
    conv_no: 1,
    task_type: 1,
    rotate_type: 1,
    group: 1,
    group_index: 1,
    connection_prev: 0,
    connection_next: 0,
    info: {
      id,
      use: true,
      blend_use: false,
      section: 3,
      row: 1,
      col: 1,
      length: 1500,
      width: 1500,
      position: [20000, 1000, 1450],
    },
    sensor: {} as StationUpsert['sensor'],
    io_block_no: 0,
  }) as StationUpsert

describe('registryGrid', () => {
  it('parses cells: numbers with commas, booleans, rejects junk', () => {
    const c = cell(101)
    expect(applyCellEdit(c, 'x', '12,345.5')?.position).toEqual([12345.5, 2000, 1500])
    expect(applyCellEdit(c, 'z', '0')?.position[2]).toBe(0)
    expect(applyCellEdit(c, 'use', '미사용')?.use).toBe(false)
    expect(applyCellEdit(c, 'row', '3.9')?.row).toBe(3)
    expect(applyCellEdit(c, 'x', 'abc')).toBeNull()
    expect(applyCellEdit(c, 'nope', '1')).toBeNull()
    expect(parseBool('Y')).toBe(true)
    expect(parseBool('maybe')).toBeNull()
    expect(pasteInt('12.7')).toBe('12')
    expect(pasteNum(' ')).toBeNull()
  })

  it('station edits keep Info.Id in step with the station id', () => {
    const s = applyStationEdit(station(2101), 'id', '2103')!
    expect(s.id).toBe(2103)
    expect(s.info.id).toBe(2103)
    expect(applyStationEdit(station(2101), 'y', '1500')!.info.position).toEqual([20000, 1500, 1450])
    expect(applyStationEdit(station(2101), 'conv', 'x')).toBeNull()
  })

  it('row state: changed, back to same when reverted by hand, added stays added', () => {
    const saved = [cell(101), cell(102)]
    const original = new Map(saved.map((c) => [savedKey(c.id), c]))
    let rows = draftFrom(saved)
    rows = setRow(rows, savedKey(101), applyCellEdit(rows[0].value, 'x', '999')!, original)
    expect(rows[0].state).toBe('changed')
    rows = setRow(rows, savedKey(101), applyCellEdit(rows[0].value, 'x', '1000')!, original)
    expect(rows[0].state).toBe('same')
    const added = newCellRow(rows)!
    rows = [...rows, added]
    rows = setRow(rows, added.key, { ...added.value, section: 1 }, original)
    expect(rows[2].state).toBe('added')
  })

  it('diff: upserts, deletes (incl. id rename), duplicate ids', () => {
    const saved = [cell(101), cell(102), cell(103)]
    const original = new Map(saved.map((c) => [savedKey(c.id), c]))
    let rows = draftFrom(saved)
    rows = setRow(rows, savedKey(101), applyCellEdit(rows[0].value, 'id', '150')!, original) // rename
    rows = rows.filter((r) => r.value.id !== 103) // delete
    rows = [...rows, { key: 'n1', value: cell(102), state: 'added' as const }] // duplicate of 102
    const d = diffDraft(
      rows,
      saved.map((c) => c.id),
    )
    expect(d.upserts.map((c) => c.id).sort()).toEqual([102, 150])
    expect(d.deletes.sort()).toEqual([101, 103])
    expect(d.dupIds).toEqual([102])
    expect(d.added).toBe(1)
    expect(d.changed).toBe(1)
  })

  it('next ids and new rows follow the last row (same row X, next column Y)', () => {
    expect(nextCellId([101, 102, 1000])).toBe(1)
    expect(nextCellId([101, 102])).toBe(103)
    expect(nextStationId([2101, 2132])).toBe(2201)
    expect(nextStationId([])).toBe(2001)
    const rows = draftFrom([cell(101, 1000, 2000)])
    const r = newCellRow(rows)!
    expect(r.value.id).toBe(102)
    expect(r.value.position).toEqual([1000, 3100, 1500])
    expect(r.value.col).toBe(2)
    const st = newStationRow(draftFrom([station(2101)]), station(0))!
    expect(st.value.id).toBe(2102)
    expect(st.value.info.id).toBe(2102)
    expect(newStationRow([], station(0))!.value.id).toBe(2001)
  })

  // 셀 바닥 Z 는 바닥 평탄도 보정 — 0·음수도 맞는 값이고, PLC 도 셀은 검사하지 않으므로 경고도 없다.
  it('floor Z ≤ 0 is neither an error nor a warning; X/Y·id·section stay errors', () => {
    const z = (v: number): CellUpsert => ({ ...cell(301), position: [12000, 3000, v] })
    for (const v of [-8.8, -23.5, 0]) {
      expect(cellErrors(z(v))).toEqual([])
      expect(cellPositionErrors(z(v).position)).toEqual([])
      expect(cellWarnings(z(v))).toEqual([])
    }
    expect(CELL_POSZ_WARNING).toContain('INVALID_CELL_POSZ')
    expect(cellWarnings(z(1500))).toEqual([])
    expect(cellPositionWarnings([0, 0, 1])).toEqual([])
    // 숫자가 아닌 Z 는 오류이고 경고는 붙지 않는다
    expect(cellErrors(z(Number.NaN))[0]).toContain('숫자')
    expect(cellWarnings(z(Number.NaN))).toEqual([])
    // 나머지 규칙은 그대로 오류
    expect(cellErrors({ ...cell(301), position: [-1, 0, 1500] })[0]).toContain('위치 X')
    expect(cellErrors({ ...cell(301), position: [0, -1, 1500] })[0]).toContain('위치 Y')
    expect(cellErrors({ ...cell(0) })).toContain('셀 Id는 1..1000')
    expect(cellErrors({ ...cell(301), section: 4 })).toContain('구역은 1..3')
    // Z 칸 편집은 음수도 받아들인다(그리드가 그대로 저장한다)
    expect(applyCellEdit(cell(301), 'z', '-8.8')!.position[2]).toBe(-8.8)
  })
})
