// 측정 모니터 공용 조각 — 비트 격자, 키/값 표, 추세 셀, PLC 작업 표.
//
// 소제목(`Section`)은 킷으로 올라갔다(`lib/ui/Section`) — 측정 말고도 같은 물건이 필요했고, 두 벌은
// 한쪽만 고쳐진다.
//
// 통계 카드 격자(`StatCards`)는 없앴다. 화면마다 아홉 개씩 서던 카드가 값 하나를 보더+면+그림자로
// 감싸 화면의 첫 3분의 1을 먹었고, 카드 안의 값들이 서로 다른 x에 서서 세로로 훑을 수 없었다.
// 그 자리는 화면 하나에 하나뿐인 머리 숫자 띠(`lib/ui/StatRow`)가 대신한다.
import type { ReactNode } from 'react'
import { KIND, STATUS } from '../../lib/gr/const'
import { delta, flagStr, pos, tt } from '../../lib/meas/format'
import { DataTable } from '../../lib/ui/DataTable'
import { FieldList } from '../../lib/ui/FieldList'
import { StatusDot } from '../../lib/ui/StatusDot'
import { statusTone } from '../../lib/ui/status'
import type { Column } from '../../lib/ui/table'
import type { PlcTask, Trend } from '../../lib/types'

export { KIND, STATUS }

/**
 * 불리언 비트 묶음 — **고정 격자**의 점+이름. 칩 구름이었을 때는 켜진 비트가 줄바꿈 자리에 따라
 * 매번 다른 x에 떠서 "무엇이 켜졌나"를 훑을 수 없었다. 자리를 고정하고 켜진 것만 색을 받으면
 * 꺼진 비트도 같은 자리에 흐리게 남아 "무엇이 꺼졌나"까지 읽힌다.
 */
export function Bits({
  obj,
  keys,
  bad = [],
  warn = [],
  inline = false,
}: {
  obj: Record<string, unknown> | null | undefined
  keys: readonly string[]
  bad?: readonly string[]
  warn?: readonly string[]
  /**
   * 표 셀 안의 한 줄 — 격자(7rem 열)는 폭이 내용에 맞춰진 칸에서 한 열로 접혀 행이 네 배로
   * 높아진다. 같은 열의 모든 행이 **같은 키 묶음**을 그리므로 한 줄로 펴도 열은 그대로 맞는다.
   */
  inline?: boolean
}) {
  if (!obj) return <span className="text-content-muted">-</span>
  return (
    <ul
      className={
        inline
          ? 'm-0 flex list-none items-center gap-x-3 p-0 whitespace-nowrap'
          : 'ds-bitgrid m-0 list-none p-0'
      }
    >
      {keys
        .filter((k) => k in obj)
        .map((k) => {
          const on = Boolean(obj[k])
          const tone = bad.includes(k) ? 'fault' : warn.includes(k) ? 'warn' : 'ok'
          return (
            <li key={k} className="flex items-center gap-1.5 text-2xs whitespace-nowrap">
              <StatusDot status={on ? tone : 'neutral'} size="sm" />
              <span className={on ? statusTone(tone).text : 'text-content-faint'}>{k}</span>
            </li>
          )
        })}
    </ul>
  )
}

/**
 * 라벨/값 짝 — 킷의 `FieldList` 한 줄 래퍼다.
 *
 * 전에는 제 `<table>` 을 그렸다. 같은 일을 하는 물건이 둘이면(킷의 인스펙터 + 이 표) 한쪽만
 * 고쳐지고, 실제로 값 열 정렬·빈 값 표시가 갈려 있었다(킷은 "없는 값"을 점선 원으로 그리는데
 * 이 표는 빈 칸으로 뒀다). 라벨 열 폭이 px 로 고정이라 여러 묶음의 값 열이 한 줄에 선다.
 */
export function KvTable({
  rows,
  labelWidth = 150,
  className = '',
}: {
  /** 라벨은 **짧게**(≤3 낱말) — 라벨 열은 줄바꿈하지 않는다. 설명은 `tooltip` 으로. */
  rows: readonly (readonly [string, ReactNode, string?])[]
  labelWidth?: number
  className?: string
}) {
  return (
    <FieldList
      className={className}
      columns={1}
      dense
      labelWidth={labelWidth}
      items={rows.map(([label, value, tooltip]) => ({ label, value, tooltip, mono: true }))}
    />
  )
}

/**
 * 추세 값 셀 — `평균 / EMA (최소~최대, n)` **한 줄**.
 *
 * 두 줄(`<br/>`)이었을 때는 이 셀이 든 행만 두 배로 높아져서, 옆의 한 줄짜리 숫자 열들이 행마다
 * 위아래로 흩어졌다. 세로로 훑는 눈은 자리가 맞는 것만 비교할 수 있다.
 */
