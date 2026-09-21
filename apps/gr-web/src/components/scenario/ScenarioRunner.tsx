// 실행 띠 — 편집기 **위 한 줄**. 실행(옵션 다이얼로그) / 일시정지 / 재개 / 정지(확인) + 지금 상태.
// 상태는 `runsFeed`(SSE `/api/scenarios/runs/stream`)에서 온다.
//
// 예전에는 오른쪽에 판넬 하나가 통째로 서서 게이지 둘 · 라벨+값 여섯 · 로그 표를 늘 그렸다. 시나리오를
// **고치는 동안**에는 그 대부분이 빈칸이었고(실행이 없으니까), 실행 중에는 편집기가 그만큼 좁아졌다.
// 그래서 늘 보이는 것은 한 줄로 줄이고 — 상태 점 · 회차 · 스텝 · 지금 Task — 나머지(실행 로그 ·
// 실행 이력)는 `⋯`로 옮겼다. 멈춤/재개/정지는 **실행 중일 때만** 선다(돌지 않을 때 멈출 것이 없다).
import { useEffect, useState } from 'react'
import { ExternalLink, Pause, Play, Square } from 'lucide-react'
import { runsFeed } from '../../lib/feeds'
import { nav } from '../../lib/nav'
import { robots } from '../../lib/robots'
import { robotChip, robotFailure, withRobot } from '../../lib/robotContext'
import { robotField } from '../shared/RobotChip'
import { useSse } from '../../lib/sse'
import { useStore } from '../../lib/store'
import { tasks as taskStore } from '../../lib/tasks'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { Dialog } from '../../lib/ui/Dialog'
import { FieldList } from '../../lib/ui/FieldList'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import { StatusDot } from '../../lib/ui/StatusDot'
import { Gauge } from '../../lib/ui/viz/Gauge'
import { toast } from '../../lib/ui/toast'
import type { Column } from '../../lib/ui/table'
import type { Status } from '../../lib/ui/status'
import { STATE_LABEL, STATE_TONE } from '../../lib/gr/const'
import type { Scenario, ScenarioRun } from '../../lib/types'
import { scenarioApi, type RunOptions } from '../../lib/scenario/api'
import {
  RUN_ACTIVE,
  RUN_STATE_LABEL,
  phaseSummary,
  stepPhases,
  type ScenarioRunView,
  type StepResultView,
  stepSummary,
} from '../../lib/scenario/model'
import { RunDialog } from './RunDialog'

export interface ScenarioRunnerProps {
  /** 편집 중인(저장된) 시나리오 — 실행 대상. */
  scenario: Scenario | null
  dirty: boolean
  onOpenScenario: (id: string) => void
}

const RUN_TONE: Record<ScenarioRun['state'], Status> = {
  idle: 'neutral',
  running: 'ok',
  paused: 'warn',
  stopping: 'warn',
  stopped: 'neutral',
  done: 'ok',
  failed: 'fault',
}

