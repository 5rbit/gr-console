// 스테이션 보정 표 — 운용 모드(모니터링·명령 생성) 레일의 스테이션 표.
//
// 배치 파라미터(위치·크기·연결)는 레이아웃 편집 모드의 스테이션 표가 맡는다. 여기는 **지금 스테이션에
// 작업을 보내면 얼마를 더하나**다: GRM 트래킹(측정 OD · TaskOffset) → RotateType 식 → 더한 값 → 최종 XY.
// 계산은 백엔드(`GET /api/stations/offsets`, 작성 미리보기와 같은 `evaluate`)가 하고 1초마다 다시 받는다.
// 행을 펼치면 경고 전부가 보인다.
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Crosshair } from 'lucide-react'
import { visibleInterval } from '../../lib/poll'
import { taskApi } from '../../lib/task/api'
import type { MenuEntry } from '../../lib/task/menuEntries'
import { menuItems } from '../../lib/task/menuEntries'
import {
  fmtAge,
  mm,
  offsetRowState,
  rotateLabel,
  signedMm,
  xy,
} from '../../lib/task/stationOffsetModel'
import type { StationOffsetRow } from '../../lib/task/types'
import { DataTable } from '../../lib/ui/DataTable'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import type { Column } from '../../lib/ui/table'
import { Toolbar } from '../../lib/ui/Toolbar'

const POLL_MS = 1000

export interface StationOffsetTableProps {
  q: string
  selectedId?: number | null
  onSelect?: (id: number | null) => void
  compact?: boolean
  /** 레일이 넘기는 머리줄 조작(표 종류 토글 + 검색)과 ⋯ 보기 항목. */
  lead?: ReactNode
  menuExtra?: MenuEntry[]
}

const faint = <span className="text-content-faint">-</span>

