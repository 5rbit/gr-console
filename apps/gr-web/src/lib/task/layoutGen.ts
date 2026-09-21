// 셀 레이아웃 생성 규칙 — 구간(Section)별로 원형 셀을 격자 또는 허니콤(지그재그)으로 깔아 좌표를 만든다.
//
// 축 규약: **행(Row) = X 방향, 열(Col) = Y 방향**. 행 번호가 늘면 X 로 한 행 간격만큼 나아가고,
// 한 행 안에서 열 번호가 늘면 Y 로 한 칸(지름 + 간격)씩 나아간다.
// 허니콤은 홀수 행(2,4,…)을 Y 로 반 칸 어긋나게 두고 행 간격을 pitch·√3/2 로 좁혀 격자보다 약 13% 좁게 들어간다.
// 셀은 id 로만 구분하므로 규칙은 "시작 id 부터 순서대로" 번호를 매긴다. Row/Col 은 규칙 안의 위치다.
//
// 좌표 조건(PLC 근거): 셀 X/Y 는 0 이상이어야 한다(태스크 위치는 MACHINE.Args.RangeMin..Max 로만 검사).
// 진행 방향 때문에 좌표가 음수가 되면 오류다. 바닥 Z 는 **바닥 평탄도 보정**이라 0·음수도 맞는 값이다 —
// isValidTaskData 의 INVALID_CELL_POSZ 는 스테이션(Cell.Id > 2000)만 보므로 셀에는 오류도 경고도 없다.
import type { Cell, CellBulkMode, CellBulkOutcome, CellUpsert } from '../types'

export type Pattern = 'grid' | 'honeycomb'

export interface LayoutRule {
  /** 구간 번호(1..3). */
  section: number
  /** 첫 셀 id. 이후 셀은 +1 씩. */
  startId: number
  /** 첫 셀(1행 1열)의 중심 X/Y (mm). 0 도 된다. */
  originX: number
  originY: number
  /** 바닥 Z (mm) — 바닥 평탄도 보정이라 0·음수도 된다. */
  z: number
  /** 셀(원) 지름 (mm). */
  diameter: number
  /** 원과 원 사이 띄우는 간격 (mm). 중심 간 거리 = diameter + gap. */
  gap: number
  /** 열 수 — 한 행 안의 셀 수(Y 방향). */
  cols: number
  /** 행 수(X 방향). */
  rows: number
  pattern: Pattern
  /** 허니콤일 때 홀수 행(2,4,…)을 한 칸 짧게 해서 Y 폭 안에 맞춘다. */
  shortOddRows: boolean
  /** 행이 나아가는 X 방향: +1 = X 증가, -1 = X 감소. */
  dirX: 1 | -1
  /** 열이 나아가는 Y 방향: +1 = Y 증가, -1 = Y 감소. */
  dirY: 1 | -1
  /** 번호 순서: 행 우선(한 행의 열을 다 매기고 다음 행) / 열 우선. */
  order: 'row' | 'col'
  /** 뱀 번호(짝수 번째 행/열은 반대 방향으로 번호). */
  serpentine: boolean
  /** 슬롯 length/width 로 기록할 값(0 이면 diameter). */
  slot: number
}

export const DEFAULT_RULE: LayoutRule = {
  section: 1,
  startId: 101,
  originX: 1000,
  originY: 1000,
  z: 1500,
  diameter: 800,
  gap: 100,
  cols: 10,
  rows: 4,
  pattern: 'honeycomb',
  shortOddRows: true,
  dirX: 1,
  dirY: 1,
  order: 'row',
  serpentine: false,
  slot: 0,
}

export const HEX_ROW = Math.sqrt(3) / 2

const pitchOf = (r: LayoutRule) => r.diameter + r.gap
const rowPitchOf = (r: LayoutRule) =>
  r.pattern === 'honeycomb' ? pitchOf(r) * HEX_ROW : pitchOf(r)

