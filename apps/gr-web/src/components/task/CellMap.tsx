// 셀/스테이션 레이아웃 맵 — PLC 좌표계(mm) 위에 셀은 원(중심 = Position X/Y), 스테이션은 정사각형.
// 스테이션 Position 은 정렬 벽이다 — 굵은 변(벽)을 Position 에 두고 몸체·타이어를 RotateType 방향으로 민다
// (`layoutModel.stationAlign`). RotateType 0/정의 밖만 예전처럼 Position 이 가운데.
// 스테이션 몸체는 슬롯 실제 크기(L × W). 몸체 테두리 = 정합(초록/빨강), 안쪽 테두리 = 인터록(파랑·주황·보라),
// 화물 원 = 트래킹 OD·TaskOffset(없으면 품목 외경) — 규칙은 `lib/task/stationLiveModel`. 호버에 IN/OUT 인디케이터.
// 화물 코드가 없는 스테이션은 더블 클릭으로 바로 입력(`onDouble`).
//
// 플롯 밖에는 아무것도 두지 않는다. 모드 토글은 왼쪽 위(`topLeft` 슬롯), 오른쪽 위에는 **자주 쓰는 넷**
// (맞춤·확대·축소·로봇 위치)만 세우고 회전·보기 설정·범례는 그 아래 ⋯ 하나로 접었다 — 회전은 이미
// 보기 설정 안에 있었고, 범례는 한 번 읽고 마는 것이라 늘 자리를 차지할 이유가 없다.
// 좌표·호버 정보는 왼쪽 아래 한 줄, 축 방향은 오른쪽 아래.
// 셀 안에는 재고 개수만 크게 그린다.
// 화면은 시계 방향 0·90·180·270° 로 돌릴 수 있고, 돌린 뒤 좌우·상하 반전도 된다(설정은 브라우저에 저장).
//
// **표현 규칙**(`lib/task/mapStyleModel`, 2026-09-21 사용자와 조율) — 채널마다 뜻 하나:
//   채움 = 칸 상태 — 빈 칸(흰) · 재고 있음(연한 파랑) · Max(단수 Max 도달, 진한 파랑) · 비활성(Use=false,
//          회색 + 빗금). 스테이션 = 진한 슬레이트 사각.
//   윤곽 = **한 줄**, 겹치면 우선순위 하나만: 선택(흐르는 점선) > 로봇 작업(로봇 색) > 계획(파랑) >
//          품목 강조 > 못 쓰는 칸(주황) > 로컬 수정(편집 모드, 점선) > 호버.
//   구역 = 칸 색이 아니라 바탕의 옅은 영역 + "S2" 이름.
// 셀 바닥 Z ≤ 0 은 정상이다 — PLC 는 스테이션(Id > 2000)에만 INVALID_CELL_POSZ 를 낸다.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Crosshair, Maximize2, Minus, MoreHorizontal, Plus } from 'lucide-react'
import {
  ROTATIONS,
  SIZE_PRESETS,
  boundsOf,
  gridStep,
  panBy,
  resolveView,
  sameTarget,
  shapeCentre,
  shapesFrom,
  stationHalf,
  toScreen,
  toWorld,
  zoomAt,
  type Rotation,
  type Shape,
  type View,
} from '../../lib/task/layoutModel'
import type { PreviewCell } from '../../lib/task/layoutGen'
import {
  fillState,
  outlineOf,
  unusable,
  type FillState,
  type OutlineKind,
} from '../../lib/task/mapStyleModel'
import type { PlanStep } from '../../lib/task/plan'
import { overlay as planOverlay } from '../../lib/task/plan'
import { f1 } from '../../lib/meas/format'
import { rotateLabel } from '../../lib/task/stationOffsetModel'
import {
  interlockLamps,
  MATCH_LABEL,
  RING_LABEL,
  stateLabel,
  stationFill,
  stationMatch,
  stationRing,
  tireOf,
  type Lamp,
  type StationFill,
  type StationRing,
} from '../../lib/task/stationLiveModel'
import type { StationLive } from '../../lib/task/types'
import { EmptyState } from '../../lib/ui/EmptyState'
import { IconPopover, MapIconButton } from '../../lib/ui/IconPopover'
import { Input } from '../../lib/ui/Input'
import { Segmented } from '../../lib/ui/Segmented'
import { Select } from '../../lib/ui/Select'
import { Switch } from '../../lib/ui/Switch'
import type { Cell, Item, Station, StockEntry, Target, TaskType } from '../../lib/types'
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

