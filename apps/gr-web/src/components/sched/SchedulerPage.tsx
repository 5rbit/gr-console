// 스케줄러 — 요청 · 수요 · 결정 · 정책 · 기록. 머리띠 한 줄(하위 탭 · 자동 생성 · 버전 · 엔진 메모 · ⋯).
// 엔진 상태(`GET /api/taskgen`)는 여기서 2 초마다 읽어 탭에 내려 준다. 계약: docs/scheduler.md
import { JobsTable } from '../jobs/JobsTable'
import { useRegistry } from '../../lib/registry'
import type { Item } from '../../lib/types'
import { useCallback, useEffect, useState } from 'react'
import { schedApi, type SchedState } from '../../lib/sched'
import { mergePolicy } from '../../lib/sched/policyModel'
import { normalizeState } from '../../lib/sched/decisionModel'
import { SCHED_TABS, SCHED_TAB_KEY, parseSchedTab, type SchedTab } from '../../lib/sched/pageModel'
import { withPolicy } from '../../lib/sched/policyModel'
import { api, loadFailureText } from '../../lib/api'
import { visibleInterval } from '../../lib/poll'
import { menuItems } from '../../lib/task/menuEntries'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { ErrorBoundary } from '../../lib/ui/ErrorBoundary'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import { Segmented } from '../../lib/ui/Segmented'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import { Switch } from '../../lib/ui/Switch'
import { toast } from '../../lib/ui/toast'
import { ParamsDialog, SimulateDialog, WeightsDialog } from './dialogs'
import DecisionTab from './DecisionTab'
import DemandTab from './DemandTab'
import HistoryTab from './HistoryTab'
import { StationsSection } from './PolicyTab'
import RulesTab from './RulesTab'
import RuleSetsTab from './RuleSetsTab'
import RequestsTab from './RequestsTab'

const AUTO_ON_TEXT =
  '요청 · 정책 수요 · 켠 사용자 규칙이 참이 되면 콘솔이 스스로 Task 를 만들어 로봇에 보냅니다. 로봇이 AUTO 이면 그대로 움직입니다.'

type Dlg = 'params' | 'weights' | 'simulate' | null

function loadTab(): SchedTab {
  try {
    return parseSchedTab(localStorage.getItem(SCHED_TAB_KEY))
  } catch {
    return 'requests'
  }
}

function saveTab(t: SchedTab): void {
  try {
    localStorage.setItem(SCHED_TAB_KEY, t)
  } catch {
    // 저장소를 못 쓰면 이번 방문만 기억한다.
  }
}

/** 작업 탭 — 모든 로봇의 작업을 시간순 세 묶음 표 하나로(설계 7판). */
function JobsTab() {
  const items = useRegistry<Item>(api.items)
  return <JobsTable items={items.items} variant="full" testid="sched-jobs" />
}

function TabBody({
  tab,
  state,
  reload,
}: {
  tab: SchedTab
  state: SchedState | null
  reload: () => void
}) {
  switch (tab) {
    case 'jobs':
      return <JobsTab />
    case 'requests':
      return <RequestsTab state={state} reload={reload} />
    case 'demand':
      // 스테이션 — 준비 · 수요(옛 수요 탭) + 프로파일(옛 정책 탭의 스테이션 섹션)
      return (
        <div className="flex flex-col gap-3">
          <DemandTab state={state} reload={reload} />
          <StationsSection policy={mergePolicy(state?.config?.policy ?? null)} reload={reload} />
        </div>
      )
    case 'decision':
      return <DecisionTab state={state} reload={reload} />
    case 'rules':
      return <RulesTab state={state} reload={reload} />
    case 'sets':
      return <RuleSetsTab state={state} reload={reload} />
    case 'history':
      return <HistoryTab state={state} reload={reload} />
  }
}

