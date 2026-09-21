// 조작 띠 한 줄 — 검색칸 + `필터` 팝오버 + **걸린 조건 칩**. 화면에 늘 서 있는 것은 이 셋뿐이다.
//
// 예전에는 상태 칩 열 개 · 로봇 · 종류 · 검색 · 스위치 · 초기화가 한 줄에 펼쳐져 있었다. 그중 아홉은
// 평소에 아무 일도 하지 않으면서 표가 설 자리를 먹었다. 그래서 고르는 자리는 팝오버로 접고,
// **지금 걸려 있는 것만** 칩으로 남긴다 — 칩은 조건을 말하면서 그 자리에서 지워진다(×).
import { useEffect, useRef, useState } from 'react'
import { ListFilter, Search, X } from 'lucide-react'
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
  /** 상태별 건수 — 팝오버의 칩 옆 숫자(스토어 집계). */
  counts?: Partial<Record<TaskState, number>>
  /** 오른쪽 끝(보기 전환 같은 것). */
  children?: React.ReactNode
  /** 필터가 뜻이 없는 보기(종결 이력은 서버 페이징이라 이 필터를 타지 않는다). */
  disabled?: boolean
  /** 띠 왼쪽의 건수 표시. */
  meta?: React.ReactNode
}

/** 걸린 조건 하나 — 누르면 그것만 풀린다. */
function Chip({
  label,
  tone,
  onClear,
  testid,
}: {
  label: string
  tone?: string
  onClear: () => void
  testid?: string
}) {
  const t = tone ? statusTone(tone) : null
  return (
    <button
      type="button"
      data-testid={testid}
      title={`${label} 조건 해제`}
      onClick={onClear}
      className={cn(
        'inline-flex items-center gap-1 rounded border px-1.5 py-0.5 text-2xs whitespace-nowrap transition-colors',
        t
          ? `${t.soft} ${t.text} ${t.border}`
          : 'border-line-default bg-surface-panel text-content-tertiary hover:bg-surface-inset',
      )}
    >
      {t ? <span className={`h-1.5 w-1.5 rounded-full ${t.dot}`} /> : null}
      {label}
      <X size={10} className="opacity-70" />
    </button>
  )
}

/** `필터` 손잡이 + 그 아래 패널. 바깥 클릭·Escape로 닫는다(`IconPopover`와 같은 규칙). */
function FilterPopover({
  n,
  disabled,
  children,
}: {
  n: number
  disabled: boolean
  children: React.ReactNode
}) {
  const [open, setOpen] = useState(false)
  const root = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!open) return
    const onDown = (e: MouseEvent) => {
      if (root.current && !root.current.contains(e.target as Node)) setOpen(false)
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false)
    }
    document.addEventListener('mousedown', onDown)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('mousedown', onDown)
      document.removeEventListener('keydown', onKey)
    }
  }, [open])

  return (
    <div ref={root} className="relative">
      <Button
        size="sm"
        intent={n > 0 ? 'outline' : 'ghost'}
        icon={<ListFilter size={13} />}
        aria-expanded={open}
        disabled={disabled}
        title={disabled ? '종결 이력은 서버 페이징이라 이 필터를 타지 않습니다' : '상태·로봇·종류'}
        data-testid="filter-open"
        onClick={() => setOpen((o) => !o)}
      >
        필터
        {n > 0 ? <span className="tabular-nums text-accent-text">{n}</span> : null}
      </Button>
      {open ? (
        <div
          role="dialog"
          aria-label="필터"
          data-testid="filter-panel"
          className="absolute top-8 left-0 z-30 w-80 rounded-lg border border-line-default bg-surface-panel p-3 shadow-lg"
        >
          {children}
        </div>
      ) : null}
    </div>
  )
}

