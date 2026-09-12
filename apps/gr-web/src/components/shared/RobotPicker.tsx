// 로봇 선택 — GRM 뒤 로봇 목록에서 명령을 보낼 로봇을 고른다. 상태 점 = 명령 경로 + 상태 PLC + 게이트.
import { Bot } from 'lucide-react'
import { robots } from '../../lib/robots'
import { useStore } from '../../lib/store'
import { StatusDot } from '../../lib/ui/StatusDot'
import type { Robot } from '../../lib/types'
import { cn } from '../../lib/utils'

export function robotTone(r: Robot): 'ok' | 'warn' | 'fault' | 'neutral' {
  if (!r.cmd_ready || !r.plc_connected) return 'fault'
  if (r.layout_ok === false) return 'fault'
  if (!r.gate.can_submit) return 'warn'
  return 'ok'
}

export function RobotPicker({ compact = false }: { compact?: boolean }) {
  useStore(robots)
  const list = robots.list
  const sel = robots.selected
  if (!list.length)
    return (
      <span className="text-[11px] text-slate-400">
        {robots.error ? '로봇 목록 없음' : '로봇…'}
      </span>
    )
  return (
    <div
      className="inline-flex items-center gap-1"
      data-testid="robot-picker"
      title="명령을 보낼 로봇"
    >
      {!compact ? <Bot className="h-3.5 w-3.5 text-slate-500" /> : null}
      <div
        className="inline-flex rounded-md border border-slate-300 p-0.5 dark:border-slate-600"
        role="radiogroup"
        aria-label="로봇"
      >
        {list.map((r) => {
          const on = r.id === sel
          return (
            <button
              key={r.id}
              type="button"
              role="radio"
              aria-checked={on}
              className={cn(
                'flex items-center gap-1 rounded px-2 py-0.5 text-xs',
                on
                  ? 'bg-indigo-600 text-white'
                  : 'text-slate-600 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800',
              )}
              onClick={() => robots.select(r.id)}
              data-testid={`robot-${r.id}`}
              title={`${r.name} · ${r.opcua_root} · DST ${r.dst}${r.gate.can_submit ? '' : ` · 게이트 닫힘: ${r.gate.reasons.join('; ')}`}`}
            >
              <StatusDot status={robotTone(r)} />
              {r.name}
              {r.active_tasks ? (
                <span className={cn('font-mono text-[10px]', on ? 'opacity-80' : 'text-slate-400')}>
                  {r.active_tasks}
                </span>
              ) : null}
            </button>
          )
        })}
      </div>
    </div>
  )
}