/** 규칙의 문제점(빈 배열이면 생성 가능). */
export function validateRule(r: LayoutRule): string[] {
  const out: string[] = []
  if (!(r.section >= 1 && r.section <= 3)) out.push('구간은 1..3')
  if (!(r.startId >= 1 && r.startId <= 1000)) out.push('시작 id 는 1..1000')
  if (!(r.diameter > 0)) out.push('지름 > 0')
  if (r.gap < 0) out.push('간격 ≥ 0')
  if (!(r.cols >= 1 && r.cols <= 200)) out.push('열 수 1..200')
  if (!(r.rows >= 1 && r.rows <= 200)) out.push('행 수 1..200')
  if (r.startId + r.cols * r.rows - 1 > 1000)
    out.push(`마지막 id ${r.startId + r.cols * r.rows - 1} > 1000`)
  if (!Number.isFinite(r.originX) || !Number.isFinite(r.originY))
    out.push('원점 X/Y 가 숫자가 아님')
  if (!Number.isFinite(r.z)) out.push('바닥 Z 가 숫자가 아님')
  if (r.diameter > 0 && Number.isFinite(r.originX) && Number.isFinite(r.originY)) {
    const minX = r.originX + Math.min(0, r.dirX * (r.rows - 1) * rowPitchOf(r))
    const yReach =
      (r.cols - 1) * pitchOf(r) + (r.pattern === 'honeycomb' && r.rows > 1 ? pitchOf(r) / 2 : 0)
    const minY = r.originY + Math.min(0, r.dirY * yReach)
    if (minX < 0 || minY < 0)
      out.push(
        `셀 좌표가 음수가 됩니다 (최소 X ${minX.toFixed(0)} · Y ${minY.toFixed(0)}) — 원점·진행 방향 확인`,
      )
  }
  return out
}

/** 생성은 되지만 알려야 하는 것 — 지금은 없다(셀 바닥 Z 는 음수도 맞고 PLC 도 검사하지 않는다). */
export function ruleWarnings(_r: LayoutRule): string[] {
  return []
}

/** 규칙으로 셀 목록을 만든다(번호 순). */
export function generate(r: LayoutRule): CellUpsert[] {
  if (validateRule(r).length) return []
  const pitch = pitchOf(r)
  const rowPitch = rowPitchOf(r)
  const slot = r.slot > 0 ? r.slot : r.diameter
  // 위치 격자 (row → X, col → Y) → 중심
  const spots: { row: number; col: number; x: number; y: number }[] = []
  for (let row = 0; row < r.rows; row++) {
    const odd = row % 2 === 1
    const offset = r.pattern === 'honeycomb' && odd ? pitch / 2 : 0
    const n = r.pattern === 'honeycomb' && odd && r.shortOddRows ? Math.max(r.cols - 1, 1) : r.cols
    for (let col = 0; col < n; col++) {
      spots.push({
        row,
        col,
        x: r.originX + r.dirX * row * rowPitch,
        y: r.originY + r.dirY * (col * pitch + offset),
      })
    }
  }
  // 번호 순서
  const ordered = [...spots].sort((a, b) => {
    if (r.order === 'row') {
      if (a.row !== b.row) return a.row - b.row
      const flip = r.serpentine && a.row % 2 === 1
      return flip ? b.col - a.col : a.col - b.col
    }
    if (a.col !== b.col) return a.col - b.col
    const flip = r.serpentine && a.col % 2 === 1
    return flip ? b.row - a.row : a.row - b.row
  })
  return ordered.map((s, i) => ({
    id: r.startId + i,
    use: true,
    blend_use: false,
    section: r.section,
    row: s.row + 1,
    col: s.col + 1,
    length: slot,
    width: slot,
    position: [round1(s.x), round1(s.y), round1(r.z)],
  }))
}

function round1(n: number): number {
  return Math.round(n * 10) / 10
}

/** 생성 결과의 외곽 크기(mm) — w = X 길이, h = Y 길이. */
export function extent(
  cells: readonly CellUpsert[],
  diameter: number,
): { w: number; h: number; area: number } {
  if (!cells.length) return { w: 0, h: 0, area: 0 }
  let minX = Infinity
  let minY = Infinity
  let maxX = -Infinity
  let maxY = -Infinity
  for (const c of cells) {
    minX = Math.min(minX, c.position[0])
    maxX = Math.max(maxX, c.position[0])
    minY = Math.min(minY, c.position[1])
    maxY = Math.max(maxY, c.position[1])
  }
  const w = maxX - minX + diameter
  const h = maxY - minY + diameter
  return { w: round1(w), h: round1(h), area: round1(w * h) }
}

