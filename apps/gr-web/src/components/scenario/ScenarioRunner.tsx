// 실행 판넬 — `OpsPanel`: 실행(옵션 다이얼로그) / 일시정지 / 재개 / 정지(확인) + 진행 게이지 + 현재 값 +
// 실시간 로그(Task 관리로 이동). 상태는 `runsFeed`(SSE `/api/scenarios/runs/stream`)에서 온다.
import { useState } from 'react'
import { ExternalLink, History, Pause, Play, Square } from 'lucide-react'
import { runsFeed } from '../../lib/feeds'
import { nav } from '../../lib/nav'
import { useSse } from '../../lib/sse'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { FieldList } from '../../lib/ui/FieldList'
import { Modal } from '../../lib/ui/Modal'
import { OpsPanel } from '../../lib/ui/OpsPanel'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import { Gauge } from '../../lib/ui/viz/Gauge'
import { toast } from '../../lib/ui/toast'
import type { Column } from '../../lib/ui/table'
import type { Status } from '../../lib/ui/status'
import { STATE_LABEL, STATE_TONE } from '../../lib/gr/const'
import type { Scenario, ScenarioRun } from '../../lib/types'
import { scenarioApi, type RunOptions } from '../../lib/scenario/api'
import { RUN_ACTIVE, RUN_STATE_LABEL, type ScenarioRunView, type StepResultView, stepSummary } from '../../lib/scenario/model'
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

async function act(label: string, run: () => Promise<ScenarioRun>): Promise<void> {
  try {
    await run()
    toast.ok(label)
  } catch (e) {
    toast.error(`${label} 실패 — ${e instanceof Error ? e.message : String(e)}`)
  }
}

