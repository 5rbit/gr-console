// 셀/스테이션 레이아웃 맵 — PLC 좌표계(mm) 위에 셀은 원, 스테이션은 정사각형(중심 = Position X/Y).
//
// 플롯 밖에는 아무것도 두지 않는다. 모드 토글은 왼쪽 위(`topLeft` 슬롯), 오른쪽 위에는 **자주 쓰는 넷**
// (맞춤·확대·축소·로봇 위치)만 세우고 회전·보기 설정·범례는 그 아래 ⋯ 하나로 접었다 — 회전은 이미
// 보기 설정 안에 있었고, 범례는 한 번 읽고 마는 것이라 늘 자리를 차지할 이유가 없다.
// 좌표·호버 정보는 왼쪽 아래 한 줄, 축 방향은 오른쪽 아래.
// 셀 안에는 재고 개수만 크게 그린다(재고 0 = 회색). 로봇이 작업 중인 셀은 그 로봇 색 테두리(대기 = 점선).
// 화면은 시계 방향 0·90·180·270° 로 돌릴 수 있고, 돌린 뒤 좌우·상하 반전도 된다(설정은 브라우저에 저장).
//
// **테두리 언어**(한 번에 한 겹만 주장한다): 선택 = 실선 액센트 링 + 후광 + 옅은 채움(맨 위에 하나),
// 호버 = 옅은 실선 링, 로컬 수정 = 오른쪽 위 점 하나, 문제(겹침·바닥 Z ≤ 0) = 경고색 **점선** 링.
// 점선은 문제에만 쓴다 — 고친 칸이 40개여도 화면이 점선 밭이 되지 않는다.
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Crosshair, Maximize2, Minus, MoreHorizontal, Plus } from 'lucide-react'
import {
  ROTATIONS,
  SIZE_PRESETS,
  boundsOf,
  fitView,
  gridStep,
  panBy,
  sameTarget,
  shapeInfo,
  shapesFrom,
  toScreen,
  toWorld,
  zoomAt,
  type Rotation,
  type Shape,
  type View,
} from '../../lib/task/layoutModel'
import type { PreviewCell } from '../../lib/task/layoutGen'
import type { PlanStep } from '../../lib/task/plan'
import { overlay as planOverlay } from '../../lib/task/plan'
import { f1 } from '../../lib/meas/format'
import { EmptyState } from '../../lib/ui/EmptyState'
import { IconPopover, MapIconButton } from '../../lib/ui/IconPopover'
import { Input } from '../../lib/ui/Input'
import { Segmented } from '../../lib/ui/Segmented'
import { Select } from '../../lib/ui/Select'
import { Switch } from '../../lib/ui/Switch'
import type { Cell, Station, StockEntry, Target, TaskType } from '../../lib/types'
import { cn } from '../../lib/utils'

const SIZE_KEY = 'gr-cellmap-size'
const FLIP_KEY = 'gr-cellmap-flipy'
const FLIPX_KEY = 'gr-cellmap-flipx'
const ROT_KEY = 'gr-cellmap-rot'

type RotId = '0' | '90' | '180' | '270'

function loadNum(key: string, def: number): number {
  try {
    const v = Number(localStorage.getItem(key))
    return v > 0 ? v : def
  } catch {
    return def
  }
}
function loadBool(key: string, def: boolean): boolean {
  try {
    const v = localStorage.getItem(key)
    return v === null ? def : v === '1'
  } catch {
    return def
  }
}
function loadRot(): Rotation {
  try {
    const n = Number(localStorage.getItem(ROT_KEY))
    return (ROTATIONS as readonly number[]).includes(n) ? (n as Rotation) : 0
  } catch {
    return 0
  }
}
function save(key: string, v: string) {
  try {
    localStorage.setItem(key, v)
  } catch {
    /* 저장 못 해도 동작 */
  }
}

/** 재고가 있는 셀의 채움색 — 구역(Section)별. */
const SECTION_FILL = [
  'fill-content-muted',
  'fill-sky-500',
  'fill-emerald-500',
  'fill-violet-500',
  'fill-rose-500',
  'fill-teal-500',
]
const sectionFill = (s: number) => SECTION_FILL[s % SECTION_FILL.length]
const TYPE_SHORT: Record<TaskType, string> = {
  UP: 'U',
  PICK: 'P',
  DROP: 'D',
  MOVE: 'M',
  MEASURE: 'S',
}

/** 로봇이 이 도형에서 작업 중/대기 중임을 알리는 테두리. 키 = `${kind}-${id}`. */
export interface WorkMark {
  color: string
  robot: string
  running: boolean
  label: string
}

/** 로봇 현재 X/Y 표식. */
export interface RobotMarker {
  id: number
  name: string
  color: string
  x: number
  y: number
  moving: boolean
}

