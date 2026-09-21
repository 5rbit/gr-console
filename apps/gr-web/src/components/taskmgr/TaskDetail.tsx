// Task 상세 — 사이드바 시트(`panels.open({mode:'sidebar'})`). 행을 누르면 열리고 Escape로 닫힌다.
// 스토어에 있으면 SSE로 살아 움직이고, 없으면(오래된 종결 건) `GET /api/tasks/{id}`로 한 번 받는다.
//
// **탭 넷**이다: 개요(무엇이 어디까지) · 요청(무엇을 시켰나) · PLC(PLC가 뭐라 하나) · JSON(원본).
// 예전에는 열 개 섹션이 한 열에 쌓여 있어서 Ack 하나를 보려고 스크롤을 두 번 굴렸다. 탭으로 가르면
// 한 화면에 한 가지만 서고, 여는 사람이 "무엇을 보러 왔는지"를 먼저 고른다.
//
// `←`/`→`는 **표의 순서대로** 앞뒤 Task로 옮긴다 — 시트를 닫고 다음 행을 찾아 누르는 왕복을 없앤다.
import { useEffect, useMemo, useState } from 'react'
import { ChevronLeft, ChevronRight } from 'lucide-react'
import { api } from '../../lib/api'
import { allStatus } from '../../lib/feeds'
import { PARAM_LABELS } from '../../lib/gr/const'
import { pos } from '../../lib/meas/format'
import { panels } from '../../lib/panels'
import { visibleInterval } from '../../lib/poll'
import { robots } from '../../lib/robots'
import { useStore } from '../../lib/store'
import { tasks } from '../../lib/tasks'
import { Button } from '../../lib/ui/Button'
import { EmptyState } from '../../lib/ui/EmptyState'
import { FieldList, type FieldItem } from '../../lib/ui/FieldList'
import { JsonView } from '../../lib/ui/JsonView'
import { Segmented } from '../../lib/ui/Segmented'
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
import type { StationOffsetAudit } from '../../lib/task/types'
import type { TaskParams, TaskTransition } from '../../lib/types'
import { StationOffsetBlock } from '../task/StationOffsetBlock'
import { PlcView } from './PlcView'
import { TaskActions } from './TaskActions'

export const TASK_DETAIL_PANEL = 'task-detail'

type Tab = 'overview' | 'request' | 'plc' | 'json'

export interface TaskDetailProps {
  id: string
  /** 표에 보이는 순서 — `←`/`→`가 이 순서로 옮긴다. */
  ids?: readonly string[]
  /** 다른 Task로 옮겨 달라는 요청(부모가 시트를 다시 연다 — 제목도 같이 바뀐다). */
  onNavigate?: (id: string) => void
}

