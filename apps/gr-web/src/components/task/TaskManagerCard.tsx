// Task Manager 카드 — 작업 명령 화면 오른쪽 칸, 작업 카드 **아래**. 원장(`/api/tasks`)에 오른 Task 를
// 고른 로봇 것만 두 보기로 본다(콘솔·시나리오 제출 + PLC 에서 처음 본 외부 Task 모두):
//   진행     = 스토어(SSE) — 실행 → PLC 대기열 순서.
//   히스토리 = 서버 페이징(종결, 최신 순) — 필터(State·TaskType·Origin·Since)는 대화상자로 접는다.
// 칸이 좁아 열은 다섯뿐이다. 나머지(Ack·사유·시각·요청·PLC)는 행을 누르면 뜨는 Task 상세 팝업에 있다.
import { useCallback, useEffect, useMemo, useState } from 'react'
import { ArrowLeftRight, Filter, History, ListChecks, ListPlus, RefreshCw } from 'lucide-react'
import { api } from '../../lib/api'
import { panels } from '../../lib/panels'
import { visibleInterval } from '../../lib/poll'
import { robots } from '../../lib/robots'
import { robotLabel } from '../../lib/robotContext'
import { useStore } from '../../lib/store'
import { tasks } from '../../lib/tasks'
import {
  ORIGIN_LABEL,
  STATE_LABEL,
  TERMINAL,
  elapsed,
  fmtElapsed,
  fmtTime,
  targetLabel,
  typeName,
} from '../../lib/task/state'
import {
  EMPTY_HISTORY_FILTER,
  filterCount,
  historyQuery,
  liveQueue,
  terminalCount,
  type HistoryFilter,
} from '../../lib/task/tmList'
import type { Task, TaskPage, TaskState, TaskType } from '../../lib/types'
import { Button } from '../../lib/ui/Button'
import { Card } from '../../lib/ui/Card'
import { DataTable } from '../../lib/ui/DataTable'
import { Dialog } from '../../lib/ui/Dialog'
import { Input } from '../../lib/ui/Input'
import { Pagination } from '../../lib/ui/Pagination'
import { Segmented } from '../../lib/ui/Segmented'
import { Select } from '../../lib/ui/Select'
import type { Column } from '../../lib/ui/table'
import { RobotChip } from '../shared/RobotChip'
import TaskDetail, { TASK_DETAIL_PANEL } from '../taskmgr/TaskDetail'
import { StateCell } from '../taskmgr/TaskTable'
import { TransferOrdersDialog } from './TransferOrdersDialog'
import { TaskGenDialog } from './TaskGenDialog'
import { TaskActions } from '../taskmgr/TaskActions'

const LIMIT = 20
const VIEW_KEY = 'gr-issue-tm-view'
const TYPES: TaskType[] = ['PICK', 'DROP', 'MOVE', 'MEASURE', 'UP']
const ORIGINS = Object.keys(ORIGIN_LABEL) as Task['origin'][]

type View = 'live' | 'history'

function loadView(): View {
  try {
    return localStorage.getItem(VIEW_KEY) === 'history' ? 'history' : 'live'
  } catch {
    return 'live'
  }
}

function columns(view: View, now: number): Column<Task>[] {
  return [
    { key: 'seq', label: 'Seq', get: (t) => t.seq, numeric: true },
    { key: 'type', label: 'TaskType', get: (t) => typeName(t.plc_task?.TaskType) },
    { key: 'target', label: 'Target', get: (t) => targetLabel(t) },
    {
      key: 'state',
      label: 'State',
      get: (t) => t.state,
      cell: (t) => <StateCell task={t} area={null} />,
    },
    view === 'live'
      ? {
          key: 'elapsed',
          label: 'Elapsed',
          get: (t) => elapsed(t, now),
          cell: (t) => (
            <span className="font-mono tabular-nums">{fmtElapsed(elapsed(t, now))}</span>
          ),
          numeric: true,
        }
      : {
          key: 'ended',
          label: 'EndedAt',
          get: (t) => t.ended_at ?? '',
          cell: (t) => <span className="tabular-nums">{fmtTime(t.ended_at, now)}</span>,
        },
  ]
}

