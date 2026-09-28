// 작업 명령 화면의 자동 생성 요약 — 스위치 · 다음 후보 셋 · 예정 수. 규칙 · 정책 · 요청은 스케줄러 탭에서.
import { useCallback, useEffect, useState } from 'react'
import {
  breakdownText,
  taskgenApi,
  type GenCandidate,
  type GenConfig,
  type GenState,
} from '../../lib/taskgen'
import { candidateRoute, nextCandidates } from '../../lib/sched/pageModel'
import { loadFailureText } from '../../lib/api'
import { nav } from '../../lib/nav'
import { visibleInterval } from '../../lib/poll'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { ErrorBoundary } from '../../lib/ui/ErrorBoundary'
import { Switch } from '../../lib/ui/Switch'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'

const AUTO_ON_TEXT =
  '요청 · 정책 수요 · 켠 규칙이 참이 되면 콘솔이 스스로 Task 를 만들어 로봇에 보냅니다. 로봇이 AUTO 이면 그대로 움직입니다.'

const cols: Column<GenCandidate>[] = [
  { key: 'robot', label: 'Robot', get: (c) => c.robot_name, priority: 1 },
  {
    key: 'route',
    label: 'Route',
    get: (c) => candidateRoute(c),
    sortable: false,
    priority: 1,
    cell: (c) => (
      <span className="block truncate" title={c.rule_name}>
        {candidateRoute(c)}
      </span>
    ),
  },
  {
    key: 'score',
    label: 'Score',
    get: (c) => c.score,
    numeric: true,
    sortable: false,
    priority: 1,
    cell: (c) => (
      <span className="font-mono tabular-nums" title={breakdownText(c)}>
        {c.score.toFixed(1)}
      </span>
    ),
  },
  {
    key: 'state',
    label: 'State',
    sortable: false,
    priority: 1,
    cell: (c) =>
      c.reason ? (
        <span className="text-warn-fg" title={c.reason}>
          대기
        </span>
      ) : (
        <span className="text-ok-fg">생성</span>
      ),
  },
]

function Strip() {
  const [state, setState] = useState<GenState | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const [confirmAuto, setConfirmAuto] = useState(false)

  const load = useCallback(() => {
    taskgenApi
      .get()
      .then((s) => {
        setState(s)
        setErr(null)
      })
      .catch((e: unknown) => {
        setErr((prev) => {
          const why = loadFailureText(e, '생성 엔진', false)
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

  const cfg = state?.config ?? null
  const save = (c: GenConfig) =>
    void taskgenApi
      .save(c)
      .then((s) => {
        toast.ok(`자동 생성 ${c.auto ? '켬' : '끔'} (v${s.version})`)
        load()
      })
      .catch((e) => toast.error(`저장 실패 — ${e instanceof Error ? e.message : String(e)}`))

  const next = nextCandidates(state?.candidates)
  const queued = state?.queue?.length ?? 0

  return (
    <div className="flex min-h-0 flex-col gap-2 text-xs" data-testid="autogen-strip">
      <div className="flex flex-wrap items-center gap-2">
        <Switch
          inline
          label="자동 생성"
          checked={!!cfg?.auto}
          disabled={!cfg}
          title={
            cfg
              ? '켜면 스케줄러가 후보를 예정으로 만들어 보냅니다'
              : '생성 엔진을 읽지 못해 지금 값을 모릅니다'
          }
          onCheckedChange={(v) => (v ? setConfirmAuto(true) : cfg && save({ ...cfg, auto: false }))}
          testid="autogen-strip-auto"
        />
        <span className="text-2xs text-content-muted">
          예정 <span className="font-mono tabular-nums text-content-secondary">{queued}</span>
        </span>
        {err || state?.note ? (
          <span
            className="min-w-0 flex-1 truncate text-2xs text-warn-fg"
            title={err ?? state?.note ?? undefined}
          >
            {err ?? state?.note}
          </span>
        ) : (
          <span className="flex-1" />
        )}
        <Button
          size="sm"
          intent="ghost"
          onClick={() => nav.go('scheduler')}
          title="요청 · 수요 · 결정 · 정책 · 기록"
          data-testid="autogen-strip-open"
        >
          스케줄러 ↗
        </Button>
      </div>
      <DataTable
        rows={next}
        columns={cols}
        rowKey={(c) => `${c.rule_id}-${c.robot}`}
        density="compact"
        emptyDense
        empty="후보 없음"
        testid="autogen-strip-candidates"
      />
      <ConfirmDialog
        open={confirmAuto}
        onOpenChange={setConfirmAuto}
        scope="fleet"
        title="자동 생성 켜기"
        confirmLabel="켜기"
        onConfirm={() => {
          setConfirmAuto(false)
          if (cfg) save({ ...cfg, auto: true })
        }}
      >
        <div className="text-xs">{AUTO_ON_TEXT}</div>
      </ConfirmDialog>
    </div>
  )
}

export function AutoGenStrip() {
  return (
    <ErrorBoundary label="자동 생성">
      <Strip />
    </ErrorBoundary>
  )
}
