// 예정 스텝 — 실행 중인 시나리오에서 아직 로봇에 보내지 않은(콘솔만 들고 있는) 스텝. PLC 와 무관하게 어느 모드에서나
// 지울 수 있다. PICK/DROP 짝은 같이 지운다(서버 `runner::skip_set`, 화면 `skipSet` 이 같은 규칙).
import { useState } from 'react'
import { robots } from '../../lib/robots'
import { robotLabel } from '../../lib/robotContext'
import { scenarioApi } from '../../lib/scenario/api'
import { skipSet, type StepPhase, type StepResultView } from '../../lib/scenario/model'
import type { Scenario, ScenarioRun } from '../../lib/types'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { Dialog } from '../../lib/ui/Dialog'
import { toast } from '../../lib/ui/toast'
import { RobotChip } from '../shared/RobotChip'

function stepName(s: Scenario['steps'][number], i: number): string {
  const t = s.target ? `${s.target.kind === 'cell' ? 'Cell' : 'Station'} #${s.target.id}` : ''
  return `${i + 1}. ${s.type} ${t}`.trim()
}

export function ScheduledStepsDialog({
  open,
  onOpenChange,
  run,
  scenario,
  phases,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  run: ScenarioRun
  scenario: Scenario
  phases: readonly StepPhase[]
}) {
  const [pending, setPending] = useState<number | null>(null)
  const [busy, setBusy] = useState(false)
  const runRobot = run.robot ?? null
  const cur = { iteration: run.iteration, step_index: run.step_index, sent: !!run.current_task_id }
  const rows = scenario.steps.map((s, i) => ({ s, i })).filter(({ i }) => phases[i] === '예정')
  const plan =
    pending === null
      ? null
      : skipSet(scenario.steps, runRobot, cur, run.results as StepResultView[], pending)
  const chipOf = (i: number) => robots.chipOf(scenario.steps[i].robot ?? runRobot)

  async function remove(i: number) {
    setBusy(true)
    try {
      await scenarioApi.skip(i)
      toast.ok(`스텝 ${i + 1} 을(를) 지웠습니다 (보내지 않음)`)
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
      setPending(null)
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="예정 스텝"
      meta={`${rows.length}건`}
      size="md"
      closeLabel="닫기"
      testid="scheduled-dialog"
    >
      <div className="flex flex-col gap-1 text-xs">
        {rows.length === 0 ? (
          <span className="text-content-faint">이번 회차에 남은 예정 스텝 없음</span>
        ) : (
          rows.map(({ s, i }) => (
            <div key={i} className="flex items-center gap-2">
              <RobotChip chip={chipOf(i)} title={robotLabel(chipOf(i))} />
              <span className="flex-1 truncate">{stepName(s, i)}</span>
              <Button
                size="sm"
                intent="outline"
                disabled={busy}
                onClick={() => setPending(i)}
                data-testid={`scheduled-del-${i + 1}`}
              >
                삭제
              </Button>
            </div>
          ))
        )}
      </div>
      {pending !== null ? (
        <ConfirmDialog
          open
          onOpenChange={(o) => {
            if (!o) setPending(null)
          }}
          scope="single"
          title={`예정 스텝 삭제 — ${chipOf(pending).name}`}
          danger
          confirmLabel="삭제"
          onConfirm={() => void remove(pending)}
        >
          <div className="flex flex-col gap-2 text-xs">
            <span className="flex items-center gap-2">
              <RobotChip chip={chipOf(pending)} title={robotLabel(chipOf(pending))} />
              {stepName(scenario.steps[pending], pending)} 을(를) 보내지 않고 지울까요?
            </span>
            {plan && 'error' in plan ? (
              <span className="text-fault-fg">{plan.error}</span>
            ) : plan && plan.steps.length > 1 ? (
              <span className="text-warn-fg">
                PICK/DROP 짝 —{' '}
                {plan.steps
                  .filter((j) => j !== pending)
                  .map((j) => stepName(scenario.steps[j], j))
                  .join(' · ')}{' '}
                도 함께 지웁니다
              </span>
            ) : null}
            <span className="text-content-faint">
              콘솔만 들고 있는 스텝이라 로봇·PLC 와 무관하게 지워집니다.
            </span>
          </div>
        </ConfirmDialog>
      ) : null}
    </Dialog>
  )
}
