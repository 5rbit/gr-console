// 셀/스테이션 레이아웃 맵의 순수 계산 — PLC 좌표계(mm) ↔ 화면 픽셀 변환, 도형 목록, 맞춤(fit).
//
// 좌표계: 기본은 PLC X+ = 화면 오른쪽, PLC Y+ = 화면 위쪽. 현장 배치에 맞춰 화면을 시계 방향으로
// 0·90·180·270° 돌리고(`rot`), 돌린 뒤의 화면 가로/세로를 각각 뒤집을 수 있다(`flipX` / `flipY`).
// 변환 순서: 월드 (x, y) → 회전 (u, v) → 반전·배율·원점 → 화면 (sx, sy).
// 도형은 모두 **중심점** = PLC Position[0..1] 에 놓인다 — 셀은 원(지름은 UI 에서 고른다), 스테이션은 정사각형.
import type { Cell, Station, Target } from '../types'

export interface Shape {
  kind: 'cell' | 'station'
  id: number
  /** PLC 좌표 (mm) — 도형 중심. */
  x: number
  y: number
  z: number
  /** 슬롯 실제 크기 (mm, length × width). 0 이면 모른다. */
  length: number
  width: number
  section: number
  row: number
  col: number
  use: boolean
  dirty: boolean
  label: string
}

export interface Bounds {
  minX: number
  minY: number
  maxX: number
  maxY: number
}

/** 화면 회전(시계 방향, 도). */
export type Rotation = 0 | 90 | 180 | 270
export const ROTATIONS: readonly Rotation[] = [0, 90, 180, 270]

/** 화면 변환 — 회전 좌표 (u, v) 에 대해 `sx = ox ± u·k`, `sy = oy ∓ v·k`. */
export interface View {
  k: number
  ox: number
  oy: number
  /** 회전 후 v+ 가 화면 위쪽(기본). false 면 상하 반전. */
  flipY: boolean
  /** 회전 후 u+ 가 화면 왼쪽(좌우 반전). */
  flipX: boolean
  /** 시계 방향 회전. 없으면 0. */
  rot?: Rotation
}

/** 월드 (x, y) → 회전 좌표 (u, v): 화면에서 시계 방향으로 `rot` 만큼 돌린 좌표. */
export function rotate(rot: Rotation | undefined, x: number, y: number): [number, number] {
  switch (rot) {
    case 90:
      return [y, -x]
    case 180:
      return [-x, -y]
    case 270:
      return [-y, x]
    default:
      return [x, y]
  }
}

/** 회전 좌표 (u, v) → 월드 (x, y). `rotate` 의 역. */
export function unrotate(rot: Rotation | undefined, u: number, v: number): [number, number] {
  switch (rot) {
    case 90:
      return [-v, u]
    case 180:
      return [-u, -v]
    case 270:
      return [v, -u]
    default:
      return [u, v]
  }
}

export function shapesFrom(cells: readonly Cell[], stations: readonly Station[]): Shape[] {
  const out: Shape[] = []
  for (const c of cells) {
    out.push({
      kind: 'cell',
      id: c.id,
      x: c.position[0],
      y: c.position[1],
      z: c.position[2],
      length: c.length,
      width: c.width,
      section: c.section,
      row: c.row,
      col: c.col,
      use: c.use,
      dirty: c.dirty,
      label: `Cell #${c.id}`,
    })
  }
  for (const s of stations) {
    const i = s.info
    out.push({
      kind: 'station',
      id: s.id,
      x: i.position[0],
      y: i.position[1],
      z: i.position[2],
      length: i.length,
      width: i.width,
      section: i.section,
      row: i.row,
      col: i.col,
      use: i.use,
      dirty: s.dirty,
      label: `Station #${s.id} (ConvNo ${s.conv_no})`,
    })
  }
  return out
}

/** 도형 중심에 반경 `pad` 를 더한 경계. 도형이 없으면 null. */
export function boundsOf(shapes: readonly Shape[], pad = 0): Bounds | null {
  if (!shapes.length) return null
  let minX = Infinity
  let minY = Infinity
  let maxX = -Infinity
  let maxY = -Infinity
  for (const s of shapes) {
    if (!Number.isFinite(s.x) || !Number.isFinite(s.y)) continue
    minX = Math.min(minX, s.x - pad)
    minY = Math.min(minY, s.y - pad)
    maxX = Math.max(maxX, s.x + pad)
    maxY = Math.max(maxY, s.y + pad)
  }
  if (!Number.isFinite(minX)) return null
  return { minX, minY, maxX, maxY }
}

/**
 * 경계가 `w × h` 픽셀 안에 여백 `margin` 을 두고 들어가는 변환.
 * 회전하면 돌린 뒤의 가로·세로 범위로 배율을 잡는다(90°/270° 에서는 폭과 높이가 바뀐다).
 */
