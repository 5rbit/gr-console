// 스택 옆모습 — 편집 중인 단별 표(유효값)로 타이어 단면을 쌓아 그린다. 차트 라이브러리 없이 SVG 한 장.
//
// 가로는 타이어 단면(외경 대비 벽 두께 비율), 세로는 셀 바닥 기준 높이(mm → px). 단마다 좌우 벽 두 칸,
// LowerBead(점선)·UpperBead(실선)·PickZ(짧은 점선, 집는 높이) 표시. 측정 곡선 점에서 온 단은 테두리를
// accent, 보간은 info, 계산값은 흐린 선으로 칠해 가른다 — 미리보기 스택 크기를 바꾸면 하중이 달라져
// 비드 높이가 눈에 보이게 움직인다.
//
// **그림 안에는 값 글자를 쓰지 않는다.** 예전에는 맨 윗단 옆에 StackHeight·UpperBead·LowerBead·PickZ
// 넷을 한꺼번에 찍었는데, 네 값이 몇 mm 안에 몰리는 스택(얇은 타이어·큰 오프셋)에서는 글자가 서로
// 겹치고 범례와도 겹쳐 아무것도 못 읽었다. 지금은 **고른 단 하나의 값만** 그림 아래 라벨+값 줄로
// 내고(마우스·키보드로 단을 고른다, 기본은 맨 윗단), 그림에는 그 단으로 가는 안내선만 긋는다.
// 선 뜻은 그림 아래 범례가 한 번만 말한다.
import { useState } from 'react'
import type { TireBox } from '../../lib/items/levelsModel'
import { round1 } from '../../lib/items/levelsModel'
import { cn } from '../../lib/utils'

const W = 250
const H = 250
const PAD_T = 12
const PAD_B = 18
const LEFT = 34
const TIRE_W = 150

export interface StackSvgProps {
  boxes: readonly TireBox[]
  outer: number
  inner: number
  /** 0 = 제한 없음 */
  stackMax: number
}

const SOURCE_LABEL = (b: TireBox) => (b.measured ? 'measured' : b.fromTable ? 'interpolated' : 'computed')

