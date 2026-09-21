// 작은 패턴 그림 — 비교 창의 사양서/현재 나란히 보기. 생성기와 같은 배율(1 / MinDistance)로 그리고
// 바뀐 슬롯은 경고색으로 칠한다.
import { scaleOf, toDraft } from '../../lib/pallet/editorModel'
import { axisLabels, dragArrow, toScreen, type DragKind, type ScreenAxes, type SpecPattern } from '../../lib/pallet/model'
import { cn } from '../../lib/utils'

export interface PatternMiniProps {
  pattern: SpecPattern | null
  axes: ScreenAxes
  od: number
  gap: number
  palletSize: number
  dragKind: DragKind
  label: string
  highlight: readonly string[]
  testid?: string
}

export function PatternMini({ pattern, axes, od, gap, palletSize, dragKind, label, highlight, testid }: PatternMiniProps) {
  const pitch = od + gap > 0 ? od + gap : 1
  const draft = pattern ? toDraft(pattern) : null
  const k = draft ? pitch * scaleOf(draft.slots) : pitch
  const half = palletSize / 2
  const far = draft ? draft.slots.reduce((m, s) => Math.max(m, Math.abs(s.u[0]) * k, Math.abs(s.u[1]) * k), 0) : 0
  const ext = Math.max(half, far + od / 2) + 160
  const font = Math.max(40, palletSize / 30)
  const lab = axisLabels(axes)
  return (
    <figure className="flex min-w-0 flex-col gap-1" data-testid={testid}>
      <figcaption className="text-2xs text-content-muted">{label}</figcaption>
      <svg
        viewBox={`${-ext} ${-ext} ${2 * ext} ${2 * ext}`}
        role="img"
        aria-label={label}
        className="aspect-square w-full rounded border border-line-default bg-surface-panel"
      >
        <rect x={-half} y={-half} width={palletSize} height={palletSize} rx={20} className="fill-surface-inset stroke-line-strong" strokeWidth={1} vectorEffect="non-scaling-stroke" />
        <text x={half + 80} y={0} textAnchor="middle" dominantBaseline="middle" className="fill-content-faint" fontSize={font * 0.7}>
          {lab.right}
        </text>
        <text x={0} y={-half - 80} textAnchor="middle" dominantBaseline="middle" className="fill-content-faint" fontSize={font * 0.7}>
          {lab.up}
        </text>
        {!draft && (
          <text x={0} y={0} textAnchor="middle" dominantBaseline="middle" className="fill-content-faint" fontSize={font}>
            없음
          </text>
        )}
        {draft?.slots.map((s) => {
          const off: [number, number] = [s.u[0] * k, s.u[1] * k]
          const [cx, cy] = toScreen(axes, off)
          const hot = highlight.includes(s.slot)
          const a = dragArrow(off, s.drag_dir, od / 2, 150, dragKind)
          const a1 = a ? toScreen(axes, a.from) : null
          const a2 = a ? toScreen(axes, a.to) : null
          return (
            <g key={s.slot}>
              <circle cx={cx} cy={cy} r={od / 2} className={cn(hot ? 'fill-warn-soft stroke-warn' : 'fill-surface-panel stroke-accent')} strokeWidth={hot ? 3 : 1.2} vectorEffect="non-scaling-stroke" />
              <text x={cx} y={cy - font * 0.6} textAnchor="middle" className="fill-content-muted" fontSize={font * 0.7}>
                {s.slot}
              </text>
              <text x={cx} y={cy + font * 0.5} textAnchor="middle" className="fill-content-primary font-semibold" fontSize={font * 1.3}>
                {s.seq}
              </text>
              <text x={cx} y={cy + font * 1.4} textAnchor="middle" className="fill-accent-text" fontSize={font * 0.6}>
                {`DragDir ${s.drag_dir}`}
              </text>
              {a1 && a2 && <line x1={a1[0]} y1={a1[1]} x2={a2[0]} y2={a2[1]} className="stroke-accent" strokeWidth={2} vectorEffect="non-scaling-stroke" />}
            </g>
          )
        })}
      </svg>
    </figure>
  )
}