function Scheduler() {
  const [tab, setTab] = useState<SchedTab>(loadTab)
  const [state, setState] = useState<SchedState | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const [confirmAuto, setConfirmAuto] = useState(false)
  const [dlg, setDlg] = useState<Dlg>(null)

  // 실패는 한 번만 토스트하고 그 뒤로는 띠에 남긴다(2 초마다 쌓이지 않게).
  const load = useCallback(() => {
    schedApi
      .get()
      .then((s) => {
        setState(normalizeState(s))
        setErr(null)
      })
      .catch((e: unknown) => {
        setErr((prev) => {
          const why = loadFailureText(e, '스케줄러', false)
          if (prev === null) toast.error(why)
          return why
        })
      })
  }, [])
  useEffect(() => {
    load()
    const t = visibleInterval(load, 2000)
    return () => clearInterval(t)
  }, [load])

  const cfg = state?.config ? withPolicy(state.config) : null
  const save = useCallback(
    async (c: NonNullable<typeof cfg>, what: string) => {
      try {
        const saved = await schedApi.save(c)
        toast.ok(`${what} (v${saved.version})`)
        load()
      } catch (e) {
        toast.error(`저장 실패 — ${e instanceof Error ? e.message : String(e)}`)
      }
    },
    [load],
  )

  const pick = (t: SchedTab) => {
    setTab(t)
    saveTab(t)
  }

  const openReq = state?.requests.length ?? 0
  const hands = state?.hand_alerts.length ?? 0
  const badge = (t: SchedTab): string =>
    t === 'requests' && openReq ? String(openReq) : t === 'demand' && hands ? `Hand ${hands}` : ''

  const noCfg = cfg ? undefined : '스케줄러 설정을 읽지 못했습니다'
  const menu = menuItems([
    { label: '파라미터…', run: () => setDlg('params'), testid: 'sched-params' },
    { label: '가중치…', run: () => setDlg('weights'), disabled: noCfg, testid: 'sched-weights' },
    {
      label: '시뮬레이션…',
      run: () => setDlg('simulate'),
      disabled: noCfg,
      testid: 'sched-simulate',
    },
  ])

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2 p-3 text-xs" data-testid="scheduler-page">
      <div className="flex flex-wrap items-center gap-2">
        <Segmented
          ariaLabel="스케줄러 화면"
          value={tab}
          onChange={pick}
          options={SCHED_TABS.map((t) => ({
            id: t.id,
            label: t.label,
            badge: badge(t.id),
            testid: `sched-tab-${t.id}`,
          }))}
        />
        <span className="flex-1" />
        {state?.note ? (
          <StatusBadge status="warn" title={state.note} data-testid="sched-note">
            <span className="max-w-64 truncate">{state.note}</span>
          </StatusBadge>
        ) : null}
        <Switch
          inline
          label="자동 생성"
          checked={!!cfg?.auto}
          disabled={!cfg}
          title={
            cfg
              ? '켜면 요청 · 정책 수요 · 규칙의 후보를 예정으로 만들어 보냅니다'
              : '스케줄러를 읽지 못해 지금 값을 모릅니다'
          }
          onCheckedChange={(v) =>
            v ? setConfirmAuto(true) : cfg && void save({ ...cfg, auto: false }, '자동 생성 끔')
          }
          testid="sched-auto"
        />
        {cfg ? (
          <span className="font-mono text-2xs text-content-faint" data-testid="sched-version">
            v{cfg.version}
          </span>
        ) : null}
        <OverflowMenu items={menu} title="도구" testid="sched-more" />
      </div>

      {err ? (
        <div className="flex items-center gap-2 rounded border border-warn-border bg-warn-soft px-2 py-1 text-2xs text-warn-fg">
          <span className="min-w-0 flex-1 truncate" title={err}>
            {err} · 2초마다 다시 시도{state ? ' · 아래는 마지막으로 읽은 값' : ''}
          </span>
          <Button size="sm" intent="ghost" onClick={load} data-testid="sched-retry">
            다시 시도
          </Button>
        </div>
      ) : null}

      <div className="flex min-h-0 flex-1 flex-col overflow-auto">
        <ErrorBoundary label={SCHED_TABS.find((t) => t.id === tab)?.label ?? tab} resetKey={tab}>
          <TabBody tab={tab} state={state} reload={load} />
        </ErrorBoundary>
      </div>

      <ConfirmDialog
        open={confirmAuto}
        onOpenChange={setConfirmAuto}
        scope="fleet"
        title="자동 생성 켜기"
        confirmLabel="켜기"
        onConfirm={() => {
          setConfirmAuto(false)
          if (cfg) void save({ ...cfg, auto: true }, '자동 생성 켬')
        }}
      >
        <div className="flex flex-col gap-1 text-xs">
          <div>{AUTO_ON_TEXT}</div>
          {cfg ? (
            <div className="text-content-faint">
              열린 요청 {openReq} · 사용자 규칙 {cfg.rules.filter((r) => r.enabled).length} /{' '}
              {cfg.rules.length}
            </div>
          ) : null}
        </div>
      </ConfirmDialog>

      {dlg === 'params' ? <ParamsDialog onClose={() => setDlg(null)} /> : null}
      {dlg === 'weights' && cfg ? (
        <WeightsDialog
          cfg={cfg}
          onClose={() => setDlg(null)}
          onSave={(c) => {
            setDlg(null)
            void save(c, '가중치 저장')
          }}
        />
      ) : null}
      {dlg === 'simulate' && cfg ? (
        <SimulateDialog config={cfg} onClose={() => setDlg(null)} />
      ) : null}
    </div>
  )
}

export default function SchedulerPage() {
  return (
    <ErrorBoundary label="스케줄러">
      <Scheduler />
    </ErrorBoundary>
  )
}
