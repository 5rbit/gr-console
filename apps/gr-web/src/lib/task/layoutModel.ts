// 셀/스테이션 레이아웃 맵의 순수 계산 — PLC 좌표계(mm) ↔ 화면 픽셀 변환, 도형 목록, 맞춤(fit).
//
// 좌표계: 기본은 PLC X+ = 화면 오른쪽, PLC Y+ = 화면 위쪽. 현장 배치가 반대면 `flipX` / `flipY` 로 각각 뒤집는다.
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

/** 화면 변환 — `sx = ox + x * k`, `sy = oy ∓ y * k` (flipY 에 따라). */
export interface View {
  k: number
  ox: number
  oy: number
  /** PLC Y+ 가 화면 위쪽. */
  flipY: boolean
  /** PLC X+ 가 화면 왼쪽(현장 배치가 반대일 때). */
  flipX: boolean
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
      label: `셀 #${c.id}`,
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
      label: `스테이션 #${s.id} (CV${s.conv_no})`,
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

/** 경계가 `w × h` 픽셀 안에 여백 `margin` 을 두고 들어가는 변환. 폭·높이가 0 이면 1 mm = 1 px 로 둔다. */
export function fitView(
  b: Bounds,
  w: number,
  h: number,
  margin = 24,
  flipY = true,
  flipX = false,
): View {
  const bw = Math.max(b.maxX - b.minX, 1)
  const bh = Math.max(b.maxY - b.minY, 1)
  const k = Math.max(Math.min((w - 2 * margin) / bw, (h - 2 * margin) / bh), 1e-6)
  const cx = (b.minX + b.maxX) / 2
  const cy = (b.minY + b.maxY) / 2
  const ox = flipX ? w / 2 + cx * k : w / 2 - cx * k
  const oy = flipY ? h / 2 + cy * k : h / 2 - cy * k
  return { k, ox, oy, flipY, flipX }
}

export function toScreen(v: View, x: number, y: number): [number, number] {
  return [v.flipX ? v.ox - x * v.k : v.ox + x * v.k, v.flipY ? v.oy - y * v.k : v.oy + y * v.k]
}

export function toWorld(v: View, sx: number, sy: number): [number, number] {
  return [
    v.flipX ? (v.ox - sx) / v.k : (sx - v.ox) / v.k,
    v.flipY ? (v.oy - sy) / v.k : (sy - v.oy) / v.k,
  ]
}

/** 화면 점 `(sx, sy)` 를 고정한 채 배율을 `factor` 배 한다. */
export function zoomAt(
  v: View,
  sx: number,
  sy: number,
  factor: number,
  min = 1e-4,
  max = 10,
): View {
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
  return `${s.label} · S${s.section} R${s.row} C${s.col}${s.use ? '' : ' (미사용)'} · ${pos}${slot}`
}