/** 영역(X 길이 × Y 길이)에 들어가는 최대 행·열 수 — 행은 X, 열은 Y 로 센다. */
export function fitCount(
  xLen: number,
  yLen: number,
  diameter: number,
  gap: number,
  pattern: Pattern,
): { cols: number; rows: number } {
  const pitch = diameter + gap
  if (!(pitch > 0) || xLen < diameter || yLen < diameter) return { cols: 0, rows: 0 }
  const rowPitch = pattern === 'honeycomb' ? pitch * HEX_ROW : pitch
  const rows = Math.floor((xLen - diameter) / rowPitch) + 1
  const cols = Math.floor((yLen - diameter) / pitch) + 1
  return { cols, rows }
}

export interface Conflict {
  id: number
  reason: string
}

/** 기존 셀과의 충돌 — 같은 id(덮어쓰기) 또는 다른 구간 셀과 중심 거리 < 지름(겹침). */
export function conflicts(
  gen: readonly CellUpsert[],
  existing: readonly Cell[],
  diameter: number,
  replaceSection: number | null,
): Conflict[] {
  const out: Conflict[] = []
  const keep = existing.filter((c) => replaceSection === null || c.section !== replaceSection)
  const genIds = new Set(gen.map((c) => c.id))
  for (const c of keep) {
    if (genIds.has(c.id))
      out.push({ id: c.id, reason: `기존 셀 #${c.id} (구간 ${c.section}) 와 id 중복` })
  }
  for (const g of gen) {
    for (const c of keep) {
      if (c.id === g.id) continue
      const d = Math.hypot(c.position[0] - g.position[0], c.position[1] - g.position[1])
      if (d < diameter - 0.5) {
        out.push({
          id: g.id,
          reason: `#${g.id} 가 기존 셀 #${c.id} 와 겹침 (중심 거리 ${d.toFixed(0)} < ${diameter})`,
        })
        break
      }
    }
  }
  return out
}

/** 기존 셀들에서 규칙을 역추정한다(구간 선택 시 폼 채우기용) — 대략값. 행 = X, 열 = Y. */
export function inferRule(cells: readonly Cell[], section: number): Partial<LayoutRule> | null {
  const cs = cells.filter((c) => c.section === section).sort((a, b) => a.id - b.id)
  if (cs.length < 2) return null
  // 1행 안의 이웃 열 간격(Y) = pitch, 1행 → 2행(X) = 행 간격, 허니콤이면 Y 로 반 칸 어긋남.
  const row1 = cs.filter((c) => c.row === 1).sort((a, b) => a.col - b.col)
  const row2 = cs.filter((c) => c.row === 2).sort((a, b) => a.col - b.col)
  const diameter = cs[0].length || 800
  const dy = row1.length > 1 ? row1[1].position[1] - row1[0].position[1] : 0
  const pitch = Math.abs(dy) > 0 ? Math.abs(dy) : diameter
  const dx = row2.length && row1.length ? row2[0].position[0] - row1[0].position[0] : 0
  const shift = row2.length && row1.length ? Math.abs(row2[0].position[1] - row1[0].position[1]) : 0
  const honeycomb =
    dx !== 0 && Math.abs(shift - pitch / 2) < 1 && Math.abs(Math.abs(dx) / pitch - HEX_ROW) < 0.05
  return {
    section,
    startId: cs[0].id,
    originX: cs[0].position[0],
    originY: cs[0].position[1],
    z: cs[0].position[2],
    diameter,
    gap: Math.max(round1(pitch - diameter), 0),
    cols: Math.max(...cs.map((c) => c.col)),
    rows: Math.max(...cs.map((c) => c.row)),
    pattern: honeycomb ? 'honeycomb' : 'grid',
    dirX: dx < 0 ? -1 : 1,
    dirY: dy < 0 ? -1 : 1,
  }
}

// ── 적용 미리보기 ────────────────────────────────────────────────────────────
//
// 서버(`registry/bulk.rs`)와 **같은 규칙**으로 "이대로 적용하면 무엇이 되는지"를 먼저 센다.
// 창이 여럿이면 그 사이에 셀이 바뀔 수 있으니 **판정은 서버가 진실**이고, 여기 결과는 미리보기다
// (적용 뒤에는 서버가 돌려준 줄별 결과를 그대로 보여 준다).
export const OVERLAP_TOLERANCE = 0.5

