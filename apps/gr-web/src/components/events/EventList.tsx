// 이벤트 목록 — 수천 행을 고정 행 높이로 창만 그린다(킷의 DataTable/DataGrid 는 가상화가 없다).
//
// 라이브로 위에 행이 붙을 때, 사람이 아래로 내려가 읽고 있으면 **보던 행이 제자리에 남는다**
// (보던 행의 id 로 스크롤을 다시 맞춘다). 맨 위에 있으면 새 행이 그대로 보인다.
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type UIEvent,
} from 'react'
import { ArrowUp } from 'lucide-react'
import type { EventRow } from '../../lib/evtlog/api'
import { fmtEvtTime } from '../../lib/evtlog/evtRowsModel'
import { rowText, type EvtLang } from '../../lib/evtlog/evtTypeModel'
import { Button } from '../../lib/ui/Button'
import { columnLevel } from '../../lib/ui/table'
import { cn } from '../../lib/utils'
import { EvtLevel, EvtPlcChip, EvtType } from './EvtBits'

const ROW_H = 24
const OVERSCAN = 12
/** 바닥에서 이만큼(행) 남으면 다음 쪽을 부른다. */
const MORE_AHEAD = 20
/** 머리글과 몸이 같은 스크롤바 자리를 비워 둬야 열이 맞는다. */
const GUTTER: CSSProperties = { scrollbarGutter: 'stable' }

interface Col {
  key: string
  label: string
  width: string
  level: 1 | 2 | 3
  cls?: string
}

const COLS: Col[] = [
  { key: 'ts', label: 'Time', width: '8.5rem', level: 1 },
  { key: 'plc', label: 'PLC', width: '4.5rem', level: 2 },
  { key: 'lvl', label: 'Level', width: '4.5rem', level: 1 },
  { key: 'type', label: 'Type', width: '7.5rem', level: 1 },
  { key: 'cat', label: 'Cat', width: '5rem', level: 3 },
  { key: 'text', label: 'Text', width: 'minmax(0,1fr)', level: 1 },
  { key: 'ctx', label: 'WorkId', width: '4.5rem', level: 2, cls: 'text-right' },
]

export interface EventListProps {
  rows: readonly EventRow[]
  now: number
  selectedId: number | null
  live: boolean
  hasMore: boolean
  loadingMore: boolean
  trimmed: boolean
  lang: EvtLang
  onPick: (r: EventRow) => void
  onCtx: (ctx: number) => void
  onMore: () => void
  /** 바뀌면 맨 위로 — 필터를 바꿔 목록이 통째로 갈렸을 때. */
  resetKey?: unknown
}