function fmtTime(s: string | null | undefined): string {
  if (!s) return ''
  const d = new Date(s)
  if (Number.isNaN(d.getTime())) return s
  const p = (n: number) => String(n).padStart(2, '0')
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`
}
const shortId = (id: string | null) => (id ? id.slice(0, 8) : '')

/** 실행 조작 한 건 — 메시지는 **그 실행이 도는 로봇**을 말한다(`who`). */
async function act(
  who: string | null,
  label: string,
  run: () => Promise<ScenarioRun>,
): Promise<void> {
  try {
    await run()
    toast.ok(withRobot(who, label))
  } catch (e) {
    toast.error(robotFailure(who, `${label} 실패`, e instanceof Error ? e.message : String(e)))
  }
}

export function ScenarioRunner({ scenario, dirty, onOpenScenario }: ScenarioRunnerProps) {
  useStore(robots, taskStore)
  useEffect(() => taskStore.start(), [])
  const feed = useSse(runsFeed)
  const run = feed.data as ScenarioRunView | null
  const state = run?.state ?? 'idle'
  const active = RUN_ACTIVE.has(state)
  // 도는 실행이 있으면 그 실행의 로봇, 없으면 다음 실행이 갈 사이드바 선택.
  const runChip =
    run && state !== 'idle' && (run.robot_name || run.robot)
      ? robotChip(robots.byId(run.robot ?? null), {
          name: run.robot_name ?? null,
          id: run.robot ?? null,
        })
      : robots.chip
  const [runOpen, setRunOpen] = useState(false)
  const [stopOpen, setStopOpen] = useState(false)
  const [histOpen, setHistOpen] = useState(false)
  const [logOpen, setLogOpen] = useState(false)
  const [hist, setHist] = useState<ScenarioRun[]>([])

  // 실행 로그의 스텝 라벨 — 도는 시나리오가 편집 중인 것과 같을 때만 안다.
  const stepLabel = (i: number): string => {
    if (run && scenario && run.scenario_id === scenario.id && scenario.steps[i]) {
      const s = scenario.steps[i]
      return s.label || stepSummary(s)
    }
    return `스텝 ${i + 1}`
  }

  const start = async (opts: RunOptions) => {
    if (!scenario) return
    setRunOpen(false)
    const who =
      opts.robot !== null && opts.robot !== undefined ? robots.nameOf(opts.robot) : robots.chip.name
    await act(who, '실행 시작', () => scenarioApi.run(scenario.id, opts))
  }
  const openHistory = async () => {
    setHistOpen(true)
    try {
      setHist(await scenarioApi.runs(50))
    } catch (e) {
      toast.error(`이력 조회 실패 — ${e instanceof Error ? e.message : String(e)}`)
    }
  }

  // 결과는 도달 때(접수) 상태로 남는다 — 미리 넣은 Task 는 그 뒤에도 움직이므로 원장의 **지금** 상태로 본다.
  const live = (id: string) => taskStore.get(id)
  const phases = run
    ? stepPhases(run.step_count, run.iteration, run.results, (id) => live(id)?.state)
    : []
  const results = (run?.results ?? []).slice().reverse()
  const last = run?.results.length ? run.results[run.results.length - 1] : null
  const lastAck = last?.ack
    ? `${last.ack.accepted ? '수락' : '거부'} ${last.ack.code ? `(${last.ack.code})` : ''} ${last.ack.reason}`.trim()
    : null

  const logColumns: Column<StepResultView>[] = [
    { key: 'iter', label: 'Iteration', get: (r) => r.iteration, numeric: true, sortable: false },
    {
      key: 'step',
      label: 'StepIndex',
      get: (r) => r.step_index,
      sortable: false,
      cell: (r) => (
        <span title={stepLabel(r.step_index)}>
          {r.step_index + 1}
          {r.attempt && r.attempt > 1 ? (
            <span className="text-content-faint"> ·{r.attempt}회</span>
          ) : null}
        </span>
      ),
    },
    {
      key: 'state',
      label: 'State',
      sortable: false,
      cell: (r) => {
        const s = (r.task_id ? live(r.task_id)?.state : null) ?? r.state
        return (
          <StatusBadge status={STATE_TONE[s]} dot={false}>
            {STATE_LABEL[s]}
          </StatusBadge>
        )
      },
    },
    {
      key: 'to',
      label: 'TransferOrder',
      sortable: false,
      priority: 3,
      cell: (r) => {
        const to = r.task_id ? live(r.task_id)?.transfer_order_id : null
        return to ? (
          <span className="font-mono text-2xs">{to}</span>
        ) : (
          <span className="text-content-faint">—</span>
        )
      },
    },
    {
      key: 'task',
      label: 'Task',
      sortable: false,
      cell: (r) =>
        r.task_id ? (
          <span className="inline-flex items-center gap-0.5 font-mono text-accent-text">
            {shortId(r.task_id)}
            <ExternalLink size={10} />
          </span>
        ) : (
          <span className="text-content-faint" title={r.error ?? ''}>
            —
          </span>
        ),
    },
    {
      key: 'ended',
      label: 'EndedAt',
      get: (r) => r.ended_at ?? '',
      cell: (r) => fmtTime(r.ended_at),
    },
  ]
  // 실행 이력을 훑는 이유는 **무엇이 언제 돌았고 왜 멈췄나**다: 시나리오·상태·오류가 1, 시작 시각과
  // 회차가 2, 결과 건수가 3이다(`docs/DESIGN.md` 4절).
  const histColumns: Column<ScenarioRun>[] = [
    {
      key: 'started',
      label: 'StartedAt',
      get: (r) => r.started_at,
      cell: (r) => r.started_at.replace('T', ' ').slice(0, 19),
      priority: 2,
    },
    { key: 'name', label: 'ScenarioName', get: (r) => r.scenario_name, priority: 1 },
    { key: 'robot', label: 'RobotName', get: (r) => r.robot_name ?? '', priority: 2 },
    {
      key: 'state',
      label: 'State',
      get: (r) => r.state,
      cell: (r) => <StatusBadge status={RUN_TONE[r.state]}>{RUN_STATE_LABEL[r.state]}</StatusBadge>,
      priority: 1,
    },
    {
      key: 'iter',
      label: 'Iteration',
      get: (r) => r.iteration,
      numeric: true,
      cell: (r) => `${r.iteration}/${r.total_iterations ?? '∞'}`,
      priority: 2,
    },
    { key: 'n', label: 'Results', get: (r) => r.results.length, numeric: true, priority: 3 },
    {
      key: 'err',
      label: 'Error',
      get: (r) => r.error ?? '',
      class: 'max-w-64 truncate',
      priority: 1,
    },
  ]

  const iterMax = run?.total_iterations ?? null
  // 스텝 게이지: 끝난 스텝은 꽉, 진행 중인 스텝은 반 칸. 무한 반복은 회차 게이지를 꽉 채운다.
  const stepDone = !!(
    last &&
    run &&
    last.iteration === run.iteration &&
    last.step_index === run.step_index &&
    last.ended_at
  )
  const stepProgress =
    !run || state === 'idle'
      ? 0
      : state === 'done'
        ? run.step_count
        : run.step_index + (stepDone ? 1 : 0.5)
  const iterProgress = iterMax ? (run?.iteration ?? 0) : run && state !== 'idle' ? 1 : 0
  const showRun = run && state !== 'idle'
  const note = run?.note ?? run?.error ?? (!feed.connected ? (feed.error ?? '') : '')

  return (
    <>
      <div
        className="flex min-h-control-md flex-wrap items-center gap-x-2 gap-y-1 border-b border-line-default bg-surface-inset px-2 py-1"
        data-testid="run-strip"
      >
        <Button
          size="sm"
          intent="primary"
          icon={<Play size={13} />}
          disabled={!scenario || active}
          title={
            !scenario
              ? '시나리오를 선택·저장하세요'
              : active
                ? '이미 실행 중입니다'
                : '반복 · 시작 스텝 · 로봇을 정하고 실행'
          }
          data-testid="run-start"
          onClick={() => setRunOpen(true)}
        >
          실행
        </Button>
        {active ? (
          <>
            {state === 'paused' ? (
              <Button
                size="sm"
                icon={<Play size={13} />}
                data-testid="run-resume"
                onClick={() => void act(runChip.name, '재개', scenarioApi.resume)}
              >
                재개
              </Button>
            ) : (
              <Button
                size="sm"
                icon={<Pause size={13} />}
                disabled={state !== 'running'}
                title="현재 스텝이 끝난 뒤 다음 제출을 멈춥니다"
                data-testid="run-pause"
                onClick={() => void act(runChip.name, '일시정지', scenarioApi.pause)}
              >
                일시정지
              </Button>
            )}
            <Button
              size="sm"
              intent="outline"
              icon={<Square size={13} />}
              disabled={state === 'stopping'}
              title="실행을 끝냅니다 — 이미 PLC 로 넘어간 태스크는 취소하지 않습니다."
              data-testid="run-stop"
              onClick={() => setStopOpen(true)}
            >
              정지
            </Button>
          </>
        ) : null}

        {/* 지금 상태 — 점(상태) · 이름 · 회차 · 스텝 · Task. 돌지 않을 때는 점 하나만 남는다. */}
        <StatusDot status={RUN_TONE[state]} size="sm" label={RUN_STATE_LABEL[state]} />
        {showRun ? (
          <>
            <span className="max-w-48 truncate text-xs text-content-secondary">
              {run.scenario_name}
            </span>
            <span className="flex items-center gap-1 text-2xs text-content-muted">
              Iteration
              <span className="w-24">
                <Gauge
                  value={iterProgress}
                  max={iterMax ?? 1}
                  label={`${run.iteration}/${iterMax ?? '∞'}`}
                  status={RUN_TONE[state]}
                />
              </span>
            </span>
            <span className="flex items-center gap-1 text-2xs text-content-muted">
              Step
              <span className="w-24">
                <Gauge
                  value={stepProgress}
                  max={run.step_count || 1}
                  label={`${Math.min(run.step_index + 1, run.step_count)}/${run.step_count}`}
                  status={RUN_TONE[state]}
                />
              </span>
            </span>
            <span
              className="max-w-56 truncate text-2xs text-content-tertiary"
              title={lastAck ?? undefined}
            >
              {stepLabel(run.step_index)}
            </span>
            {run.current_task_id ? (
              <button
                type="button"
                className="inline-flex items-center gap-1 font-mono text-2xs text-accent-text hover:underline"
                title="이 Task를 Task 관리에서 열기"
                onClick={() => nav.goTask(run.current_task_id!)}
              >
                {shortId(run.current_task_id)}
                <ExternalLink size={10} />
              </button>
            ) : null}
          </>
        ) : null}
        {showRun && phases.length ? (
          <span
            className="text-2xs text-content-muted"
            title={phases.map((p, i) => `${i + 1} ${p}`).join(' · ')}
            data-testid="run-phases"
          >
            {phaseSummary(phases)}
          </span>
        ) : null}
        {note ? (
          <span className="max-w-72 truncate text-2xs text-warn-fg" title={note}>
            {note}
          </span>
        ) : null}

        <span className="ml-auto flex items-center gap-1">
          <OverflowMenu
            testid="run-more"
            title="실행 로그 · 이력"
            items={[
              {
                label: `실행 로그 (${results.length})`,
                disabled: results.length === 0 ? '이번 실행의 기록이 아직 없습니다' : undefined,
                run: () => setLogOpen(true),
              },
              { label: '실행 이력 (최근 50)', run: () => void openHistory() },
            ]}
          />
        </span>
      </div>

      <RunDialog
        open={runOpen}
        scenario={scenario}
        dirty={dirty}
        onOpenChange={setRunOpen}
        onRun={(o) => void start(o)}
      />

      <ConfirmDialog
        open={stopOpen}
        onOpenChange={setStopOpen}
        scope="single-robot"
        title={`시나리오 정지 — ${runChip.name}`}
        danger
        confirmLabel="정지"
        onConfirm={() => void act(runChip.name, '정지', scenarioApi.stop)}
      >
        {/* 버튼의 `title` 이 이미 "무엇이 일어나나"를 말한다 — 여기서는 묻고, 대상만 짚는다. */}
        <div className="flex flex-col gap-2 text-xs">
          <p className="m-0">실행을 정지할까요?</p>
          <FieldList
            columns={2}
            dense
            labelWidth={64}
            items={[
              robotField(runChip),
              { label: '시나리오', value: run?.scenario_name ?? '', missing: '이름 없음' },
              {
                label: '회차',
                value: run ? `${run.iteration}/${run.total_iterations ?? '∞'}` : '',
              },
              {
                label: '스텝',
                value: run
                  ? `${Math.min(run.step_index + 1, run.step_count)}/${run.step_count}`
                  : '',
              },
              { label: '제출된 Task', value: '취소하지 않음' },
            ]}
          />
        </div>
      </ConfirmDialog>

      {/* 띠가 들지 않는 값(로봇·시작/끝 시각·마지막 응답)은 여기 머리에 그대로 남는다 — 예전 판넬의
          라벨+값 여섯이 사라진 것이 아니라 한 번의 클릭 뒤로 옮겨졌다. */}
      <Dialog
        open={logOpen}
        onOpenChange={setLogOpen}
        title="실행 상태 · 로그"
        size="lg"
        meta={`${results.length}건`}
        footer={
          <Button size="sm" intent="ghost" onClick={() => setLogOpen(false)}>
            닫기
          </Button>
        }
        testid="run-log-dialog"
      >
        <FieldList
          className="mb-3"
          columns={3}
          dense
          labelWidth={64}
          items={[
            robotField(runChip, showRun ? 'Robot' : 'Robot (다음 실행)'),
            {
              label: 'Step',
              value: showRun ? `${run.step_index + 1}. ${stepLabel(run.step_index)}` : null,
              missing: '실행 없음',
            },
            {
              label: 'Task',
              value: run?.current_task_id ? shortId(run.current_task_id) : null,
              mono: true,
              missing: '진행 중인 Task 없음',
            },
            {
              label: 'LastAck',
              value: lastAck,
              status: last?.ack ? (last.ack.accepted ? 'ok' : 'fault') : undefined,
              missing: '아직 응답 없음',
            },
            { label: 'StartedAt', value: fmtTime(run?.started_at) || null },
            { label: 'EndedAt', value: fmtTime(run?.ended_at) || null, missing: '진행 중' },
          ]}
        />
        <DataTable<StepResultView>
          rows={results}
          columns={logColumns}
          rowKey={(r) => `${r.iteration}-${r.step_index}-${r.attempt ?? 0}-${r.started_at}`}
          onPick={(r) => {
            if (!r.task_id) return
            setLogOpen(false)
            nav.goTask(r.task_id)
          }}
          empty="기록 없음"
          emptyHint="이번 실행에서 끝난 스텝이 아직 없습니다."
          testid="run-log"
        />
      </Dialog>

      <Dialog
        open={histOpen}
        onOpenChange={setHistOpen}
        title="실행 이력"
        size="lg"
        meta={`최근 ${hist.length}건`}
        footer={
          <Button size="sm" intent="ghost" onClick={() => setHistOpen(false)}>
            닫기
          </Button>
        }
        testid="run-hist-dialog"
      >
        <DataTable<ScenarioRun>
          rows={hist}
          columns={histColumns}
          rowKey={(r) => r.run_id}
          onPick={(r) => {
            setHistOpen(false)
            onOpenScenario(r.scenario_id)
          }}
          empty="실행 이력 없음"
          emptyHint="시나리오를 한 번 실행하면 여기에 남습니다."
        />
      </Dialog>
    </>
  )
}
