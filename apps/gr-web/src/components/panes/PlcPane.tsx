// PLC 패널 — 접속·레이아웃 검사 상태 목록. 행을 누르면 원본 JSON 팝업(`panels`)이 뜬다.
// `Sidebar.tsx`의 PLC 섹션을 그대로 떼어 냈다(클래스도 그대로 — 이동이지 재도색이 아니다).
import { useEffect, useState } from 'react'
import { Link as LinkIcon, RefreshCw } from 'lucide-react'
import { density } from '../../lib/density'
import { panels } from '../../lib/panels'
import { plcs } from '../../lib/plcs'
import { useStore } from '../../lib/store'
import { Button } from '../../lib/ui/Button'
import { JsonView } from '../../lib/ui/JsonView'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import { StatusDot } from '../../lib/ui/StatusDot'
import type { PlcId, PlcStatus } from '../../lib/types'
import type { Status } from '../../lib/ui/status'

/** PLC 연결 → 점 상태. 한 번도 붙은 적 없고 오류도 없으면 중립(아직 모른다). */
export function plcDot(p: PlcStatus): Status {
  if (p.connected) return 'ok'
  return p.last_error || p.last_ok_at ? 'fault' : 'neutral'
}

/** 레이아웃 검사 → 배지. */
export function layoutBadge(p: PlcStatus): { status: Status; label: string } {
  if (p.layout.ok === true) return { status: 'ok', label: '레이아웃 OK' }
  if (p.layout.ok === false) return { status: 'fault', label: '불일치' }
  return { status: 'neutral', label: '미검사' }
}

/** PLC 상세 팝업 — 원본 JSON과 재검사·재연결. `panels.open`으로 띄운다. */
export function PlcDetail({ id }: { id: PlcId }) {
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

export default function PlcPane() {
  useStore(plcs, density)
  useEffect(() => plcs.start(), [])

  const rowPad = density.isCompact ? 'py-0.5' : 'py-1.5'

  return (
    <ul className="min-h-0 flex-1 overflow-y-auto" data-testid="plc-list">
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
  )
}

/** 머리띠 요약 — `전체 · 연결 n`. */
export function PlcSummary() {
  useStore(plcs)
  const connected = plcs.list.filter((p) => p.connected).length
  return (
    <span className="tabular-nums">
      {plcs.list.length}
      {plcs.list.length > 0 ? ` · 연결 ${connected}` : ''}
    </span>
  )
}