export interface CellMapProps {
  cells: readonly Cell[]
  stations: readonly Station[]
  /** 강조할 대상(작성 카드 대상 / 계획 스텝 / 편집 선택). */
  selected: Target | null
  /** 도형 좌클릭. */
  onPick: (t: Target, shape: Shape) => void
  /** 도형 우클릭(명령 팔레트). */
  onContext?: (t: Target, shape: Shape, e: React.MouseEvent) => void
  /** 셀 재고 — 원 안의 개수. */
  stock?: ReadonlyMap<number, StockEntry>
  /** 순차 계획 — 순번 배지 + 경로(명령 생성 모드에서만 넘긴다). */
  plan?: readonly PlanStep[]
  /** 생성 예정 셀(레이아웃 편집 모드에서만 넘긴다) — 충돌 판정이 실려 있으면 색과 툴팁이 달라진다. */
  preview?: readonly PreviewCell[]
  /** 로봇 작업 테두리. */
  work?: ReadonlyMap<string, WorkMark>
  /** 로봇 위치 표식. */
  robots?: readonly RobotMarker[]
  /** 범례에 보일 로봇 색. */
  robotLegend?: readonly { name: string; color: string }[]
  /** 플롯 왼쪽 위(모드 토글 등). */
  topLeft?: ReactNode
  /** 이 대상으로 화면을 옮긴다(nonce 가 바뀔 때마다). */
  focus?: { target: Target; nonce: number } | null
  /** 플롯 안에 띄울 카드(정보 패널 등). */
  children?: ReactNode
}

