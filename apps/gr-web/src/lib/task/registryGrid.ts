// 레지스트리 그리드 편집(엑셀형)의 순수 모델 — 초안 행, 칸 편집 파싱, 행 상태(같음/수정/신규), 저장할 차이.
//
// 그리드 행 키(`key`)는 id 와 따로 둔다: 칸에서 id 를 고쳐도 행이 흔들리지 않고, 저장 때
// "옛 id 삭제 + 새 id 올리기" 로 풀린다(`diffDraft`).
import type { Cell, CellUpsert, Station, StationUpsert } from '../types'

export type RowState = 'same' | 'changed' | 'added'

export interface DraftRow<T> {
  /** 그리드 행 키 — id 를 고쳐도 바뀌지 않는다. */
  key: string
  value: T
  state: RowState
}

let seq = 0
export function newRowKey(): string {
  seq++
  return `n${Date.now().toString(36)}${seq}`
}
export const savedKey = (id: number): string => `id${id}`

export function cellToUpsert(c: Cell): CellUpsert {
  return {
    id: c.id,
    use: c.use,
    blend_use: c.blend_use,
    section: c.section,
    row: c.row,
    col: c.col,
    length: c.length,
    width: c.width,
    position: [c.position[0], c.position[1], c.position[2]],
  }
}

export function stationToUpsert(s: Station): StationUpsert {
  return {
    id: s.id,
    conv_no: s.conv_no,
    task_type: s.task_type,
    rotate_type: s.rotate_type,
    group: s.group,
    group_index: s.group_index,
    connection_prev: s.connection_prev,
    connection_next: s.connection_next,
    info: { ...s.info, position: [s.info.position[0], s.info.position[1], s.info.position[2]] },
    sensor: { ...s.sensor },
    io_block_no: s.io_block_no,
  }
}

export function draftFrom<T extends { id: number }>(items: readonly T[]): DraftRow<T>[] {
  return items.map((value) => ({ key: savedKey(value.id), value, state: 'same' as const }))
}

const sameValue = (a: unknown, b: unknown): boolean => JSON.stringify(a) === JSON.stringify(b)

/** 한 행의 값을 바꾼다 — 원본과 같아지면 'same', 신규 행은 계속 'added'. */
export function setRow<T>(
  rows: readonly DraftRow<T>[],
  key: string,
  next: T,
  original: ReadonlyMap<string, T>,
): DraftRow<T>[] {
  return rows.map((r) => {
    if (r.key !== key) return r
    if (r.state === 'added') return { ...r, value: next }
    const o = original.get(key)
    return { ...r, value: next, state: o !== undefined && sameValue(o, next) ? 'same' : 'changed' }
  })
}

// ── 칸 값 해석 ────────────────────────────────────────────────────────────────

/** 숫자(천 단위 쉼표 허용). 빈 칸·문자는 null. */
export function parseNum(v: string): number | null {
  const t = v.trim().replace(/,/g, '')
  if (t === '') return null
  const n = Number(t)
  return Number.isFinite(n) ? n : null
}
export function parseInteger(v: string): number | null {
  const n = parseNum(v)
  return n === null ? null : Math.trunc(n)
}
/** Y/N · true/false · 1/0 · 사용/미사용. */
export function parseBool(v: string): boolean | null {
  const t = v.trim().toLowerCase()
  if (['y', 'yes', 'true', '1', '사용', 'o', 'on'].includes(t)) return true
  if (['n', 'no', 'false', '0', '미사용', 'x', 'off'].includes(t)) return false
  return null
}
export const boolText = (b: boolean): string => (b ? 'Y' : 'N')

/** 붙여넣기 정규화 — 해석 못 하면 null(그 칸은 건너뛴다). */
export const pasteInt = (v: string): string | null => {
  const n = parseInteger(v)
  return n === null ? null : String(n)
}
export const pasteNum = (v: string): string | null => {
  const n = parseNum(v)
  return n === null ? null : String(n)
}
export const pasteBool = (v: string): string | null => {
  const b = parseBool(v)
  return b === null ? null : boolText(b)
}

function withPos<P extends [number, number, number]>(
  p: P,
  i: 0 | 1 | 2,
  n: number,
): [number, number, number] {
  const q: [number, number, number] = [p[0], p[1], p[2]]
  q[i] = n
  return q
}

