// 실행 옵션 — 반복(또는 무한) · 시작 스텝 · **로봇**. 실행 버튼이 여는 유일한 대화상자다.
//
// 로봇이 여기로 들어온 이유: 예전에는 사이드바에서 고르고 이 상자는 "지금 고른 것"을 글자로만
// 알렸다. 실행 직전에 바꾸려면 상자를 닫고 사이드바로 갔다가 다시 열어야 했다. 기본값은 그대로
// 사이드바 선택이고(`robots.selected`), 여기서 고른 것은 이번 실행에만 실린다.
// 저장되지 않은 변경은 실행에 반영되지 않음을 알린다.
import { useEffect, useState } from 'react'
import { robots } from '../../lib/robots'
import { robotLabel } from '../../lib/robotContext'
import { RobotChip } from '../shared/RobotChip'
import { useStore } from '../../lib/store'
import { FormDialog } from '../../lib/ui/Dialog'
import { Input } from '../../lib/ui/Input'
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
  useStore(robots)
  const [infinite, setInfinite] = useState(false)
  const [repeat, setRepeat] = useState(1)
  const [start, setStart] = useState(0)
  const [robot, setRobot] = useState<number | null>(null)
  useEffect(() => {
    if (!open || !scenario) return
    setInfinite(scenario.repeat === 0)
    setRepeat(scenario.repeat === 0 ? 1 : scenario.repeat)
    setStart(0)
    setRobot(robots.selected)
  }, [open, scenario])

  // 빈 로봇 칸의 스텝이 실제로 갈 곳 — `기본 로봇`은 백엔드의 첫 호기다. 말없이 두지 않고 이름을 보인다.
  const fallback = robots.list.find((r) => r.default) ?? robots.list[0] ?? null
  const chip = robot !== null ? robots.chipOf(robot) : robots.chipOf(fallback?.id ?? null)

  return (
    <FormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={`실행 — ${chip.name}`}
      size="sm"
      meta={
        <>
          <RobotChip chip={chip} prefix="로봇" testid="run-robot-chip" />
          <span className="truncate">{scenario?.name ?? ''}</span>
          {/* 저장 안 한 변경은 **라벨+값 한 짝**으로 말한다 — 같은 말을 바깥 토스트가 이미 한다. */}
          {dirty ? <span className="text-warn-fg">저장 안 함 · 이번 실행 제외</span> : null}
        </>
      }
      submitLabel={`${chip.name} 에서 실행`}
      disabledReason={!scenario || scenario.steps.length === 0 ? '스텝이 없습니다' : undefined}
      onSubmit={() => {
        if (!scenario || scenario.steps.length === 0) return
        onRun({ repeat: infinite ? 0 : repeat, start_step: start, robot })
      }}
      testid="run-dialog"
    >
      {scenario ? (
        <>
          <div className="flex items-end gap-3">
            <Input
              label="Repeat"
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
            <Switch
              inline
              label="무한 반복"
              checked={infinite}
              title="정지할 때까지 돕니다"
              testid="run-infinite"
              onCheckedChange={setInfinite}
            />
          </div>
          <Select
            label="Robot"
            hint="빈 로봇 칸에 적용 · 이번 실행만"
            value={robot === null ? '' : String(robot)}
            onValueChange={(v) => setRobot(v === '' ? null : Number(v))}
            data-testid="run-robot"
          >
            <option value="">{`기본 로봇 (${fallback ? robotLabel(robots.chipOf(fallback.id)) : '미확인'})`}</option>
            {robots.list.map((r) => (
              <option key={r.id} value={String(r.id)}>
                {r.name}
              </option>
            ))}
          </Select>
          <Select
            label="StartStep (첫 회차만)"
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
        </>
      ) : null}
    </FormDialog>
  )
}