export interface PreviewRow {
  /** 보낼 id. */
  id: number
  /** `autoId` 로 옮길 id. */
  newId?: number
  outcome: CellBulkOutcome
  reason?: string
}

export interface PreviewPlan {
  rows: PreviewRow[]
  /** 맵·표에서 셀 하나의 표시를 찾을 때(보낸 id 기준). */
  byId: Map<number, PreviewRow>
  counts: Record<CellBulkOutcome, number>
  /** 구역 교체에서 지워질 기존 셀 수. */
  removed: number
}

export interface PreviewOptions {
  mode: CellBulkMode
  /** `replace_section` 대상 구역. */
  section: number
  /** 겹침 판정 지름(mm). 없으면 셀의 max(length, width). */
  diameter?: number
  autoId?: boolean
  tolerance?: number
}

const sameCell = (a: CellUpsert, b: CellUpsert): boolean =>
  a.id === b.id &&
  a.use === b.use &&
  a.blend_use === b.blend_use &&
  a.section === b.section &&
  a.row === b.row &&
  a.col === b.col &&
  a.length === b.length &&
  a.width === b.width &&
  a.position[0] === b.position[0] &&
  a.position[1] === b.position[1] &&
  a.position[2] === b.position[2]

const cellDiameter = (c: CellUpsert, d?: number): number =>
  d && d > 0 ? d : Math.max(c.length, c.width)

/** 남는 셀 중 중심이 가장 가까우면서 지름 안으로 들어온 것. */
function nearestOverlap(
  c: CellUpsert,
  placed: readonly CellUpsert[],
  diameter: number,
  tolerance: number,
): { hit: CellUpsert; d: number } | null {
  const limit = diameter - tolerance
  if (!(limit > 0)) return null
  let best: { hit: CellUpsert; d: number } | null = null
  for (const k of placed) {
    if (k.id === c.id) continue
    const d = Math.hypot(k.position[0] - c.position[0], k.position[1] - c.position[1])
    if (d < limit && (!best || d < best.d)) best = { hit: k, d }
  }
  return best
}

/** 그 구역 뒤의 첫 빈 id — 이미 깔린 블록 다음으로 이어 붙인다. */
function nextFreeId(
  section: number,
  want: number,
  placed: readonly CellUpsert[],
  used: Set<number>,
): number | null {
  const secMax = placed.reduce((m, k) => (k.section === section ? Math.max(m, k.id) : m), 0)
  for (let id = Math.max(secMax, want) + 1; id <= 1000; id++) if (!used.has(id)) return id
  return null
}

