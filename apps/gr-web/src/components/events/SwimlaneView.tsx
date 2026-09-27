// 인터록 스윔레인 — 한 스테이션의 로봇 PI/PO · GRM In/Out 비트를 계단 선으로, 위에 점 표식.
//
// 확대는 **창 자체를 좁혀 다시 받는다**(끌어 고른 구간 → 스토어 창). 그래서 링크·저장된 필터가 확대한
// 보기까지 담고, 확대한 창의 첫 상태도 서버가 창 앞의 행으로 채운다.
import { useMemo, useRef, useState, type MouseEvent } from 'react'
import type { SwimLane, SwimMarker, Swimlane } from '../../lib/evtlog/api'
import { msToStamp } from '../../lib/evtlog/evtRowsModel'
import type { Loaded } from '../../lib/evtlog/store'
import {
  MARKER_LABEL,
  MARKER_TONE,
  groupLanes,
  highSegs,
  stepPath,
  tOf,
  ticks,
  valueAt,
  xOf,
  zoomTo,
  type View,
} from '../../lib/evtlog/swimlaneModel'
import { Button } from '../../lib/ui/Button'
import { EmptyState } from '../../lib/ui/EmptyState'
import { useElementWidth } from '../../lib/ui/useElementWidth'
import { cn } from '../../lib/utils'
import type { Status } from '../../lib/ui/status'

const ROW_H = 20
const GROUP_H = 20
const MARK_H = 18
const AXIS_H = 18
const LABEL_W = 128

const TONE_FILL: Record<Status, string> = {
  fault: 'fill-fault',
  warn: 'fill-warn',
  info: 'fill-info',
  neutral: 'fill-content-muted',
  ok: 'fill-ok',
  degraded: 'fill-degraded',
}

type Row = { kind: 'group'; group: string; y: number } | { kind: 'lane'; lane: SwimLane; y: number }

function layout(lanes: readonly SwimLane[]): { rows: Row[]; height: number } {
  const rows: Row[] = []
  let y = MARK_H
  for (const g of groupLanes(lanes)) {
    rows.push({ kind: 'group', group: g.group, y })
    y += GROUP_H
    for (const lane of g.lanes) {
      rows.push({ kind: 'lane', lane, y })
      y += ROW_H
    }
  }
  return { rows, height: y + AXIS_H }
}

const hms = (t: number) => msToStamp(t).slice(11)

