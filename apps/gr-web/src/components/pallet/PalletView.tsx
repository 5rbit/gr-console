// 팔렛 그림 — 기계 좌표(mm)를 보기 방향으로 바꿔 SVG 한 장에. viewBox 단위가 mm 라 원 크기가 곧 실제 크기다.
//
// 원 = 타이어(외경), 큰 숫자 = Seq(작업 순서), 위 글자 = Slot, 아래 = DragDir. 화살표는 드래그 — 입고(Drag-in)는
// 슬롯 쪽으로, 출하(Drag-out)는 슬롯 밖으로, 길이 = DragDist. 팔렛 밖·GR2 영역 거부 예상은 경고색, 겹침은 위험색.
import { useId } from 'react'
import {
  axisLabels,
  dragArrow,
  slotTone,
  toScreen,
  type PalletPlan,
  type ScreenAxes,
  type SlotTone,
} from '../../lib/pallet/model'
import { cn } from '../../lib/utils'

/** 팔렛 둘레의 여백(mm) — 축 글자와 바깥으로 나가는 화살표 자리. */
const MARGIN = 300

const TONE: Record<SlotTone, string> = {
  ok: 'fill-surface-panel stroke-accent',
  warn: 'fill-warn-soft stroke-warn',
  error: 'fill-danger-soft stroke-danger',
}

export interface PalletViewProps {
  plan: PalletPlan
  axes: ScreenAxes
  /** 화살표 길이(mm) — 기본값의 DragInDist/DragOutDist. */
  dragDist: number
  selected: number | null
  onSelect: (seq: number | null) => void
}

export function PalletView({ plan, axes, dragDist, selected, onSelect }: PalletViewProps) {
  const uid = useId().replace(/[^a-zA-Z0-9_-]/g, '')
  const half = plan.pallet_size / 2
  const ext = half + MARGIN
  const r = plan.od / 2
  const font = Math.max(34, plan.pallet_size / 40)
  const lab = axisLabels(axes)
  const pick = (seq: number) => onSelect(selected === seq ? null : seq)

  return (
    <svg
      viewBox={`${-ext} ${-ext} ${2 * ext} ${2 * ext}`}
      role="img"
      aria-label={`Pallet ${plan.pallet_size} — Pattern ${plan.pattern}, ${plan.slots.length} slots`}
      className="aspect-square w-full max-w-2xl rounded-md border border-line-default bg-surface-panel"
      data-testid="pallet-svg"
    >
      <defs>
        <marker
          id={`${uid}-head`}
          viewBox="0 0 10 10"
          refX="9"
          refY="5"
          markerUnits="userSpaceOnUse"
          markerWidth={70}
          markerHeight={70}
          orient="auto"
        >
          <path d="M0,0 L10,5 L0,10 z" className="fill-accent" />
        </marker>
      </defs>
      <rect
        x={-half}
        y={-half}
        width={plan.pallet_size}
        height={plan.pallet_size}
        rx={20}
        className="fill-surface-inset stroke-line-strong"
        strokeWidth={1.5}
        vectorEffect="non-scaling-stroke"
      />
      <line x1={-60} x2={60} y1={0} y2={0} className="stroke-content-faint" strokeWidth={1} vectorEffect="non-scaling-stroke" />
      <line x1={0} x2={0} y1={-60} y2={60} className="stroke-content-faint" strokeWidth={1} vectorEffect="non-scaling-stroke" />
      <g className="fill-content-muted" fontSize={font}>
        <text x={half + MARGIN / 2} y={0} textAnchor="middle" dominantBaseline="middle">
          {lab.right}
        </text>
        <text x={-half - MARGIN / 2} y={0} textAnchor="middle" dominantBaseline="middle">
          {lab.left}
        </text>
        <text x={0} y={-half - MARGIN / 2} textAnchor="middle" dominantBaseline="middle">
          {lab.up}
        </text>
        <text x={0} y={half + MARGIN / 2} textAnchor="middle" dominantBaseline="middle">
          {lab.down}
        </text>
      </g>
      <text
        x={half}
        y={half + MARGIN * 0.8}
        textAnchor="end"
        className="fill-content-faint"
        fontSize={font * 0.7}
      >
        PalletSize {plan.pallet_size}
      </text>

      {plan.slots.map((s) => {
        const [cx, cy] = toScreen(axes, [s.offset_x, s.offset_y])
        const on = selected === s.seq
        return (
          <g
            key={s.slot}
            role="button"
            tabIndex={0}
            aria-label={`Seq ${s.seq} ${s.slot} X ${s.x} Y ${s.y} DragDir ${s.drag_dir}`}
            aria-pressed={on}
            className="cursor-pointer outline-none"
            onClick={() => pick(s.seq)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault()
                pick(s.seq)
              }
            }}
            data-testid="pallet-slot"
          >
            <circle
              cx={cx}
              cy={cy}
              r={r}
              className={cn(TONE[slotTone(plan, s)])}
              strokeWidth={on ? 4 : 1.5}
              vectorEffect="non-scaling-stroke"
            />
            <text x={cx} y={cy - font * 1.1} textAnchor="middle" className="fill-content-muted" fontSize={font * 0.8}>
              {s.slot}
            </text>
            <text
              x={cx}
              y={cy + font * 0.6}
              textAnchor="middle"
              className="fill-content-primary font-semibold"
              fontSize={font * 1.9}
            >
              {s.seq}
            </text>
            {s.drag_dir ? (
              <text x={cx} y={cy + font * 1.8} textAnchor="middle" className="fill-accent-text" fontSize={font * 0.75}>
                DragDir {s.drag_dir}
              </text>
            ) : null}
          </g>
        )
      })}

      {plan.slots.map((s) => {
        const a = dragArrow([s.offset_x, s.offset_y], s.drag_dir, r, dragDist, plan.drag_kind)
        if (!a) return null
        const [x1, y1] = toScreen(axes, a.from)
        const [x2, y2] = toScreen(axes, a.to)
        return (
          <line
            key={`arrow-${s.slot}`}
            x1={x1}
            y1={y1}
            x2={x2}
            y2={y2}
            className="stroke-accent"
            strokeWidth={2.5}
            vectorEffect="non-scaling-stroke"
            markerEnd={`url(#${uid}-head)`}
            pointerEvents="none"
          />
        )
      })}
    </svg>
  )
}
