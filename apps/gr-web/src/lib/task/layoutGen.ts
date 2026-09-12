// 셀 레이아웃 생성 규칙 — 구간(Section)별로 원형 셀을 격자 또는 허니콤(지그재그)으로 깔아 좌표를 만든다.
//
// 허니콤은 같은 지름·간격에서 행 간격이 pitch·√3/2 라 격자보다 약 13% 좁게 들어간다(최소 영역에 최대 배치).
// 셀은 id 로만 구분하므로 규칙은 "시작 id 부터 순서대로" 번호를 매긴다(행 우선). Row/Col 은 규칙 안의 위치다.
import type { Cell, CellUpsert } from '../types'

export type Pattern = 'grid' | 'honeycomb'

export interface LayoutRule {
  /** 구간 번호(1..3, PLC 검증 규칙). */
  section: number
  /** 첫 셀 id. 이후 셀은 +1 씩(행 우선). */
  startId: number
  /** 첫 셀(1행 1열)의 중심 X/Y (mm). */
  originX: number
  originY: number
  /** 바닥 Z (mm). */
  z: number
  /** 셀(원) 지름 (mm). */
  diameter: number
  /** 원과 원 사이 띄우는 간격 (mm). 중심 간 거리 = diameter + gap. */
  gap: number
  /** 열 수(행 방향 셀 수). */
  cols: number
  /** 행 수. */
  rows: number
  pattern: Pattern
  /** 허니콤일 때 홀수 행(2,4,…)을 한 칸 짧게 해서 폭 안에 맞춘다. */
  shortOddRows: boolean
  /** X 진행 방향: +1 오른쪽(PLC X 증가), -1 왼쪽. */
  dirX: 1 | -1
  /** Y 진행 방향: +1 (PLC Y 증가), -1. */
  dirY: 1 | -1
  /** 행 우선(가로로 번호 증가) 인지 열 우선인지. */
  order: 'row' | 'col'
  /** 지그재그 번호(뱀 모양: 짝수 행은 반대 방향으로 번호). */
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
  if (!(r.originX > 0) || !(r.originY > 0) || !(r.z > 0)) out.push('원점 X/Y/Z > 0 (PLC 규칙)')
  return out
}

/** 규칙으로 셀 목록을 만든다(번호 순). */
export function generate(r: LayoutRule): CellUpsert[] {
  if (validateRule(r).length) return []
  const pitch = r.diameter + r.gap
  const rowPitch = r.pattern === 'honeycomb' ? pitch * HEX_ROW : pitch
  const slot = r.slot > 0 ? r.slot : r.diameter
  // 위치 격자 (row, col) → 중심
  const spots: { row: number; col: number; x: number; y: number }[] = []
  for (let row = 0; row < r.rows; row++) {
    const odd = row % 2 === 1
    const offset = r.pattern === 'honeycomb' && odd ? pitch / 2 : 0
    const n = r.pattern === 'honeycomb' && odd && r.shortOddRows ? Math.max(r.cols - 1, 1) : r.cols
    for (let col = 0; col < n; col++) {
      spots.push({
        row,
        col,
        x: r.originX + r.dirX * (col * pitch + offset),
        y: r.originY + r.dirY * row * rowPitch,
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

/** 생성 결과의 외곽 크기(mm) — 최소 영역 비교용. */
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

/** 주어진 영역(폭 × 높이)에 들어가는 최대 열·행 수 — 규칙의 열/행 자동 채움용. */
export function fitCount(
  w: number,
  h: number,
  diameter: number,
  gap: number,
  pattern: Pattern,
): { cols: number; rows: number } {
  const pitch = diameter + gap
  if (!(pitch > 0) || w < diameter || h < diameter) return { cols: 0, rows: 0 }
  const cols = Math.floor((w - diameter) / pitch) + 1
  const rowPitch = pattern === 'honeycomb' ? pitch * HEX_ROW : pitch
  const rows = Math.floor((h - diameter) / rowPitch) + 1
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

/** 기존 셀들에서 규칙을 역추정한다(구간 선택 시 폼 채우기용) — 대략값. */
export function inferRule(cells: readonly Cell[], section: number): Partial<LayoutRule> | null {
  const cs = cells.filter((c) => c.section === section).sort((a, b) => a.id - b.id)
  if (cs.length < 2) return null
  // 같은 행의 이웃 열 간격 = pitch, 1행→2행 = 행 간격·반 칸 어긋남(허니콤).
  const row1 = cs.filter((c) => c.row === 1).sort((a, b) => a.col - b.col)
  const row2 = cs.filter((c) => c.row === 2).sort((a, b) => a.col - b.col)
  const diameter = cs[0].length || 800
  const dx = row1.length > 1 ? row1[1].position[0] - row1[0].position[0] : 0
  const pitch = Math.abs(dx) > 0 ? Math.abs(dx) : diameter
  const dy = row2.length && row1.length ? row2[0].position[1] - row1[0].position[1] : 0
  const shift = row2.length && row1.length ? Math.abs(row2[0].position[0] - row1[0].position[0]) : 0
  const honeycomb =
    dy !== 0 && Math.abs(shift - pitch / 2) < 1 && Math.abs(Math.abs(dy) / pitch - HEX_ROW) < 0.05
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