/** 셀 채움 — 칸 상태(`mapStyleModel.fillState`). 같은 파랑 계열의 농도로 빈 칸 → 재고 → Max. */
const FILL_CLS: Record<FillState, string> = {
  empty: 'fill-surface-panel stroke-line-strong',
  stocked: 'fill-info-soft stroke-info/50',
  full: 'fill-info stroke-info-fg',
  disabled: 'fill-surface-active stroke-line-default',
}
const TEXT_CLS: Record<FillState, string> = {
  empty: 'fill-content-faint',
  stocked: 'fill-content-primary',
  full: 'fill-content-on-accent',
  disabled: 'fill-content-faint',
}
/** 스테이션 몸체 채움 — 상태를 싣지 않는 바탕(모름 = 진한 슬레이트, Use=false = 빗금). */
const STATION_FILL_CLS: Record<StationFill, string> = {
  disabled: 'fill-surface-active',
  unknown: 'fill-content-secondary',
  normal: 'fill-surface-inset',
}
const STATION_TEXT_CLS: Record<StationFill, string> = {
  disabled: 'fill-content-faint',
  unknown: 'fill-surface-panel',
  normal: 'fill-content-muted',
}
/** 윤곽 한 줄의 모양(로봇 작업은 로봇 색을 따로 싣는다). */
type ShapeProps = React.SVGAttributes<SVGElement>
const OUTLINE_PROPS: Record<OutlineKind, ShapeProps> = {
  selected: { className: 'stroke-accent', strokeWidth: 2.5, strokeDasharray: '6 4' },
  work: { strokeWidth: 2 },
  planned: { className: 'stroke-pending-fg', strokeWidth: 2 },
  highlight: { className: 'stroke-accent', strokeWidth: 1.5 },
  unusable: { className: 'stroke-warn', strokeWidth: 1.5 },
  dirty: { className: 'stroke-content-muted', strokeWidth: 1.5, strokeDasharray: '4 3' },
  hover: { className: 'stroke-content-faint', strokeWidth: 1.5 },
}
/** 스테이션 몸체 테두리 — 정합(`stationMatch`): 초록 = 맞음, 빨강 = 어긋남, 그 밖 = 기본 선. */
const MATCH_PROPS: Record<'ok' | 'mismatch' | 'none', { cls: string; w: number }> = {
  ok: { cls: 'stroke-ok', w: 3 },
  mismatch: { cls: 'stroke-fault', w: 3 },
  none: { cls: 'stroke-line-strong', w: 1 },
}
/** 스테이션 테두리의 CV 인터록 색(`stationRing`) — 몸체 테두리 한 줄에 싣는다. 초록·빨강은 정합이 쓰므로 쓰지 않는다. */
const RING_PROPS: Record<StationRing, ShapeProps> = {
  cv_not_ready: { className: 'stroke-content-muted', strokeDasharray: '4 3' },
  robot_in: { className: 'stroke-warn' },
  comp: { className: 'stroke-pending-fg' },
  req: { className: 'stroke-info' },
  meas_req: { className: 'stroke-info', strokeDasharray: '4 3' },
}
/** 인디케이터 켜짐 색 — 테두리 색과 같은 뜻. */
const LAMP_ON: Record<string, string> = {
  cvok: 'bg-ok',
  req: 'bg-info',
  meas_req: 'bg-info',
  item_exist: 'bg-ok',
  cvno: 'bg-warn',
  comp: 'bg-pending-fg',
  meas_comp: 'bg-ok',
  meas_err: 'bg-fault',
}
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
  /** 더블 클릭(스테이션 화물 코드 입력 등). `deferClick` 이 참인 도형은 단일 클릭을 잠시 미뤄 겹치지 않게 한다. */
  onDouble?: (t: Target, shape: Shape) => void
  deferClick?: (shape: Shape) => boolean
  /** 셀 재고 — 원 안의 개수. */
  stock?: ReadonlyMap<number, StockEntry>
  /** GRM 스테이션 실시간 상태 — 채움(트래킹·화물·오류)과 안쪽 테두리(CV 인터록). 없으면 예전 모양. */
  stationLive?: ReadonlyMap<number, StationLive>
  /** 호버 카드의 재고 줄에 화물 규격을 붙일 품목 목록(선택). */
  items?: readonly Item[]
  /** 로컬 수정(PLC 미반영) 윤곽 점선을 그린다 — 레이아웃 편집 모드에서만 켠다. */
  showDirty?: boolean
  /** 함께 강조할 셀들(품목 선택 → 그 품목이 든 셀). 선택 링보다 한 단계 약한 실선 링. */
  highlight?: { label: string; cells: ReadonlySet<number> }
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
  onDouble,
  deferClick,
  stock,
  stationLive,
  items,
  showDirty = false,
  highlight,
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
  const clickTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  useEffect(() => () => void (clickTimer.current && clearTimeout(clickTimer.current)), [])

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
        align: null,
        rotate: 0,
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

  // `view` = 사용자가 끌기·확대·지목으로 만든 변환. `null` 이면 맞춤 모드 — 매 렌더 지금 컨테이너
  // 크기와 경계로 맞춘다(`resolveView`). 맞춤 버튼·회전·반전은 맞춤 모드로 돌아간다.
  const fit = () => setView(null)
  useEffect(() => setView(null), [flipY, flipX, rot])

  const v: View = resolveView(view, bounds, dim.w, dim.h, 48, flipY, flipX, rot)
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
    const [sx, sy] = toScreen(v, ...shapeCentre(s, size / 2))
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
  /** 도형을 그리는 화면 중심 — 정렬 스테이션은 벽(Position)에서 타이어 쪽으로 반지름만큼 민 자리. */
  const centre = (s: Shape) => toScreen(v, ...shapeCentre(s, size / 2))
  /** 스테이션 몸체의 화면 반폭(px) — 슬롯 실제 크기, 90°/270° 에서는 가로세로가 바뀐다. */
  const stationHalfPx = (s: Shape): [number, number] => {
    const [hx, hy] = stationHalf(s, size / 2)
    return [Math.max((sideways ? hy : hx) * v.k, 3), Math.max((sideways ? hx : hy) * v.k, 3)]
  }
  // 구역 영역 — 같은 Section 셀들의 화면 외곽(+여백). 회전해도 화면 좌표로 모으니 늘 축 정렬 사각이다.
  // 스테이션은 넣지 않는다(셀과 멀리 떨어져 있어 영역이 레이아웃 전체로 번진다).
  const zones = (() => {
    const by = new Map<number, [number, number, number, number]>()
    for (const c of shapes) {
      if (c.kind !== 'cell') continue
      const [x, y] = centre(c)
      const b = by.get(c.section)
      by.set(
        c.section,
        b
          ? [Math.min(b[0], x), Math.min(b[1], y), Math.max(b[2], x), Math.max(b[3], y)]
          : [x, y, x, y],
      )
    }
    const pad = rr + 8
    return [...by].map(([section, [x0, y0, x1, y1]]) => ({
      section,
      x: x0 - pad,
      y: y0 - pad - 14,
      w: x1 - x0 + pad * 2,
      h: y1 - y0 + pad * 2 + 14,
    }))
  })()
  const zoomCenter = (f: number) => setView(zoomAt(v, dim.w / 2, dim.h / 2, f))

  // 호버 카드 — 도형 옆에 뜨는 정보(자리·좌표·크기 + 셀 재고·화물 규격 / 스테이션 파라미터 + 상태).
  // 예전엔 왼쪽 아래 상태 줄 한 줄에 몰아 적어 길면 잘렸다.
  const cardRef = useRef<HTMLDivElement>(null)
  const [cardH, setCardH] = useState(200)
  useLayoutEffect(() => {
    const h = cardRef.current?.offsetHeight
    if (h && Math.abs(h - cardH) > 1) setCardH(h)
  })
  const hoverCard = (() => {
    if (!hover || drag.current?.moved) return null
    const [hx, hy] = centre(hover)
    const W = hover.kind === 'station' ? 300 : 248
    const left = hx + rr + 10 + W > dim.w ? Math.max(4, hx - rr - 10 - W) : hx + rr + 10
    // 높이는 내용마다 다르다 — 그린 뒤 잰 값(`cardH`)으로 아래가 넘치면 위로 올린다.
    const top = Math.max(4, Math.min(hy - rr, dim.h - cardH - 4))
    return (
      <div
        ref={cardRef}
        className="pointer-events-none absolute z-20 flex flex-col overflow-hidden rounded-md border border-line-default bg-surface-panel text-2xs text-content-secondary shadow-lg"
        style={{ left, top, width: W }}
        data-testid="map-hover-card"
      >
        <HoverBody
          s={hover}
          cell={hover.kind === 'cell' ? cells.find((c) => c.id === hover.id) : undefined}
          station={hover.kind === 'station' ? stations.find((c) => c.id === hover.id) : undefined}
          live={hover.kind === 'station' ? stationLive?.get(hover.id) : undefined}
          st={stock?.get(hover.id) ?? null}
          items={items}
          work={work?.get(`${hover.kind}-${hover.id}`)}
        />
      </div>
    )
  })()
  const statusText = cursor
    ? `X ${f1(cursor[0])} · Y ${f1(cursor[1])} · 눈금 ${step} mm`
    : `${highlight ? `${highlight.label} ${highlight.cells.size}칸 · ` : ''}셀 ${cells.length} · 스테이션 ${stations.length}${previewShapes.length ? ` · 생성 예정 ${previewShapes.length}` : ''} · 눈금 ${step} mm${rot ? ` · 회전 ${rot}°` : ''}`

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
            <path d="M 0 0 L 10 5 L 0 10 z" className="fill-pending-fg" />
          </marker>
          {/* 비활성(Use=false) 빗금 — 화면 px 간격이라 확대해도 선 굵기·간격이 같다. */}
          <pattern
            id="off-hatch"
            patternUnits="userSpaceOnUse"
            width={6}
            height={6}
            patternTransform="rotate(45)"
          >
            <line x1={0} y1={0} x2={0} y2={6} className="stroke-content-faint" strokeWidth={1.5} />
          </pattern>
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
              // 스테이션 몸체는 이미 실제 크기다 — 셀만.
              .filter((s) => s.kind === 'cell' && s.length > 0 && s.width > 0)
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
          // 표현 규칙(`mapStyleModel`)에 맞춘다: 예정 = 보라(pending) 점선 — 새 칸은 옅은 보라 채움, 갱신은
          // 채움 없이 윤곽만(아래 기존 칸이 비친다), 그대로면 조용한 회색, 건너뜀 = 못 쓰는 칸과 같은 주황.
          // 파랑(info)은 재고 채움이 쓰므로 여기서 쓰지 않는다.
          const tone =
            out === 'skipped'
              ? 'fill-warn-soft stroke-warn'
              : out === 'updated'
                ? 'fill-none stroke-pending-fg'
                : out === 'unchanged'
                  ? 'fill-surface-inset stroke-line-strong'
                  : 'fill-pending-soft stroke-pending-fg'
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
                  className="pointer-events-none stroke-pending-fg"
                  strokeWidth={2}
                  strokeDasharray="6 4"
                  markerEnd="url(#plan-arrow)"
                />
              )
            })
          : null}

        {/* 구역 — 칸 색이 아니라 바탕의 옅은 영역 + 이름. 도형보다 먼저 그려 뒤에 깔린다. */}
        {zones.map((z) => (
          <g
            key={`zone-${z.section}`}
            className="pointer-events-none"
            data-testid={`map-zone-${z.section}`}
          >
            <rect
              x={z.x}
              y={z.y}
              width={z.w}
              height={z.h}
              rx={10}
              className="fill-content-muted stroke-line-default"
              fillOpacity={0.07}
              strokeWidth={1}
            />
            <text
              x={z.x + 8}
              y={z.y + 13}
              fontSize={11}
              fontWeight={600}
              className="fill-content-muted"
            >
              S{z.section}
            </text>
          </g>
        ))}

        {/* 도형 */}
        {shapes.map((s) => {
          const [sx, sy] = centre(s)
          const key = `${s.kind}-${s.id}`
          // 스테이션 재고 = 컨베이어가 옮겨 온 화물(백엔드 `stock::conveyor`) — 품목 코드를 도형 안에 적는다.
          const st = stock?.get(s.id)
          const n = st?.count ?? 0
          const stackMax =
            (st?.item_code ? items?.find((i) => i.code === st.item_code)?.spec?.stack_max : 0) ?? 0
          const sl = s.kind === 'station' ? stationLive?.get(s.id) : undefined
          const sf = s.kind === 'station' ? stationFill(s.use, sl) : null
          const ring = s.kind === 'station' && s.use ? stationRing(sl) : null
          const match = s.kind === 'station' && s.use ? stationMatch(sl, st).state : null
          const mp = MATCH_PROPS[match ?? 'none']
          const fs: FillState =
            s.kind === 'station'
              ? s.use
                ? 'stocked'
                : 'disabled'
              : fillState({ use: s.use, count: n, stackMax })
          const w = work?.get(key)
          // 윤곽은 **한 줄** — 켜진 것 중 우선순위가 가장 높은 하나(`mapStyleModel.OUTLINE_ORDER`).
          const outline = outlineOf({
            selected: sameTarget(selected, s),
            work: !!w,
            planned: !!ov?.badges.get(key)?.length,
            highlight: s.kind === 'cell' && !!highlight?.cells.has(s.id),
            unusable: !!unusable(s),
            dirty: s.dirty && showDirty,
            hover: hover === s,
          })
          // 스테이션 몸체 테두리는 **한 줄** — 어긋남(빨강) > CV 인터록 > 정합(초록) > 기본 선.
          // 인터록을 안쪽에 한 줄 더 그리면 테두리가 이중으로 보였다(2026-09-22).
          const body: { cls: string; w: number; dash?: string } =
            match === 'mismatch' || !ring
              ? mp
              : {
                  cls: RING_PROPS[ring].className ?? mp.cls,
                  w: 2.5,
                  dash: RING_PROPS[ring].strokeDasharray as string | undefined,
                }
          const shapeCls = cn(
            'cursor-pointer',
            sf ? cn(STATION_FILL_CLS[sf], body.cls) : FILL_CLS[fs],
          )
          const handlers = {
            onMouseEnter: () => setHover(s),
            onMouseLeave: () => setHover(null),
            onClick: (e: React.MouseEvent) => {
              if (drag.current?.moved) return
              e.stopPropagation()
              if (clickTimer.current) clearTimeout(clickTimer.current)
              clickTimer.current = null
              if (onDouble && deferClick?.(s)) {
                // 더블 클릭이 올 수 있는 도형 — 단일 클릭은 잠시 뒤에(두 번째 클릭이 오면 취소).
                if (e.detail > 1) return
                clickTimer.current = setTimeout(() => {
                  clickTimer.current = null
                  onPick({ kind: s.kind, id: s.id }, s)
                }, 220)
                return
              }
              onPick({ kind: s.kind, id: s.id }, s)
            },
            onDoubleClick: (e: React.MouseEvent) => {
              if (!onDouble || drag.current?.moved) return
              e.stopPropagation()
              if (clickTimer.current) clearTimeout(clickTimer.current)
              clickTimer.current = null
              onDouble({ kind: s.kind, id: s.id }, s)
            },
            onContextMenu: (e: React.MouseEvent) => {
              e.preventDefault()
              e.stopPropagation()
              onContext?.({ kind: s.kind, id: s.id }, s, e)
            },
          }
          // 셀 = 원(지름 = 도형 크기), 스테이션 = 실제 크기 사각 — 채움·빗금·윤곽이 같은 모양을 쓴다.
          // `pad` = 바깥(+)·안쪽(−)으로 넓히는 픽셀.
          const [hw, hh] = s.kind === 'station' ? stationHalfPx(s) : [rr, rr]
          const shapeEl = (props: ShapeProps, pad: number, children?: ReactNode) =>
            s.kind === 'cell' ? (
              <circle cx={sx} cy={sy} r={Math.max(rr + pad, 1)} {...props}>
                {children}
              </circle>
            ) : (
              <rect
                x={sx - hw - pad}
                y={sy - hh - pad}
                width={Math.max((hw + pad) * 2, 2)}
                height={Math.max((hh + pad) * 2, 2)}
                rx={3}
                {...props}
              >
                {children}
              </rect>
            )
          return (
            <g
              key={key}
              data-testid={`map-${key}`}
              data-fill={sf ?? fs}
              data-ring={ring ?? undefined}
              data-match={match ?? undefined}
            >
              {shapeEl(
                {
                  className: shapeCls,
                  strokeWidth: sf ? body.w : 1,
                  strokeDasharray: sf ? body.dash : undefined,
                  ...handlers,
                },
                0,
              )}
              {fs === 'disabled'
                ? shapeEl(
                    {
                      fill: 'url(#off-hatch)',
                      className: 'pointer-events-none',
                      opacity: 0.6,
                    },
                    0,
                  )
                : null}
              {s.kind === 'station' ? (
                <StationMarks
                  s={s}
                  v={v}
                  size={size}
                  half={[hw, hh]}
                  live={sl}
                  st={st ?? null}
                  item={st?.item_code ? items?.find((i) => i.code === st.item_code) : undefined}
                  fill={sf ?? 'unknown'}
                />
              ) : null}
              {outline ? (
                <g
                  className="pointer-events-none"
                  data-testid={`map-outline-${key}`}
                  data-outline={outline}
                >
                  {outline === 'selected'
                    ? shapeEl(
                        { fill: 'none', className: 'stroke-surface-panel', strokeWidth: 5 },
                        3,
                      )
                    : null}
                  {shapeEl(
                    {
                      fill: 'none',
                      ...OUTLINE_PROPS[outline],
                      ...(outline === 'work' && w
                        ? {
                            stroke: w.color,
                            strokeWidth: w.running ? 3 : 2,
                            strokeDasharray: w.running ? undefined : '5 3',
                          }
                        : {}),
                    },
                    3,
                    outline === 'selected' ? (
                      // 흐르는 점선 — 점선 한 주기(6+4)씩 밀어 끊김 없이 돈다.
                      <animate
                        attributeName="stroke-dashoffset"
                        values="0;-10"
                        dur="0.6s"
                        repeatCount="indefinite"
                      />
                    ) : null,
                  )}
                </g>
              ) : null}
              {s.kind === 'cell' && rr >= 6 ? (
                <text
                  x={sx}
                  y={sy}
                  textAnchor="middle"
                  dominantBaseline="central"
                  fontSize={Math.min(Math.max(rr * 0.95, 9), 30)}
                  fontWeight={600}
                  className={cn('pointer-events-none tabular-nums', TEXT_CLS[fs])}
                  data-testid={`map-stock-${s.id}`}
                >
                  {n}
                </text>
              ) : null}
            </g>
          )
        })}

        {/* 계획 순번 태그 — 모든 도형·윤곽 **위** 한 층(이웃 칸 윤곽이 가리지 않게). 색은 "계획" 윤곽과 같은
            보라 하나 — 태그와 윤곽이 한 표시로 읽힌다. 글자로 종류(P/D/M…)를 가른다. */}
        {ov
          ? shapes.map((s) => {
              const key = `${s.kind}-${s.id}`
              const badges = ov.badges.get(key) ?? []
              if (!badges.length) return null
              const [sx, sy] = centre(s)
              return (
                <g key={`badge-${key}`} className="pointer-events-none">
                  {badges.slice(0, 3).map((b, i) => (
                    <g key={b.no} data-testid={`map-badge-${key}-${b.no}`}>
                      <rect
                        x={sx + rr * 0.5 - 10 + i * 20}
                        y={sy - rr - 10}
                        width={20}
                        height={13}
                        rx={3}
                        className="fill-pending-fg stroke-surface-panel"
                        strokeWidth={1}
                      />
                      <text
                        x={sx + rr * 0.5 + i * 20}
                        y={sy - rr - 3.5}
                        textAnchor="middle"
                        dominantBaseline="central"
                        fontSize={9}
                        fontWeight={600}
                        className="fill-content-on-accent"
                      >
                        {b.no}
                        {TYPE_SHORT[b.type]}
                      </text>
                    </g>
                  ))}
                </g>
              )
            })
          : null}

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
              <li className="text-3xs font-semibold text-content-muted">채움 — 칸 상태</li>
              <LegendRow
                swatch={
                  <span className="h-4 w-4 rounded-full border border-line-strong bg-surface-panel" />
                }
                text="빈 칸"
              />
              <LegendRow
                swatch={
                  <span className="h-4 w-4 rounded-full border border-info/50 bg-info-soft" />
                }
                text="재고 있음 (숫자 = 개수)"
              />
              <LegendRow
                swatch={<span className="h-4 w-4 rounded-full bg-info" />}
                text="Max — 품목 단수 Max 도달"
              />
              <LegendRow
                swatch={
                  <span className="h-4 w-4 rounded-full border border-line-default bg-surface-active" />
                }
                text="비활성 — Use = false (빗금)"
              />
              <LegendRow
                swatch={<span className="h-4 w-4 rounded-sm bg-content-secondary" />}
                text="스테이션"
              />
              <LegendRow
                swatch={
                  <span className="relative h-4 w-4 rounded-sm bg-content-secondary">
                    <span className="absolute inset-y-0 left-0 w-1 bg-content-primary" />
                  </span>
                }
                text="스테이션 굵은 변 = 정렬 벽(Position) — RotateType 방향으로 타이어가 닿는 쪽"
              />
              <li className="mt-1 text-3xs font-semibold text-content-muted">
                스테이션 테두리 — 정합(화물 유무 · 트래킹 · 화물 코드)
              </li>
              {(['ok', 'mismatch'] as const).map((k) => (
                <LegendRow
                  key={k}
                  swatch={
                    <svg width={16} height={16} aria-hidden>
                      <rect
                        x={2}
                        y={2}
                        width={12}
                        height={12}
                        rx={2}
                        strokeWidth={2.5}
                        className={cn('fill-surface-inset', MATCH_PROPS[k].cls)}
                      />
                    </svg>
                  }
                  text={k === 'ok' ? MATCH_LABEL.ok : '불일치 · GRM 오류 — 호버에 이유'}
                />
              ))}
              <LegendRow
                swatch={
                  <svg width={16} height={16} aria-hidden>
                    <circle
                      cx={8}
                      cy={8}
                      r={6}
                      className="fill-content-primary stroke-content-primary"
                      fillOpacity={0.14}
                      strokeWidth={1.5}
                    />
                  </svg>
                }
                text="화물 — 트래킹 OD·위치(없으면 품목 외경, 점선 = 크기 모름) · 코드 없으면 더블 클릭으로 입력"
              />
              <li className="mt-1 text-3xs font-semibold text-content-muted">
                스테이션 안쪽 테두리 — CV 인터록, 위가 우선
              </li>
              {(['cv_not_ready', 'robot_in', 'comp', 'req', 'meas_req'] as const).map((k) => (
                <LegendRow
                  key={k}
                  swatch={
                    <svg width={16} height={16} aria-hidden>
                      <rect
                        x={2}
                        y={2}
                        width={12}
                        height={12}
                        rx={2}
                        fill="none"
                        strokeWidth={2}
                        {...RING_PROPS[k]}
                      />
                    </svg>
                  }
                  text={RING_LABEL[k]}
                />
              ))}
              <LegendRow
                swatch={
                  <span className="h-4 w-6 rounded border border-line-default bg-content-muted/10" />
                }
                text="구역(Section) — 바탕 영역 + 이름"
              />
              <li className="mt-1 text-3xs font-semibold text-content-muted">
                윤곽 — 한 줄, 위가 우선
              </li>
              <LegendRow
                swatch={
                  <span className="h-4 w-4 rounded-full border-[2.5px] border-dashed border-accent" />
                }
                text="1 선택 (흐르는 점선)"
              />
              <LegendRow
                swatch={<span className="h-4 w-4 rounded-full border-2 border-content-tertiary" />}
                text="2 로봇 작업 — 로봇 색 (점선 = 대기)"
              />
              <LegendRow
                swatch={<span className="h-4 w-4 rounded-full border-2 border-pending-fg" />}
                text="3 계획에 든 칸"
              />
              <LegendRow
                swatch={<span className="h-4 w-4 rounded-full border-[1.5px] border-accent" />}
                text="4 고른 품목이 든 셀"
              />
              <LegendRow
                swatch={<span className="h-4 w-4 rounded-full border-[1.5px] border-warn" />}
                text="5 못 쓰는 칸 — Use=false · 스테이션 Z ≤ 0"
              />
              <LegendRow
                swatch={
                  <span className="h-4 w-4 rounded-full border-[1.5px] border-dashed border-content-muted" />
                }
                text="6 로컬 수정 (PLC 미반영, 편집 모드)"
              />
              <LegendRow
                swatch={
                  <span className="h-4 w-4 rounded-full border-[1.5px] border-content-faint" />
                }
                text="7 호버"
              />
              <li className="mt-1 text-3xs font-semibold text-content-muted">레이아웃 편집</li>
              <LegendRow
                swatch={
                  <span className="h-4 w-4 rounded-full border-2 border-dashed border-pending-fg bg-pending-soft" />
                }
                text="생성 예정 셀 (보라 점선 · 채움 없음 = 갱신)"
              />
              <LegendRow
                swatch={
                  <span className="h-4 w-4 rounded-full border-2 border-dashed border-warn bg-warn-soft" />
                }
                text="생성 예정 충돌 — 건너뜀 · 겹침"
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
                  <span className="rounded bg-pending-fg px-1 text-3xs font-semibold text-content-on-accent">
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
      {hoverCard}

      <div
        className="pointer-events-none absolute bottom-2 left-2 z-10 max-w-[calc(100%-6rem)] truncate rounded-md border border-line-default bg-surface-panel/90 px-2 py-1 font-mono text-2xs text-content-tertiary tabular-nums"
        data-testid="map-status"
      >
        {statusText}
      </div>
    </div>
  )
}

