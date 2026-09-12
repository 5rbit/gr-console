// 셀/스테이션 레이아웃 맵 — PLC 좌표계(mm) 위에 셀은 원, 스테이션은 정사각형으로 그린다(중심 = Position X/Y).
// 원 지름은 툴바에서 고른다. 휠 = 확대/축소(커서 고정), 드래그 = 이동, 도형 클릭 = 대상 선택 → 작성 카드.
// 로봇 현재 X/Y(WEBMON Axis) 는 십자 표식으로 겹쳐 보인다.
import { useEffect, useMemo, useRef, useState } from 'react'
import { Crosshair, Maximize2 } from 'lucide-react'
import { statusFeed } from '../../lib/feeds'
import { useSse } from '../../lib/sse'
import {
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
  type Shape,
  type View,
} from '../../lib/task/layoutModel'
import { Button } from '../../lib/ui/Button'
import { Input } from '../../lib/ui/Input'
import { Select } from '../../lib/ui/Select'
import { Switch } from '../../lib/ui/Switch'
import type { Cell, Station, Target } from '../../lib/types'
import { cn } from '../../lib/utils'

const SIZE_KEY = 'gr-cellmap-size'
const FLIP_KEY = 'gr-cellmap-flipy'

function loadNum(key: string, def: number): number {
  try {
    const v = Number(localStorage.getItem(key))
    return v > 0 ? v : def
  } catch {
    return def
  }
}
function save(key: string, v: string) {
  try {
    localStorage.setItem(key, v)
  } catch {
    /* 저장 못 해도 동작 */
  }
}

const SECTION_FILL = [
  'fill-slate-400',
  'fill-sky-500',
  'fill-emerald-500',
  'fill-amber-500',
  'fill-violet-500',
  'fill-rose-500',
]
const sectionFill = (s: number) => SECTION_FILL[s % SECTION_FILL.length]

export interface CellMapProps {
  cells: readonly Cell[]
  stations: readonly Station[]
  /** 작성 카드에 현재 잡힌 대상 — 강조 표시. */
  selected: Target | null
  /** 도형 클릭 → 대상 선택. */
  onPick: (t: Target, shape: Shape) => void
}