export function StationOffsetTable({
  q,
  selectedId,
  onSelect,
  compact = false,
  lead,
  menuExtra = [],
}: StationOffsetTableProps) {
  const [rows, setRows] = useState<StationOffsetRow[]>([])
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(true)
  const load = useRef(async () => {})
  load.current = async () => {
    try {
      setRows(await taskApi.stationOffsets())
      setError('')
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }
  useEffect(() => {
    void load.current()
    const id = visibleInterval(() => void load.current(), POLL_MS)
    return () => clearInterval(id)
  }, [])

  const [selLocal, setSelLocal] = useState<number | null>(null)
  const selected = selectedId !== undefined ? selectedId : selLocal
  const pick = (id: number) => {
    const next = id === selected ? null : id
    if (selectedId === undefined) setSelLocal(next)
    onSelect?.(next)
  }
  // 맵에서 고른 스테이션이 표 밖(스크롤 아래)에 있으면 선택이 안 보인다 — 선택 행을 끌어온다.
  const box = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (selected === null) return
    box.current
      ?.querySelector('[data-testid="dt-row"].bg-accent-soft')
      ?.scrollIntoView({ block: 'nearest' })
  }, [selected])

  const shown = useMemo(() => {
    const needle = q.trim().toLowerCase()
    if (!needle) return rows
    return rows.filter((r) =>
      `${r.station_id} cv${r.conv_no} g${r.group} s${r.slot}`.toLowerCase().includes(needle),
    )
  }, [rows, q])
  const measured = rows.filter((r) => r.od_source === 'tracking').length
  const problems = rows.filter((r) => {
    const s = offsetRowState(r).status
    return s === 'fault' || s === 'warn'
  }).length
  const age = rows.find((r) => r.age_ms !== null)?.age_ms ?? null

  // `priority` — 이 표를 보는 이유는 **어느 스테이션에 얼마를 더하나**다: Id·상태·더한 값이 1,
  // 측정 OD·최종 XY·RotateType 이 2, 원시 TaskOffset·Info·출처가 3이다.
  const columns: Column<StationOffsetRow>[] = [
    {
      key: 'id',
      label: 'Id',
      get: (r) => r.station_id,
      numeric: true,
      class: 'font-mono',
      priority: 1,
    },
    {
      key: 'state',
      label: 'State',
      get: (r) => offsetRowState(r).text,
      cell: (r) => {
        const s = offsetRowState(r)
        return s.status === 'ok' || s.status === 'neutral' ? (
          <span className={s.status === 'ok' ? 'text-2xs' : 'text-2xs text-content-faint'}>
            {s.text}
          </span>
        ) : (
          <StatusBadge status={s.status}>{s.text}</StatusBadge>
        )
      },
      priority: 1,
    },
    { key: 'conv', label: 'ConvNo', get: (r) => r.conv_no, numeric: true, priority: 3 },
    {
      key: 'rot',
      label: 'RotateType',
      get: (r) => r.rotate_type,
      cell: (r) => (
        <span title={rotateLabel(r.rotate_type)}>
          {r.rotate_type}
          {r.rotate_type !== r.registry_rotate_type ? (
            <span className="text-warn-fg"> ≠ {r.registry_rotate_type}</span>
          ) : null}
        </span>
      ),
      numeric: true,
      priority: 2,
    },
    {
      key: 'od',
      label: 'OD',
      get: (r) => r.od,
      numeric: true,
      cell: (r) => (r.od ? mm(r.od) : faint),
      priority: 2,
    },
    {
      key: 'now',
      label: 'TaskOffset X/Y',
      get: (r) => Math.hypot(r.now_tx, r.now_ty),
      numeric: true,
      cell: (r) => (r.now_tx || r.now_ty ? `${mm(r.now_tx)} / ${mm(r.now_ty)}` : faint),
      priority: 3,
    },
    {
      key: 'applied',
      label: 'Offset TX/TY',
      get: (r) => Math.hypot(r.tx_applied, r.ty_applied),
      numeric: true,
      cell: (r) =>
        r.tx_applied || r.ty_applied ? (
          <b className="tabular-nums">
            {signedMm(r.tx_applied)} / {signedMm(r.ty_applied)}
          </b>
        ) : (
          faint
        ),
      priority: 1,
    },
    {
      key: 'base',
      label: 'Info X/Y',
      get: (r) => r.base_xy[0],
      numeric: true,
      cell: (r) => xy(r.base_xy),
      priority: 3,
    },
    {
      key: 'final',
      label: 'Final X/Y',
      get: (r) => r.final_xy[0],
      numeric: true,
      cell: (r) => xy(r.final_xy),
      priority: 2,
    },
    {
      key: 'src',
      label: 'Source',
      get: (r) => r.source ?? '',
      cell: (r) =>
        r.source ? (
          <span className="text-2xs text-content-muted">
            {r.source} · {fmtAge(r.age_ms)}
          </span>
        ) : (
          faint
        ),
      priority: 3,
    },
  ]
  const COMPACT_KEYS = ['id', 'state', 'od', 'applied', 'final']

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="station-offset-table">
      <Toolbar
        icon={<Crosshair size={14} />}
        title="스테이션 보정"
        lead={lead}
        dense
        meta={
          <span className="tabular-nums">
            {rows.length}개 · 측정 {measured}
            {problems ? ` · 확인 ${problems}` : ''}
            {age !== null ? ` · ${fmtAge(age)} 전` : ''}
          </span>
        }
      >
        <OverflowMenu
          items={menuItems([
            { label: '목록' },
            { label: '지금 다시 읽기', run: () => void load.current() },
            ...menuExtra,
          ])}
          title="스테이션 보정 — 다시 읽기 · 보기"
          testid="offset-more"
        />
      </Toolbar>
      <div className="min-h-0 flex-1 overflow-auto" ref={box}>
        {error ? <div className="p-2 text-xs text-fault-fg">{error}</div> : null}
        <DataTable
          rows={shown}
          columns={compact ? columns.filter((c) => COMPACT_KEYS.includes(c.key)) : columns}
          rowKey={(r) => String(r.station_id)}
          selected={selected === null ? null : String(selected)}
          onPick={(r) => pick(r.station_id)}
          loading={loading}
          rowDetail={(r) =>
            r.warnings.length || r.blocked ? (
              <ul className="m-0 flex list-disc flex-col gap-0.5 pl-4 text-2xs text-content-muted">
                {r.blocked ? <li className="text-fault-fg">{r.blocked}</li> : null}
                {r.warnings.map((w) => (
                  <li key={w}>{w}</li>
                ))}
              </ul>
            ) : null
          }
          empty={q ? '검색 결과 없음' : '스테이션 없음'}
          emptyHint={
            q
              ? '검색어를 지우거나 Id·ConvNo·Group 의 다른 조각으로 찾으세요.'
              : '레이아웃 편집 모드의 스테이션 표에서 스테이션을 먼저 등록하세요.'
          }
          testid="offset-table"
        />
      </div>
    </div>
  )
}