/** 두 칸 격자(이름 · 값). */
function KvGrid({ rows }: { rows: [string, ReactNode][] }) {
  return (
    <dl className="m-0 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 tabular-nums">
      {rows.map(([k, v]) => (
        <div key={k} className="contents">
          <dt className="text-content-muted">{k}</dt>
          <dd className="m-0 min-w-0 truncate">{v}</dd>
        </div>
      ))}
    </dl>
  )
}

/**
 * 호버 카드 본문 — **위 = 제품·재고**(셀) / 운용 파라미터(스테이션), **아래 = 자리·치수**.
 * 명령을 만들 때 먼저 보는 것은 "무엇이 몇 개"라 위에 크게, 좌표는 확인용이라 아래에 옅은 면으로 가른다.
 */
function HoverBody({
  s,
  cell,
  station,
  live,
  st,
  items,
  work,
}: {
  s: Shape
  cell?: Cell
  station?: Station
  live?: StationLive
  st: StockEntry | null
  items?: readonly Item[]
  work?: WorkMark
}) {
  const it = st?.item_code ? items?.find((i) => i.code === st.item_code) : undefined
  const n = st?.count ?? 0

  const place: [string, ReactNode][] = [
    ['S / R / C', `${s.section} / ${s.row} / ${s.col}`],
    ['X / Y / Z', `${f1(s.x)} / ${f1(s.y)} / ${f1(s.z)}`],
  ]
  if (s.length || s.width) place.push(['L × W', `${f1(s.length)} × ${f1(s.width)}`])
  if (cell && !cell.blend_use) place.push(['BlendUse', 'N'])

  const notes: string[] = []
  const off = unusable(s)
  if (off) notes.push(off)
  if (s.dirty) notes.push('로컬 수정 — PLC 미반영')

  let top: ReactNode
  if (s.kind === 'cell') {
    top =
      n > 0 ? (
        <>
          <div className="flex items-baseline gap-2">
            <span className="text-sm font-semibold text-content-primary tabular-nums">{n}개</span>
            <span className="min-w-0 truncate font-medium text-content-primary">
              {st?.item_code ?? '-'}
              {it?.name ? ` · ${it.name}` : ''}
            </span>
          </div>
          {it ? (
            <KvGrid
              rows={[
                [
                  'ID / OD / H',
                  `${f1(it.inner_diameter)} / ${f1(it.outer_diameter)} / ${f1(it.height)}`,
                ],
                ['StackHeight', `${f1(it.height * n)} mm`],
                ...(it.note.trim() ? ([['Note', it.note]] as [string, ReactNode][]) : []),
              ]}
            />
          ) : (
            <span className="text-content-faint">등록되지 않은 품목</span>
          )}
        </>
      ) : (
        <span className="text-content-faint">재고 없음</span>
      )
  } else {
    top = station ? (
      <KvGrid
        rows={[
          ['ConvNo', String(station.conv_no)],
          ['Group', `${station.group}-${station.group_index}`],
          ['RotateType', rotateLabel(station.rotate_type)],
          ['TaskType', String(station.task_type)],
          ...(n > 0
            ? ([
                [
                  'ItemCode',
                  `${st?.item_code || '?'}${it?.name ? ` · ${it.name}` : ''}${n > 1 ? ` ×${n}` : ''}`,
                ],
              ] as [string, ReactNode][])
            : []),
          ...stationLiveRows(live, st),
        ]}
      />
    ) : null
  }

  return (
    <>
      <div className="flex items-baseline gap-2 px-2.5 pt-2">
        <span className="text-xs font-semibold text-content-primary">
          {s.kind === 'cell' ? 'Cell' : 'Station'} #{s.id}
        </span>
        {work ? <span className="ml-auto truncate text-content-muted">{work.label}</span> : null}
      </div>
      <section className="flex flex-col gap-1 px-2.5 pt-1 pb-2" data-testid="hover-stock">
        <span className="text-3xs font-semibold tracking-wide text-content-muted">
          {s.kind === 'cell' ? '재고 · 제품' : '스테이션'}
        </span>
        {top}
      </section>
      {s.kind === 'station' && live?.live ? <InterlockLamps l={live} /> : null}
      <section
        className="flex flex-col gap-1 border-t border-line-default bg-surface-inset px-2.5 py-2"
        data-testid="hover-place"
      >
        <span className="text-3xs font-semibold tracking-wide text-content-muted">
          {s.kind === 'cell' ? '셀' : '위치'}
        </span>
        <KvGrid rows={place} />
      </section>
      {notes.length ? (
        <ul className="m-0 flex list-none flex-col gap-0.5 border-t border-line-default bg-warn-soft px-2.5 py-1.5 text-warn-fg">
          {notes.map((m) => (
            <li key={m}>{m}</li>
          ))}
        </ul>
      ) : null}
    </>
  )
}