export function TaskFilters({
  value,
  onChange,
  counts = {},
  children,
  disabled = false,
  meta,
}: TaskFiltersProps) {
  useStore(robots)
  const toggle = (s: TaskState) => {
    const states = value.states.includes(s)
      ? value.states.filter((x) => x !== s)
      : [...value.states, s]
    onChange({ ...value, states })
  }
  const active =
    value.states.length +
    (value.type ? 1 : 0) +
    (value.robot ? 1 : 0) +
    (value.includeTerminal !== EMPTY_FILTER.includeTerminal ? 1 : 0)

  return (
    <div
      className="flex min-h-control-md flex-wrap items-center gap-x-2 gap-y-1 border-b border-line-default bg-surface-inset px-2 py-1"
      data-testid="task-filters"
    >
      <div className="relative">
        <Search size={13} className="pointer-events-none absolute top-2 left-2 text-content-faint" />
        <Input
          aria-label="검색"
          placeholder="Seq · WorkId · Cell · ItemCode · Note"
          value={value.q}
          disabled={disabled}
          onValueChange={(q) => onChange({ ...value, q })}
          className="w-56 [&>input]:h-control-sm [&>input]:pl-7 [&>input]:text-xs"
        />
      </div>

      <FilterPopover n={active} disabled={disabled}>
        <div className="space-y-2.5">
          <div>
            <div className="mb-1 text-2xs font-medium text-content-muted">상태</div>
            <div className="flex flex-wrap gap-1" role="group" aria-label="상태 필터">
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
                        : 'border-line-default text-content-muted hover:bg-surface-inset',
                    )}
                  >
                    <span className={`h-1.5 w-1.5 rounded-full ${tone.dot}`} />
                    {STATE_LABEL[s]}
                    {n > 0 ? <span className="tabular-nums opacity-70">{n}</span> : null}
                  </button>
                )
              })}
            </div>
          </div>

          <div className="flex items-end gap-2">
            {robots.multi ? (
              <Select
                dense
                label="Robot"
                value={value.robot}
                onValueChange={(robot) => onChange({ ...value, robot })}
                className="w-28"
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
              label="TaskType"
              value={value.type}
              onValueChange={(type) => onChange({ ...value, type })}
              className="w-32"
              data-testid="filter-type"
            >
              <option value="">모든 종류</option>
              {TASK_TYPES.map((t) => (
                <option key={t} value={t}>
                  {t}
                </option>
              ))}
            </Select>
          </div>

          {/* 상태 칩을 골랐을 때는 칩이 우선이라 이 스위치는 아무 뜻이 없다 — 잠가서 두 조작이 같은
              것을 두 번 말하지 않게 한다. */}
          <Switch
            inline
            label="종결 포함"
            checked={value.includeTerminal}
            disabled={value.states.length > 0}
            title={
              value.states.length > 0
                ? '상태를 골랐을 때는 그것이 우선입니다'
                : '완료·취소·거부·실패도 표에 보입니다'
            }
            testid="switch-terminal"
            onCheckedChange={(includeTerminal) => onChange({ ...value, includeTerminal })}
          />

          <div className="flex justify-end border-t border-line-subtle pt-2">
            <Button
              size="sm"
              intent="ghost"
              disabled={active === 0 && value.q === ''}
              onClick={() => onChange(EMPTY_FILTER)}
              data-testid="filter-reset"
            >
              모두 해제
            </Button>
          </div>
        </div>
      </FilterPopover>

      {/* 걸린 조건만 칩으로 — 없으면 아무것도 서지 않는다. */}
      {!disabled ? (
        <div className="flex flex-wrap items-center gap-1" data-testid="filter-chips">
          {value.states.map((s) => (
            <Chip
              key={s}
              label={STATE_LABEL[s]}
              tone={STATE_TONE[s]}
              testid={`chip-on-${s}`}
              onClear={() => toggle(s)}
            />
          ))}
          {value.robot ? (
            <Chip
              label={robots.list.find((r) => r.plc === value.robot)?.name ?? value.robot}
              testid="chip-on-robot"
              onClear={() => onChange({ ...value, robot: '' })}
            />
          ) : null}
          {value.type ? (
            <Chip
              label={value.type}
              testid="chip-on-type"
              onClear={() => onChange({ ...value, type: '' })}
            />
          ) : null}
          {value.includeTerminal ? (
            <Chip
              label="종결 포함"
              testid="chip-on-terminal"
              onClear={() => onChange({ ...value, includeTerminal: false })}
            />
          ) : null}
        </div>
      ) : null}

      {meta ? <span className="text-2xs text-content-faint tabular-nums">{meta}</span> : null}
      <span className="ml-auto flex items-center gap-1">{children}</span>
    </div>
  )
}