function Section({
  title,
  children,
}: {
  title: string
  children: React.ReactNode
}) {
  return (
    <section className="space-y-1.5">
      <h4 className="m-0 flex items-center text-2xs font-semibold tracking-wide text-content-muted uppercase">
        {title}
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

export default function TaskDetail({ id, ids = [], onNavigate }: TaskDetailProps) {
  useStore(tasks, robots)
  const live = tasks.get(id) as TaskEx | null
  const [fetched, setFetched] = useState<TaskEx | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [tab, setTab] = useState<Tab>('overview')
  const [now, setNow] = useState(() => Date.now())
  // 대조는 **이 Task 의 로봇** PLC 배열과 한다(다른 호기 배열과 비교하면 늘 불일치).
  const [area, setArea] = useState<PlcTaskArea | null>(
    () => allStatus.ofPlc(tasks.get(id)?.plc_name)?.webmon.Stat.Task ?? null,
  )
  useEffect(() => allStatus.start(), [])
  useEffect(() => robots.start(), [])

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
      setArea(allStatus.ofPlc((tasks.get(id) ?? fetched)?.plc_name)?.webmon.Stat.Task ?? null)
    }, 1000)
    return () => clearInterval(t)
  }, [id, fetched])

  const at = ids.indexOf(id)
  const go = (dir: -1 | 1) => {
    if (at < 0 || !onNavigate) return
    const next = ids[at + dir]
    if (next) onNavigate(next)
  }

  // `←`/`→` — 글자를 넣는 중이면(사유 입력 따위) 가로채지 않는다.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return
      const el = document.activeElement
      const tag = el instanceof HTMLElement ? el.tagName : ''
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return
      // 확인 대화상자가 그 위에 떠 있으면(시트도 모달이라 둘이 된다) 화살표는 그쪽 것이다.
      if (document.querySelectorAll('[aria-modal="true"]').length > 1) return
      if (at < 0 || !onNavigate) return
      const next = ids[at + (e.key === 'ArrowLeft' ? -1 : 1)]
      if (!next) return
      e.preventDefault()
      onNavigate(next)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [at, ids, onNavigate])

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
  const robotId = robots.list.find((r) => r.plc === t.plc_name)?.id ?? null
  const overview: FieldItem[] = [
    { label: 'Seq', value: t.seq, mono: true },
    { label: 'Origin', value: ORIGIN_LABEL[t.origin] },
    { label: 'WorkId', value: t.work_id, mono: true },
    { label: 'TaskId', value: t.task_id, mono: true },
    { label: 'TaskType', value: typeName(t.plc_task?.TaskType) },
    {
      label: 'Target',
      value: target ? `${target.kind === 'station' ? 'Station' : 'Cell'} ${target.id}` : null,
      missing: '대상 없음(MOVE 또는 외부 Task)',
    },
    { label: 'Item.Code', value: t.plc_task?.Item?.Code || null },
    { label: 'Item.Count', value: t.plc_task?.Item?.Count ?? null },
    { label: 'PLC', value: t.plc_name ?? null },
    {
      label: 'Position',
      // 위치는 **한 벌의 규칙**으로 — `lib/meas/format.pos`(1자리). 같은 판넬의 Cell.Position 이
      // 0자리로 서 있어서 같은 축의 두 값이 다른 정밀도로 읽혔다.
      value: t.position.map((v) => pos(v)).join(' / '),
      tooltip: 'X / Y / Z / G (mm)',
    },
    { label: 'CreatedAt', value: fmtTime(t.created_at, now), tooltip: t.created_at },
    {
      label: 'SubmittedAt',
      value: t.submitted_at ? fmtTime(t.submitted_at, now) : null,
      tooltip: t.submitted_at ?? undefined,
      missing: '아직 제출되지 않음',
    },
    {
      label: 'EndedAt',
      value: t.ended_at ? fmtTime(t.ended_at, now) : null,
      tooltip: t.ended_at ?? undefined,
      missing: '아직 진행 중',
    },
    { label: 'Elapsed', value: fmtElapsed(elapsed(t, now)) },
    {
      label: 'Error',
      value: t.error,
      status: t.error ? 'fault' : undefined,
      wide: true,
      missing: '오류 없음',
    },
  ]
  const ack: FieldItem[] = t.ack
    ? [
        {
          label: 'Accepted',
          value: t.ack.accepted ? '수락' : '거부',
          status: t.ack.accepted ? 'ok' : 'fault',
        },
        { label: 'Code', value: t.ack.code, mono: true },
        {
          label: 'RejectBits',
          value:
            typeof t.ack.reject_bits === 'number'
              ? `0b${t.ack.reject_bits.toString(2).padStart(8, '0')}`
              : null,
          mono: true,
          tooltip: 'X0 로봇번호 · X1 중복 · X2 태스크 무효 · X3 버퍼 풀 · X7 오프라인',
        },
        { label: 'At', value: fmtTime(t.ack.at, now), tooltip: t.ack.at },
        { label: 'Reason', value: t.ack.reason, wide: true },
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
      label: 'Location',
      value: derived
        ? derived.location === 'absent'
          ? '없음'
          : `${derived.location}${derived.index !== null && derived.location !== 'now' ? `[${derived.index}]` : ''}`
        : null,
      status: derived?.mismatch ? 'warn' : undefined,
      tooltip: derived?.reason ?? undefined,
      missing: '상태 스트림 없음',
    },
    { label: 'Step', value: t.plc?.step ?? null, mono: true, missing: 'PLC에서 아직 못 봄' },
    {
      label: 'QueueIndex',
      value: t.plc?.queue_index ?? null,
      mono: true,
      missing: 'PLC에서 아직 못 봄',
    },
    {
      label: 'LastSeenAt',
      value: t.plc ? fmtTime(t.plc.last_seen_at, now) : null,
      tooltip: t.plc?.last_seen_at,
      missing: 'PLC에서 아직 못 봄',
    },
  ]
  const req = t.request
  const request: FieldItem[] = req
    ? [
        { label: 'Type', value: req.type },
        {
          label: 'Target',
          value: req.target ? `${req.target.kind} ${req.target.id}` : null,
          missing: '대상 없음',
        },
        { label: 'ItemCode', value: req.item_code ?? null, missing: '품목 없음' },
        { label: 'Count', value: req.count },
        {
          label: 'PositionOverride',
          value: req.position_override ? req.position_override.join(' / ') : null,
          missing: '레지스트리 위치 사용',
        },
        {
          label: 'Params',
          value: Object.keys(req.params).length ? Object.keys(req.params).join(', ') : null,
          wide: true,
          missing: '기본값 그대로',
        },
        { label: 'Note', value: req.note || null, wide: true, missing: '메모 없음' },
        ...(req.source
          ? [
              {
                label: 'Source',
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
          label: 'Cell.Section/Row/Col',
          value: pt.Cell ? `S${pt.Cell.Section} R${pt.Cell.Row} C${pt.Cell.Col}` : null,
        },
        {
          label: 'Cell.Position',
          value: pt.Cell?.Position?.map((v) => pos(v)).join(' / '),
          tooltip: 'X / Y / Z (mm)',
        },
        { label: 'Item', value: pt.Item ? `${pt.Item.Code} × ${pt.Item.Count}` : null },
        {
          label: 'Item.InnerDiameter/OuterDiameter/Height',
          value: pt.Item
            ? `ID ${pt.Item.InnerDiameter} · OD ${pt.Item.OuterDiameter} · H ${pt.Item.Height}`
            : null,
        },
      ]
    : []
  const offset = (t as TaskEx & { station_offset?: StationOffsetAudit | null }).station_offset

  return (
    <div className="flex h-full min-h-0 flex-col gap-3 text-sm" data-testid="task-detail">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-sm font-semibold tabular-nums">Task #{t.seq}</span>
        <StatusBadge status={STATE_TONE[t.state]} data-testid="detail-state">
          {STATE_LABEL[t.state]}
        </StatusBadge>
        {derived?.mismatch ? (
          <StatusDot status="warn" label="PLC와 불일치" title={derived.reason ?? undefined} />
        ) : null}
        <span className="text-xs text-content-faint">{ORIGIN_LABEL[t.origin]}</span>
        {at >= 0 && ids.length > 1 ? (
          <span className="ml-auto flex items-center gap-1">
            <Button
              size="icon-sm"
              intent="ghost"
              aria-label="앞 Task"
              title="앞 Task (←)"
              disabled={at <= 0}
              data-testid="detail-prev"
              onClick={() => go(-1)}
            >
              <ChevronLeft size={14} />
            </Button>
            <span className="text-2xs text-content-faint tabular-nums">
              {at + 1}/{ids.length}
            </span>
            <Button
              size="icon-sm"
              intent="ghost"
              aria-label="다음 Task"
              title="다음 Task (→)"
              disabled={at >= ids.length - 1}
              data-testid="detail-next"
              onClick={() => go(1)}
            >
              <ChevronRight size={14} />
            </Button>
          </span>
        ) : null}
      </div>

      <TaskActions
        task={t}
        overflow
        onDone={(a, ok) => {
          if (a === 'delete' && ok) panels.close(TASK_DETAIL_PANEL)
        }}
      />

      <Segmented
        value={tab}
        onChange={setTab}
        ariaLabel="상세 탭"
        options={[
          { id: 'overview', label: '개요', testid: 'tab-overview' },
          { id: 'request', label: '요청', testid: 'tab-request' },
          { id: 'plc', label: 'PLC', testid: 'tab-plc' },
          { id: 'json', label: 'JSON', testid: 'tab-json' },
        ]}
      />

      <div className="min-h-0 flex-1 space-y-4 overflow-auto">
        {tab === 'overview' ? (
          <>
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
            <Section title="이력">
              <Timeline history={t.history} now={now} />
            </Section>
          </>
        ) : null}

        {tab === 'request' ? (
          <>
            <Section title="요청">
              {request.length ? (
                <FieldList items={request} columns={2} dense labelWidth={72} />
              ) : (
                <p className="m-0 text-xs text-content-faint">
                  콘솔 요청 없음 — PLC에서 처음 본 외부 Task
                </p>
              )}
            </Section>
            {offset ? (
              <Section title="스테이션 보정">
                <StationOffsetBlock bare audit={offset} />
              </Section>
            ) : null}
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
            {header.length ? (
              <Section title="명령 헤더">
                <FieldList items={header} columns={2} dense labelWidth={72} />
              </Section>
            ) : null}
            <Section title="PLC Task">
              {plcTask.length ? (
                <FieldList items={plcTask} columns={2} dense labelWidth={72} />
              ) : (
                <p className="m-0 text-xs text-content-faint">없음</p>
              )}
            </Section>
          </>
        ) : null}

        {tab === 'plc' ? (
          <>
            <Section title="원장 ↔ PLC 대조">
              <FieldList items={plc} columns={2} dense labelWidth={72} />
            </Section>
            {/* PLC 배열은 **이 Task 의 로봇** 것을 본다 — 예전 화면은 사이드바에서 고른 로봇을 봤다. */}
            <PlcView
              bare
              robot={robotId}
              robotName={t.plc_name ?? undefined}
              onPickKey={(w, k) => {
                const hit = tasks.list.find((x) => x.work_id === w && x.task_id === k)
                if (hit && onNavigate) onNavigate(hit.id)
              }}
              highlight={{ work_id: t.work_id, task_id: t.task_id }}
            />
          </>
        ) : null}

        {tab === 'json' ? (
          <JsonView value={t} rootLabel="task" defaultDepth={1} height={520} highlightChanges={false} />
        ) : null}
      </div>
    </div>
  )
}