export function EventList({
  rows,
  now,
  selectedId,
  live,
  hasMore,
  loadingMore,
  trimmed,
  lang,
  onPick,
  onCtx,
  onMore,
  resetKey,
}: EventListProps) {
  const [box, setBox] = useState<HTMLDivElement | null>(null)
  const [size, setSize] = useState<{ w: number | null; h: number }>({ w: null, h: 600 })
  const [top, setTop] = useState(0)
  /** 보던 행 — 위에 행이 붙어도 이 행이 같은 자리에 선다. */
  const anchor = useRef<{ id: number; off: number } | null>(null)
  /** 맨 위를 떠날 때의 첫 행 — 그 위로 새로 붙은 수를 센다. */
  const [leftTopId, setLeftTopId] = useState<number | null>(null)

  useEffect(() => {
    if (!box || typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver((es) => {
      const r = es[0]?.contentRect
      if (r) setSize({ w: Math.round(r.width), h: Math.round(r.height) })
    })
    ro.observe(box)
    return () => ro.disconnect()
  }, [box])

  useLayoutEffect(() => {
    if (!box) return
    box.scrollTop = 0
    anchor.current = null
    setTop(0)
    setLeftTopId(null)
  }, [resetKey, box])

  useLayoutEffect(() => {
    if (!box) return
    const a = anchor.current
    if (!a || box.scrollTop < ROW_H / 2) return
    const i = rows.findIndex((r) => r.id === a.id)
    if (i < 0) return
    const want = i * ROW_H + a.off
    if (Math.abs(box.scrollTop - want) >= 1) {
      box.scrollTop = want
      setTop(want)
    }
  }, [rows, box])

  const level = columnLevel(size.w)
  const cols = COLS.filter((c) => c.level <= level)
  const grid: CSSProperties = { gridTemplateColumns: cols.map((c) => c.width).join(' ') }

  const first = Math.max(0, Math.floor(top / ROW_H) - OVERSCAN)
  const last = Math.min(rows.length, Math.ceil((top + size.h) / ROW_H) + OVERSCAN)
  const slice = rows.slice(first, last)

  useEffect(() => {
    if (!hasMore || loadingMore) return
    if (last >= rows.length - MORE_AHEAD) onMore()
  }, [last, rows.length, hasMore, loadingMore, onMore])

  function onScroll(e: UIEvent<HTMLDivElement>) {
    const st = e.currentTarget.scrollTop
    setTop(st)
    const i = Math.floor(st / ROW_H)
    const r = rows[i]
    anchor.current = r ? { id: r.id, off: st - i * ROW_H } : null
    if (st < ROW_H / 2) setLeftTopId(null)
    else if (leftTopId === null && rows[0]) setLeftTopId(rows[0].id)
  }

  const fresh =
    live && leftTopId !== null
      ? Math.max(
          0,
          rows.findIndex((r) => r.id === leftTopId),
        )
      : 0

  return (
    <div className="relative flex min-h-0 flex-1 flex-col" data-testid="evt-list">
      <div
        className="grid gap-2 border-b border-line-default bg-surface-inset px-3 py-1 text-2xs font-medium text-content-muted"
        style={{ ...grid, ...GUTTER, overflow: 'hidden' }}
        role="row"
      >
        {cols.map((c) => (
          <span key={c.key} role="columnheader" className={c.cls}>
            {c.label}
          </span>
        ))}
      </div>
      <div
        ref={setBox}
        className="min-h-0 flex-1 overflow-y-auto"
        style={GUTTER}
        onScroll={onScroll}
        role="rowgroup"
      >
        <div className="relative" style={{ height: rows.length * ROW_H }}>
          {slice.map((r, k) => {
            const i = first + k
            return (
              <div
                key={r.id}
                role="row"
                tabIndex={0}
                aria-selected={r.id === selectedId}
                data-testid="evt-row"
                className={cn(
                  'absolute inset-x-0 grid cursor-pointer items-center gap-2 border-b border-line-subtle px-3 text-xs',
                  r.id === selectedId ? 'bg-accent-soft' : 'hover:bg-surface-hover',
                )}
                style={{ ...grid, top: i * ROW_H, height: ROW_H }}
                onClick={() => onPick(r)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') onPick(r)
                }}
                title={r.detail ? `${rowText(r, lang)}\n${r.detail}` : rowText(r, lang)}
              >
                {cols.map((c) => (
                  <Cell key={c.key} col={c.key} r={r} now={now} lang={lang} onCtx={onCtx} />
                ))}
              </div>
            )
          })}
        </div>
        <div className="flex items-center justify-center gap-2 py-2 text-2xs text-content-faint">
          {trimmed ? (
            <span>상한 {rows.length} 행 — 더 오래된 것은 필터를 좁혀 보세요</span>
          ) : hasMore ? (
            <Button
              size="sm"
              intent="ghost"
              loading={loadingMore}
              onClick={onMore}
              data-testid="evt-more"
            >
              더 보기
            </Button>
          ) : rows.length ? (
            <span>끝</span>
          ) : null}
        </div>
      </div>
      {fresh > 0 ? (
        <Button
          size="sm"
          intent="outline"
          icon={<ArrowUp size={12} />}
          className="absolute top-8 left-1/2 z-20 -translate-x-1/2 bg-surface-panel shadow-sm"
          onClick={() => {
            if (box) box.scrollTop = 0
            setLeftTopId(null)
          }}
          data-testid="evt-fresh"
        >
          새 이벤트 {fresh}
        </Button>
      ) : null}
    </div>
  )
}

function Cell({
  col,
  r,
  now,
  lang,
  onCtx,
}: {
  col: string
  r: EventRow
  now: number
  lang: EvtLang
  onCtx: (ctx: number) => void
}) {
  switch (col) {
    case 'ts':
      return (
        <span className="truncate font-mono text-2xs text-content-muted" title={`rx ${r.rx_ts}`}>
          {fmtEvtTime(r.ts, now)}
        </span>
      )
    case 'plc':
      return <EvtPlcChip plc={r.plc} />
    case 'lvl':
      return (
        <span className="truncate">
          <EvtLevel row={r} />
        </span>
      )
    case 'type':
      return (
        <span className="truncate">
          <EvtType row={r} />
        </span>
      )
    case 'cat':
      return <span className="truncate font-mono text-2xs text-content-muted">{r.cat_name}</span>
    case 'text':
      return (
        <span className="truncate text-content-primary">
          {r.name ? (
            <span className="mr-1.5 font-mono text-2xs text-content-faint">{r.name}</span>
          ) : null}
          {rowText(r, lang)}
        </span>
      )
    case 'ctx':
      return r.ctx ? (
        <button
          type="button"
          className="justify-self-end rounded px-1 font-mono text-2xs text-content-secondary tabular-nums hover:bg-surface-inset hover:text-content-primary"
          title={`WorkId ${r.ctx} — 스텝 막대`}
          onClick={(e) => {
            e.stopPropagation()
            onCtx(r.ctx)
          }}
          data-testid="evt-ctx"
        >
          {r.ctx}
        </button>
      ) : (
        <span className="text-right text-content-disabled">-</span>
      )
    default:
      return null
  }
}
