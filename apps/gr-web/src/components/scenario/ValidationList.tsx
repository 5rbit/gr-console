// 검증 결과 목록 — 행을 누르면 그 스텝이 선택된다. 없으면 그리지 않는다(빈 띠는 노이즈).
import { AlertTriangle } from 'lucide-react'
import type { ValidationIssue } from '../../lib/scenario/model'

export function ValidationList({ issues, onPick }: { issues: readonly ValidationIssue[]; onPick: (stepIndex: number | null) => void }) {
  if (issues.length === 0) return null
  return (
    <div className="border-t border-amber-200 bg-amber-50/60 dark:border-amber-500/30 dark:bg-amber-500/10" data-testid="validation-list">
      <div className="flex items-center gap-1.5 px-3 py-1 text-2xs font-semibold text-amber-700 dark:text-amber-300">
        <AlertTriangle size={12} /> 검증 {issues.length}건
      </div>
      <ul className="m-0 max-h-40 list-none overflow-auto p-0">
        {issues.map((it, i) => (
          <li key={i}>
            <button
              type="button"
              className="flex w-full items-baseline gap-2 px-3 py-0.5 text-left text-xs hover:bg-amber-100/70 dark:hover:bg-amber-500/15"
              onClick={() => onPick(it.step_index)}
            >
              <span className="w-14 shrink-0 font-mono text-slate-500">{it.step_index === null ? '시나리오' : `스텝 ${it.step_index + 1}`}</span>
              <span className="w-24 shrink-0 font-mono text-slate-400">{it.field}</span>
              <span className="min-w-0 flex-1 truncate">{it.message}</span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  )
}