/** 셀 한 칸 편집. 해석할 수 없는 값이면 null(그 칸은 그대로 둔다). */
export function applyCellEdit(c: CellUpsert, col: string, value: string): CellUpsert | null {
  const int = parseInteger(value)
  const num = parseNum(value)
  const bool = parseBool(value)
  switch (col) {
    case 'id':
      return int === null ? null : { ...c, id: int }
    case 'use':
      return bool === null ? null : { ...c, use: bool }
    case 'blend':
      return bool === null ? null : { ...c, blend_use: bool }
    case 'section':
      return int === null ? null : { ...c, section: int }
    case 'row':
      return int === null ? null : { ...c, row: int }
    case 'col':
      return int === null ? null : { ...c, col: int }
    case 'x':
      return num === null ? null : { ...c, position: withPos(c.position, 0, num) }
    case 'y':
      return num === null ? null : { ...c, position: withPos(c.position, 1, num) }
    case 'z':
      return num === null ? null : { ...c, position: withPos(c.position, 2, num) }
    case 'length':
      return num === null ? null : { ...c, length: num }
    case 'width':
      return num === null ? null : { ...c, width: num }
    default:
      return null
  }
}

/** 스테이션 한 칸 편집(Info.Id 는 스테이션 id 와 같이 움직인다). */
export function applyStationEdit(
  s: StationUpsert,
  col: string,
  value: string,
): StationUpsert | null {
  const int = parseInteger(value)
  const num = parseNum(value)
  const bool = parseBool(value)
  const info = (patch: Partial<StationUpsert['info']>): StationUpsert => ({
    ...s,
    info: { ...s.info, ...patch },
  })
  switch (col) {
    case 'id':
      return int === null ? null : { ...s, id: int, info: { ...s.info, id: int } }
    case 'conv':
      return int === null ? null : { ...s, conv_no: int }
    case 'group':
      return int === null ? null : { ...s, group: int }
    case 'group_index':
      return int === null ? null : { ...s, group_index: int }
    case 'task_type':
      return int === null ? null : { ...s, task_type: int }
    case 'rotate_type':
      return int === null ? null : { ...s, rotate_type: int }
    case 'use':
      return bool === null ? null : info({ use: bool })
    case 'section':
      return int === null ? null : info({ section: int })
    case 'row':
      return int === null ? null : info({ row: int })
    case 'col':
      return int === null ? null : info({ col: int })
    case 'x':
      return num === null ? null : info({ position: withPos(s.info.position, 0, num) })
    case 'y':
      return num === null ? null : info({ position: withPos(s.info.position, 1, num) })
    case 'z':
      return num === null ? null : info({ position: withPos(s.info.position, 2, num) })
    default:
      return null
  }
}

// ── 셀 검증(백엔드 `registry/xlsx.rs::validate_cell` 과 같은 규칙) ───────────
//
// 셀 바닥 Z 는 **바닥 평탄도 보정**이라 0·음수도 맞는 값이다(현장 CELL 301..305 = -8.8 … -23.5).
// GR2 `isValidTaskData`(79–83행)의 INVALID_CELL_POSZ 검사는 `IF #Task.Cell.Id > 2000` 안에 있어
// 스테이션 대상에만 걸리므로(셀 id 는 1..1000) 셀에는 오류도 경고도 내지 않는다.

/** 바닥 Z ≤ 0 경고 — 그리드 툴팁·폼·레이아웃 규칙이 모두 이 문구를 쓴다. */
export const CELL_POSZ_WARNING =
  '바닥 Z ≤ 0 — GR2 가 INVALID_CELL_POSZ 로 작업을 거부합니다 (검사는 Cell.Id > 2000 대상, 셀 값은 그대로 저장됩니다)'

/** 막아야 하는 좌표 — X/Y 음수, 숫자가 아닌 값. */
export const cellPositionErrors = (p: readonly [number, number, number]): string[] => [
  ...(['X', 'Y'] as const)
    .filter((_, i) => !(p[i] >= 0))
    .map((a) => `위치 ${a}는 0 이상이어야 합니다`),
  ...(Number.isFinite(p[2]) ? [] : ['위치 Z(바닥)가 숫자가 아닙니다']),
]

/** 막지 않는 좌표 경고 — 셀 바닥 Z 는 PLC 가 검사하지 않으므로 지금은 없다(채널만 유지). */
export const cellPositionWarnings = (_p: readonly [number, number, number]): string[] => []