export function fitView(b: Bounds, w: number, h: number, margin = 24, flipY = true, flipX = false, rot: Rotation = 0): View {
  const corners = [rotate(rot, b.minX, b.minY), rotate(rot, b.maxX, b.minY), rotate(rot, b.minX, b.maxY), rotate(rot, b.maxX, b.maxY)]
  const us = corners.map((c) => c[0])
  const vs = corners.map((c) => c[1])
  const minU = Math.min(...us)
  const maxU = Math.max(...us)
  const minV = Math.min(...vs)
  const maxV = Math.max(...vs)
  const bw = Math.max(maxU - minU, 1)
  const bh = Math.max(maxV - minV, 1)
  const k = Math.max(Math.min((w - 2 * margin) / bw, (h - 2 * margin) / bh), 1e-6)
  const cu = (minU + maxU) / 2
  const cv = (minV + maxV) / 2
  const ox = flipX ? w / 2 + cu * k : w / 2 - cu * k
  const oy = flipY ? h / 2 + cv * k : h / 2 - cv * k
  return { k, ox, oy, flipY, flipX, rot }
}

/**
 * 지금 그릴 변환. `manual` = 사용자가 끌기·확대·지목으로 만든 변환 — 회전·반전이 지금과 같을 때만 쓴다.
 * 없으면 **지금 크기(`w × h`)와 지금 경계로 매번 맞춘다**.
 *
 * 맞춤을 한 번 계산해 상태로 굳히면 안 된다: 첫 맞춤은 컨테이너를 재기 전(기본 800×500)이나 창·분할
 * 크기가 바뀌기 전의 크기로 계산되고, 그 뒤 크기가 바뀌어도 굳은 변환이 남아 레이아웃이 한쪽 귀퉁이에
 * 작게 몰리거나 화면 밖으로 잘린다(2026-09-21 "레이아웃이 화면에서 안 보임").
 */
export function resolveView(
  manual: View | null,
  b: Bounds | null,
  w: number,
  h: number,
  margin: number,
  flipY: boolean,
  flipX: boolean,
  rot: Rotation,
): View {
  if (manual && manual.flipY === flipY && manual.flipX === flipX && (manual.rot ?? 0) === rot) return manual
  return b ? fitView(b, w, h, margin, flipY, flipX, rot) : { k: 0.05, ox: w / 2, oy: h / 2, flipY, flipX, rot }
}

export function toScreen(v: View, x: number, y: number): [number, number] {
  const [u, w] = rotate(v.rot, x, y)
  return [v.flipX ? v.ox - u * v.k : v.ox + u * v.k, v.flipY ? v.oy - w * v.k : v.oy + w * v.k]
}

export function toWorld(v: View, sx: number, sy: number): [number, number] {
  const u = v.flipX ? (v.ox - sx) / v.k : (sx - v.ox) / v.k
  const w = v.flipY ? (v.oy - sy) / v.k : (sy - v.oy) / v.k
  return unrotate(v.rot, u, w)
}

/** 화면 점 `(sx, sy)` 를 고정한 채 배율을 `factor` 배 한다. */
export function zoomAt(v: View, sx: number, sy: number, factor: number, min = 1e-4, max = 10): View {
  const k = Math.min(Math.max(v.k * factor, min), max)
  const f = k / v.k
  return { ...v, k, ox: sx - (sx - v.ox) * f, oy: sy - (sy - v.oy) * f }
}

export function panBy(v: View, dx: number, dy: number): View {
  return { ...v, ox: v.ox + dx, oy: v.oy + dy }
}

/** 눈금 간격(mm) — 화면에서 최소 `minPx` 픽셀 이상 떨어지는 1·2·5 계열. */
export function gridStep(k: number, minPx = 60): number {
  const raw = minPx / k
  const p = 10 ** Math.floor(Math.log10(raw))
  for (const m of [1, 2, 5, 10]) if (m * p >= raw) return m * p
  return 10 * p
}

export function sameTarget(a: Target | null | undefined, s: Shape): boolean {
  return !!a && a.kind === s.kind && a.id === s.id
}

export const SIZE_PRESETS = [200, 300, 400, 500, 600, 700, 800, 1000] as const

/** 도형 하나의 정보 문장(툴팁·상태줄). */
export function shapeInfo(s: Shape): string {
  const pos = `X ${s.x.toFixed(0)} · Y ${s.y.toFixed(0)} · Z ${s.z.toFixed(0)}`
  const slot = s.length || s.width ? ` · ${s.length.toFixed(0)}×${s.width.toFixed(0)}` : ''
  return `${s.label} · S${s.section} R${s.row} C${s.col}${s.use ? '' : ' (Use=false)'} · ${pos}${slot}`
}