export function StackSvg({ boxes, outer, inner, stackMax }: StackSvgProps) {
  /** 값을 보여 줄 단(null = 맨 윗단). 호버·포커스로 따라가고, 눌러서 고정한다. */
  const [level, setLevel] = useState<number | null>(null)
  const total = Math.max(1, ...boxes.map((b) => Math.max(b.top, b.upper)))
  const y = (z: number) => H - PAD_B - (z / total) * (H - PAD_T - PAD_B)
  const wall =
    outer > 0 && inner > 0 && inner < outer
      ? Math.max(8, ((outer - inner) / 2 / outer) * TIRE_W)
      : TIRE_W * 0.28
  const x0 = LEFT
  const x1 = LEFT + TIRE_W
  const top = boxes.length ? boxes[boxes.length - 1] : null
  const cur = (level === null ? null : boxes.find((b) => b.level === level)) ?? top

  return (
    <figure
      // `max-w-full` 이 없으면 값 줄이 한 줄로 펴지며 그림이 패널보다 넓어지고, 마지막 값(PickZ)이
      // 잘려 나간다 — 좁은 패널(460px)에서는 값 줄이 접혀야 한다.
      className="flex max-w-full min-w-0 flex-none flex-col gap-1 rounded-md border border-line-default bg-surface-panel p-1.5"
      data-testid="beads-stack"
    >
      <svg
        viewBox={`0 0 ${W} ${H}`}
        width={W}
        height={H}
        role="img"
        aria-label={`Stack side view — ${boxes.length} levels`}
        className="flex-none"
        data-testid="beads-svg"
      >
        <line x1={LEFT - 12} x2={x1 + 6} y1={y(0)} y2={y(0)} className="stroke-content-muted" strokeWidth={1.5} />
        <text x={LEFT - 12} y={y(0) + 12} className="fill-content-faint text-3xs">
          0
        </text>
        <text x={W - 2} y={y(0) + 12} textAnchor="end" className="fill-content-faint text-3xs">
          StackMax {stackMax > 0 ? stackMax : '∞'}
        </text>
        {boxes.map((b) => {
          const yt = y(b.top)
          const yb = y(b.bottom)
          const h = Math.max(1, yb - yt)
          const on = cur?.level === b.level
          const cls = cn(
            'fill-surface-inset',
            b.measured ? 'stroke-accent' : b.fromTable ? 'stroke-info-fg' : 'stroke-line-strong',
          )
          return (
            <g
              key={b.level}
              data-testid="beads-svg-tire"
              data-measured={b.measured ? '1' : undefined}
              tabIndex={0}
              role="button"
              aria-label={`${b.level}단 값 보기`}
              className="focus-visible:outline-none"
              onMouseEnter={() => setLevel(b.level)}
              onFocus={() => setLevel(b.level)}
              onClick={() => setLevel(b.level)}
            >
              <title>{`L${b.level} · Above ${b.above} · ${SOURCE_LABEL(b)}`}</title>
              <rect x={x0} y={yt} width={wall} height={h} rx={3} className={cls} strokeWidth={on ? 2 : 1} />
              <rect x={x1 - wall} y={yt} width={wall} height={h} rx={3} className={cls} strokeWidth={on ? 2 : 1} />
              {/* 가운데는 비었다 — 여기에 투명 판을 깔아야 타이어 안쪽에서도 단이 고른 것으로 남는다. */}
              <rect x={x0 + wall} y={yt} width={Math.max(1, x1 - x0 - wall * 2)} height={h} className="fill-transparent" />
              <line
                x1={x0 + wall - 5}
                x2={x1 - wall + 5}
                y1={y(b.lower)}
                y2={y(b.lower)}
                className="stroke-content-muted"
                strokeWidth={1}
                strokeDasharray="3 2"
              />
              <line
                x1={x0 + wall - 5}
                x2={x1 - wall + 5}
                y1={y(b.upper)}
                y2={y(b.upper)}
                className="stroke-accent"
                strokeWidth={on ? 2 : 1.5}
              />
              {b.pick === null ? null : (
                <line
                  x1={x0 - 8}
                  x2={x0 + wall + 8}
                  y1={y(b.pick)}
                  y2={y(b.pick)}
                  className="stroke-warn-fg"
                  strokeWidth={1.5}
                  strokeDasharray="4 2"
                  data-testid="beads-svg-pick"
                >
                  <title>{`L${b.level} PickZ ${round1(b.pick)} (위 ${b.above}개)`}</title>
                </line>
              )}
              <text
                x={4}
                y={(yt + yb) / 2 + 3}
                className={cn('text-3xs', on ? 'fill-content-secondary' : 'fill-content-faint')}
              >
                L{b.level}
              </text>
            </g>
          )
        })}
        {/* 고른 단 → 아래 값 줄로 가는 안내선(값 글자는 그림 밖에 있다). */}
        {cur ? (
          <g data-testid="beads-svg-leader">
            <line
              x1={x1 - wall + 5}
              x2={W - 8}
              y1={y(cur.upper)}
              y2={y(cur.upper)}
              className="stroke-line-strong"
              strokeWidth={1}
              strokeDasharray="2 3"
            />
            <line
              x1={W - 8}
              x2={W - 8}
              y1={y(cur.upper)}
              y2={H - 2}
              className="stroke-line-strong"
              strokeWidth={1}
              strokeDasharray="2 3"
            />
          </g>
        ) : null}
      </svg>
      <figcaption className="flex flex-col gap-1">
        {cur ? (
          <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-2xs" data-testid="beads-svg-readout">
            <span className="font-mono text-content-secondary">L{cur.level}</span>
            <span className="font-mono text-content-faint">Above {cur.above}</span>
            <Val label="StackHeight" v={cur.top} />
            <Val label="UpperBead" v={cur.upper} tone="accent" />
            <Val label="LowerBead" v={cur.lower} />
            <Val label="PickZ" v={cur.pick} tone="warn" />
          </div>
        ) : null}
        <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-3xs text-content-faint">
          <Key kind="upper" label="UpperBead" />
          <Key kind="lower" label="LowerBead" />
          <Key kind="pick" label="PickZ" />
          <Key kind="measured" label="measured" />
          <span>mm · 셀 바닥 기준</span>
        </div>
      </figcaption>
    </figure>
  )
}

/** 값 한 조각 — 이름은 흐리게, 숫자는 고정폭. 단위는 범례가 한 번만 말한다. */
function Val({ label, v, tone }: { label: string; v: number | null; tone?: 'accent' | 'warn' }) {
  return (
    <span className="inline-flex items-baseline gap-1">
      <span className="font-mono text-3xs text-content-faint">{label}</span>
      <span
        className={cn(
          'font-mono tabular-nums',
          tone === 'accent' ? 'text-accent-text' : tone === 'warn' ? 'text-warn-fg' : 'text-content-secondary',
        )}
      >
        {v === null ? '—' : round1(v)}
      </span>
    </span>
  )
}

/** 범례 한 조각 — 선 모양을 글자로 설명하지 않고 **선 그대로** 보인다. */
function Key({ kind, label }: { kind: 'upper' | 'lower' | 'pick' | 'measured'; label: string }) {
  return (
    <span className="inline-flex items-center gap-1">
      <svg width={14} height={8} aria-hidden="true" className="flex-none">
        {kind === 'measured' ? (
          <rect x={1} y={1} width={12} height={6} rx={1} className="fill-surface-inset stroke-accent" strokeWidth={1.5} />
        ) : (
          <line
            x1={0}
            x2={14}
            y1={4}
            y2={4}
            className={kind === 'upper' ? 'stroke-accent' : kind === 'pick' ? 'stroke-warn-fg' : 'stroke-content-muted'}
            strokeWidth={1.5}
            strokeDasharray={kind === 'upper' ? undefined : kind === 'pick' ? '4 2' : '3 2'}
          />
        )}
      </svg>
      <span className="font-mono">{label}</span>
    </span>
  )
}
