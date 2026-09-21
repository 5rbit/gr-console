// 이력 — 측정 기록 표(최신순)와 고른 행의 상세(Data[0..19] + 명령).
//
// 통계 카드 여덟 개(Total · Count · 표시 · 종류 다섯)를 걷어 냈다. 누적·버퍼는 화면 머리 숫자 띠가
// 항상 말하고 있고, 종류별 건수는 집계 보기의 표에 같은 값이 더 자세히 있다. 남은 "표시 N / 전체 M"
// 하나만 표 머리줄로 옮겼다 — 필터 때문에 비었는지 정말 없는지는 그 자리에서 읽혀야 한다.
import { DATA_LABEL } from '../../lib/meas/const'
import { delta, pos, tt } from '../../lib/meas/format'
import { summary, type MeasRow } from '../../lib/meas/rows'
import { Card } from '../../lib/ui/Card'
import { DataTable } from '../../lib/ui/DataTable'
import { EmptyState } from '../../lib/ui/EmptyState'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import type { Column } from '../../lib/ui/table'
import { Section } from '../../lib/ui/Section'
import { KvTable, TaskKv } from './helpers'

/** 고른 기록의 `Data[0..19]` 한 줄 — 인덱스·라벨·값. */
interface DataRow {
  i: number
  label: string
  value: number
}

// 값은 **편차 규칙(2자리)** 을 따른다 — 같은 화면의 Delta 열이 2자리인데 여기만 3자리였다.
const DATA_COLS: Column<DataRow>[] = [
  { key: 'i', label: 'Idx', get: (r) => r.i, numeric: true, priority: 1 },
  { key: 'label', label: 'Label', get: (r) => r.label, priority: 1 },
  { key: 'v', label: 'Value (mm)', get: (r) => r.value, cell: (r) => delta(r.value), numeric: true, priority: 1 },
]

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
  { key: 'time', label: 'TimeStamp', get: (r) => r.time, priority: 1 },
  {
    key: 'kind',
    label: 'Kind',
    get: (r) => r.kindName,
    cell: (r) => <span className={KIND_TONE[r.kind] ?? ''}>{r.kindName}</span>,
    priority: 2,
  },
  {
    key: 'status',
    label: 'Status',
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
  { key: 'cmdId', label: 'Cmd.InnerDiameter (mm)', get: (r) => pos(r.cmdId), numeric: true, priority: 3 },
  { key: 'cmdH', label: 'Cmd.Height (mm)', get: (r) => pos(r.cmdHeight), numeric: true, priority: 3 },
  { key: 'cnt', label: 'Cmd.Count', get: (r) => r.cmdCount, numeric: true, priority: 3 },
  { key: 'zrel', label: 'CmdZRel (mm)', get: (r) => pos(r.cmdZRel), numeric: true, priority: 3 },
  { key: 'sum', label: 'Summary', get: (r) => summary(r), priority: 2 },
  { key: 'dId', label: 'Delta.InnerDia (mm)', get: (r) => delta(r.dInnerDia), numeric: true, priority: 3 },
  { key: 'dH', label: 'Delta.Height (mm)', get: (r) => delta(r.dHeight), numeric: true, priority: 3 },
  { key: 'dZ', label: 'Delta.Z (mm)', get: (r) => delta(r.dZ), numeric: true, priority: 3 },
  { key: 'dOff', label: 'Delta.Offset (mm)', get: (r) => delta(r.dOffset), numeric: true, priority: 3 },
]

export function History({
  rows,
  selected,
  onSelect,
  allCount,
  filtered,
}: {
  rows: MeasRow[]
  selected: number | null
  onSelect: (seq: number | null) => void
  allCount: number
  /** 필터가 걸려 있나 — 빈 목록이 필터 때문인지 정말 없는 것인지 구분해 말한다. */
  filtered: boolean
}) {
  const sel = rows.find((r) => r.seq === selected) ?? null
  const dataRows: DataRow[] = sel
    ? sel.data
        .map((v, i) => ({ i, label: DATA_LABEL[sel.kind]?.[i] || 'Reserved', value: Number(v) }))
        .filter((r) => r.value || r.label !== 'Reserved')
    : []
  return (
    // 도킹 존 안에서도 표가 **남는 높이를 먹는다** — 뷰포트 비율(`max-h-[62vh]`)로 잡아 두면
    // 존이 화면의 3분의 1일 때 표가 존 밖으로 넘치고, 존이 전체 화면일 때는 아래가 비었다.
    <div className="grid h-full min-h-0 gap-3 lg:grid-cols-[2fr_1fr]">
      {/* `min-w-0` — 표가 열 열여섯을 내용 폭으로 요구하면 격자 칸이 그만큼 벌어져 오른쪽 상세가
          화면 밖으로 밀려난다. 좁힐 수 있게 해 두면 표 안에서 가로 스크롤이 난다. */}
      <Card padded={false} className="flex min-h-0 min-w-0 flex-col">
        <div className="flex flex-none items-baseline gap-2 border-b border-line-default px-3 py-1.5">
          <h3 className="text-xs font-semibold text-content-muted">측정 기록</h3>
          <span className="ml-auto text-2xs text-content-faint tabular-nums">
            {rows.length} / {allCount}
          </span>
        </div>
        <DataTable
          rows={rows}
          columns={COLS}
          rowKey={(r) => String(r.seq)}
          selected={selected === null ? null : String(selected)}
          onPick={(r) => onSelect(r.seq)}
          density="compact"
          stickyHeader
          zebra
          className="min-h-0 flex-1 overflow-y-auto"
          empty={filtered ? '필터에 맞는 기록 없음' : '측정 기록 없음'}
          emptyHint={filtered ? 'Kind · Code 필터를 지우면 전체가 보입니다.' : undefined}
        />
      </Card>
      <Card className="min-h-0 overflow-auto">
        {sel ? (
          <>
            <Section
              title={`#${sel.seq} ${sel.kindName} ${sel.statusName}`}
              right={sel.time}
              first
            >
              <TaskKv t={sel.cmd} />
            </Section>
            <Section title="Data" right={`${dataRows.length} 줄`}>
              <DataTable
                rows={dataRows}
                columns={DATA_COLS}
                rowKey={(r) => String(r.i)}
                density="compact"
                empty="Data 없음"
              />
            </Section>
            <Section title="Delta">
              <KvTable
                labelWidth={116}
                rows={[
                  [
                    'InnerDia / Height (mm)',
                    `${delta(sel.dInnerDia)} / ${delta(sel.dHeight)}`,
                  ],
                  [
                    'Z / Offset (mm)',
                    `${delta(sel.dZ)} / ${delta(sel.dOffset)}`,
                  ],
                  ['Count', delta(sel.dCount)],
                ]}
              />
            </Section>
          </>
        ) : (
          <EmptyState title="기록을 고르세요" hint="왼쪽 표의 행을 누르면 명령·Data·Δ 가 여기 섭니다." />
        )}
      </Card>
    </div>
  )
}