export function CellMap({
  cells,
  stations,
  selected,
  onPick,
  onContext,
  stock,
  plan,
  preview,
  work,
  robots,
  robotLegend = [],
  topLeft,
  focus,
  children,
}: CellMapProps) {
  const wrap = useRef<HTMLDivElement>(null)
  const [dim, setDim] = useState({ w: 800, h: 500 })
  const [size, setSize] = useState(() => loadNum(SIZE_KEY, 600))
  const [flipY, setFlipY] = useState(() => loadBool(FLIP_KEY, true))
  const [flipX, setFlipX] = useState(() => loadBool(FLIPX_KEY, false))
  const [rot, setRot] = useState<Rotation>(loadRot)
  const [footprint, setFootprint] = useState(false)
  const [view, setView] = useState<View | null>(null)
  const [hover, setHover] = useState<Shape | null>(null)
  const [cursor, setCursor] = useState<[number, number] | null>(null)
  const drag = useRef<{ x: number; y: number; moved: boolean } | null>(null)

  const changeRot = (r: Rotation) => {
    setRot(r)
    save(ROT_KEY, String(r))
  }

  const shapes = useMemo(() => shapesFrom(cells, stations), [cells, stations])
  const previewShapes = useMemo(
    () =>
      (preview ?? []).map<Shape>((c) => ({
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
        dirty: true,
        label: `생성 예정 셀 #${c.id}`,
      })),
    [preview],
  )
  /** 생성 예정 칸의 판정(색·툴팁) — 보낸 id 로 찾는다. */
  const previewInfo = useMemo(() => new Map((preview ?? []).map((c) => [c.id, c])), [preview])
  const bounds = useMemo(
    () => boundsOf([...shapes, ...previewShapes], size / 2),
    [shapes, previewShapes, size],
  )
  const ov = useMemo(() => (plan && plan.length ? planOverlay(plan) : null), [plan])

  // 컨테이너 크기 추적.
  useEffect(() => {
    const el = wrap.current
    if (!el) return
    const update = () =>
      setDim({ w: Math.max(el.clientWidth, 100), h: Math.max(el.clientHeight, 100) })
    update()
    if (typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(update)
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  const fit = () => {
    if (bounds) setView(fitView(bounds, dim.w, dim.h, 48, flipY, flipX, rot))
  }
  // 처음 도형이 생기거나 회전·반전이 바뀌면 맞춤.
  const boundsKey = bounds ? `${bounds.minX},${bounds.minY},${bounds.maxX},${bounds.maxY}` : ''
  useEffect(() => {
    if (!bounds) return
    setView((cur) =>
      cur && cur.flipY === flipY && cur.flipX === flipX && (cur.rot ?? 0) === rot
        ? cur
        : fitView(bounds, dim.w, dim.h, 48, flipY, flipX, rot),
    )
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 경계 문자열·회전·반전이 바뀔 때만
  }, [boundsKey, flipY, flipX, rot, dim.w, dim.h])

  const v: View =
    view ??
    (bounds
      ? fitView(bounds, dim.w, dim.h, 48, flipY, flipX, rot)
      : { k: 0.05, ox: dim.w / 2, oy: dim.h / 2, flipY, flipX, rot })
  const r = (size / 2) * v.k
  const rr = Math.max(r, 3)
  const step = gridStep(v.k)

  // 바깥에서 지목한 대상으로 화면 이동.
  useEffect(() => {
    if (!focus) return
    const s = [...shapes, ...previewShapes].find(
      (x) => x.kind === focus.target.kind && x.id === focus.target.id,
    )
    if (!s) return
    const [sx, sy] = toScreen(v, s.x, s.y)
    setView(panBy(v, dim.w / 2 - sx, dim.h / 2 - sy))
    // eslint-disable-next-line react-hooks/exhaustive-deps -- nonce 가 바뀔 때만
  }, [focus?.nonce])

  // 눈금선 — 화면 네 귀퉁이의 월드 좌표 범위(회전해도 네 귀퉁이의 min/max 로 충분하다).
  const corners = [
    toWorld(v, 0, 0),
    toWorld(v, dim.w, 0),
    toWorld(v, 0, dim.h),
    toWorld(v, dim.w, dim.h),
  ]
  const wxs = corners.map((c) => c[0])
  const wys = corners.map((c) => c[1])
  const gx: number[] = []
  const gy: number[] = []
  for (let x = Math.floor(Math.min(...wxs) / step) * step; x <= Math.max(...wxs); x += step)
    gx.push(x)
  for (let y = Math.floor(Math.min(...wys) / step) * step; y <= Math.max(...wys); y += step)
    gy.push(y)
  const tooMany = gx.length > 60 || gy.length > 60
  /** 월드 좌표선 하나의 화면 모양 — 회전하면 X 선이 가로선, Y 선이 세로선이 된다. */
  const gridLine = (axis: 'x' | 'y', val: number) => {
    const p = axis === 'x' ? toScreen(v, val, 0) : toScreen(v, 0, val)
    const q = axis === 'x' ? toScreen(v, val, 1) : toScreen(v, 1, val)
    const vertical = Math.abs(p[0] - q[0]) < 1e-6
    return { vertical, at: vertical ? p[0] : p[1] }
  }
  const gridLines = tooMany
    ? []
    : [...gx.map((val) => ['x', val] as const), ...gy.map((val) => ['y', val] as const)]

  // 축 방향 표시(+X 빨강, +Y 초록).
  const [o0, o1] = toScreen(v, 0, 0)
  const unit = (x: number, y: number): [number, number] => {
    const [a, b] = toScreen(v, x, y)
    const len = Math.hypot(a - o0, b - o1) || 1
    return [(a - o0) / len, (b - o1) / len]
  }
  const [xu, xv] = unit(1000, 0)
  const [yu, yv] = unit(0, 1000)
  const sideways = rot % 180 !== 0

  function local(e: React.MouseEvent | React.WheelEvent): [number, number] {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
    return [e.clientX - rect.left, e.clientY - rect.top]
  }
  const centre = (s: Shape) => toScreen(v, s.x, s.y)
  const zoomCenter = (f: number) => setView(zoomAt(v, dim.w / 2, dim.h / 2, f))

  const hoverText = (() => {
    if (!hover) return null
    const st = hover.kind === 'cell' ? stock?.get(hover.id) : undefined
    const w = work?.get(`${hover.kind}-${hover.id}`)
    const stockText =
      hover.kind === 'cell'
        ? ` · Count ${st?.count ?? 0}${st?.item_code ? ` (ItemCode ${st.item_code})` : ''}`
        : ''
    return `${shapeInfo(hover)}${stockText}${w ? ` · ${w.label}` : ''}`
  })()
  const statusText =
    hoverText ??
    (cursor
      ? `X ${f1(cursor[0])} · Y ${f1(cursor[1])} · 눈금 ${step} mm`
      : `셀 ${cells.length} · 스테이션 ${stations.length}${previewShapes.length ? ` · 생성 예정 ${previewShapes.length}` : ''} · 눈금 ${step} mm${rot ? ` · 회전 ${rot}°` : ''}`)

  return (
    <div
      ref={wrap}
      className="relative h-full min-h-0 overflow-hidden bg-surface-panel"
      data-testid="cell-map"
    >
      {!shapes.length && !previewShapes.length ? (
        <div className="absolute inset-0">
          <EmptyState
            title="셀·스테이션 없음"
            hint="PLC 읽기 · Excel 가져오기 · 레이아웃 편집으로 만듭니다"
          />
        </div>
      ) : null}
      <svg
        width={dim.w}
        height={dim.h}
        className={cn('block select-none', drag.current ? 'cursor-grabbing' : 'cursor-grab')}
        onWheel={(e) => {
          e.preventDefault()
          const [sx, sy] = local(e)
          setView(zoomAt(v, sx, sy, e.deltaY < 0 ? 1.2 : 1 / 1.2))
        }}
        onMouseDown={(e) => {
          if (e.button !== 0) return
          drag.current = { x: e.clientX, y: e.clientY, moved: false }
        }}
        onMouseMove={(e) => {
          const [sx, sy] = local(e)
          setCursor(toWorld(v, sx, sy))
          const d = drag.current
          if (d) {
            const dx = e.clientX - d.x
            const dy = e.clientY - d.y
            if (Math.abs(dx) + Math.abs(dy) > 2) d.moved = true
            if (d.moved) {
              setView(panBy(v, dx, dy))
              d.x = e.clientX
              d.y = e.clientY
            }
          }
        }}
        onMouseUp={() => {
          drag.current = null
        }}
        onMouseLeave={() => {
          drag.current = null
          setCursor(null)
          setHover(null)
        }}
        onContextMenu={(e) => e.preventDefault()}
        role="img"
        aria-label="셀 레이아웃"
      >
        <defs>
          <marker
            id="plan-arrow"
            viewBox="0 0 10 10"
            refX="9"
            refY="5"
            markerWidth="7"
            markerHeight="7"
            orient="auto-start-reverse"
          >
            <path d="M 0 0 L 10 5 L 0 10 z" className="fill-accent" />
          </marker>
        </defs>

        {/* 눈금 — 세로선 값은 아래, 가로선 값은 왼쪽. 회전해도 어느 축인지 보이게 X/Y 를 붙인다. */}
        {gridLines.map(([axis, val]) => {
          const g = gridLine(axis, val)
          const cls = cn('stroke-line-subtle', val === 0 && 'stroke-line-strong')
          return (
            <g key={`${axis}${val}`}>
              {g.vertical ? (
                <line x1={g.at} y1={0} x2={g.at} y2={dim.h} className={cls} strokeWidth={1} />
              ) : (
                <line x1={0} y1={g.at} x2={dim.w} y2={g.at} className={cls} strokeWidth={1} />
              )}
              <text
                x={g.vertical ? g.at + 2 : 3}
                y={g.vertical ? dim.h - 3 : g.at - 2}
                className="fill-content-faint text-3xs"
              >
                {axis.toUpperCase()}
                {val}
              </text>
            </g>
          )
        })}

        {/* 슬롯 실제 크기 (length × width) — 90°/270° 에서는 화면 가로세로가 바뀐다 */}
        {footprint
          ? shapes
              .filter((s) => s.length > 0 && s.width > 0)
              .map((s) => {
                const [sx, sy] = centre(s)
                const w = (sideways ? s.width : s.length) * v.k
                const h = (sideways ? s.length : s.width) * v.k
                return (
                  <rect
                    key={`fp-${s.kind}-${s.id}`}
                    x={sx - w / 2}
                    y={sy - h / 2}
                    width={w}
                    height={h}
                    className="fill-none stroke-line-strong"
                    strokeDasharray="4 3"
                  />
                )
              })
          : null}

        {/* 생성 예정 셀 */}
        {previewShapes.map((s) => {
          const [sx, sy] = centre(s)
          const info = previewInfo.get(s.id)
          const out = info?.outcome ?? 'added'
          // 건너뛸 칸은 **경고 톤 + 점선**(문제에만 쓰는 그 표시), 갱신은 정보 톤, 그대로면 조용하게.
          const tone =
            out === 'skipped'
              ? 'fill-warn-soft stroke-warn'
              : out === 'updated'
                ? 'fill-info-soft stroke-info'
                : out === 'unchanged'
                  ? 'fill-surface-inset stroke-line-strong'
                  : 'fill-degraded-soft stroke-degraded'
          const tip =
            (out === 'skipped'
              ? `건너뜀 — ${info?.reason ?? '충돌'}`
              : out === 'remapped'
                ? `번호 변경 → #${info?.newId}`
                : out === 'updated'
                  ? `기존 셀 #${s.id} 갱신`
                  : out === 'unchanged'
                    ? '값이 같아 그대로'
                    : `생성 예정 셀 #${s.id}`) +
            (info?.reason && out !== 'skipped' ? ` · ${info.reason}` : '')
          return (
            <g key={`pv-${s.id}`} data-testid={`map-preview-${s.id}`} data-outcome={out}>
              <title>{tip}</title>
              <circle
                cx={sx}
                cy={sy}
                r={rr}
                className={tone}
                fillOpacity={0.5}
                strokeWidth={1.5}
                strokeDasharray="6 3"
              />
              {rr >= 10 ? (
                <text
                  x={sx}
                  y={sy}
                  textAnchor="middle"
                  dominantBaseline="central"
                  fontSize={Math.min(rr * 0.5, 13)}
                  className={cn(
                    'pointer-events-none font-semibold',
                    out === 'skipped' ? 'fill-warn-fg' : 'fill-content-secondary',
                  )}
                >
                  {info?.newId ?? s.id}
                </text>
              ) : null}
            </g>
          )
        })}

        {/* 계획 경로 */}
        {ov && ov.path.length > 1
          ? ov.path.slice(1).map((t, i) => {
              const a = shapes.find((s) => s.kind === ov.path[i].kind && s.id === ov.path[i].id)
              const b = shapes.find((s) => s.kind === t.kind && s.id === t.id)
              if (!a || !b) return null
              const [x1, y1] = centre(a)
              const [x2, y2] = centre(b)
              const dx = x2 - x1
              const dy = y2 - y1
              const len = Math.hypot(dx, dy) || 1
              const ux = dx / len
              const uy = dy / len
              const pad = Math.min(rr + 4, len / 2 - 1)
              return (
                <line
                  key={`path-${i}`}
                  x1={x1 + ux * pad}
                  y1={y1 + uy * pad}
                  x2={x2 - ux * pad}
                  y2={y2 - uy * pad}
                  className="pointer-events-none stroke-accent"
                  strokeWidth={2}
                  strokeDasharray="6 4"
                  markerEnd="url(#plan-arrow)"
                />
              )
            })
          : null}

        {/* 도형 */}
        {shapes.map((s) => {
          const [sx, sy] = centre(s)
          const key = `${s.kind}-${s.id}`
          const sel = sameTarget(selected, s)
          const hov = hover === s
          const n = s.kind === 'cell' ? (stock?.get(s.id)?.count ?? 0) : 0
          const empty = s.kind === 'cell' && n === 0
          const w = work?.get(key)
          const fill =
            s.kind === 'station'
              ? 'fill-amber-300'
              : empty
                ? 'fill-surface-active'
                : sectionFill(s.section)
          // 테두리는 **한 겹만** 주장한다: 선택 링은 맨 위 따로 한 번 그리고(아래 오버레이),
          // 도형 자체는 조용한 윤곽만 둔다. 로컬 수정은 점선 링이 아니라 작은 점 하나,
          // 문제(바닥 Z ≤ 0)는 경고색 점선 링 하나 — 점선은 **문제에만** 쓴다.
          const stroke = empty ? 'stroke-line-strong' : 'stroke-surface-panel/70'
          const shapeCls = cn('cursor-pointer', fill, stroke, !s.use && 'opacity-40')
          const sw = 1
          const problem =
            s.kind === 'cell' && s.z <= 0
              ? `바닥 Z ${f1(s.z)} ≤ 0 — 이 셀로 나간 작업을 PLC 가 거부합니다`
              : null
          const ringR = rr + 4
          const handlers = {
            onMouseEnter: () => setHover(s),
            onMouseLeave: () => setHover(null),
            onClick: (e: React.MouseEvent) => {
              if (drag.current?.moved) return
              e.stopPropagation()
              onPick({ kind: s.kind, id: s.id }, s)
            },
            onContextMenu: (e: React.MouseEvent) => {
              e.preventDefault()
              e.stopPropagation()
              onContext?.({ kind: s.kind, id: s.id }, s, e)
            },
          }
          const badges = ov?.badges.get(key) ?? []
          return (
            <g key={key} data-testid={`map-${key}`}>
              {w ? (
                s.kind === 'cell' ? (
                  <circle
                    cx={sx}
                    cy={sy}
                    r={ringR}
                    fill="none"
                    stroke={w.color}
                    strokeWidth={w.running ? 3.5 : 2}
                    strokeDasharray={w.running ? undefined : '5 3'}
                    className="pointer-events-none"
                    data-testid={`map-work-${key}`}
                  />
                ) : (
                  <rect
                    x={sx - ringR}
                    y={sy - ringR}
                    width={ringR * 2}
                    height={ringR * 2}
                    fill="none"
                    stroke={w.color}
                    strokeWidth={w.running ? 3.5 : 2}
                    strokeDasharray={w.running ? undefined : '5 3'}
                    className="pointer-events-none"
                    data-testid={`map-work-${key}`}
                  />
                )
              ) : null}
              {hov && !sel ? (
                // 호버 — 선택과 헷갈리지 않게 **옅은** 링 하나(색도 굵기도 선택보다 약하다).
                <circle
                  cx={sx}
                  cy={sy}
                  r={rr + 3}
                  fill="none"
                  className="pointer-events-none stroke-content-faint"
                  strokeWidth={1.5}
                  data-testid={`map-hover-${key}`}
                />
              ) : null}
              {problem ? (
                <circle
                  cx={sx}
                  cy={sy}
                  r={rr + 2}
                  fill="none"
                  className="pointer-events-none stroke-warn"
                  strokeWidth={2}
                  strokeDasharray="5 3"
                  data-testid={`map-problem-${key}`}
                >
                  <title>{problem}</title>
                </circle>
              ) : null}
              {s.kind === 'cell' ? (
                <circle
                  cx={sx}
                  cy={sy}
                  r={rr}
                  className={shapeCls}
                  strokeWidth={sw}
                  {...handlers}
                />
              ) : (
                <rect
                  x={sx - rr}
                  y={sy - rr}
                  width={rr * 2}
                  height={rr * 2}
                  rx={2}
                  className={shapeCls}
                  strokeWidth={sw}
                  {...handlers}
                />
              )}
              {s.dirty ? (
                // 로컬 수정 — 도형을 둘러싸지 않고 오른쪽 위 점 하나로만 알린다(40칸을 고쳐도 조용하다).
                <circle
                  cx={sx + rr * 0.72}
                  cy={sy - rr * 0.72}
                  r={Math.min(Math.max(rr * 0.18, 2), 4)}
                  className="pointer-events-none fill-accent stroke-surface-panel"
                  strokeWidth={1}
                  data-testid={`map-dirty-${key}`}
                >
                  <title>로컬 수정 — PLC 미반영</title>
                </circle>
              ) : null}
              {s.kind === 'cell' && rr >= 6 ? (
                <text
                  x={sx}
                  y={sy}
                  textAnchor="middle"
                  dominantBaseline="central"
                  fontSize={Math.min(Math.max(rr * 0.95, 9), 30)}
                  fontWeight={600}
                  className={cn(
                    'pointer-events-none tabular-nums',
                    empty ? 'fill-content-faint' : 'fill-surface-panel',
                  )}
                  data-testid={`map-stock-${s.id}`}
                >
                  {n}
                </text>
              ) : null}
              {s.kind === 'station' && rr >= 12 ? (
                <text
                  x={sx}
                  y={sy}
                  textAnchor="middle"
                  dominantBaseline="central"
                  fontSize={Math.min(rr * 0.42, 12)}
                  className="pointer-events-none fill-amber-900 font-semibold"
                >
                  {s.id}
                </text>
              ) : null}
              {badges.slice(0, 3).map((b, i) => (
                <g
                  key={b.no}
                  className="pointer-events-none"
                  data-testid={`map-badge-${key}-${b.no}`}
                >
                  <rect
                    x={sx + rr * 0.5 - 10 + i * 20}
                    y={sy - rr - 10}
                    width={20}
                    height={13}
                    rx={3}
                    className={
                      b.type === 'PICK'
                        ? 'fill-accent'
                        : b.type === 'DROP'
                          ? 'fill-emerald-600'
                          : 'fill-content-tertiary'
                    }
                  />
                  <text
                    x={sx + rr * 0.5 + i * 20}
                    y={sy - rr - 3.5}
                    textAnchor="middle"
                    dominantBaseline="central"
                    fontSize={9}
                    fontWeight={600}
                    className="fill-surface-panel"
                  >
                    {b.no}
                    {TYPE_SHORT[b.type]}
                  </text>
                </g>
              ))}
            </g>
          )
        })}

        {/* 로봇 현재 위치 — 셀 바깥에만 눈금(가운데 개수를 가리지 않음) */}
        {(robots ?? [])
          .filter((m) => Number.isFinite(m.x) && Number.isFinite(m.y))
          .map((m) => {
            const [sx, sy] = toScreen(v, m.x, m.y)
            const R = Math.max(rr + 6, 10)
            const tick = { stroke: m.color, strokeWidth: 3, strokeLinecap: 'round' as const }
            return (
              <g
                key={m.id}
                className="pointer-events-none"
                data-testid="map-robot"
                opacity={m.moving ? 1 : 0.85}
              >
                <line x1={sx - R - 9} y1={sy} x2={sx - R} y2={sy} {...tick} />
                <line x1={sx + R} y1={sy} x2={sx + R + 9} y2={sy} {...tick} />
                <line x1={sx} y1={sy - R - 9} x2={sx} y2={sy - R} {...tick} />
                <line x1={sx} y1={sy + R} x2={sx} y2={sy + R + 9} {...tick} />
                <text
                  x={sx + R * 0.7 + 4}
                  y={sy - R * 0.7 - 4}
                  fontSize={11}
                  fontWeight={600}
                  fill={m.color}
                  stroke="white"
                  strokeWidth={3}
                  paintOrder="stroke"
                >
                  {m.name}
                </text>
              </g>
            )
          })}

        {/* 선택 링 — 맨 위에 **하나만** 선다. 실선 액센트 + 바깥 후광이라 어두운 칸 위에서도 보이고,
            점선은 문제 표시에만 남겨 둔다(고친 칸이 40개라도 화면이 조용하다). */}
        {(() => {
          const s = shapes.find((x) => sameTarget(selected, x))
          if (!s) return null
          const [sx, sy] = centre(s)
          const key = `${s.kind}-${s.id}`
          const selR = rr + (work?.get(key) ? 10 : 6)
          return (
            <g className="pointer-events-none" data-testid={`map-sel-${key}`}>
              {s.kind === 'cell' ? (
                <>
                  <circle cx={sx} cy={sy} r={rr} className="fill-accent" fillOpacity={0.16} />
                  <circle
                    cx={sx}
                    cy={sy}
                    r={selR}
                    fill="none"
                    className="stroke-surface-panel"
                    strokeWidth={5}
                  />
                  <circle
                    cx={sx}
                    cy={sy}
                    r={selR}
                    fill="none"
                    className="stroke-accent"
                    strokeWidth={2.5}
                  />
                </>
              ) : (
                <>
                  <rect
                    x={sx - rr}
                    y={sy - rr}
                    width={rr * 2}
                    height={rr * 2}
                    rx={2}
                    className="fill-accent"
                    fillOpacity={0.16}
                  />
                  <rect
                    x={sx - selR}
                    y={sy - selR}
                    width={selR * 2}
                    height={selR * 2}
                    rx={4}
                    fill="none"
                    className="stroke-surface-panel"
                    strokeWidth={5}
                  />
                  <rect
                    x={sx - selR}
                    y={sy - selR}
                    width={selR * 2}
                    height={selR * 2}
                    rx={4}
                    fill="none"
                    className="stroke-accent"
                    strokeWidth={2.5}
                  />
                </>
              )}
            </g>
          )
        })()}

        {/* 축 방향 — 회전·반전 후 +X / +Y 가 화면 어느 쪽인지 */}
        <g
          className="pointer-events-none"
          data-testid="map-axes"
          transform={`translate(${dim.w - 40} ${dim.h - 40})`}
        >
          <circle r={32} className="fill-surface-panel/85 stroke-line-default" />
          <line
            x1={0}
            y1={0}
            x2={xu * 20}
            y2={xv * 20}
            stroke="var(--color-fault)"
            strokeWidth={2.5}
            strokeLinecap="round"
          />
          <line
            x1={0}
            y1={0}
            x2={yu * 20}
            y2={yv * 20}
            stroke="var(--color-ok)"
            strokeWidth={2.5}
            strokeLinecap="round"
          />
          <text
            x={xu * 27}
            y={xv * 27}
            textAnchor="middle"
            dominantBaseline="central"
            fontSize={10}
            fontWeight={600}
            fill="var(--color-fault)"
          >
            X
          </text>
          <text
            x={yu * 27}
            y={yv * 27}
            textAnchor="middle"
            dominantBaseline="central"
            fontSize={10}
            fontWeight={600}
            fill="var(--color-ok)"
          >
            Y
          </text>
        </g>
      </svg>

      {topLeft ? (
        <div className="absolute top-2 left-2 z-10 flex items-center gap-2">{topLeft}</div>
      ) : null}

      <div className="absolute top-2 right-2 z-10 flex flex-col gap-1" data-testid="map-controls">
        <MapIconButton
          icon={<Maximize2 size={15} />}
          title="전체 맞춤"
          onClick={fit}
          testid="map-fit"
        />
        <MapIconButton
          icon={<Plus size={15} />}
          title="확대"
          onClick={() => zoomCenter(1.25)}
          testid="map-zoom-in"
        />
        <MapIconButton
          icon={<Minus size={15} />}
          title="축소"
          onClick={() => zoomCenter(1 / 1.25)}
          testid="map-zoom-out"
        />
        <MapIconButton
          icon={<Crosshair size={15} />}
          title="로봇 위치로 이동"
          disabled={!robots?.length}
          testid="map-robot-center"
          onClick={() => {
            const m = robots?.[0]
            if (!m) return
            const [sx, sy] = toScreen(v, m.x, m.y)
            setView(panBy(v, dim.w / 2 - sx, dim.h / 2 - sy))
          }}
        />
        <IconPopover
          icon={<MoreHorizontal size={15} />}
          title="더 보기 — 회전 · 보기 설정 · 범례"
          testid="map-more"
          width={260}
        >
          <div className="flex flex-col gap-2">
            <div className="flex items-end gap-2">
              <Select
                label="원 지름 (mm)"
                value={
                  SIZE_PRESETS.includes(size as (typeof SIZE_PRESETS)[number])
                    ? String(size)
                    : 'custom'
                }
                onValueChange={(s) => {
                  if (s === 'custom') return
                  setSize(Number(s))
                  save(SIZE_KEY, s)
                }}
                data-testid="map-size"
              >
                {SIZE_PRESETS.map((p) => (
                  <option key={p} value={String(p)}>
                    {p}
                  </option>
                ))}
                <option value="custom">직접 입력</option>
              </Select>
              <Input
                label="직접"
                type="number"
                mono
                min={50}
                max={5000}
                step="10"
                className="w-24"
                value={String(size)}
                onValueChange={(s) => {
                  const n = Number(s)
                  if (n >= 50 && n <= 5000) {
                    setSize(n)
                    save(SIZE_KEY, String(n))
                  }
                }}
                data-testid="map-size-input"
              />
            </div>
            <div className="flex flex-col gap-1">
              <span className="text-2xs text-content-muted">회전 (시계 방향)</span>
              <Segmented<RotId>
                ariaLabel="회전"
                value={String(rot) as RotId}
                onChange={(s) => changeRot(Number(s) as Rotation)}
                options={ROTATIONS.map((deg) => ({
                  id: String(deg) as RotId,
                  label: `${deg}°`,
                  testid: `map-rot-${deg}`,
                }))}
              />
            </div>
            <Switch
              inline
              label="좌우 반전"
              checked={flipX}
              onCheckedChange={(b) => {
                setFlipX(b)
                save(FLIPX_KEY, b ? '1' : '0')
              }}
            />
            <Switch
              inline
              label="상하 반전"
              checked={!flipY}
              onCheckedChange={(b) => {
                setFlipY(!b)
                save(FLIP_KEY, b ? '0' : '1')
              }}
            />
            <Switch
              inline
              label="슬롯 실제 크기"
              checked={footprint}
              onCheckedChange={setFootprint}
            />
            <div className="mt-1 border-t border-line-default pt-2 text-2xs font-semibold text-content-muted">
              범례
            </div>
            <ul className="flex flex-col gap-1.5 text-2xs text-content-tertiary">
              <LegendRow
                swatch={
                  <span className="flex h-4 w-4 items-center justify-center rounded-full bg-info text-3xs font-semibold text-content-on-accent">
                    3
                  </span>
                }
                text="셀 · 재고 있음 (숫자 = 개수, 색 = 구역)"
              />
              <LegendRow
                swatch={
                  <span className="h-4 w-4 rounded-full border border-line-strong bg-surface-active" />
                }
                text="셀 · 재고 없음"
              />
              <LegendRow swatch={<span className="h-4 w-4 rounded-sm bg-warn" />} text="스테이션" />
              <LegendRow
                swatch={
                  <span className="h-4 w-4 rounded-full border-2 border-dashed border-degraded bg-degraded-soft" />
                }
                text="생성 예정 셀 (파랑 = 갱신)"
              />
              <LegendRow
                swatch={
                  <span className="h-4 w-4 rounded-full border-2 border-dashed border-warn bg-warn-soft" />
                }
                text="충돌·경고 — 건너뜀 · 겹침 · 바닥 Z ≤ 0"
              />
              <LegendRow
                swatch={<span className="h-2 w-2 rounded-full bg-accent" />}
                text="로컬 수정 (PLC 미반영) — 칸 오른쪽 위 점"
              />
              <LegendRow
                swatch={
                  <span className="h-4 w-4 rounded-full border-[3px] border-accent bg-accent-soft" />
                }
                text="선택 / 대상"
              />
              {robotLegend.map((rb) => (
                <LegendRow
                  key={rb.name}
                  swatch={
                    <span
                      className="h-4 w-4 rounded-full border-[3px]"
                      style={{ borderColor: rb.color }}
                    />
                  }
                  text={`${rb.name} 작업 중 (점선 = 대기)`}
                />
              ))}
              <LegendRow
                swatch={
                  <span className="rounded bg-accent px-1 text-3xs font-semibold text-content-on-accent">
                    1P
                  </span>
                }
                text="계획 순번 (P = PICK, D = DROP)"
              />
              <LegendRow
                swatch={
                  <span className="text-3xs font-semibold">
                    <span className="text-fault-fg">X</span>
                    <span className="text-ok-fg">Y</span>
                  </span>
                }
                text="오른쪽 아래 = +X / +Y 방향"
              />
            </ul>
          </div>
        </IconPopover>
      </div>

      {children}

      <div
        className="pointer-events-none absolute bottom-2 left-2 z-10 max-w-[calc(100%-6rem)] truncate rounded-md border border-line-default bg-surface-panel/90 px-2 py-1 font-mono text-2xs text-content-tertiary tabular-nums"
        data-testid="map-status"
      >
        {statusText}
      </div>
    </div>
  )
}

function LegendRow({ swatch, text }: { swatch: ReactNode; text: string }) {
  return (
    <li className="flex items-center gap-2">
      <span className="flex w-6 flex-none justify-center">{swatch}</span>
      <span>{text}</span>
    </li>
  )
}
