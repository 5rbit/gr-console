// 통계 보기의 조작 — 기간(오늘 · 7일 · 30일 · 직접 지정) · PLC. 알람은 파레토 척도, 스텝은 TaskType 나누기.
import { useState } from 'react'
import {
  STATS_PERIODS,
  periodRange,
  type ParetoMetric,
  type StatsPeriod,
  type StatsState,
} from '../../lib/evtlog/evtStatsModel'
import { localInputToMs, msToLocalInput } from '../../lib/evtlog/evtFilterModel'
import { FormDialog } from '../../lib/ui/Dialog'
import { Input } from '../../lib/ui/Input'
import { Select } from '../../lib/ui/Select'
import { Switch } from '../../lib/ui/Switch'
import { EvtMultiPick, TYPE_OPTIONS, type PickOption } from './EventFilters'

function short(ms: number): string {
  return msToLocalInput(ms).slice(5, 16).replace('T', ' ')
}

export function StatsControls({
  value,
  onChange,
  plcs,
  metric,
  onMetric,
  split,
  types,
}: {
  value: StatsState
  onChange: (s: StatsState) => void
  plcs: readonly PickOption[]
  /** 알람 통계 — 파레토 척도. */
  metric?: ParetoMetric
  onMetric?: (m: ParetoMetric) => void
  /** 스텝 통계 — TaskType 나누기 스위치를 보인다. */
  split?: boolean
  /** 알람 통계 — ErrorList 유형 고르기를 보인다(비면 Alarm, Warn). */
  types?: boolean
}) {
  const [open, setOpen] = useState(false)
  const [from, setFrom] = useState('')
  const [to, setTo] = useState('')

  function pick(v: string) {
    if (v === 'custom') {
      const r = periodRange(value, Date.now())
      setFrom(msToLocalInput(value.from ?? r.from))
      setTo(msToLocalInput(value.to))
      setOpen(true)
      return
    }
    onChange({ ...value, period: v as StatsPeriod, from: null, to: null })
  }

  const fromMs = localInputToMs(from)
  const toMs = localInputToMs(to)
  const why =
    fromMs === null
      ? '시작을 적으세요'
      : to && toMs === null
        ? '시각 형식이 잘못됐습니다'
        : toMs !== null && fromMs >= toMs
          ? '시작이 끝보다 앞이어야 합니다'
          : undefined

  return (
    <>
      <Select
        dense
        aria-label="기간"
        value={value.period}
        onValueChange={pick}
        data-testid="evt-stats-period"
      >
        {STATS_PERIODS.map((p) => (
          <option key={p.id} value={p.id}>
            {p.label}
          </option>
        ))}
        <option value="custom">
          {value.period === 'custom'
            ? `${short(periodRange(value, Date.now()).from)} ~ ${value.to === null ? '지금' : short(value.to)}`
            : '직접 지정…'}
        </option>
      </Select>
      <EvtMultiPick
        label="PLC"
        options={plcs}
        value={value.plcs}
        onChange={(p) => onChange({ ...value, plcs: p })}
        testid="evt-stats-plc"
      />
      {types ? (
        <EvtMultiPick
          label="Type"
          allLabel="Alarm · Warn"
          options={TYPE_OPTIONS}
          value={value.types}
          onChange={(t) => onChange({ ...value, types: t })}
          testid="evt-stats-type"
        />
      ) : null}
      {metric && onMetric ? (
        // 보기 전환이 이미 채운 세그먼트라 기준은 선택 칸으로 — 초록 면을 둘 세우지 않는다
        <Select
          dense
          aria-label="Pareto"
          title="파레토 기준 — 줄 순서와 막대"
          value={metric}
          onValueChange={(m) => onMetric(m as ParetoMetric)}
          data-testid="evt-pareto"
        >
          <option value="count">Pareto · Count</option>
          <option value="duration">Pareto · Duration</option>
        </Select>
      ) : null}
      {split ? (
        <Switch
          inline
          label="TaskType"
          title="Task 종류(TASK_LOADED 의 Src)로 나눈다"
          checked={value.split}
          onCheckedChange={(on) => onChange({ ...value, split: on })}
          testid="evt-stats-split"
        />
      ) : null}
      <FormDialog
        open={open}
        onOpenChange={setOpen}
        title="기간 직접 지정"
        size="sm"
        submitLabel="적용"
        disabledReason={why}
        onSubmit={() => {
          onChange({ ...value, period: 'custom', from: fromMs, to: toMs })
          setOpen(false)
        }}
        testid="evt-stats-range"
      >
        <Input type="datetime-local" step={1} label="from" value={from} onValueChange={setFrom} />
        <Input
          type="datetime-local"
          step={1}
          label="to"
          value={to}
          onValueChange={setTo}
          hint="비우면 지금까지"
        />
      </FormDialog>
    </>
  )
}
