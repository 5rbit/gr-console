// 스텝 파라미터 덮어쓰기 — `TaskParamFields partial`. 기본값(`/api/defaults`의 base)이 흐리게 보이고
// 체크한 키만 스텝에 남는다. 변경은 즉시 draft에 반영된다(별도 확인 없음 — 저장이 확인이다).
import { useEffect, useState } from 'react'
import { api } from '../../lib/api'
import { Modal } from '../../lib/ui/Modal'
import { Button } from '../../lib/ui/Button'
import type { ScenarioStep, TaskParams } from '../../lib/types'
import { TaskParamFields } from '../shared/TaskParamFields'
import { stepSummary } from '../../lib/scenario/model'

export interface StepParamsDialogProps {
  step: ScenarioStep | null
  onOpenChange: (open: boolean) => void
  onChange: (params: Partial<TaskParams>) => void
}

export function StepParamsDialog({ step, onOpenChange, onChange }: StepParamsDialogProps) {
  const [base, setBase] = useState<TaskParams | null>(null)
  useEffect(() => {
    if (!step) return
    let alive = true
    api
      .defaults()
      .then((d) => {
        if (alive) setBase(d.base)
      })
      .catch(() => {
        /* 기본값이 없어도 편집은 된다(0/false로 보인다) */
      })
    return () => {
      alive = false
    }
  }, [step?.id]) // eslint-disable-line react-hooks/exhaustive-deps -- 스텝이 바뀔 때만 다시 받는다

  return (
    <Modal open={step !== null} onOpenChange={onOpenChange} title={step ? `파라미터 덮어쓰기 — ${step.label || stepSummary(step)}` : ''} wide>
      {step ? (
        <div className="flex flex-col gap-3">
          <div className="flex items-center justify-between text-xs text-slate-500">
            <span>체크한 항목만 기본값 대신 보낸다. 나머지는 기본 파라미터(PICK/DROP × 셀/스테이션 프로필 포함)를 따른다.</span>
            <Button size="sm" intent="ghost" disabled={Object.keys(step.params).length === 0} onClick={() => onChange({})}>
              모두 해제
            </Button>
          </div>
          <TaskParamFields value={step.params} base={base} partial onChange={onChange} />
        </div>
      ) : null}
    </Modal>
  )
}