/**
 * 스테이션 표식 — 정렬 벽(Position 을 지나는 굵은 변), 화물 원(트래킹 OD·TaskOffset 또는 품목 외경, 내경 점선),
 * 화물 코드, 스테이션 번호. 월드 좌표로 계산해 `toScreen` 하므로 화면 회전·반전을 따른다.
 * 화물 원은 GRM 이 화물을 감지할 때(모르면 콘솔 재고가 있을 때)만 그린다.
 */
function StationMarks({
  s,
  v,
  size,
  half,
  live,
  st,
  item,
  fill,
}: {
  s: Shape
  v: View
  size: number
  /** 몸체 화면 반폭(px). */
  half: [number, number]
  live?: StationLive
  st: StockEntry | null
  item?: Item
  fill: StationFill
}) {
  const [hx, hy] = stationHalf(s, size / 2)
  const [sx, sy] = toScreen(v, ...shapeCentre(s, size / 2))
  const [hw, hh] = half
  const n = st?.count ?? 0
  const known = !!live?.live && !live.stale
  const present = known ? !!live?.pi.item_exist : n > 0

  let wall: ReactNode = null
  if (s.align) {
    const [ax, ay] = s.align
    // 벽은 흐름에 수직 — 흐름이 X 축이면 Y 반폭, Y 축이면 X 반폭만큼 뻗는다.
    const w = ax !== 0 ? hy : hx
    // 스토퍼는 타이어 **반대쪽**(−align)에 그린다 — 닿는 선(Position) 위에 얹으면 화물 원 가장자리를 가린다.
    // 가는 막대 + 양 끝 짧은 꺾임(⊐)으로, 두께만큼 바깥으로 비켜 놓는다.
    const px = 1 / v.k
    const bar = 2
    const tick = 5
    const off = (bar / 2 + 0.5) * px
    const at = (u: number, back: number) =>
      toScreen(v, s.x - ay * u - ax * back, s.y + ax * u - ay * back)
    const [x1, y1] = at(-w, off)
    const [x2, y2] = at(w, off)
    const [t1x, t1y] = at(-w, off + tick * px)
    const [t2x, t2y] = at(w, off + tick * px)
    wall = (
      <path
        d={`M${t1x},${t1y} L${x1},${y1} L${x2},${y2} L${t2x},${t2y}`}
        fill="none"
        className="stroke-content-secondary"
        strokeWidth={bar}
        strokeLinecap="round"
        strokeLinejoin="round"
        data-testid={`map-station-wall-${s.id}`}
      />
    )
  }

  let tire: ReactNode = null
  if (present) {
    const t = tireOf(
      known && live ? live.rotate_type : s.rotate,
      known && live ? live.od : 0,
      live?.tx ?? 0,
      live?.ty ?? 0,
      item?.outer_diameter ?? 0,
      Math.min(hx, hy) * 2 * 0.8,
    )
    const [cx, cy] = toScreen(v, s.x + t.dx, s.y + t.dy)
    const r = Math.max((t.d / 2) * v.k, 2)
    const ri = item?.inner_diameter ? (item.inner_diameter / 2) * v.k : 0
    const noCode = !(n > 0 && st?.item_code)
    tire = (
      <g data-testid={`map-station-tire-${s.id}`} data-source={t.source}>
        <circle
          cx={cx}
          cy={cy}
          r={r}
          className="fill-content-primary stroke-content-primary"
          fillOpacity={0.14}
          strokeWidth={1.5}
          strokeDasharray={t.source === 'nominal' ? '4 3' : undefined}
        />
        {ri > 3 && ri < r ? (
          <circle
            cx={cx}
            cy={cy}
            r={ri}
            fill="none"
            className="stroke-content-muted"
            strokeDasharray="2 2"
          />
        ) : null}
        {r >= 10 ? (
          <text
            x={cx}
            y={cy}
            textAnchor="middle"
            dominantBaseline="central"
            fontSize={Math.min(Math.max(r * 0.32, 9), 14)}
            fontWeight={600}
            className={cn('tabular-nums', noCode ? 'fill-fault-fg' : 'fill-content-primary')}
            data-testid={`map-station-item-${s.id}`}
          >
            {noCode ? '코드?' : `${st?.item_code}${n > 1 ? ` ×${n}` : ''}`}
          </text>
        ) : null}
      </g>
    )
  } else if (n > 0 && Math.min(hw, hh) >= 14) {
    // 화물은 떠났고 코드만 남음 — 다음 스테이션 도착을 기다린다(`stock::conveyor`).
    tire = (
      <text
        x={sx}
        y={sy}
        textAnchor="middle"
        dominantBaseline="central"
        fontSize={10}
        className={cn('tabular-nums', STATION_TEXT_CLS[fill])}
        data-testid={`map-station-item-${s.id}`}
      >
        → {st?.item_code || '?'}
      </text>
    )
  }

  return (
    <g className="pointer-events-none">
      {tire}
      {wall}
      {Math.min(hw, hh) >= 12 ? (
        <text
          x={sx - hw + 5}
          y={sy - hh + 5}
          dominantBaseline="hanging"
          fontSize={10}
          fontWeight={600}
          className={STATION_TEXT_CLS[fill]}
        >
          {s.id}
        </text>
      ) : null}
    </g>
  )
}

