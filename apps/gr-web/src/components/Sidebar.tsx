// 왼쪽 사이드바 — `PLC`(연결·레이아웃)와 `상태`(모드·작업·SSE) 두 섹션.
//
// sh4w-web의 사이드바에서 폭 리사이즈·밀도 토글·접기 크롬을 가져오고 로봇/Instance 목록은 걷어 냈다 —
// GR 콘솔은 선택 축이 없고, 사이드바는 "지금 PLC가 어떤가"를 한눈에 보이는 자리다.
import { useEffect, useRef, useState } from 'react'
import type * as React from 'react'
import { ChevronRight, Link as LinkIcon, RefreshCw, Rows2, Rows3 } from 'lucide-react'
import { plcs } from '../lib/plcs'
import { statusFeed } from '../lib/feeds'
import { useSse } from '../lib/sse'
import { useStore } from '../lib/store'
import { panels } from '../lib/panels'
import { modeName } from '../lib/gr/const'
import { StatusDot } from '../lib/ui/StatusDot'
import { StatusBadge } from '../lib/ui/StatusBadge'
import { JsonView } from '../lib/ui/JsonView'
import { Button } from '../lib/ui/Button'
import type { PlcId, PlcStatus } from '../lib/types'
import type { Status } from '../lib/ui/status'

const LS_WIDTH = 'gr-sidebar-w'
const LS_DENSE = 'gr-sidebar-dense'
const MIN_W = 200
const MAX_W = 480

function readNum(key: string, fallback: number): number {
  const v = Number(localStorage.getItem(key))
  return Number.isFinite(v) && v >= MIN_W && v <= MAX_W ? v : fallback
}

const headCls =
  'flex shrink-0 items-center gap-1.5 border-b border-slate-200 px-2 py-1.5 dark:border-slate-700'
const countCls =
  'rounded-full bg-slate-200 px-1.5 text-[11px] tabular-nums text-slate-600 dark:bg-slate-700 dark:text-slate-300'
const iconCls =
  'rounded p-1 text-slate-400 transition-colors hover:bg-slate-200 hover:text-slate-800 dark:hover:bg-slate-700 dark:hover:text-slate-100'

/** PLC 연결 → 점 상태. 한 번도 붙은 적 없고 오류도 없으면 중립(아직 모른다). */
function plcDot(p: PlcStatus): Status {
  if (p.connected) return 'ok'
  return p.last_error || p.last_ok_at ? 'fault' : 'neutral'
}

/** 레이아웃 검사 → 배지. */
function layoutBadge(p: PlcStatus): { status: Status; label: string } {
  if (p.layout.ok === true) return { status: 'ok', label: '레이아웃 OK' }
  if (p.layout.ok === false) return { status: 'fault', label: '불일치' }
  return { status: 'neutral', label: '미검사' }
}

/** PLC 상세 팝업 — 원본 JSON과 재검사·재연결. `panels.open`으로 띄운다. */
function PlcDetail({ id }: { id: PlcId }) {
  useStore(plcs)
  const p = plcs.byId(id)
  const [busy, setBusy] = useState<'check' | 'reconnect' | null>(null)
  if (!p) return <p className="text-sm text-slate-500">PLC `{id}`가 목록에 없습니다.</p>
  const lb = layoutBadge(p)
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <StatusDot status={plcDot(p)} label={p.connected ? '연결됨' : '미연결'} />
        <StatusBadge status={lb.status}>{lb.label}</StatusBadge>
        <span className="font-mono text-xs text-slate-400">{p.endpoint}</span>
        <span className="flex-1" />
        <Button
          size="sm"
          icon={<RefreshCw className="h-3.5 w-3.5" />}
          loading={busy === 'check'}
          disabled={busy !== null}
          data-testid="plc-check"
          onClick={() => {
            setBusy('check')
            void plcs.check(id).finally(() => setBusy(null))
          }}
        >
          재검사
        </Button>
        <Button
          size="sm"
          intent="outline"
          icon={<LinkIcon className="h-3.5 w-3.5" />}
          loading={busy === 'reconnect'}
          disabled={busy !== null}
          data-testid="plc-reconnect"
          onClick={() => {
            setBusy('reconnect')
            void plcs.reconnect(id).finally(() => setBusy(null))
          }}
        >
          재연결
        </Button>
      </div>
      {p.last_error ? <p className="text-xs text-red-500">{p.last_error}</p> : null}
      <JsonView value={p} rootLabel={p.id} defaultDepth={2} height={360} />
    </div>
  )
}