export function SwimlaneView({
  data,
  hasTarget,
  onZoom,
  onMarker,
  onReload,
}: {
  data: Loaded<Swimlane>
  /** 스테이션(또는 슬롯·로봇)이 골라졌나. */
  hasTarget: boolean
  onZoom: (v: View) => void
  onMarker: (m: SwimMarker) => void
  onReload: () => void
}) {
  const [box, boxW] = useElementWidth<HTMLDivElement>()
  const [hover, setHover] = useState<{ x: number; y: number } | null>(null)
  const [drag, setDrag] = useState<{ x0: number; x1: number } | null>(null)
  const svgRef = useRef<SVGSVGElement>(null)
  const s = data.data
  const { rows, height } = useMemo(() => layout(s?.lanes ?? []), [s])

  if (!hasTarget)
    return (
      <EmptyState
        title="스테이션을 고르세요"
        hint="도구 띠의 Station 에서 고르거나, 이벤트 행의 전후 창에서 인터록 보기를 누릅니다."
        testid="evt-swim-empty"
      />
    )
  if (!s) {
    if (data.error)
      return (
        <EmptyState
          title="인터록을 불러오지 못했습니다"
          hint={data.error}
          action={
            <Button size="sm" onClick={onReload}>
              다시 시도
            </Button>
          }
        />
      )
    return <EmptyState title="불러오는 중" />
  }

  const w = Math.max(200, (boxW ?? 800) - LABEL_W - 16)
  const view: View = { t0: s.from, t1: s.to }
  const now = Date.now()
  const px = (e: MouseEvent) => {
    const r = svgRef.current?.getBoundingClientRect()
    return r ? { x: Math.min(w, Math.max(0, e.clientX - r.left)), y: e.clientY - r.top } : null
  }
  const hovered = hover
    ? rows.find((r) => hover.y >= r.y && hover.y < r.y + (r.kind === 'lane' ? ROW_H : GROUP_H))
    : null
  const hoverT = hover ? tOf(hover.x, view, w) : null
  const tip =
    hover && hoverT !== null
      ? hovered?.kind === 'lane'
        ? `${hms(hoverT)} · ${hovered.lane.group} ${hovered.lane.label} = ${valueAt(hovered.lane.points, hoverT) ?? '–'}`
        : hms(hoverT)
      : null
  const spansFor = (group: string) => s.spans.filter((sp) => sp.group === group)

  return (
    <div ref={box} className="min-h-0 flex-1 overflow-auto p-3" data-testid="evt-swimlane">
      <div className="flex">
        <div className="shrink-0 text-2xs" style={{ width: LABEL_W }}>
          <div style={{ height: MARK_H }} className="truncate pr-2 text-3xs text-content-faint">
            {s.markers.length ? `markers ${s.markers.length}` : ''}
          </div>
          {rows.map((r) =>
            r.kind === 'group' ? (
              <div
                key={`g-${r.group}`}
                style={{ height: GROUP_H }}
                className="flex items-end truncate pr-2 font-semibold text-content-secondary"
              >
                {r.group}
              </div>
            ) : (
              <div
                key={r.lane.key}
                style={{ height: ROW_H }}
                className="flex items-center truncate pr-2 font-mono text-content-tertiary"
                title={`${r.lane.group} ${r.lane.label}`}
              >
                {r.lane.label}
              </div>
            ),
          )}
        </div>
        <div className="relative">
          <svg
            ref={svgRef}
            width={w}
            height={height}
            className="block cursor-crosshair select-none"
            role="img"
            aria-label={`Station ${s.station ?? `slot ${s.slot}`} interlock`}
            onMouseMove={(e) => {
              const p = px(e)
              setHover(p)
              if (drag && p) setDrag({ ...drag, x1: p.x })
            }}
            onMouseLeave={() => {
              setHover(null)
              setDrag(null)
            }}
            onMouseDown={(e) => {
              const p = px(e)
              if (p && e.button === 0) setDrag({ x0: p.x, x1: p.x })
            }}
            onMouseUp={() => {
              if (drag) {
                const z = zoomTo(drag.x0, drag.x1, view, w)
                if (z) onZoom(z)
              }
              setDrag(null)
            }}
          >
            {rows.map((r) => {
              if (r.kind === 'group')
                return (
                  <g key={`g-${r.group}`}>
                    <line
                      x1={0}
                      x2={w}
                      y1={r.y + GROUP_H - 0.5}
                      y2={r.y + GROUP_H - 0.5}
                      className="stroke-line-default"
                    />
                    {spansFor(r.group).map((sp) => {
                      const x0 = Math.max(0, xOf(sp.from, view, w))
                      const x1 = Math.min(w, xOf(sp.to, view, w))
                      return x1 > x0 ? (
                        <rect
                          key={`${sp.from}`}
                          x={x0}
                          y={r.y + 4}
                          width={x1 - x0}
                          height={GROUP_H - 8}
                          rx={2}
                          className="fill-accent-soft"
                        />
                      ) : null
                    })}
                  </g>
                )
              const l = r.lane
              return (
                <g key={l.key}>
                  <line
                    x1={0}
                    x2={w}
                    y1={r.y + ROW_H - 0.5}
                    y2={r.y + ROW_H - 0.5}
                    className="stroke-line-subtle"
                  />
                  {highSegs(l.points, view, now).map((sg) => (
                    <rect
                      key={sg.t0}
                      x={xOf(sg.t0, view, w)}
                      y={r.y + 3}
                      width={Math.max(1, xOf(sg.t1, view, w) - xOf(sg.t0, view, w))}
                      height={ROW_H - 6}
                      className="fill-info-soft"
                    />
                  ))}
                  <path
                    d={stepPath(l.points, view, w, ROW_H, now)}
                    transform={`translate(0 ${r.y})`}
                    className="fill-none stroke-info"
                    strokeWidth={1.5}
                  />
                </g>
              )
            })}
            {s.markers.map((m) => {
              const x = xOf(m.ts_ms, view, w)
              if (x < 0 || x > w) return null
              return (
                <g
                  key={m.id}
                  className="cursor-pointer"
                  onMouseDown={(e) => e.stopPropagation()}
                  onClick={() => onMarker(m)}
                  data-testid="evt-swim-marker"
                >
                  <title>{`${hms(m.ts_ms)} ${MARKER_LABEL[m.kind]} · ${m.plc} ${m.text}`}</title>
                  <line
                    x1={x}
                    x2={x}
                    y1={MARK_H}
                    y2={height - AXIS_H}
                    className="stroke-line-strong"
                    strokeDasharray="2 3"
                  />
                  <circle
                    cx={x}
                    cy={MARK_H / 2}
                    r={4.5}
                    className={TONE_FILL[MARKER_TONE[m.kind]]}
                  />
                </g>
              )
            })}
            {ticks(view).map((t) => {
              const x = xOf(t, view, w)
              return (
                <g key={t}>
                  <line
                    x1={x}
                    x2={x}
                    y1={height - AXIS_H}
                    y2={height - AXIS_H + 4}
                    className="stroke-content-faint"
                  />
                  <text
                    x={x}
                    y={height - 3}
                    textAnchor="middle"
                    className="fill-content-faint text-3xs"
                  >
                    {hms(t).slice(0, 8)}
                  </text>
                </g>
              )
            })}
            {hover ? (
              <line
                x1={hover.x}
                x2={hover.x}
                y1={0}
                y2={height - AXIS_H}
                className="stroke-content-muted"
              />
            ) : null}
            {drag && Math.abs(drag.x1 - drag.x0) > 3 ? (
              <rect
                x={Math.min(drag.x0, drag.x1)}
                y={0}
                width={Math.abs(drag.x1 - drag.x0)}
                height={height - AXIS_H}
                className="fill-accent-soft stroke-accent"
              />
            ) : null}
          </svg>
          {tip && hover ? (
            <div
              className={cn(
                'pointer-events-none absolute z-10 rounded border border-line-default bg-surface-panel px-1.5 py-0.5 font-mono text-2xs whitespace-nowrap text-content-primary shadow-lg',
              )}
              style={{ left: Math.min(hover.x + 8, w - 220), top: Math.max(0, hover.y - 24) }}
              data-testid="evt-swim-tip"
            >
              {tip}
            </div>
          ) : null}
        </div>
      </div>
      {rows.length === 0 ? (
        <EmptyState
          compact
          title="이 창에 인터록 행 없음"
          hint="창을 넓히거나 다른 시각을 고르세요."
        />
      ) : null}
    </div>
  )
}
