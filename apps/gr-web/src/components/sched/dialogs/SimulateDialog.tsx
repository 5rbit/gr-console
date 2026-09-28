// 시뮬레이션 — 저장 전 설정으로 한 번 판정해 후보 · 선택을 본다(만들지 않음). 페이지 ⋯ 와 정책 탭 "미리보기" 가 연다.
import { useCallback, useEffect, useRef, useState } from 'react'
import type { GenConfig } from '../../../lib/taskgen'
import { schedApi, type Policy, type SimResult } from '../../../lib/sched'
import {
  ORIGIN_LABEL,
  ORIGIN_ORDER,
  originCounts,
  simSummary,
  skippedLine,
} from '../../../lib/sched/decisionModel'
import { Button } from '../../../lib/ui/Button'
import { Dialog } from '../../../lib/ui/Dialog'
import { CandidateTable } from './CandidateTable'

export function SimulateDialog({
  config,
  title = '시뮬레이션',
  onClose,
}: {
  config: GenConfig & { policy: Policy }
  title?: string
  onClose: () => void
}) {
  const [res, setRes] = useState<SimResult | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  // 여는 순간의 설정으로 판정한다 — 부모가 폴링으로 새 객체를 넘겨도 다시 돌지 않게.
  const cfg = useRef(config)
  const run = useCallback(() => {
    setBusy(true)
    setErr(null)
    schedApi
      .simulate(cfg.current)
      .then((r) => setRes(r))
      .catch((e: unknown) => setErr(e instanceof Error ? e.message : String(e)))
      .finally(() => setBusy(false))
  }, [])
  useEffect(run, [run])

  const sum = simSummary(res)
  const counts = originCounts(res?.derived ?? [])
  const skip = skippedLine(res?.skipped)
  const origins = ORIGIN_ORDER.filter((o) => counts[o])
    .map((o) => `${ORIGIN_LABEL[o]} ${counts[o]}`)
    .join(' · ')

  return (
    <Dialog
      open
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
      title={title}
      size="lg"
      meta={
        res ? (
          <span className="font-mono text-2xs tabular-nums text-content-muted">
            v{cfg.current.version}
          </span>
        ) : null
      }
      footer={
        <>
          <Button type="button" size="sm" intent="ghost" loading={busy} onClick={run}>
            다시 판정
          </Button>
          <Button type="button" size="sm" intent="primary" onClick={onClose}>
            닫기
          </Button>
        </>
      }
      testid="sched-simulate-dialog"
    >
      <div className="flex min-h-0 flex-col gap-2 text-xs">
        {err ? (
          <span className="text-fault-fg" title={err}>
            시뮬레이션 실패 — {err}
          </span>
        ) : !res ? (
          <span className="text-content-faint">판정 중…</span>
        ) : (
          <>
            <span className="font-mono text-2xs tabular-nums text-content-muted" data-testid="sched-sim-summary">
              파생 {sum.derived}
              {origins ? ` (${origins})` : ''} · 후보 {sum.candidates} · 생성 {sum.generate} · 못 됨{' '}
              {sum.skipped}
            </span>
            {skip ? (
              <span className="truncate text-2xs text-warn-fg" title={skip.title}>
                {skip.text}
              </span>
            ) : null}
            <CandidateTable
              rows={res.candidates ?? []}
              derived={res.derived ?? []}
              empty="후보 없음"
              testid="sched-sim-candidates"
            />
          </>
        )}
      </div>
    </Dialog>
  )
}
