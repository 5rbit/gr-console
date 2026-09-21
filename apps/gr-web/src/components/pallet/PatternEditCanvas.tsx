// 패턴 편집 그림 — 원을 끌어 슬롯을 옮긴다(보기 방향 → 기계 축 변환, 스냅), 두 번 누르면 DragDir 순환,
// "Seq 재정렬" 중에는 누른 순서가 Seq 가 된다. 좌표는 편집 좌표(u × (OuterDiameter + Gap), mm)로 그리고,
// MinDistance ≠ 1 이면 생성기가 맞춘 결과를 점선 원으로 겹쳐 보인다.
//
// 키보드: 원에 초점을 두고 방향키 = 스냅 단위로 이동(스냅 끔 = 0.01), Enter/Space = 선택, D = DragDir 순환.
import { useId, useRef, useState } from 'react'
import type * as React from 'react'
import { dragToU, scaleOf, viewHalfExtent, type DraftSlot, type LiveIssue, type SnapMode } from '../../lib/pallet/editorModel'
import { axisLabels, dragArrow, toScreen, type DragKind, type ScreenAxes } from '../../lib/pallet/model'
import { cn } from '../../lib/utils'

const MARGIN = 300

export interface PatternEditCanvasProps {
  slots: DraftSlot[]
  axes: ScreenAxes
  pitch: number
  od: number
  palletSize: number
  dragKind: DragKind
  dragDist: number
  snap: SnapMode
  issues: LiveIssue[]
  selected: string | null
  onSelect: (slot: string | null) => void
  onMove: (slot: string, u: [number, number]) => void
  onCycleDir: (slot: string) => void
  seqMode: boolean
  clicked: string[]
  onSeqClick: (slot: string) => void
}

interface DragState {
  slot: string
  startU: [number, number]
  start: [number, number]
  moved: boolean
}

