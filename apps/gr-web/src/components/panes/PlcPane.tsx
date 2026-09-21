// PLC 패널 — 접속·레이아웃 검사 상태 목록. 행을 누르면 원본 JSON 팝업(`panels`)이 뜬다.
// `Sidebar.tsx`의 PLC 섹션을 떼어 냈다.
//
// 행 = 이름 · (불일치일 때만) `레이아웃 불일치` 칩 · 연결 칩(`연결 4ms`/`끊김`/`연결 중`, OPC UA 는 `준비`).
// 예전의 글자 없는 초록·빨강 사각 둘을 글자 칩으로 바꿨다(2026-09-21, 어휘는 `lib/indicators`).
import { useEffect, useState } from 'react'
import { Link as LinkIcon, RefreshCw } from 'lucide-react'
import { density } from '../../lib/density'
import { panels } from '../../lib/panels'
import { plcs } from '../../lib/plcs'
import { useStore } from '../../lib/store'
import { Button } from '../../lib/ui/Button'
import { JsonView } from '../../lib/ui/JsonView'
import { IndicatorChip } from '../../lib/ui/IndicatorChip'
import { plcConnection, plcLayout, toneStatus } from '../../lib/indicators'
import type { PlcId, PlcStatus } from '../../lib/types'
import type { Status } from '../../lib/ui/status'

/** PLC 연결 → 점 상태(`lib/indicators.plcConnection`의 톤). */
export function plcDot(p: PlcStatus): Status {
  return toneStatus(plcConnection(p).tone)
}

/** 레이아웃 검사 → 점 상태 + 글자. 불일치는 빨강이 아니라 degraded(노랑) — 끊김과 가른다. */
export function layoutMark(p: PlcStatus): { status: Status; label: string } {
  const l = plcLayout(p)
  return { status: toneStatus(l.tone), label: l.label || '검사 대상 아님' }
}

/** PLC 상세 팝업 — 원본 JSON과 재검사·재연결. `panels.open`으로 띄운다. */
export function PlcDetail({ id }: { id: PlcId }) {
  useStore(plcs)
  const p = plcs.byId(id)
  const [busy, setBusy] = useState<'check' | 'reconnect' | null>(null)
  if (!p) return <p className="text-sm text-content-muted">PLC `{id}`가 목록에 없습니다.</p>
  const lb = layoutMark(p)
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-3">
        <IndicatorChip ind={plcConnection(p)} />
        {p.kind === 's7' ? (
          <IndicatorChip ind={{ ...plcLayout(p), label: lb.label }} />
        ) : null}
        <span className="font-mono text-xs text-content-faint">{p.endpoint}</span>
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
      {p.last_error ? <p className="text-xs text-fault-fg">{p.last_error}</p> : null}
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
        <li className="px-2 py-2 text-2xs text-content-faint">
          {plcs.error ? `PLC 목록 조회 실패 — ${plcs.error}` : 'PLC 목록 없음'}
        </li>
      ) : null}
      {plcs.list.map((p) => {
        const conn = plcConnection(p)
        const lay = plcLayout(p)
        return (
          <li key={p.id}>
            <button
              type="button"
              className={`flex w-full items-center gap-1.5 px-2 text-left hover:bg-surface-inset ${rowPad}`}
              data-testid={`plc-${p.id}`}
              title={`${p.label} · ${p.endpoint} — 누르면 상세`}
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
              <span className="min-w-0 flex-1 truncate text-xs font-medium">{p.label}</span>
              {/* 정상 레이아웃은 침묵한다 — 불일치일 때만 사유를 툴팁에 든 칩이 선다. */}
              {lay.hidden ? null : <IndicatorChip ind={lay} data-testid={`plc-layout-${p.id}`} />}
              <IndicatorChip ind={conn} data-testid={`plc-conn-${p.id}`} />
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