export function ScenarioRunner({ scenario, dirty, onOpenScenario }: ScenarioRunnerProps) {
  const feed = useSse(runsFeed)
  const run = feed.data as ScenarioRunView | null
  const state = run?.state ?? 'idle'
  const active = RUN_ACTIVE.has(state)
  const [runOpen, setRunOpen] = useState(false)
  const [stopOpen, setStopOpen] = useState(false)
  const [histOpen, setHistOpen] = useState(false)
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
    await act('실행 시작', () => scenarioApi.run(scenario.id, opts))
  }
  const openHistory = async () => {
    setHistOpen(true)
    try {
      setHist(await scenarioApi.runs(50))
    } catch (e) {
      toast.error(`이력 조회 실패 — ${e instanceof Error ? e.message : String(e)}`)
    }
  }

  const results = (run?.results ?? []).slice().reverse()
  const last = run?.results.length ? run.results[run.results.length - 1] : null
  const lastAck = last?.ack ? `${last.ack.accepted ? '수락' : '거부'} ${last.ack.code ? `(${last.ack.code})` : ''} ${last.ack.reason}`.trim() : null

  const logColumns: Column<StepResultView>[] = [
    { key: 'iter', label: '회차', get: (r) => r.iteration, numeric: true, sortable: false },
    { key: 'step', label: '스텝', get: (r) => r.step_index, sortable: false, cell: (r) => <span title={stepLabel(r.step_index)}>{r.step_index + 1}{r.attempt && r.attempt > 1 ? <span className="text-slate-400"> ·{r.attempt}회</span> : null}</span> },
    { key: 'state', label: '상태', sortable: false, cell: (r) => <StatusBadge status={STATE_TONE[r.state]} dot={false}>{STATE_LABEL[r.state]}</StatusBadge> },
    { key: 'task', label: 'Task', sortable: false, cell: (r) => (r.task_id ? <span className="inline-flex items-center gap-0.5 font-mono text-accent-text">{shortId(r.task_id)}<ExternalLink size={10} /></span> : <span className="text-slate-400" title={r.error ?? ''}>—</span>) },
  ]
  const histColumns: Column<ScenarioRun>[] = [
    { key: 'started', label: '시작', get: (r) => r.started_at, cell: (r) => r.started_at.replace('T', ' ').slice(0, 19) },
    { key: 'name', label: '시나리오', get: (r) => r.scenario_name },
    { key: 'state', label: '상태', get: (r) => r.state, cell: (r) => <StatusBadge status={RUN_TONE[r.state]}>{RUN_STATE_LABEL[r.state]}</StatusBadge> },
    { key: 'iter', label: '회차', get: (r) => r.iteration, numeric: true, cell: (r) => `${r.iteration}/${r.total_iterations ?? '∞'}` },
    { key: 'n', label: '결과', get: (r) => r.results.length, numeric: true },
    { key: 'err', label: '오류', get: (r) => r.error ?? '', class: 'max-w-64 truncate' },
  ]

  const iterMax = run?.total_iterations ?? null
  const iterLabel = run ? `${run.iteration}/${iterMax ?? '∞'}` : ''
  // 스텝 게이지: 끝난 스텝은 꽉, 진행 중인 스텝은 반 칸. 무한 반복은 회차 게이지를 꽉 채운다.
  const stepDone = !!(last && run && last.iteration === run.iteration && last.step_index === run.step_index && last.ended_at)
  const stepProgress = !run || state === 'idle' ? 0 : state === 'done' ? run.step_count : run.step_index + (stepDone ? 1 : 0.5)
  const iterProgress = iterMax ? (run?.iteration ?? 0) : run && state !== 'idle' ? 1 : 0
  const stepLabelText = run ? `${Math.min(run.step_index + 1, run.step_count)}/${run.step_count}` : ''

  return (
    <>
      <OpsPanel
        title="실행"
        target={run && state !== 'idle' ? run.scenario_name : (scenario?.name ?? '')}
        segments={[]}
        grid={[
          { label: '실행', icon: Play, intent: 'primary', disabled: !scenario || active, title: !scenario ? '시나리오를 선택·저장하세요' : active ? '실행 중' : '반복/시작 스텝을 정하고 실행', run: () => setRunOpen(true), testid: 'run-start' },
          { label: '이력', icon: History, run: () => void openHistory(), testid: 'run-history' },
          { label: '일시정지', icon: Pause, disabled: state !== 'running', title: '현재 스텝이 끝난 뒤 다음 제출을 멈춥니다', run: () => void act('일시정지', scenarioApi.pause), testid: 'run-pause' },
          { label: '재개', icon: Play, disabled: state !== 'paused', run: () => void act('재개', scenarioApi.resume), testid: 'run-resume' },
        ]}
        note={run?.note ?? run?.error ?? (!feed.connected ? (feed.error ?? '') : '')}
        stop={{ label: '정지', icon: Square, disabled: !active || state === 'stopping', immediate: false, title: '실행을 끝냅니다. PLC에 이미 넘어간 태스크는 취소하지 않습니다.', run: () => setStopOpen(true) }}
        lastAction={last ? `${fmtTime(last.ended_at ?? last.started_at)} ${last.iteration}회차 ${last.step_index + 1}번 → ${STATE_LABEL[last.state]}` : ''}
      >
        <div className="flex flex-col gap-1.5 border-b border-line-subtle p-2" data-testid="run-progress">
          <div className="flex items-center justify-between text-3xs text-content-muted">
            <span>회차</span>
            <StatusBadge status={RUN_TONE[state]}>{RUN_STATE_LABEL[state]}</StatusBadge>
          </div>
          <Gauge value={iterProgress} max={iterMax ?? 1} label={iterLabel} status={RUN_TONE[state]} />
          <div className="text-3xs text-content-muted">스텝</div>
          <Gauge value={stepProgress} max={run?.step_count || 1} label={stepLabelText} status={RUN_TONE[state]} />
        </div>
        <div className="border-b border-line-subtle px-2 py-1">
          <FieldList
            columns={1}
            dense
            labelWidth={64}
            items={[
              { label: '현재 스텝', value: run && state !== 'idle' ? `${run.step_index + 1}. ${stepLabel(run.step_index)}` : null, missing: '실행 없음' },
              { label: 'Task', value: run?.current_task_id ? <button type="button" className="inline-flex items-center gap-1 font-mono text-accent-text hover:underline" onClick={() => nav.goTask(run.current_task_id!)}>{shortId(run.current_task_id)}<ExternalLink size={10} /></button> : null, missing: '진행 중인 Task 없음' },
              { label: '마지막 Ack', value: lastAck, status: last?.ack ? (last.ack.accepted ? 'ok' : 'fault') : undefined, missing: '아직 응답 없음' },
              { label: '시작', value: fmtTime(run?.started_at) || null },
              { label: '종료', value: fmtTime(run?.ended_at) || null, missing: '진행 중' },
            ]}
          />
        </div>
        <div className="min-h-0" data-testid="run-log">
          <div className="px-2 pt-1.5 text-3xs font-medium text-content-muted">실행 로그 {results.length ? `(${results.length})` : ''}</div>
          <DataTable<StepResultView> rows={results} columns={logColumns} rowKey={(r) => `${r.iteration}-${r.step_index}-${r.attempt ?? 0}-${r.started_at}`} onPick={(r) => r.task_id && nav.goTask(r.task_id)} empty="기록 없음" />
        </div>
      </OpsPanel>

      <RunDialog open={runOpen} scenario={scenario} dirty={dirty} onOpenChange={setRunOpen} onRun={(o) => void start(o)} />

      <ConfirmDialog open={stopOpen} onOpenChange={setStopOpen} scope="single-robot" title="시나리오 정지" danger confirmLabel="정지" onConfirm={() => void act('정지', scenarioApi.stop)}>
        <p className="m-0">
          <b>{run?.scenario_name}</b> 실행을 끝냅니다. 이미 PLC에 제출된 태스크는 그대로 진행되며(취소하지 않음), 다음 스텝은 제출하지 않습니다.
        </p>
      </ConfirmDialog>

      <Modal open={histOpen} onOpenChange={setHistOpen} title="실행 이력 (최근 50)" wide>
        <DataTable<ScenarioRun> rows={hist} columns={histColumns} rowKey={(r) => r.run_id} onPick={(r) => { setHistOpen(false); onOpenScenario(r.scenario_id) }} empty="실행 이력 없음" />
      </Modal>
    </>
  )
}