export function TaskManagerCard() {
  useStore(tasks, robots)
  useEffect(() => tasks.start(), [])
  const chip = robots.chip
  const robot = robots.selected
  const plc = robots.current?.plc ?? null

  const [view, setViewState] = useState<View>(loadView)
  const setView = useCallback((v: View) => {
    setViewState(v)
    try {
      localStorage.setItem(VIEW_KEY, v)
    } catch {
      /* 저장 못 해도 동작 */
    }
  }, [])
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const t = visibleInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(t)
  }, [])

  const live = useMemo(() => liveQueue(tasks.list, plc), [tasks.list, plc])
  const ended = useMemo(() => terminalCount(tasks.list, plc), [tasks.list, plc])

  // ── 히스토리(서버 페이징) ──
  const [filter, setFilter] = useState<HistoryFilter>(EMPTY_HISTORY_FILTER)
  const [filterOpen, setFilterOpen] = useState(false)
  const [ordersOpen, setOrdersOpen] = useState(false)
  const [genOpen, setGenOpen] = useState(false)
  const [offset, setOffset] = useState(0)
  const [page, setPage] = useState<TaskPage | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(
    async (off: number) => {
      setLoading(true)
      try {
        setPage(await api.tasks(historyQuery(filter, robot, off, LIMIT)))
        setError(null)
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e))
      } finally {
        setLoading(false)
      }
    },
    [filter, robot],
  )
  // 로봇·필터가 바뀌면 첫 쪽부터.
  useEffect(() => {
    setOffset(0)
    setPage(null)
  }, [robot, filter])
  useEffect(() => {
    if (view === 'history') void load(offset)
  }, [view, load, offset])
  // 새 종결 — 첫 쪽을 보고 있을 때만 조용히 갱신.
  useEffect(() => {
    if (view === 'history' && offset === 0) void load(0)
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 종결 건수 변화에만 반응한다
  }, [ended])

  const rows = view === 'live' ? live : (page?.items ?? [])
  const ids = useMemo(() => rows.map((t) => t.id), [rows])
  const cols = useMemo(() => columns(view, now), [view, now])
  const nFilter = filterCount(filter)

  const open = useCallback((id: string, order: readonly string[]) => {
    const t = tasks.get(id) ?? rows.find((r) => r.id === id)
    panels.open({
      id: TASK_DETAIL_PANEL,
      mode: 'popup',
      title: t ? `Task #${t.seq}` : 'Task 상세',
      component: TaskDetail,
      props: { id, ids: order, onNavigate: (next: string) => open(next, order) },
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps -- rows 는 제목에만 쓴다
  }, [])

  const setState = (s: string) => setFilter((f) => ({ ...f, states: s ? [s as TaskState] : [] }))

  return (
    <Card padded={false} className="flex flex-none flex-col" data-testid="tm-card">
      <div className="flex min-h-screen-header flex-none items-center gap-2 border-b border-line-default px-3 py-1">
        <ListChecks className="h-4 w-4 text-content-muted" />
        <Segmented
          ariaLabel="Task Manager 보기"
          value={view}
          onChange={setView}
          options={[
            { id: 'live', label: '진행', badge: live.length || '', testid: 'tm-live' },
            {
              id: 'history',
              label: '히스토리',
              icon: <History size={12} />,
              testid: 'tm-history',
            },
          ]}
        />
        <span
          className="truncate text-2xs text-content-faint"
          title="Task Manager 원장 — 콘솔·시나리오·외부(PLC) Task"
        >
          Task Manager
        </span>
        <span className="flex-1" />
        {view === 'history' ? (
          <Button
            size="icon-sm"
            intent={nFilter ? 'outline' : 'ghost'}
            aria-label="히스토리 필터"
            title={nFilter ? `필터 ${nFilter}개 적용 중` : '히스토리 필터'}
            onClick={() => setFilterOpen(true)}
            data-testid="tm-filter"
          >
            <Filter size={13} />
          </Button>
        ) : null}
        <Button
          size="icon-sm"
          intent="ghost"
          aria-label="생성 규칙"
          title="Task 생성 규칙 — 조건 · 우선순위 · 영역 (콘솔이 GCS 로 생성)"
          onClick={() => setGenOpen(true)}
          data-testid="tm-taskgen"
        >
          <ListPlus size={13} />
        </Button>
        <Button
          size="icon-sm"
          intent="ghost"
          aria-label="이송 이력"
          title="이송 이력 — PICK/DROP 짝(TO-…)별 재고 이동"
          onClick={() => setOrdersOpen(true)}
          data-testid="tm-orders"
        >
          <ArrowLeftRight size={13} />
        </Button>
        <RobotChip
          chip={chip}
          testid="tm-robot"
          title={`이 목록은 ${robotLabel(chip)} 의 Task — 사이드바 로봇 목록에서 바꿉니다`}
        />
      </div>

      <DataTable
        rows={rows}
        columns={cols}
        rowKey={(t) => t.id}
        onPick={(t) => open(t.id, ids)}
        density="compact"
        stickyHeader
        className="max-h-72 overflow-y-auto"
        loading={view === 'history' && loading && !page}
        emptyDense
        empty={view === 'live' ? '진행 중인 Task 없음' : '종결된 Task 없음'}
        testid={view === 'live' ? 'tm-live-table' : 'tm-history-table'}
        // 진행 중 Task 는 여기서 바로 취소(로봇 대기열에서 삭제) — 확인 창이 로봇과 짝(PICK/DROP)을 말한다.
        actions={
          view === 'live'
            ? (t) => <TaskActions task={t} row only={['cancel']} testid="tm-row-action" />
            : undefined
        }
      />

      {view === 'history' ? (
        <div className="flex flex-none items-center gap-2 border-t border-line-default px-2 py-1">
          {error ? <span className="truncate text-2xs text-fault-fg">{error}</span> : null}
          {page && page.total > 0 ? (
            <Pagination
              total={page.total}
              limit={LIMIT}
              offset={offset}
              onMove={setOffset}
              testid="tm-pager"
            />
          ) : null}
          <Button
            size="icon-sm"
            intent="ghost"
            className="ml-auto"
            aria-label="다시 읽기"
            title="다시 읽기"
            loading={loading}
            onClick={() => void load(offset)}
          >
            <RefreshCw size={13} />
          </Button>
        </div>
      ) : null}

      <TaskGenDialog open={genOpen} onOpenChange={setGenOpen} />
      <TransferOrdersDialog
        open={ordersOpen}
        onOpenChange={setOrdersOpen}
        robot={robots.selected}
      />
      <Dialog
        open={filterOpen}
        onOpenChange={setFilterOpen}
        title="히스토리 필터"
        size="sm"
        closeLabel="닫기"
        testid="tm-filter-dialog"
      >
        <div className="grid grid-cols-2 gap-2">
          <Select
            dense
            label="State"
            value={filter.states[0] ?? ''}
            onValueChange={setState}
            data-testid="tm-filter-state"
          >
            <option value="">전체</option>
            {TERMINAL.map((s) => (
              <option key={s} value={s}>
                {STATE_LABEL[s]}
              </option>
            ))}
          </Select>
          <Select
            dense
            label="TaskType"
            value={filter.type}
            onValueChange={(v) => setFilter((f) => ({ ...f, type: v as TaskType | '' }))}
            data-testid="tm-filter-type"
          >
            <option value="">전체</option>
            {TYPES.map((t) => (
              <option key={t} value={t}>
                {t}
              </option>
            ))}
          </Select>
          <Select
            dense
            label="Origin"
            value={filter.origin}
            onValueChange={(v) => setFilter((f) => ({ ...f, origin: v as Task['origin'] | '' }))}
            data-testid="tm-filter-origin"
          >
            <option value="">전체</option>
            {ORIGINS.map((o) => (
              <option key={o} value={o}>
                {ORIGIN_LABEL[o]}
              </option>
            ))}
          </Select>
          <Input
            dense
            type="date"
            label="Since"
            value={filter.since}
            onValueChange={(v) => setFilter((f) => ({ ...f, since: v }))}
            data-testid="tm-filter-since"
          />
        </div>
        {nFilter ? (
          <Button
            size="sm"
            intent="ghost"
            className="mt-2"
            onClick={() => setFilter(EMPTY_HISTORY_FILTER)}
            data-testid="tm-filter-clear"
          >
            필터 지우기
          </Button>
        ) : null}
      </Dialog>
    </Card>
  )
}