export function cellErrors(c: CellUpsert): string[] {
  const out: string[] = []
  if (!isCellIdOk(c.id)) out.push('셀 Id는 1..1000')
  if (![1, 2, 3].includes(c.section)) out.push('구역은 1..3')
  out.push(...cellPositionErrors(c.position))
  return out
}

export const cellWarnings = (c: CellUpsert): string[] => cellPositionWarnings(c.position)

// ── id ───────────────────────────────────────────────────────────────────────

export const isCellIdOk = (id: number): boolean => Number.isInteger(id) && id >= 1 && id <= 1000
export const isStationIdOk = (id: number): boolean =>
  Number.isInteger(id) && id >= 2001 && id <= 2999 && id % 100 >= 1 && id % 100 <= 32

/** 다음 빈 셀 id — 가장 큰 id 다음부터, 없으면 앞에서부터. 1..1000 이 꽉 차면 null. */
export function nextCellId(ids: readonly number[]): number | null {
  const used = new Set(ids)
  for (let i = Math.max(0, ...ids) + 1; i <= 1000; i++) if (!used.has(i)) return i
  for (let i = 1; i <= 1000; i++) if (!used.has(i)) return i
  return null
}

/** 다음 빈 스테이션 id (2001..2999, id mod 100 ∈ 1..32). */
export function nextStationId(ids: readonly number[]): number | null {
  const used = new Set(ids)
  for (let i = Math.max(2000, ...ids) + 1; i <= 2999; i++)
    if (!used.has(i) && isStationIdOk(i)) return i
  for (let i = 2001; i <= 2999; i++) if (!used.has(i) && isStationIdOk(i)) return i
  return null
}

/** 새 셀 행 — 마지막 행을 본떠 같은 행(X)의 다음 열(Y)에 둔다. */
export function newCellRow(rows: readonly DraftRow<CellUpsert>[]): DraftRow<CellUpsert> | null {
  const id = nextCellId(rows.map((r) => r.value.id))
  if (id === null) return null
  const t = rows.length ? rows[rows.length - 1].value : null
  const value: CellUpsert = t
    ? {
        ...t,
        id,
        col: t.col + 1,
        position: [t.position[0], t.position[1] + (t.width || t.length || 1000), t.position[2]],
      }
    : {
        id,
        use: true,
        blend_use: false,
        section: 1,
        row: 1,
        col: 1,
        length: 1000,
        width: 1000,
        position: [0, 0, 1500],
      }
  return { key: newRowKey(), value, state: 'added' }
}

/** 새 스테이션 행 — 마지막 행을 본뜨고, 없으면 `blank` 를 쓴다. */
export function newStationRow(
  rows: readonly DraftRow<StationUpsert>[],
  blank: StationUpsert,
): DraftRow<StationUpsert> | null {
  const id = nextStationId(rows.map((r) => r.value.id))
  if (id === null) return null
  const t = rows.length ? rows[rows.length - 1].value : null
  const value: StationUpsert = t
    ? {
        ...t,
        id,
        group_index: t.group_index + 1,
        info: {
          ...t.info,
          id,
          col: t.info.col + 1,
          position: [
            t.info.position[0],
            t.info.position[1] + (t.info.width || 1500),
            t.info.position[2],
          ],
        },
        sensor: { ...t.sensor },
      }
    : { ...blank, id, info: { ...blank.info, id } }
  return { key: newRowKey(), value, state: 'added' }
}

// ── 저장할 차이 ──────────────────────────────────────────────────────────────

export interface DraftDiff<T> {
  /** 추가·수정된 행(새 값). */
  upserts: T[]
  /** 저장본에는 있는데 초안에 없는 id(행 삭제 또는 id 변경). */
  deletes: number[]
  /** 초안 안에서 두 번 이상 나오는 id. */
  dupIds: number[]
  added: number
  changed: number
}

export function diffDraft<T extends { id: number }>(
  rows: readonly DraftRow<T>[],
  savedIds: readonly number[],
): DraftDiff<T> {
  const count = new Map<number, number>()
  for (const r of rows) count.set(r.value.id, (count.get(r.value.id) ?? 0) + 1)
  const present = new Set(count.keys())
  return {
    upserts: rows.filter((r) => r.state !== 'same').map((r) => r.value),
    deletes: savedIds.filter((id) => !present.has(id)),
    dupIds: [...count]
      .filter(([, n]) => n > 1)
      .map(([id]) => id)
      .sort((a, b) => a - b),
    added: rows.filter((r) => r.state === 'added').length,
    changed: rows.filter((r) => r.state === 'changed').length,
  }
}
