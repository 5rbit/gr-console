// Task 상세 — 사이드바 드로어(`panels.open({mode:'sidebar'})`)에 뜬다. 스토어에 있으면 SSE로 살아
// 움직이고, 없으면(오래된 종결 건) `GET /api/tasks/{id}`로 한 번 받는다.
//
// 섹션: 조작 · 개요 · 응답(Ack) · 헤더 · PLC 위치 · 요청 · 적용 파라미터 · PLC Task · 이력 · 원본 JSON.
import { useEffect, useMemo, useState } from 'react'
import { Braces } from 'lucide-react'
import { api } from '../../lib/api'
import { statusFeed } from '../../lib/feeds'
import { PARAM_LABELS } from '../../lib/gr/const'
import { panels } from '../../lib/panels'
import { visibleInterval } from '../../lib/poll'
import { useStore } from '../../lib/store'
import { tasks } from '../../lib/tasks'
import { Button } from '../../lib/ui/Button'
import { EmptyState } from '../../lib/ui/EmptyState'
import { FieldList, type FieldItem } from '../../lib/ui/FieldList'
import { JsonView } from '../../lib/ui/JsonView'
import { Skeleton } from '../../lib/ui/Skeleton'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import { StatusDot } from '../../lib/ui/StatusDot'
import {
  ORIGIN_LABEL,
  STATE_LABEL,
  STATE_TONE,
  deriveState,
  elapsed,
  fmtElapsed,
  fmtTime,
  targetOf,
  typeName,
  type PlcTaskArea,
  type TaskEx,
} from '../../lib/task/state'
import type { TaskParams, TaskTransition } from '../../lib/types'
import { TaskActions } from './TaskActions'

export const TASK_DETAIL_PANEL = 'task-detail'

export interface TaskDetailProps {
  id: string
}

function Section({
  title,
  children,
  trailing,
}: {
  title: string
  children: React.ReactNode
  trailing?: React.ReactNode
}) {
  return (
    <section className="space-y-1.5">
      <h4 className="m-0 flex items-center text-2xs font-semibold tracking-wide text-content-muted uppercase">
        {title}
        <span className="flex-1" />
        {trailing}
      </h4>
      {children}
    </section>
  )
}

const ACTOR_LABEL: Record<TaskTransition['by'], string> = {
  ui: '콘솔',
  plc: 'PLC',
  scenario: '시나리오',
  system: '시스템',
}

function Timeline({ history, now }: { history: TaskTransition[]; now: number }) {
  if (history.length === 0) return <p className="m-0 text-xs text-content-faint">이력 없음</p>
  return (
    <ol className="m-0 list-none space-y-1 p-0" data-testid="task-timeline">
      {history.map((h, i) => (
        <li key={i} className="flex items-baseline gap-2 text-xs">
          <span
            className="w-20 shrink-0 font-mono text-2xs text-content-faint tabular-nums"
            title={h.at}
          >
            {fmtTime(h.at, now)}
          </span>
          {/* 고정폭 — 상태 글자 길이가 달라도 행위자·메모의 x가 줄마다 같다. */}
          <span className="w-14 shrink-0">
            <StatusDot status={STATE_TONE[h.to]} size="sm" label={STATE_LABEL[h.to]} />
          </span>
          <span className="text-2xs text-content-faint">{ACTOR_LABEL[h.by]}</span>
          {h.note ? (
            <span className="min-w-0 truncate text-content-tertiary" title={h.note}>
              {h.note}
            </span>
          ) : null}
        </li>
      ))}
    </ol>
  )
}

/** 숫자를 0x.. 로 — 값이 없으면(구버전 원장·외부 Task) 화면 전체가 죽지 않도록 '-'. */
function hex(v: number | null | undefined): string {
  return typeof v === 'number' ? `0x${v.toString(16).toUpperCase()}` : '-'
}

function paramItems(p: TaskParams | null, overrides?: Partial<TaskParams>): FieldItem[] {
  if (!p) return []
  return (Object.keys(PARAM_LABELS) as (keyof TaskParams)[]).map((k) => {
    const v = p[k]
    const over = overrides !== undefined && k in overrides
    return {
      label: PARAM_LABELS[k],
      value: typeof v === 'boolean' ? (v ? 'ON' : 'OFF') : v,
      status: over ? 'info' : undefined,
      tooltip: over ? `${k} — 요청에서 덮어씀` : k,
    }
  })
}

