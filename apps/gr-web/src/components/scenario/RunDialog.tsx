// 실행 옵션 — 반복(또는 무한)과 시작 스텝. 저장되지 않은 변경은 실행에 반영되지 않음을 알린다.
import { useEffect, useState } from 'react'
import { Play } from 'lucide-react'
import { Button } from '../../lib/ui/Button'
import { Input } from '../../lib/ui/Input'
import { Modal } from '../../lib/ui/Modal'
import { Select } from '../../lib/ui/Select'
import { Switch } from '../../lib/ui/Switch'
import type { Scenario } from '../../lib/types'
import type { RunOptions } from '../../lib/scenario/api'
import { stepSummary } from '../../lib/scenario/model'

export interface RunDialogProps {
  open: boolean
  scenario: Scenario | null
  dirty: boolean
  onOpenChange: (open: boolean) => void
  onRun: (opts: RunOptions) => void
}

export function RunDialog({ open, scenario, dirty, onOpenChange, onRun }: RunDialogProps) {
  const [infinite, setInfinite] = useState(false)
  const [repeat, setRepeat] = useState(1)
  const [start, setStart] = useState(0)
  useEffect(() => {
    if (!open || !scenario) return
    setInfinite(scenario.repeat === 0)
    setRepeat(scenario.repeat === 0 ? 1 : scenario.repeat)
    setStart(0)
  }, [open, scenario])

  return (
    <Modal open={open} onOpenChange={onOpenChange} title={`실행 — ${scenario?.name ?? ''}`}>
      {scenario ? (
        <div className="flex flex-col gap-3" data-testid="run-dialog">
          {dirty ? (
            <p className="m-0 rounded bg-warn-soft px-2 py-1 text-xs text-warn-fg">
              저장되지 않은 변경은 이번 실행에 반영되지 않습니다 — 먼저 저장하세요.
            </p>
          ) : null}
          <div className="flex items-end gap-3">
            <Input
              label="반복 횟수"
              className="w-28"
              type="number"
              mono
              min={1}
              value={infinite ? '' : String(repeat)}
              placeholder={infinite ? '∞' : ''}
              disabled={infinite}
              onValueChange={(v) => {
                const n = Math.trunc(Number(v))
                if (Number.isFinite(n) && n >= 1) setRepeat(n)
              }}
              data-testid="run-repeat"
            />
            <label className="flex h-8 items-center gap-2 text-xs">
              <Switch
                checked={infinite}
                label="무한 반복"
                testid="run-infinite"
                onCheckedChange={setInfinite}
              />
              무한 (정지할 때까지)
            </label>
          </div>
          <Select
            label="시작 스텝 (첫 회차만)"
            value={String(start)}
            onValueChange={(v) => setStart(Number(v))}
            data-testid="run-start-step"
          >
            {scenario.steps.map((s, i) => (
              <option key={s.id} value={String(i)}>
                {i + 1}. {s.label || stepSummary(s)}
              </option>
            ))}
          </Select>
          <div className="flex justify-end">
            <Button
              intent="primary"
              icon={<Play size={14} />}
              disabled={scenario.steps.length === 0}
              onClick={() => onRun({ repeat: infinite ? 0 : repeat, start_step: start })}
              data-testid="run-confirm"
            >
              실행
            </Button>
          </div>
        </div>
      ) : null}
    </Modal>
  )
}
