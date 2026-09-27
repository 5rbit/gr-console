// 상태바의 알림 배지 — 확인 안 된 알림 수. 누르면 위로 목록이 열린다(행 → 이벤트 화면의 전후 창, ✓ → 확인).
// 0 이면 그리지 않는다(상태바는 정상을 칠하지 않는다). 규칙 편집은 이벤트 화면 ⋯ 의 '알림 규칙…'.
import { useCallback, useEffect, useState } from 'react'
import { Bell, Check } from 'lucide-react'
import { alerts } from '../../lib/evtlog/alerts'
import { fmtEvtTime } from '../../lib/evtlog/evtRowsModel'
import { evtView } from '../../lib/evtlog/store'
import { nav } from '../../lib/nav'
import { useStore } from '../../lib/store'
import { Button } from '../../lib/ui/Button'
import { toast } from '../../lib/ui/toast'
import { useDismiss } from '../../lib/ui/useDismiss'

export function AlertBadge() {
  useStore(alerts)
  const [open, setOpen] = useState(false)
  const close = useCallback(() => setOpen(false), [])
  const root = useDismiss<HTMLSpanElement>(open, close)

  useEffect(() => alerts.start(), [])

  const n = alerts.unacked
  useEffect(() => {
    if (n === 0) setOpen(false)
  }, [n])
  if (!alerts.available || n === 0) return null

  const fail = (e: unknown) => toast.error(e instanceof Error ? e.message : String(e))
  const now = Date.now()

  return (
    <span ref={root} className="relative shrink-0">
      <button
        type="button"
        className="flex items-center gap-1 rounded bg-fault-soft px-1 text-fault-fg tabular-nums hover:bg-surface-active"
        title="확인 안 된 알림 — 누르면 목록"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        data-testid="sb-alerts"
      >
        <Bell size={11} />
        알림 {n}
      </button>
      {open ? (
        <div
          role="dialog"
          aria-label="알림"
          className="absolute right-0 bottom-6 z-40 w-96 rounded-lg border border-line-default bg-surface-panel text-xs text-content-primary shadow-lg"
          data-testid="sb-alerts-panel"
        >
          <div className="flex items-center gap-2 border-b border-line-subtle px-3 py-1.5">
            <span className="font-semibold">알림</span>
            <span className="text-content-muted tabular-nums">{n}</span>
            <span className="flex-1" />
            <Button
              size="sm"
              intent="ghost"
              onClick={() => void alerts.ackAll().catch(fail)}
              data-testid="sb-alerts-ackall"
            >
              모두 확인
            </Button>
          </div>
          <div className="max-h-80 overflow-y-auto py-1" role="list">
            {alerts.rows.map((a) => (
              <div
                key={a.id}
                role="listitem"
                className="flex items-start gap-2 px-3 py-1 hover:bg-surface-inset"
              >
                <button
                  type="button"
                  className="min-w-0 flex-1 text-left"
                  title={a.event ? `${a.event.ts} ${a.event.plc} ${a.event.text}` : a.rule_name}
                  disabled={!a.event}
                  onClick={() => {
                    if (!a.event) return
                    evtView.focus(a.event)
                    nav.go('events')
                    setOpen(false)
                  }}
                >
                  <span className="flex gap-2 text-2xs text-content-muted">
                    <span className="font-mono">{fmtEvtTime(a.ts, now)}</span>
                    <span className="truncate">{a.rule_name}</span>
                    {a.event ? <span className="font-mono">{a.event.plc}</span> : null}
                  </span>
                  <span className="block truncate">
                    {a.event?.text ?? '(보존 기간으로 지워진 이벤트)'}
                  </span>
                </button>
                <Button
                  size="icon-sm"
                  intent="ghost"
                  icon={<Check size={12} />}
                  aria-label="확인"
                  title="확인"
                  onClick={() => void alerts.ack(a.id).catch(fail)}
                />
              </div>
            ))}
          </div>
        </div>
      ) : null}
    </span>
  )
}
