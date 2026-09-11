// 이력 탭 — 필터, 카드, 표(최신순), 선택 행 상세(Data[0..19] + 명령).
import { KIND } from '../../lib/gr/const'
import { DATA_LABEL } from '../../lib/meas/const'
import { f1, f2, tt } from '../../lib/meas/format'
import { summary, type MeasRow } from '../../lib/meas/rows'
import { DataTable } from '../../lib/ui/DataTable'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import type { Column } from '../../lib/ui/table'
import type { MeasLogSnapshot } from '../../lib/types'
import { StatCards, TaskKv } from './helpers'

const KIND_TONE: Record<number, string> = { 1: 'text-sky-600 dark:text-sky-400', 2: 'text-violet-600 dark:text-violet-400', 3: 'text-slate-500', 4: 'text-orange-600 dark:text-orange-400', 5: 'text-teal-600 dark:text-teal-400' }

const COLS: Column<MeasRow>[] = [
  { key: 'seq', label: 'Seq', get: (r) => r.seq, numeric: true },
  { key: 'time', label: '시각', get: (r) => r.time },
  { key: 'kind', label: '종류', get: (r) => r.kindName, cell: (r) => <span className={KIND_TONE[r.kind] ?? ''}>{r.kindName}</span> },
  { key: 'status', label: '상태', get: (r) => r.statusName, cell: (r) => <StatusBadge status={r.status === 2 ? 'ok' : r.status === 3 ? 'fault' : r.status === 4 ? 'warn' : 'neutral'}>{r.statusName}</StatusBadge> },
  { key: 'wt', label: 'Work/Task', get: (r) => `${r.workId}/${r.taskId} ${tt(r.taskType)}` },
  { key: 'cell', label: 'Cell', get: (r) => r.cellId, numeric: true },
  { key: 'code', label: 'Code', get: (r) => r.code, numeric: true },
  { key: 'cmdId', label: '명령 ID', get: (r) => f1(r.cmdId), numeric: true },
  { key: 'cmdH', label: '명령 H', get: (r) => f1(r.cmdHeight), numeric: true },
  { key: 'cnt', label: '단', get: (r) => r.cmdCount, numeric: true },
  { key: 'zrel', label: '명령 Z(rel)', get: (r) => f1(r.cmdZRel), numeric: true },
  { key: 'sum', label: '측정 요약', get: (r) => summary(r) },
  { key: 'dId', label: 'Δ내경', get: (r) => f2(r.dInnerDia), numeric: true },
  { key: 'dH', label: 'Δ높이', get: (r) => f2(r.dHeight), numeric: true },
  { key: 'dZ', label: 'ΔZ', get: (r) => f2(r.dZ), numeric: true },
  { key: 'dOff', label: '편심', get: (r) => f2(r.dOffset), numeric: true },
]

export function History({ rows, snap, selected, onSelect, allCount }: { rows: MeasRow[]; snap: MeasLogSnapshot | null; selected: number | null; onSelect: (seq: number | null) => void; allCount: number }) {
  const sel = rows.find((r) => r.seq === selected) ?? null
  const cards = [
    { label: 'Total', value: snap?.total ?? '', hint: '누적 기록' },
    { label: 'Count', value: snap?.count ?? '', hint: `버퍼 ${snap?.capacity ?? 200}` },
    { label: '표시', value: rows.length, hint: `필터 적용 / 전체 ${allCount}` },
    ...(snap?.stat ?? []).map((s, i) => ({ label: KIND[i + 1] ?? String(i + 1), value: s.Count, hint: `에러 ${s.ErrorCount}` })),
  ]
  return (
    <div>
      <StatCards items={cards} />
      <div className="grid gap-3 lg:grid-cols-[2fr_1fr]">
        <div className="max-h-[62vh] overflow-auto rounded border border-line-default">
          <DataTable rows={rows} columns={COLS} rowKey={(r) => String(r.seq)} selected={selected === null ? null : String(selected)} onPick={(r) => onSelect(r.seq)} empty="측정 기록 없음" />
        </div>
        <div className="text-xs">
          {sel ? (
            <div className="space-y-2">
              <div>
                <b>
                  #{sel.seq} {sel.kindName} {sel.statusName}
                </b>{' '}
                <span className="text-content-muted">{sel.time}</span>
              </div>
              <TaskKv t={sel.cmd} />
              <table className="w-full">
                <thead>
                  <tr className="text-content-muted">
                    <th className="text-left">Idx</th>
                    <th className="text-left">항목</th>
                    <th className="text-right">값</th>
                  </tr>
                </thead>
                <tbody>
                  {sel.data.map((v, i) => {
                    const lab = DATA_LABEL[sel.kind]?.[i]
                    if (!v && !lab) return null
                    return (
                      <tr key={i} className="border-t border-line-default">
                        <td>{i}</td>
                        <td>{lab ?? '예비'}</td>
                        <td className="text-right font-mono tabular-nums">{Number(v).toFixed(3)}</td>
                      </tr>
                    )
                  })}
                  <tr className="border-t border-line-default">
                    <td colSpan={2}>편차 내경 / 높이 / Z / 편심 / 단수</td>
                    <td className="text-right font-mono tabular-nums">
                      {f2(sel.dInnerDia)} / {f2(sel.dHeight)} / {f2(sel.dZ)} / {f2(sel.dOffset)} / {f2(sel.dCount)}
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
          ) : (
            <span className="text-content-muted">행을 클릭하면 상세 데이터가 표시됩니다.</span>
          )}
        </div>
      </div>
    </div>
  )
}
