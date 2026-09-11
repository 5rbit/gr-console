// 측정 모니터 공용 조각 — 칩, 키/값 표, 카드, 추세 셀, PLC 작업 표.
import type { ReactNode } from 'react'
import { KIND, STATUS } from '../../lib/gr/const'
import { f0, f1, f2, flagStr, tt } from '../../lib/meas/format'
import { DataTable } from '../../lib/ui/DataTable'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import type { Column } from '../../lib/ui/table'
import type { PlcTask, Trend } from '../../lib/types'

export { KIND, STATUS }

/** 불리언 필드 칩 묶음(켜진 것만 강조). */
export function Chips({ obj, keys, bad = [], warn = [] }: { obj: Record<string, unknown> | null | undefined; keys: readonly string[]; bad?: readonly string[]; warn?: readonly string[] }) {
  if (!obj) return <span className="text-content-muted">-</span>
  return (
    <div className="flex flex-wrap gap-1">
      {keys
        .filter((k) => k in obj)
        .map((k) => {
          const on = Boolean(obj[k])
          const tone = bad.includes(k) ? 'fault' : warn.includes(k) ? 'warn' : 'ok'
          return (
            <StatusBadge key={k} status={on ? tone : 'neutral'} dot={on}>
              {k}
            </StatusBadge>
          )
        })}
    </div>
  )
}