export function Sidebar() {
  useStore(plcs)
  useSse(statusFeed)
  useEffect(() => plcs.start(), [])

  const [width, setWidth] = useState(() => readNum(LS_WIDTH, 240))
  const [dense, setDense] = useState(() => localStorage.getItem(LS_DENSE) === '1')
  const [plcOpen, setPlcOpen] = useState(true)
  const [statOpen, setStatOpen] = useState(true)

  function toggleDense(): void {
    const next = !dense
    setDense(next)
    localStorage.setItem(LS_DENSE, next ? '1' : '0')
  }

  // ── 폭 드래그 ──
  const dragging = useRef(false)
  function startDrag(e: React.PointerEvent<HTMLDivElement>): void {
    dragging.current = true
    e.currentTarget.setPointerCapture(e.pointerId)
  }
  function onDrag(e: React.PointerEvent<HTMLDivElement>): void {
    if (!dragging.current) return
    setWidth(Math.min(MAX_W, Math.max(MIN_W, e.clientX)))
  }
  function endDrag(e: React.PointerEvent<HTMLDivElement>): void {
    if (!dragging.current) return
    dragging.current = false
    e.currentTarget.releasePointerCapture(e.pointerId)
    localStorage.setItem(LS_WIDTH, String(Math.round(width)))
  }

  const wm = statusFeed.data?.webmon ?? null
  const sseStatus: Status = statusFeed.connected ? 'ok' : statusFeed.error ? 'fault' : 'neutral'
  const rowPad = dense ? 'py-0.5' : 'py-1.5'
  const connected = plcs.list.filter((p) => p.connected).length

  return (
    <div
      className="relative flex h-full min-h-0 flex-col"
      style={{ width: `${width}px` }}
      data-testid="sidebar"
    >
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
        {/* ── PLC ── */}
        <div className={headCls}>
          <button
            className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
            onClick={() => setPlcOpen(!plcOpen)}
            aria-expanded={plcOpen}
            data-testid="sec-plc"
          >
            <ChevronRight
              className={`h-3 w-3 shrink-0 text-slate-400 transition-transform ${plcOpen ? 'rotate-90' : ''}`}
            />
            <span className="text-xs font-semibold text-slate-500">PLC</span>
            <span className={countCls} data-testid="plc-count">
              {plcs.list.length}
            </span>
            {plcs.list.length > 0 ? (
              <span className="text-[10px] text-slate-400">연결 {connected}</span>
            ) : null}
          </button>
          <button
            className={iconCls}
            title="밀도 전환"
            aria-label="목록 밀도 전환"
            data-testid="sidebar-density"
            onClick={toggleDense}
          >
            {dense ? <Rows3 className="h-4 w-4" /> : <Rows2 className="h-4 w-4" />}
          </button>
        </div>
        {plcOpen ? (
          <ul className="shrink-0" data-testid="plc-list">
            {plcs.list.length === 0 ? (
              <li className="px-2 py-2 text-[11px] text-slate-400">
                {plcs.error ? `PLC 목록 조회 실패 — ${plcs.error}` : 'PLC 목록 없음'}
              </li>
            ) : null}
            {plcs.list.map((p) => {
              const lb = layoutBadge(p)
              return (
                <li key={p.id}>
                  <button
                    type="button"
                    className={`flex w-full items-center gap-2 px-2 text-left hover:bg-slate-100 dark:hover:bg-slate-800 ${rowPad}`}
                    data-testid={`plc-${p.id}`}
                    title={p.last_error ?? p.endpoint}
                    onClick={() =>
                      panels.open({
                        id: `plc-${p.id}`,
                        mode: 'popup',
                        title: `${p.label} (${p.id})`,
                        component: PlcDetail,
                        props: { id: p.id },
                      })
                    }
                  >
                    <StatusDot status={plcDot(p)} size="sm" />
                    <span className="min-w-0 flex-1 truncate text-xs font-medium">{p.label}</span>
                    <span className="font-mono text-[10px] tabular-nums text-slate-400">
                      {p.rtt_ms !== null ? `${Math.round(p.rtt_ms)}ms` : '—'}
                    </span>
                    <StatusBadge status={lb.status}>{lb.label}</StatusBadge>
                  </button>
                </li>
              )
            })}
          </ul>
        ) : null}

        {/* ── 상태 ── */}
        <div className={headCls}>
          <button
            className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
            onClick={() => setStatOpen(!statOpen)}
            aria-expanded={statOpen}
            data-testid="sec-status"
          >
            <ChevronRight
              className={`h-3 w-3 shrink-0 text-slate-400 transition-transform ${statOpen ? 'rotate-90' : ''}`}
            />
            <span className="text-xs font-semibold text-slate-500">상태</span>
          </button>
          <StatusDot status={sseStatus} size="sm" label="SSE" title={statusFeed.error ?? '상태 스트림'} />
        </div>
        {statOpen ? (
          <dl
            className={`grid shrink-0 grid-cols-[auto_1fr] gap-x-3 px-2 text-xs ${dense ? 'gap-y-0.5 py-1' : 'gap-y-1 py-2'}`}
            data-testid="status-rows"
          >
            <dt className="text-slate-400">모드</dt>
            <dd className="font-mono" data-testid="st-mode">
              {wm ? modeName(wm.Mode) : '—'}
            </dd>
            <dt className="text-slate-400">작업</dt>
            <dd className="font-mono tabular-nums" data-testid="st-task">
              {wm ? `W${wm.Stat.Task.Now.WorkId} / T${wm.Stat.Task.Now.TaskId}` : '—'}
            </dd>
            <dt className="text-slate-400">스텝</dt>
            <dd className="font-mono tabular-nums" data-testid="st-step">
              {wm ? `${wm.Proc.Step.Now}` : '—'}
              {wm?.Proc.Msg ? <span className="ml-1 text-slate-400">{wm.Proc.Msg}</span> : null}
            </dd>
            <dt className="text-slate-400">알람</dt>
            <dd className="flex items-center gap-2" data-testid="st-alarm">
              {wm ? (
                <>
                  <StatusDot status={wm.Alarm.Fault ? 'fault' : 'neutral'} size="sm" label="Fault" />
                  <StatusDot status={wm.Alarm.Warn ? 'warn' : 'neutral'} size="sm" label="Warn" />
                </>
              ) : (
                '—'
              )}
            </dd>
            <dt className="text-slate-400">대기열</dt>
            <dd className="font-mono tabular-nums" data-testid="st-queue">
              {wm ? wm.Stat.Task.Queue.length : '—'}
            </dd>
          </dl>
        ) : null}
      </div>

      {/* 폭 손잡이 */}
      <div
        className="absolute inset-y-0 -right-1 z-10 w-2 cursor-col-resize"
        role="separator"
        aria-orientation="vertical"
        aria-label="사이드바 폭 조절"
        data-testid="sidebar-resize"
        onPointerDown={startDrag}
        onPointerMove={onDrag}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
      ></div>
    </div>
  )
}