export function PatternEditCanvas(p: PatternEditCanvasProps) {
  const uid = useId().replace(/[^a-zA-Z0-9_-]/g, '')
  const svgRef = useRef<SVGSVGElement>(null)
  const drag = useRef<DragState | null>(null)
  const [dragging, setDragging] = useState<string | null>(null)
  const pitch = p.pitch > 0 ? p.pitch : 1
  const half = p.palletSize / 2
  const ext = viewHalfExtent(p.slots, pitch, p.od, p.palletSize) + MARGIN
  const r = p.od / 2
  const font = Math.max(34, p.palletSize / 40)
  const lab = axisLabels(p.axes)
  const scale = scaleOf(p.slots)
  const ghost = p.slots.length > 1 && Math.abs(scale - 1) > 1e-3

  function svgPoint(e: { clientX: number; clientY: number }): [number, number] | null {
    const svg = svgRef.current
    const m = svg?.getScreenCTM()
    if (!svg || !m) return null
    const pt = svg.createSVGPoint()
    pt.x = e.clientX
    pt.y = e.clientY
    const q = pt.matrixTransform(m.inverse())
    return [q.x, q.y]
  }

  function down(e: React.PointerEvent<SVGGElement>, s: DraftSlot) {
    if (e.button !== 0) return
    e.stopPropagation()
    const pt = svgPoint(e)
    if (!pt) return
    e.currentTarget.setPointerCapture(e.pointerId)
    drag.current = { slot: s.slot, startU: [s.u[0], s.u[1]], start: pt, moved: false }
    if (!p.seqMode) p.onSelect(s.slot)
  }

  function move(e: React.PointerEvent<SVGGElement>) {
    const d = drag.current
    if (!d || p.seqMode) return
    const pt = svgPoint(e)
    if (!pt) return
    if (!d.moved && Math.hypot(pt[0] - d.start[0], pt[1] - d.start[1]) < ext * 0.006) return
    d.moved = true
    setDragging(d.slot)
    p.onMove(d.slot, dragToU(d.startU, d.start, pt, p.axes, pitch, p.snap))
  }

  function up(e: React.PointerEvent<SVGGElement>) {
    const d = drag.current
    drag.current = null
    setDragging(null)
    if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId)
    if (d && !d.moved && p.seqMode) p.onSeqClick(d.slot)
  }

  function key(e: React.KeyboardEvent<SVGGElement>, s: DraftSlot) {
    const step = p.snap === 'mm10' ? 10 : p.snap === 'norm005' ? 0.05 * pitch : 0.01 * pitch
    const delta: Record<string, [number, number]> = {
      ArrowRight: [step, 0],
      ArrowLeft: [-step, 0],
      ArrowDown: [0, step],
      ArrowUp: [0, -step],
    }
    const dd = delta[e.key]
    if (dd && !p.seqMode) {
      e.preventDefault()
      p.onMove(s.slot, dragToU(s.u, [0, 0], dd, p.axes, pitch, p.snap))
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault()
      if (p.seqMode) p.onSeqClick(s.slot)
      else p.onSelect(p.selected === s.slot ? null : s.slot)
    } else if ((e.key === 'd' || e.key === 'D') && !p.seqMode) {
      e.preventDefault()
      p.onCycleDir(s.slot)
    }
  }

  const tone = (slot: string) => {
    const mine = p.issues.filter((i) => i.slot === slot)
    if (mine.some((i) => i.kind === 'overlap')) return 'fill-danger-soft stroke-danger'
    if (mine.some((i) => i.kind === 'overhang')) return 'fill-warn-soft stroke-warn'
    return 'fill-surface-panel stroke-accent'
  }

  return (
    <svg
      ref={svgRef}
      viewBox={`${-ext} ${-ext} ${2 * ext} ${2 * ext}`}
      role="application"
      aria-label={`패턴 편집 — 슬롯 ${p.slots.length}개. 원을 끌어 옮기고, 두 번 누르면 DragDir 이 바뀝니다`}
      className="aspect-square w-full max-w-2xl touch-none select-none rounded-md border border-line-default bg-surface-panel"
      onPointerDown={() => {
        if (!p.seqMode) p.onSelect(null)
      }}
      data-testid="pallet-edit-svg"
    >
      <defs>
        <marker id={`${uid}-head`} viewBox="0 0 10 10" refX="9" refY="5" markerUnits="userSpaceOnUse" markerWidth={70} markerHeight={70} orient="auto">
          <path d="M0,0 L10,5 L0,10 z" className="fill-accent" />
        </marker>
      </defs>
      <rect x={-half} y={-half} width={p.palletSize} height={p.palletSize} rx={20} className="fill-surface-inset stroke-line-strong" strokeWidth={1.5} vectorEffect="non-scaling-stroke" />
      <line x1={-60} x2={60} y1={0} y2={0} className="stroke-content-faint" strokeWidth={1} vectorEffect="non-scaling-stroke" />
      <line x1={0} x2={0} y1={-60} y2={60} className="stroke-content-faint" strokeWidth={1} vectorEffect="non-scaling-stroke" />
      <g className="fill-content-muted" fontSize={font} pointerEvents="none">
        <text x={ext - MARGIN / 2} y={0} textAnchor="middle" dominantBaseline="middle">
          {lab.right}
        </text>
        <text x={-ext + MARGIN / 2} y={0} textAnchor="middle" dominantBaseline="middle">
          {lab.left}
        </text>
        <text x={0} y={-ext + MARGIN / 2} textAnchor="middle" dominantBaseline="middle">
          {lab.up}
        </text>
        <text x={0} y={ext - MARGIN / 2} textAnchor="middle" dominantBaseline="middle">
          {lab.down}
        </text>
      </g>

      {ghost &&
        p.slots.map((s) => {
          const [cx, cy] = toScreen(p.axes, [s.u[0] * pitch * scale, s.u[1] * pitch * scale])
          return (
            <circle
              key={`ghost-${s.slot}`}
              cx={cx}
              cy={cy}
              r={r}
              className="fill-none stroke-content-faint"
              strokeDasharray="6 6"
              strokeWidth={1.5}
              vectorEffect="non-scaling-stroke"
              pointerEvents="none"
              data-testid="pallet-edit-ghost"
            />
          )
        })}

      {p.slots.map((s) => {
        const [cx, cy] = toScreen(p.axes, [s.u[0] * pitch, s.u[1] * pitch])
        const on = p.selected === s.slot
        const order = p.clicked.indexOf(s.slot)
        return (
          <g
            key={s.slot}
            role="button"
            tabIndex={0}
            aria-label={`${s.slot} Seq ${s.seq} DragDir ${s.drag_dir}`}
            aria-pressed={on}
            className={cn('outline-none', p.seqMode ? 'cursor-pointer' : dragging === s.slot ? 'cursor-grabbing' : 'cursor-grab')}
            onPointerDown={(e) => down(e, s)}
            onPointerMove={move}
            onPointerUp={up}
            onPointerCancel={up}
            onDoubleClick={() => {
              if (!p.seqMode) p.onCycleDir(s.slot)
            }}
            onKeyDown={(e) => key(e, s)}
            data-testid="pallet-edit-slot"
            data-slot={s.slot}
          >
            <circle cx={cx} cy={cy} r={r} className={tone(s.slot)} strokeWidth={on ? 5 : 1.5} vectorEffect="non-scaling-stroke" />
            <text x={cx} y={cy - font * 1.1} textAnchor="middle" className="fill-content-muted" fontSize={font * 0.8} pointerEvents="none">
              {s.slot}
            </text>
            <text x={cx} y={cy + font * 0.6} textAnchor="middle" className="fill-content-primary font-semibold" fontSize={font * 1.9} pointerEvents="none">
              {p.seqMode && order >= 0 ? `→${order + 1}` : s.seq}
            </text>
            <text x={cx} y={cy + font * 1.8} textAnchor="middle" className="fill-accent-text" fontSize={font * 0.75} pointerEvents="none">
              {`DragDir ${s.drag_dir}`}
            </text>
          </g>
        )
      })}

      {p.slots.map((s) => {
        const a = dragArrow([s.u[0] * pitch, s.u[1] * pitch], s.drag_dir, r, p.dragDist, p.dragKind)
        if (!a) return null
        const [x1, y1] = toScreen(p.axes, a.from)
        const [x2, y2] = toScreen(p.axes, a.to)
        return (
          <line key={`arrow-${s.slot}`} x1={x1} y1={y1} x2={x2} y2={y2} className="stroke-accent" strokeWidth={2.5} vectorEffect="non-scaling-stroke" markerEnd={`url(#${uid}-head)`} pointerEvents="none" />
        )
      })}
    </svg>
  )
}
