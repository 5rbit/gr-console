// 필터 띠 — 상태 칩(다중 선택) · 종류 · 검색어 · 종결 포함 스위치. 값은 부모가 들고 있고 여기서는 편집만.
//
// 상태 칩을 하나라도 고르면 그 상태만 보이고 `종결 포함`은 의미가 없어 잠근다(칩이 종결 상태를
// 직접 고를 수 있으므로) — 두 조작이 같은 것을 두 번 말하지 않게.
import { RotateCcw, Search } from 'lucide-react'
import { TASK_TYPES } from '../../lib/gr/const'
import { robots } from '../../lib/robots'
import { useStore } from '../../lib/store'
import { Button } from '../../lib/ui/Button'
import { Input } from '../../lib/ui/Input'
import { Select } from '../../lib/ui/Select'
import { Switch } from '../../lib/ui/Switch'
import { statusTone } from '../../lib/ui/status'
import { cn } from '../../lib/utils'
import {
  ALL_STATES,
  EMPTY_FILTER,
  STATE_LABEL,
  STATE_TONE,
  type TaskFilter,
} from '../../lib/task/state'
import type { TaskState } from '../../lib/types'

export interface TaskFiltersProps {
  value: TaskFilter
  onChange: (next: TaskFilter) => void
  /** 상태별 건수 — 칩 옆의 숫자(스토어 집계). */
  counts?: Partial<Record<TaskState, number>>
}

export function TaskFilters({ value, onChange, counts = {} }: TaskFiltersProps) {
  const toggle = (s: TaskState) => {
    const states = value.states.includes(s)
      ? value.states.filter((x) => x !== s)
      : [...value.states, s]
    onChange({ ...value, states })
  }
  useStore(robots)
  const dirty =
    value.states.length > 0 ||
    value.type !== '' ||
    value.q !== '' ||
    value.robot !== '' ||
    value.includeTerminal !== EMPTY_FILTER.includeTerminal

  return (
    <div
      className="flex flex-wrap items-center gap-x-3 gap-y-1.5 border-b border-line-default bg-surface-inset px-3 py-1.5"
      data-testid="task-filters"
    >
      <div className="flex flex-wrap items-center gap-1" role="group" aria-label="상태 필터">
        {ALL_STATES.map((s) => {
          const on = value.states.includes(s)
          const tone = statusTone(STATE_TONE[s])
          const n = counts[s] ?? 0
          return (
            <button
              key={s}
              type="button"
              aria-pressed={on}
              data-testid={`chip-${s}`}
              title={`${STATE_LABEL[s]} ${n}건`}
              onClick={() => toggle(s)}
              className={cn(
                'inline-flex items-center gap-1 rounded border px-2 py-0.5 text-2xs font-medium whitespace-nowrap transition-colors',
                on
                  ? `${tone.soft} ${tone.text} ${tone.border}`
                  : 'border-transparent text-content-muted hover:bg-surface-inset',
              )}
            >
              <span className={`h-1.5 w-1.5 rounded-full ${tone.dot}`} />
              {STATE_LABEL[s]}
              {n > 0 ? <span className="tabular-nums opacity-70">{n}</span> : null}
            </button>
          )
        })}
      </div>

      {robots.multi ? (
        <Select
          dense
          aria-label="로봇"
          value={value.robot}
          onValueChange={(robot) => onChange({ ...value, robot })}
          className="w-24"
          data-testid="filter-robot"
        >
          <option value="">모든 로봇</option>
          {robots.list.map((r) => (
            <option key={r.id} value={r.plc}>
              {r.name}
            </option>
          ))}
        </Select>
      ) : null}
      <Select
        dense
        aria-label="작업 종류"
        value={value.type}
        onValueChange={(type) => onChange({ ...value, type })}
        className="w-28"
      >
        <option value="">모든 종류</option>
        {TASK_TYPES.map((t) => (
          <option key={t} value={t}>
            {t}
          </option>
        ))}
      </Select>

      <div className="relative">
        <Search
          size={13}
          className="pointer-events-none absolute top-2 left-2 text-content-faint"
        />
        <Input
          aria-label="검색"
          placeholder="seq · WorkId · 셀 · 품목 · 메모"
          value={value.q}
          onValueChange={(q) => onChange({ ...value, q })}
          className="w-56 [&>input]:h-7 [&>input]:pl-7 [&>input]:text-xs"
        />
      </div>

      <span className="inline-flex items-center gap-1.5 text-2xs text-content-muted">
        <Switch
          label="종결 포함"
          checked={value.includeTerminal}
          disabled={value.states.length > 0}
          title={
            value.states.length > 0
              ? '상태 칩을 골랐을 때는 칩이 우선합니다'
              : '완료·취소·거부·실패도 표에 보입니다'
          }
          testid="switch-terminal"
          onCheckedChange={(includeTerminal) => onChange({ ...value, includeTerminal })}
        />
        종결 포함
      </span>

      {dirty ? (
        <Button
          size="sm"
          intent="ghost"
          icon={<RotateCcw size={12} />}
          onClick={() => onChange(EMPTY_FILTER)}
        >
          초기화
        </Button>
      ) : null}
    </div>
  )
}