/** 호버 카드의 GRM 줄 — 정합 · 인터록 판정 + State · 트래킹. 비트는 `InterlockLamps`. */
function stationLiveRows(l: StationLive | undefined, st: StockEntry | null): [string, ReactNode][] {
  if (!l || !l.live) return [['GRM', l?.why ?? 'GRM 상태 없음']]
  const m = stationMatch(l, st)
  const ring = stationRing(l)
  const rows: [string, ReactNode][] = [
    [
      '정합',
      <span
        key="m"
        className={cn(
          'whitespace-normal',
          m.state === 'ok' ? 'text-ok-fg' : m.state === 'mismatch' ? 'text-fault-fg' : '',
        )}
      >
        {m.state === 'ok' ? '정합 — ' : m.state === 'mismatch' ? '불일치 — ' : ''}
        {m.reasons.join(' · ') || '-'}
      </span>,
    ],
    ['인터록', ring ? RING_LABEL[ring] : '-'],
    ['State', `${stateLabel(l.state)}${l.error_code ? ` · Err ${l.error_code}` : ''}`],
  ]
  if (l.has_tracking || l.od > 0)
    rows.push(['OD / TX / TY', `${f1(l.od)} / ${f1(l.tx)} / ${f1(l.ty)}`])
  rows.push(['GRM', `${l.source}${l.stale ? ' · 오래됨 / 끊김' : ''}`])
  return rows
}