/** 라벨/값 2열 표(PLC 블록처럼 8줄 안팎의 짧은 목록). */
export function KvTable({ rows, className = '' }: { rows: readonly [ReactNode, ReactNode][]; className?: string }) {
  return (
    <table className={`w-full text-xs ${className}`}>
      <tbody>
        {rows.map(([k, v], i) => (
          <tr key={i} className="border-b border-line-default last:border-0">
            <td className="w-[40%] py-1 pr-2 text-content-muted">{k}</td>
            <td className="py-1 font-mono tabular-nums">{v}</td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}

export interface StatCardItem {
  label: string
  value: ReactNode
  hint?: ReactNode
  tone?: 'ok' | 'bad' | 'warn' | ''
  big?: boolean
}

export function StatCards({ items }: { items: readonly StatCardItem[] }) {
  return (
    <div className="mb-3 flex flex-wrap gap-2">
      {items.map((x, i) => (
        <div
          key={i}
          className={`min-w-28 rounded-md border px-3 py-1.5 ${
            x.tone === 'ok'
              ? 'border-emerald-300 bg-emerald-50 dark:border-emerald-700 dark:bg-emerald-900/30'
              : x.tone === 'bad'
                ? 'border-red-300 bg-red-50 dark:border-red-700 dark:bg-red-900/30'
                : x.tone === 'warn'
                  ? 'border-amber-300 bg-amber-50 dark:border-amber-700 dark:bg-amber-900/30'
                  : 'border-line-default bg-surface-panel'
          }`}
        >
          <div className="text-2xs text-content-muted">{x.label}</div>
          <div className={`font-mono tabular-nums ${x.big ? 'text-xl' : 'text-base'} font-semibold`}>{x.value}</div>
          {x.hint ? <div className="text-2xs text-content-muted">{x.hint}</div> : null}
        </div>
      ))}
    </div>
  )
}

/** 추세 값 셀: 평균 / EMA (최소 ~ 최대, n). */
export function TrendCell({ t }: { t: Trend | undefined | null }) {
  if (!t || !t.Count) return <span className="text-content-muted">-</span>
  return (
    <span title={`n=${t.Count}`} className="font-mono tabular-nums">
      {f2(t.Avg)} / {f2(t.Ema)}
      <br />
      <span className="text-content-muted">
        {f2(t.Min)} ~ {f2(t.Max)} ({t.Count})
      </span>
    </span>
  )
}

/** LGR_Task_Data 상세(명령 파라미터까지). */
export function TaskKv({ t }: { t: PlcTask | null | undefined }) {
  if (!t) return <span className="text-content-muted">-</span>
  const it = t.Item ?? ({} as PlcTask['Item'])
  const p = t.Position ?? []
  const c = t.Cell ?? ({} as PlcTask['Cell'])
  const cp = c.Position ?? []
  return (
    <KvTable
      rows={[
        ['Work / Task', `${t.WorkId ?? ''} / ${t.TaskId ?? ''}`],
        ['Type', `${tt(t.TaskType)} (0x${Number(t.TaskType ?? 0).toString(16).toUpperCase().padStart(2, '0')})`],
        ['Cell', `${c.Id ?? ''}  (Sec ${c.Section ?? ''} Row ${c.Row ?? ''} Col ${c.Col ?? ''})`],
        ['Item', `Code ${it.Code ?? ''} / ${it.Count ?? ''}단 / ID ${f1(it.InnerDiameter)} / OD ${f1(it.OuterDiameter)} / H ${f1(it.Height)}`],
        ['Position', `X ${f0(p[0])} Y ${f0(p[1])} Z ${f0(p[2])} G ${f0(p[3])}`],
        ['Cell 위치', `X ${f0(cp[0])} Y ${f0(cp[1])} Z ${f0(cp[2])}  (명령 Z rel ${f1((p[2] ?? 0) - (cp[2] ?? 0))})`],
        ['플래그', flagStr(t)],
        ['Grip', `H ${t.GripHeight ?? ''} / PreGrip ${t.PreGripDelta ?? ''} / GripBack ${t.GripBackDelta ?? ''}`],
        ['Lift', `Up ${t.LiftUpHeight ?? ''} / Creep ↑${t.LiftUpCreepDistance ?? ''} ↓${t.LiftDownCreepDistance ?? ''} / Partial ${t.LiftUpPartial ? 'Y' : 'N'}`],
        ['Blend', `↑${t.BlendUpDistance ?? ''} ↓${t.BlendDownDistance ?? ''}`],
        ['Drag', `Out ${t.UseDragOut ? `${t.DragOutHeight}/${t.DragOutDist}/dir${t.DragOutDir}` : '-'}  In ${t.UseDragIn ? `${t.DragInHeight}/${t.DragInDist}/dir${t.DragInDir}` : '-'}`],
      ]}
    />
  )
}

const TASK_COLS: Column<PlcTask>[] = [
  { key: 'w', label: 'Work', get: (t) => t.WorkId, numeric: true },
  { key: 't', label: 'Task', get: (t) => t.TaskId, numeric: true },
  { key: 'ty', label: 'Type', get: (t) => tt(t.TaskType) },
  { key: 'cell', label: 'Cell', get: (t) => t.Cell?.Id, numeric: true },
  { key: 'code', label: 'Code', get: (t) => t.Item?.Code, numeric: true },
  { key: 'cnt', label: '단', get: (t) => t.Item?.Count, numeric: true },
  { key: 'id', label: 'ID', get: (t) => f1(t.Item?.InnerDiameter), numeric: true },
  { key: 'h', label: 'H', get: (t) => f1(t.Item?.Height), numeric: true },
  { key: 'x', label: 'X', get: (t) => f0(t.Position?.[0]), numeric: true },
  { key: 'y', label: 'Y', get: (t) => f0(t.Position?.[1]), numeric: true },
  { key: 'z', label: 'Z', get: (t) => f0(t.Position?.[2]), numeric: true },
  { key: 'g', label: 'G', get: (t) => f0(t.Position?.[3]), numeric: true },
  { key: 'flags', label: '플래그', get: (t) => flagStr(t) },
]

/** PLC 작업 배열 표(비어 있는 항목은 뺀다). */
export function TaskTableMini({ list, empty = '없음' }: { list: readonly PlcTask[] | undefined; empty?: string }) {
  const rows = (list ?? []).filter((t) => t && (t.WorkId || t.TaskId))
  return <DataTable rows={rows} columns={TASK_COLS} rowKey={(t) => `${t.WorkId}-${t.TaskId}`} empty={empty} />
}