export default function TaskDetail({ id }: TaskDetailProps) {
  useStore(tasks)
  const live = tasks.get(id) as TaskEx | null
  const [fetched, setFetched] = useState<TaskEx | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [showJson, setShowJson] = useState(false)
  const [now, setNow] = useState(() => Date.now())
  const [area, setArea] = useState<PlcTaskArea | null>(
    () => statusFeed.data?.webmon.Stat.Task ?? null,
  )

  useEffect(() => {
    if (live) return
    let alive = true
    api
      .task(id)
      .then((t) => {
        if (alive) setFetched(t as TaskEx)
      })
      .catch((e: unknown) => {
        if (alive) setError(e instanceof Error ? e.message : String(e))
      })
    return () => {
      alive = false
    }
  }, [id, live])

  useEffect(() => {
    const t = visibleInterval(() => {
      setNow(Date.now())
      setArea(statusFeed.data?.webmon.Stat.Task ?? null)
    }, 1000)
    return () => clearInterval(t)
  }, [])

  const t = live ?? fetched
  const derived = useMemo(() => (t ? deriveState(t, area) : null), [t, area])

  if (!t) {
    if (error) return <EmptyState title="Task를 찾을 수 없음" hint={error} />
    return (
      <div className="space-y-2">
        <Skeleton className="h-5 w-40" />
        <Skeleton className="h-3 w-full" />
        <Skeleton className="h-3 w-4/5" />
      </div>
    )
  }

  const target = targetOf(t)
  const overview: FieldItem[] = [
    { label: 'Seq', value: t.seq, mono: true },
    { label: '출처', value: ORIGIN_LABEL[t.origin] },
    { label: 'WorkId', value: t.work_id, mono: true },
    { label: 'TaskId', value: t.task_id, mono: true },
    { label: '종류', value: typeName(t.plc_task?.TaskType) },
    {
      label: '대상',
      value: target ? `${target.kind === 'station' ? '스테이션' : '셀'} ${target.id}` : null,
      missing: '대상 없음(MOVE 또는 외부 Task)',
    },
    { label: '품목', value: t.plc_task?.Item?.Code || null },
    { label: '수량', value: t.plc_task?.Item?.Count ?? null },
    { label: 'PLC', value: t.plc_name ?? null },
    {
      label: '위치',
      value: t.position.map((v) => v.toFixed(1)).join(' / '),
      tooltip: 'X / Y / Z / G',
    },
    { label: '생성', value: fmtTime(t.created_at, now), tooltip: t.created_at },
    {
      label: '제출',
      value: t.submitted_at ? fmtTime(t.submitted_at, now) : null,
      tooltip: t.submitted_at ?? undefined,
      missing: '아직 제출되지 않음',
    },
    {
      label: '종결',
      value: t.ended_at ? fmtTime(t.ended_at, now) : null,
      tooltip: t.ended_at ?? undefined,
      missing: '아직 진행 중',
    },
    { label: '경과', value: fmtElapsed(elapsed(t, now)) },
    {
      label: '오류',
      value: t.error,
      status: t.error ? 'fault' : undefined,
      wide: true,
      missing: '오류 없음',
    },
  ]
  const ack: FieldItem[] = t.ack
    ? [
        {
          label: '판정',
          value: t.ack.accepted ? '수락' : '거부',
          status: t.ack.accepted ? 'ok' : 'fault',
        },
        { label: '검증 코드', value: t.ack.code, mono: true },
        {
          label: '거부 비트',
          value:
            typeof t.ack.reject_bits === 'number'
              ? `0b${t.ack.reject_bits.toString(2).padStart(8, '0')}`
              : null,
          mono: true,
          tooltip: 'X0 로봇번호 · X1 중복 · X2 태스크 무효 · X3 버퍼 풀 · X7 오프라인',
        },
        { label: '시각', value: fmtTime(t.ack.at, now), tooltip: t.ack.at },
        { label: '사유', value: t.ack.reason, wide: true },
      ]
    : []
  const header: FieldItem[] = t.header
    ? [
        { label: 'CMD_ID', value: t.header.CMD_ID, mono: true },
        { label: 'SEQ', value: t.header.SEQ, mono: true },
        { label: 'CMD', value: hex(t.header.CMD), mono: true },
        {
          label: 'SRC → DST',
          value: `${t.header.SRC ?? '-'} → ${t.header.DST ?? '-'}`,
          mono: true,
        },
      ]
    : []
  const plc: FieldItem[] = [
    {
      label: 'PLC 자리',
      value: derived
        ? derived.location === 'absent'
          ? '없음'
          : `${derived.location}${derived.index !== null && derived.location !== 'now' ? `[${derived.index}]` : ''}`
        : null,
      status: derived?.mismatch ? 'warn' : undefined,
      tooltip: derived?.reason ?? undefined,
      missing: '상태 스트림 없음',
    },
    { label: '스텝', value: t.plc?.step ?? null, mono: true, missing: 'PLC에서 아직 못 봄' },
    {
      label: '큐 슬롯',
      value: t.plc?.queue_index ?? null,
      mono: true,
      missing: 'PLC에서 아직 못 봄',
    },
    {
      label: '마지막 관측',
      value: t.plc ? fmtTime(t.plc.last_seen_at, now) : null,
      tooltip: t.plc?.last_seen_at,
      missing: 'PLC에서 아직 못 봄',
    },
  ]
  const req = t.request
  const request: FieldItem[] = req
    ? [
        { label: '종류', value: req.type },
        {
          label: '대상',
          value: req.target ? `${req.target.kind} ${req.target.id}` : null,
          missing: '대상 없음',
        },
        { label: '품목', value: req.item_code ?? null, missing: '품목 없음' },
        { label: '수량', value: req.count },
        {
          label: '위치 지정',
          value: req.position_override ? req.position_override.join(' / ') : null,
          missing: '레지스트리 위치 사용',
        },
        {
          label: '덮어쓴 파라미터',
          value: Object.keys(req.params).length ? Object.keys(req.params).join(', ') : null,
          wide: true,
          missing: '기본값 그대로',
        },
        { label: '메모', value: req.note || null, wide: true, missing: '메모 없음' },
        ...(req.source
          ? [
              {
                label: '시나리오',
                value: `${req.source.scenario_id} · run ${req.source.run_id} · ${req.source.iteration}회차 스텝 ${req.source.step_index}`,
                wide: true,
              },
            ]
          : []),
      ]
    : []
  const pt = t.plc_task
  const plcTask: FieldItem[] = pt
    ? [
        {
          label: 'TaskType',
          value: `${hex(pt.TaskType)} ${typeName(pt.TaskType)}`,
          mono: true,
        },
        { label: 'Cell.Id', value: pt.Cell?.Id, mono: true },
        {
          label: 'Cell 구역',
          value: pt.Cell ? `S${pt.Cell.Section} R${pt.Cell.Row} C${pt.Cell.Col}` : null,
        },
        { label: 'Cell 위치', value: pt.Cell?.Position?.map((v) => v.toFixed(0)).join(' / ') },
        { label: 'Item', value: pt.Item ? `${pt.Item.Code} × ${pt.Item.Count}` : null },
        {
          label: 'Item 치수',
          value: pt.Item
            ? `ID ${pt.Item.InnerDiameter} · OD ${pt.Item.OuterDiameter} · H ${pt.Item.Height}`
            : null,
        },
      ]
    : []

  return (
    <div className="space-y-4 text-sm" data-testid="task-detail">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-base font-semibold tabular-nums">Task #{t.seq}</span>
        <StatusBadge status={STATE_TONE[t.state]} data-testid="detail-state">
          {STATE_LABEL[t.state]}
        </StatusBadge>
        {derived?.mismatch ? (
          <StatusDot status="warn" label="PLC와 불일치" title={derived.reason ?? undefined} />
        ) : null}
        <span className="text-xs text-content-faint">{ORIGIN_LABEL[t.origin]}</span>
      </div>

      <TaskActions
        task={t}
        onDone={(a, ok) => {
          if (a === 'delete' && ok) panels.close(TASK_DETAIL_PANEL)
        }}
      />

      <Section title="개요">
        <FieldList items={overview} columns={2} dense labelWidth={72} />
      </Section>

      <Section title="응답 (Ack)">
        {ack.length ? (
          <FieldList items={ack} columns={2} dense labelWidth={72} />
        ) : (
          <p className="m-0 text-xs text-content-faint">아직 PLC 응답 없음</p>
        )}
      </Section>

      {header.length ? (
        <Section title="명령 헤더">
          <FieldList items={header} columns={2} dense labelWidth={72} />
        </Section>
      ) : null}

      <Section title="PLC 위치">
        <FieldList items={plc} columns={2} dense labelWidth={72} />
      </Section>

      <Section title="요청">
        {request.length ? (
          <FieldList items={request} columns={2} dense labelWidth={72} />
        ) : (
          <p className="m-0 text-xs text-content-faint">
            콘솔 요청 없음 — PLC에서 처음 본 외부 Task
          </p>
        )}
      </Section>

      <Section title="적용 파라미터">
        {t.resolved ? (
          <FieldList
            items={paramItems(t.resolved, req?.params)}
            columns={2}
            dense
            labelWidth={96}
          />
        ) : (
          <p className="m-0 text-xs text-content-faint">없음</p>
        )}
      </Section>

      <Section title="PLC Task">
        {plcTask.length ? (
          <FieldList items={plcTask} columns={2} dense labelWidth={72} />
        ) : (
          <p className="m-0 text-xs text-content-faint">없음</p>
        )}
      </Section>

      <Section title="이력">
        <Timeline history={t.history} now={now} />
      </Section>

      <Section
        title="원본 JSON"
        trailing={
          <Button
            size="sm"
            intent="ghost"
            icon={<Braces size={12} />}
            onClick={() => setShowJson(!showJson)}
            aria-expanded={showJson}
          >
            {showJson ? '접기' : '펼치기'}
          </Button>
        }
      >
        {showJson ? (
          <JsonView
            value={t}
            rootLabel="task"
            defaultDepth={1}
            height={360}
            highlightChanges={false}
          />
        ) : null}
      </Section>
    </div>
  )
}