/** IN(PI, CV → GRM) / OUT(PO, GRM → CV) 인디케이터 — 켜진 비트는 테두리와 같은 색, 한 줄 설명. */
function InterlockLamps({ l }: { l: StationLive }) {
  const { pi, po } = interlockLamps(l)
  const col = (title: string, lamps: Lamp[]) => (
    <div className="flex min-w-0 flex-col gap-1">
      <span className="text-3xs font-semibold tracking-wide text-content-muted">{title}</span>
      {lamps.map((x) => (
        <div
          key={x.key}
          className="flex items-start gap-1.5"
          data-testid={`lamp-${x.key}`}
          data-on={x.on ? '1' : '0'}
        >
          <span
            className={cn(
              'mt-1 h-2 w-2 flex-none rounded-full',
              x.on ? LAMP_ON[x.key] : 'bg-line-strong',
            )}
          />
          <span className="min-w-0 leading-tight">
            <span
              className={cn('font-semibold', x.on ? 'text-content-primary' : 'text-content-faint')}
            >
              {x.name}
            </span>
            <span className="block text-3xs text-content-muted">{x.desc}</span>
          </span>
        </div>
      ))}
    </div>
  )
  return (
    <section
      className="grid grid-cols-2 gap-2 border-t border-line-default px-2.5 py-2"
      data-testid="hover-interlock"
    >
      {col('IN · CV → GRM (PI)', pi)}
      {col('OUT · GRM → CV (PO)', po)}
    </section>
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
