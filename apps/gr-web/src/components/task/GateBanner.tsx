// 제출 게이트 배너 — `GET /api/tasks/gate`를 1초마다 폴링해 막힌 이유를 늘어놓는다.
//
// 게이트가 열려 있으면 조용한 한 줄(초록 점)만, 닫혀 있으면 이유 목록을 붉게 편다. 이유는 백엔드가
// 사람의 말로 낸다(`ledger::ops::gate`) — 여기서 다시 번역하지 않는다.
import { useEffect, useState } from 'react'
import { ShieldAlert, ShieldCheck } from 'lucide-react'
import { api } from '../../lib/api'
import { visibleInterval } from '../../lib/poll'
import type { Gate } from '../../lib/types'

const POLL_MS = 1000

/** 게이트 폴링 훅 — 화면이 살아 있는 동안만 돈다. `null`은 아직 못 받음. */
export function useGate(robot: number | null = null): { gate: Gate | null; error: string | null } {
  const [gate, setGate] = useState<Gate | null>(null)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    let alive = true
    const tick = async () => {
      try {
        const g = await api.taskGate(robot)
        if (!alive) return
        setGate(g)
        setError(null)
      } catch (e) {
        if (!alive) return
        setError(e instanceof Error ? e.message : String(e))
      }
    }
    void tick()
    const t = visibleInterval(() => void tick(), POLL_MS)
    return () => {
      alive = false
      clearInterval(t)
    }
  }, [robot])
  return { gate, error }
}

export function GateBanner({ gate, error }: { gate: Gate | null; error: string | null }) {
  if (error) {
    return (
      <div
        className="flex items-start gap-2 rounded-md border border-warn bg-warn-soft px-3 py-2 text-xs text-warn-fg"
        data-testid="gate-banner"
        data-state="error"
      >
        <ShieldAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" />
        <span>게이트 조회 실패 — {error}</span>
      </div>
    )
  }
  if (!gate) {
    return (
      <div className="rounded-md border border-line-default px-3 py-2 text-xs text-content-faint">
        게이트 확인 중…
      </div>
    )
  }
  if (gate.can_submit) {
    return (
      <div
        className="flex items-center gap-2 rounded-md border border-ok bg-ok-soft px-3 py-1.5 text-xs text-ok-fg"
        data-testid="gate-banner"
        data-state="open"
      >
        <ShieldCheck className="h-3.5 w-3.5 shrink-0" />
        <span>제출 가능 — PLC가 새 작업을 받을 수 있습니다</span>
      </div>
    )
  }
  return (
    <div
      className="flex items-start gap-2 rounded-md border border-fault bg-fault-soft px-3 py-2 text-xs text-fault-fg"
      data-testid="gate-banner"
      data-state="closed"
    >
      <ShieldAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" />
      <div className="min-w-0">
        <div className="font-medium">제출 불가</div>
        <ul className="mt-0.5 list-disc pl-4">
          {gate.reasons.map((r) => (
            <li key={r}>{r}</li>
          ))}
        </ul>
      </div>
    </div>
  )
}