export function CellMap({ cells, stations, selected, onPick }: CellMapProps) {
  useSse(statusFeed)
  const wrap = useRef<HTMLDivElement>(null)
  const [dim, setDim] = useState({ w: 800, h: 500 })
  const [size, setSize] = useState(() => loadNum(SIZE_KEY, 600))
  const [flipY, setFlipY] = useState(() => {
    try {
      return localStorage.getItem(FLIP_KEY) !== '0'
    } catch {
      return true
    }
  })
  const [footprint, setFootprint] = useState(false)
  const [labels, setLabels] = useState(true)
  const [view, setView] = useState<View | null>(null)
  const [hover, setHover] = useState<Shape | null>(null)
  const [cursor, setCursor] = useState<[number, number] | null>(null)
  const drag = useRef<{ x: number; y: number; moved: boolean } | null>(null)

  const shapes = useMemo(() => shapesFrom(cells, stations), [cells, stations])
  const bounds = useMemo(() => boundsOf(shapes, size / 2), [shapes, size])
  const axis = statusFeed.data?.webmon?.Axis
  const robot =
    axis && axis.length >= 2
      ? { x: axis[0].Position, y: axis[1].Position, moving: axis[0].Running || axis[1].Running }
      : null

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
    if (bounds) setView(fitView(bounds, dim.w, dim.h, 32, flipY))
  }
  // 처음 도형이 생기거나 Y 방향이 바뀌면 맞춤.
  const boundsKey = bounds ? `${bounds.minX},${bounds.minY},${bounds.maxX},${bounds.maxY}` : ''
  useEffect(() => {
    if (!bounds) return
    setView((v) => (v && v.flipY === flipY ? v : fitView(bounds, dim.w, dim.h, 32, flipY)))
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 경계 문자열·flipY 가 바뀔 때만
  }, [boundsKey, flipY, dim.w, dim.h])

  const v =
    view ??
    (bounds
      ? fitView(bounds, dim.w, dim.h, 32, flipY)
      : { k: 0.05, ox: dim.w / 2, oy: dim.h / 2, flipY })
  const r = (size / 2) * v.k
  const step = gridStep(v.k)

  // 눈금선.
  const [wx0, wy0] = toWorld(v, 0, flipY ? dim.h : 0)
  const [wx1, wy1] = toWorld(v, dim.w, flipY ? 0 : dim.h)
  const gx: number[] = []
  const gy: number[] = []
  for (let x = Math.floor(wx0 / step) * step; x <= wx1; x += step) gx.push(x)
  for (let y = Math.floor(Math.min(wy0, wy1) / step) * step; y <= Math.max(wy0, wy1); y += step)
    gy.push(y)
  const tooMany = gx.length > 60 || gy.length > 60

  function local(e: React.MouseEvent | React.WheelEvent): [number, number] {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
    return [e.clientX - rect.left, e.clientY - rect.top]
  }

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="cell-map">
      <div className="flex flex-none flex-wrap items-end gap-2 border-b border-slate-200 px-2 py-1.5 dark:border-slate-700">
        <Select
          dense
          label="원 지름 (mm)"
          value={
            SIZE_PRESETS.includes(size as (typeof SIZE_PRESETS)[number]) ? String(size) : 'custom'
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
          label="직접 (mm)"
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
        <Switch
          label="Y 위쪽 +"
          checked={flipY}
          onCheckedChange={(b) => {
            setFlipY(b)
            save(FLIP_KEY, b ? '1' : '0')
          }}
        />
        <Switch label="슬롯 크기" checked={footprint} onCheckedChange={setFootprint} />
        <Switch label="번호" checked={labels} onCheckedChange={setLabels} />
        <span className="flex-1" />
        <span className="font-mono text-[11px] tabular-nums text-slate-500">
          {cursor ? `X ${cursor[0].toFixed(0)} · Y ${cursor[1].toFixed(0)}` : ''} · 눈금 {step} mm ·
          배율 {v.k.toFixed(3)} px/mm
        </span>
        <Button
          size="sm"
          intent="ghost"
          icon={<Maximize2 className="h-3.5 w-3.5" />}
          onClick={fit}
          title="전체 맞춤"
          data-testid="map-fit"
        >
          맞춤
        </Button>
        {robot ? (
          <Button
            size="sm"
            intent="ghost"
            icon={<Crosshair className="h-3.5 w-3.5" />}
            title="로봇 위치로 이동"
            onClick={() =>
              setView({
                ...v,
                ox: dim.w / 2 - robot.x * v.k,
                oy: flipY ? dim.h / 2 + robot.y * v.k : dim.h / 2 - robot.y * v.k,
              })
            }
          >
            로봇
          </Button>
        ) : null}
      </div>

      <div
        ref={wrap}
        className="relative min-h-0 flex-1 overflow-hidden bg-white dark:bg-slate-900"
      >
        {!shapes.length ? (
          <div className="absolute inset-0 flex items-center justify-center text-xs text-slate-400">
            등록된 셀/스테이션이 없습니다 — PLC 읽기 또는 Excel 가져오기
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
          role="img"
          aria-label="셀 레이아웃"
        >
          {/* 눈금 */}
          {!tooMany
            ? gx.map((x) => {
                const [sx] = toScreen(v, x, 0)
                return (
                  <g key={`x${x}`}>
                    <line
                      x1={sx}
                      y1={0}
                      x2={sx}
                      y2={dim.h}
                      className={cn(
                        'stroke-slate-200 dark:stroke-slate-700',
                        x === 0 && 'stroke-slate-400',
                      )}
                      strokeWidth={x === 0 ? 1.5 : 1}
                    />
                    <text x={sx + 2} y={dim.h - 3} className="fill-slate-400 text-[9px]">
                      {x}
                    </text>
                  </g>
                )
              })
            : null}
          {!tooMany
            ? gy.map((y) => {
                const [, sy] = toScreen(v, 0, y)
                return (
                  <g key={`y${y}`}>
                    <line
                      x1={0}
                      y1={sy}
                      x2={dim.w}
                      y2={sy}
                      className={cn(
                        'stroke-slate-200 dark:stroke-slate-700',
                        y === 0 && 'stroke-slate-400',
                      )}
                      strokeWidth={y === 0 ? 1.5 : 1}
                    />
                    <text x={3} y={sy - 2} className="fill-slate-400 text-[9px]">
                      {y}
                    </text>
                  </g>
                )
              })
            : null}

          {/* 슬롯 실제 크기 (length × width) */}
          {footprint
            ? shapes
                .filter((s) => s.length > 0 && s.width > 0)
                .map((s) => {
                  const [sx, sy] = toScreen(v, s.x, s.y)
                  const w = s.length * v.k
                  const h = s.width * v.k
                  return (
                    <rect
                      key={`fp-${s.kind}-${s.id}`}
                      x={sx - w / 2}
                      y={sy - h / 2}
                      width={w}
                      height={h}
                      className="fill-none stroke-slate-300 dark:stroke-slate-600"
                      strokeDasharray="4 3"
                    />
                  )
                })
            : null}

          {/* 도형 */}
          {shapes.map((s) => {
            const [sx, sy] = toScreen(v, s.x, s.y)
            const sel = sameTarget(selected, s)
            const hov = hover === s
            const common = cn(
              'cursor-pointer transition-opacity',
              s.use ? sectionFill(s.section) : 'fill-slate-200 dark:fill-slate-700',
              sel
                ? 'stroke-indigo-600 dark:stroke-indigo-300'
                : hov
                  ? 'stroke-slate-900 dark:stroke-white'
                  : 'stroke-slate-600 dark:stroke-slate-300',
              !s.use && 'opacity-60',
            )
            const sw = sel ? 3 : hov ? 2 : 1
            const handlers = {
              onMouseEnter: () => setHover(s),
              onMouseLeave: () => setHover(null),
              onClick: (e: React.MouseEvent) => {
                if (drag.current?.moved) return
                e.stopPropagation()
                onPick({ kind: s.kind, id: s.id }, s)
              },
            }
            return (
              <g key={`${s.kind}-${s.id}`} data-testid={`map-${s.kind}-${s.id}`}>
                {s.kind === 'cell' ? (
                  <circle
                    cx={sx}
                    cy={sy}
                    r={Math.max(r, 3)}
                    className={common}
                    strokeWidth={sw}
                    strokeDasharray={s.dirty ? '5 3' : undefined}
                    fillOpacity={0.55}
                    {...handlers}
                  />
                ) : (
                  <rect
                    x={sx - Math.max(r, 3)}
                    y={sy - Math.max(r, 3)}
                    width={Math.max(r, 3) * 2}
                    height={Math.max(r, 3) * 2}
                    className={common}
                    strokeWidth={sw}
                    strokeDasharray={s.dirty ? '5 3' : undefined}
                    fillOpacity={0.55}
                    {...handlers}
                  />
                )}
                {/* 중심점 */}
                <circle
                  cx={sx}
                  cy={sy}
                  r={1.5}
                  className="pointer-events-none fill-slate-900 dark:fill-white"
                />
                {labels && r >= 9 ? (
                  <text
                    x={sx}
                    y={sy - 3}
                    textAnchor="middle"
                    className="pointer-events-none fill-slate-900 text-[10px] font-semibold dark:fill-white"
                  >
                    {s.id}
                  </text>
                ) : null}
                {labels && r >= 16 ? (
                  <text
                    x={sx}
                    y={sy + 10}
                    textAnchor="middle"
                    className="pointer-events-none fill-slate-700 text-[9px] dark:fill-slate-200"
                  >
                    S{s.section} R{s.row} C{s.col}
                  </text>
                ) : null}
              </g>
            )
          })}

          {/* 로봇 현재 위치 */}
          {robot && Number.isFinite(robot.x) && Number.isFinite(robot.y)
            ? (() => {
                const [sx, sy] = toScreen(v, robot.x, robot.y)
                const c = robot.moving ? 'stroke-emerald-500' : 'stroke-red-500'
                return (
                  <g className="pointer-events-none" data-testid="map-robot">
                    <line x1={sx - 14} y1={sy} x2={sx + 14} y2={sy} className={c} strokeWidth={2} />
                    <line x1={sx} y1={sy - 14} x2={sx} y2={sy + 14} className={c} strokeWidth={2} />
                    <circle cx={sx} cy={sy} r={7} className={cn('fill-none', c)} strokeWidth={2} />
                    <text
                      x={sx + 10}
                      y={sy - 8}
                      className={cn(
                        'text-[10px] font-semibold',
                        robot.moving ? 'fill-emerald-600' : 'fill-red-600',
                      )}
                    >
                      GR {robot.x.toFixed(0)}/{robot.y.toFixed(0)}
                    </text>
                  </g>
                )
              })()
            : null}
        </svg>
        {hover ? (
          <div className="pointer-events-none absolute bottom-2 left-2 rounded bg-slate-900/85 px-2 py-1 text-[11px] text-white">
            {shapeInfo(hover)} — 클릭하면 작성 카드 대상으로
          </div>
        ) : null}
      </div>

      <div className="flex flex-none flex-wrap items-center gap-3 border-t border-slate-200 px-2 py-1 text-[11px] text-slate-500 dark:border-slate-700">
        <span>
          <span className="mr-1 inline-block h-2.5 w-2.5 rounded-full bg-sky-500/60 align-middle" />
          셀(원)
        </span>
        <span>
          <span className="mr-1 inline-block h-2.5 w-2.5 bg-amber-500/60 align-middle" />
          스테이션(사각)
        </span>
        <span>
          색 = 구역(Section) · 회색 = 미사용 · 점선 = 로컬 수정(PLC 미반영) · 굵은 테두리 = 작성
          카드 대상 · 십자 = 로봇 X/Y
        </span>
        <span className="ml-auto">
          셀 {cells.length} · 스테이션 {stations.length}
        </span>
      </div>
    </div>
  )
}