export function TrendCell({ t }: { t: Trend | undefined | null }) {
  if (!t || !t.Count) return <span className="text-content-muted">-</span>
  return (
    <span title={`n=${t.Count}`} className="font-mono whitespace-nowrap tabular-nums">
      {delta(t.Avg)} / {delta(t.Ema)}{' '}
      <span className="text-content-muted">
        ({delta(t.Min)}~{delta(t.Max)}, {t.Count})
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
        [
          'Type',
          `${tt(t.TaskType)} (0x${Number(t.TaskType ?? 0)
            .toString(16)
            .toUpperCase()
            .padStart(2, '0')})`,
        ],
        ['Cell', `${c.Id ?? ''}  (Sec ${c.Section ?? ''} Row ${c.Row ?? ''} Col ${c.Col ?? ''})`],
        [
          'Item',
          `Code ${it.Code ?? ''} / Count ${it.Count ?? ''} / ID ${pos(it.InnerDiameter)} / OD ${pos(it.OuterDiameter)} / H ${pos(it.Height)}`,
          'Code / Count / InnerDiameter / OuterDiameter / Height (mm)',
        ],
        ['Position (mm)', `X ${pos(p[0])} Y ${pos(p[1])} Z ${pos(p[2])} G ${pos(p[3])}`],
        [
          'Cell.Position (mm)',
          `X ${pos(cp[0])} Y ${pos(cp[1])} Z ${pos(cp[2])}  (CmdZRel ${pos((p[2] ?? 0) - (cp[2] ?? 0))})`,
        ],
        ['Flags', flagStr(t)],
        [
          'Grip',
          `H ${t.GripHeight ?? ''} / PreGrip ${t.PreGripDelta ?? ''} / GripBack ${t.GripBackDelta ?? ''}`,
        ],
        [
          'Lift',
          `Up ${t.LiftUpHeight ?? ''} / Creep ↑${t.LiftUpCreepDistance ?? ''} ↓${t.LiftDownCreepDistance ?? ''} / Partial ${t.LiftUpPartial ? 'Y' : 'N'}`,
        ],
        ['Blend', `↑${t.BlendUpDistance ?? ''} ↓${t.BlendDownDistance ?? ''}`],
        [
          'Drag',
          `Out ${t.UseDragOut ? `${t.DragOutHeight}/${t.DragOutDist}/dir${t.DragOutDir}` : '-'}  In ${t.UseDragIn ? `${t.DragInHeight}/${t.DragInDist}/dir${t.DragInDir}` : '-'}`,
        ],
      ]}
    />
  )
}

/** 표 한 줄 = PLC 작업 하나. `group`은 그 작업이 어느 배열(대기열·완료·취소·거부)에서 왔는지다. */
export interface TaskRow {
  group: string
  task: PlcTask
}

// `priority` — PLC Task 표는 **어느 작업인가**로 훑는다: 구분·Work/Task가 1, 종류·Cell·Code가 2,
// 단·치수·좌표·플래그가 3이다(`docs/DESIGN.md` 4절).
const TASK_COLS: Column<TaskRow>[] = [
  { key: 'g', label: '구분', get: (r) => r.group, priority: 1 },
  { key: 'w', label: 'Work', get: (r) => r.task.WorkId, numeric: true, priority: 1 },
  { key: 't', label: 'Task', get: (r) => r.task.TaskId, numeric: true, priority: 1 },
  { key: 'ty', label: 'Type', get: (r) => tt(r.task.TaskType), priority: 2 },
  { key: 'cell', label: 'Cell', get: (r) => r.task.Cell?.Id, numeric: true, priority: 2 },
  { key: 'code', label: 'Code', get: (r) => r.task.Item?.Code, numeric: true, priority: 2 },
  { key: 'cnt', label: 'Count', get: (r) => r.task.Item?.Count, numeric: true, priority: 3 },
  {
    key: 'id',
    label: 'ID (mm)',
    get: (r) => pos(r.task.Item?.InnerDiameter),
    numeric: true,
    priority: 3,
  },
  { key: 'h', label: 'H (mm)', get: (r) => pos(r.task.Item?.Height), numeric: true, priority: 3 },
  { key: 'x', label: 'X (mm)', get: (r) => pos(r.task.Position?.[0]), numeric: true, priority: 3 },
  { key: 'y', label: 'Y (mm)', get: (r) => pos(r.task.Position?.[1]), numeric: true, priority: 3 },
  { key: 'z', label: 'Z (mm)', get: (r) => pos(r.task.Position?.[2]), numeric: true, priority: 3 },
  { key: 'g4', label: 'G (mm)', get: (r) => pos(r.task.Position?.[3]), numeric: true, priority: 3 },
  { key: 'flags', label: 'Flags', get: (r) => flagStr(r.task), priority: 3 },
]

/**
 * PLC 작업 배열들을 **한 표**로 — 대기열·완료·취소·거부가 열이 같은 표 넷이었다.
 *
 * 표가 넷이면 같은 머리글을 네 번 읽고, 어느 표가 비었는지 세로로 훑어야 한다. 구분 열 하나를
 * 앞에 세우면 정렬로 묶어 볼 수 있고 빈 배열은 자리를 먹지 않는다.
 */
export function TaskTables({
  groups,
  empty = '작업 없음',
}: {
  groups: readonly { label: string; list: readonly PlcTask[] | undefined }[]
  empty?: string
}) {
  const rows: TaskRow[] = []
  for (const g of groups) {
    for (const t of g.list ?? []) {
      if (t && (t.WorkId || t.TaskId)) rows.push({ group: g.label, task: t })
    }
  }
  return (
    <DataTable
      rows={rows}
      columns={TASK_COLS}
      rowKey={(r) => `${r.group}-${r.task.WorkId}-${r.task.TaskId}`}
      density="compact"
      stickyHeader
      empty={empty}
    />
  )
}
