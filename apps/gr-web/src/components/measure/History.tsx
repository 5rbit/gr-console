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

// 측정 종류 다섯을 **구분**하는 색 — 상태(판정)가 아니라 종류 축이라 상태 여섯의 fg 토큰을 빌려
// 쓰되 뜻(ok·warn)으로 읽히지 않게 dot이 아닌 fg만 쓴다. 다크 대비는 토큰 층이 맡는다.
const KIND_TONE: Record<number, string> = {
  1: 'text-info-fg',
  2: 'text-pending-fg',
  3: 'text-content-muted',
  4: 'text-degraded-fg',
  5: 'text-ok-fg',
}

// `priority` — 열 열여섯인 표라 좁은 존에서는 가로 스크롤이 아니라 접기로 간다. 측정 이력에서
// 먼저 읽는 것은 **언제·무엇이 어떻게 나왔나**다: Seq·시각·상태가 1, 종류·Work/Task·Cell·Code가 2,
// 명령 값과 Δ들이 3이다(접힌 열은 행을 펼치면 라벨+값 짝으로 나온다).
const COLS: Column<MeasRow>[] = [
  { key: 'seq', label: 'Seq', get: (r) => r.seq, numeric: true, priority: 1 },
  { key: 'time', label: '시각', get: (r) => r.time, priority: 1 },
  {
    key: 'kind',
    label: '종류',
    get: (r) => r.kindName,
    cell: (r) => <span className={KIND_TONE[r.kind] ?? ''}>{r.kindName}</span>,
    priority: 2,
  },
  {
    key: 'status',
    label: '상태',
    priority: 1,
    get: (r) => r.statusName,
    cell: (r) => (
      <StatusBadge
        status={
          r.status === 2 ? 'ok' : r.status === 3 ? 'fault' : r.status === 4 ? 'warn' : 'neutral'
        }
      >
        {r.statusName}
      </StatusBadge>
    ),
  },
  {
    key: 'wt',
    label: 'Work/Task',
    get: (r) => `${r.workId}/${r.taskId} ${tt(r.taskType)}`,
    priority: 2,
  },
  { key: 'cell', label: 'Cell', get: (r) => r.cellId, numeric: true, priority: 2 },
  { key: 'code', label: 'Code', get: (r) => r.code, numeric: true, priority: 2 },
  { key: 'cmdId', label: '명령 ID', get: (r) => f1(r.cmdId), numeric: true, priority: 3 },
  { key: 'cmdH', label: '명령 H', get: (r) => f1(r.cmdHeight), numeric: true, priority: 3 },
  { key: 'cnt', label: '단', get: (r) => r.cmdCount, numeric: true, priority: 3 },
  { key: 'zrel', label: '명령 Z(rel)', get: (r) => f1(r.cmdZRel), numeric: true, priority: 3 },
  { key: 'sum', label: '측정 요약', get: (r) => summary(r), priority: 2 },
  { key: 'dId', label: 'Δ내경', get: (r) => f2(r.dInnerDia), numeric: true, priority: 3 },
  { key: 'dH', label: 'Δ높이', get: (r) => f2(r.dHeight), numeric: true, priority: 3 },
  { key: 'dZ', label: 'ΔZ', get: (r) => f2(r.dZ), numeric: true, priority: 3 },
  { key: 'dOff', label: '편심', get: (r) => f2(r.dOffset), numeric: true, priority: 3 },
]

export function History({
  rows,
  snap,
  selected,
  onSelect,
  allCount,
}: {
  rows: MeasRow[]
  snap: MeasLogSnapshot | null
  selected: number | null
  onSelect: (seq: number | null) => void
  allCount: number
}) {
  const sel = rows.find((r) => r.seq === selected) ?? null
  const cards = [
    { label: 'Total', value: snap?.total ?? '', hint: '누적 기록' },
    { label: 'Count', value: snap?.count ?? '', hint: `버퍼 ${snap?.capacity ?? 200}` },
    { label: '표시', value: rows.length, hint: `필터 적용 / 전체 ${allCount}` },
    ...(snap?.stat ?? []).map((s, i) => ({
      label: KIND[i + 1] ?? String(i + 1),
      value: s.Count,
      hint: `에러 ${s.ErrorCount}`,
    })),
  ]
  return (
    <div>
      <StatCards items={cards} />
      <div className="grid gap-3 lg:grid-cols-[2fr_1fr]">
        <div className="max-h-[62vh] overflow-auto rounded border border-line-default">
          <DataTable
            rows={rows}
            columns={COLS}
            rowKey={(r) => String(r.seq)}
            selected={selected === null ? null : String(selected)}
            onPick={(r) => onSelect(r.seq)}
            empty="측정 기록 없음"
          />
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
                        <td>{lab || '예비'}</td>
                        <td className="text-right font-mono tabular-nums">
                          {Number(v).toFixed(3)}
                        </td>
                      </tr>
                    )
                  })}
                  <tr className="border-t border-line-default">
                    <td colSpan={2}>편차 내경 / 높이 / Z / 편심 / 단수</td>
                    <td className="text-right font-mono tabular-nums">
                      {f2(sel.dInnerDia)} / {f2(sel.dHeight)} / {f2(sel.dZ)} / {f2(sel.dOffset)} /{' '}
                      {f2(sel.dCount)}
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