/** 적용 결과 미리보기 — 서버 `bulk::plan` 과 같은 순서·같은 판정. */
export function previewApply(
  gen: readonly CellUpsert[],
  existing: readonly Cell[],
  o: PreviewOptions,
): PreviewPlan {
  const tolerance = o.tolerance ?? OVERLAP_TOLERANCE
  const genIds = new Set(gen.map((c) => c.id))
  const wiped =
    o.mode === 'replace_section'
      ? new Set(existing.filter((c) => c.section === o.section).map((c) => c.id))
      : new Set<number>()
  const removed = [...wiped].filter((id) => !genIds.has(id)).length
  const byIdExisting = new Map(existing.map((c) => [c.id, c]))
  const placed: CellUpsert[] = existing.filter((c) => !wiped.has(c.id))
  const used = new Set(existing.map((c) => c.id))
  const rows: PreviewRow[] = []

  for (const g of gen) {
    const prev = byIdExisting.get(g.id)
    if (prev && sameCell(prev, g)) {
      rows.push({ id: g.id, outcome: 'unchanged' })
      continue
    }
    let id = g.id
    let newId: number | undefined
    const clash = placed.find((k) => k.id === g.id)
    if (clash) {
      const why = `기존 셀 #${clash.id} (구간 ${clash.section}) 와 id 중복`
      if (o.mode === 'append' && o.autoId) {
        const free = nextFreeId(g.section, g.id, placed, used)
        if (free === null) {
          rows.push({
            id: g.id,
            outcome: 'skipped',
            reason: `${why} · 구간 ${g.section} 에 빈 id 가 없습니다`,
          })
          continue
        }
        id = free
        newId = free
      } else if (o.mode === 'append') {
        rows.push({ id: g.id, outcome: 'skipped', reason: why })
        continue
      }
    }
    const c: CellUpsert = { ...g, id }
    const diameter = cellDiameter(c, o.diameter)
    const over = nearestOverlap(c, placed, diameter, tolerance)
    if (over && o.mode === 'append') {
      rows.push({
        id: g.id,
        newId,
        outcome: 'skipped',
        reason: `기존 셀 #${over.hit.id} 와 겹침 (중심 거리 ${over.d.toFixed(0)} < 지름 ${diameter.toFixed(0)})`,
      })
      continue
    }
    const outcome: CellBulkOutcome =
      newId !== undefined ? 'remapped' : used.has(id) ? 'updated' : 'added'
    rows.push({
      id: g.id,
      newId,
      outcome,
      reason: over
        ? `기존 셀 #${over.hit.id} 와 겹치지만 덮어씁니다 (중심 거리 ${over.d.toFixed(0)})`
        : undefined,
    })
    used.add(id)
    const at = placed.findIndex((k) => k.id === id)
    if (at >= 0) placed.splice(at, 1)
    placed.push(c)
  }

  const counts: Record<CellBulkOutcome, number> = {
    added: 0,
    updated: 0,
    unchanged: 0,
    skipped: 0,
    remapped: 0,
  }
  for (const r of rows) counts[r.outcome]++
  return { rows, byId: new Map(rows.map((r) => [r.id, r])), counts, removed }
}

/** 미리보기와 서버 응답이 같은 문장을 쓴다 — 서버는 추가를 `created` 로 부른다(옛 이름). */
export interface OutcomeCounts {
  added?: number
  created?: number
  updated?: number
  unchanged?: number
  skipped?: number
  remapped?: number
  removed?: number
}

/** 결과 한 줄 — 0 인 항목은 말하지 않는다. */
export function previewLabel(c: OutcomeCounts): string {
  const parts = [
    ['추가', c.added ?? c.created ?? 0],
    ['갱신', c.updated ?? 0],
    ['번호 변경', c.remapped ?? 0],
    ['건너뜀', c.skipped ?? 0],
    ['그대로', c.unchanged ?? 0],
    ['삭제', c.removed ?? 0],
  ] as const
  const on = parts.filter(([, n]) => n > 0)
  return on.length ? on.map(([k, n]) => `${k} ${n}`).join(' · ') : '바뀌는 것 없음'
}

export const MODE_LABEL: Record<CellBulkMode, string> = {
  append: '추가',
  replace_section: '구역 교체',
  overwrite: '덮어쓰기',
}

/** 적용 버튼이 **무엇을 할지** 말한다(모드 이름만으로는 삭제 여부가 안 보인다). */
export function applyButtonLabel(mode: CellBulkMode, p: PreviewPlan): string {
  if (mode === 'replace_section')
    return `구역 교체 — 삭제 ${p.removed} · 생성 ${p.counts.added + p.counts.updated}`
  if (mode === 'overwrite')
    return `덮어쓰기 ${p.counts.added + p.counts.updated + p.counts.remapped}칸`
  const n = p.counts.added + p.counts.remapped
  return p.counts.skipped ? `추가 ${n}칸 (건너뜀 ${p.counts.skipped})` : `추가 ${n}칸`
}

/** 맵에 넘길 생성 예정 셀 — 판정을 같이 실어 충돌한 칸을 다른 색·사유 툴팁으로 그린다. */
export interface PreviewCell extends CellUpsert {
  outcome: CellBulkOutcome
  reason?: string
  newId?: number
}

/** 생성 결과 + 미리보기 판정 → 맵에 그릴 셀. */
export function annotate(gen: readonly CellUpsert[], p: PreviewPlan): PreviewCell[] {
  return gen.map((c) => {
    const r = p.byId.get(c.id)
    return { ...c, outcome: r?.outcome ?? 'added', reason: r?.reason, newId: r?.newId }
  })
}
