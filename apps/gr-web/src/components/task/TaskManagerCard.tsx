// Task Manager 카드 — 작업 명령 화면 오른쪽 칸, 작업 카드 **아래**. 원장(`/api/tasks`)에 오른 Task 를
// 고른 로봇 것만 두 보기로 본다(콘솔·시나리오 제출 + PLC 에서 처음 본 외부 Task 모두):
//   진행     = 스토어(SSE) — 실행 → PLC 대기열 순서.
//   히스토리 = 서버 페이징(종결, 최신 순) — 필터(State·TaskType·Origin·Since)는 대화상자로 접는다.
// 진행 행 끝에는 삭제·완료 버튼 둘이 선다(로봇이 AUTO 면 비활성). 칸이 좁아 열은 다섯뿐이다. 나머지(Ack·사유·시각·요청·PLC)는 행을 누르면 뜨는 Task 상세 팝업에 있다.
//
// 크기는 **고정**이다: 표 칸은 늘 20 행 높이(머리글 + 행 높이를 재서 맞춘다), 쪽 넘김 줄은 두 보기
// 모두 늘 있다 — 행 수·보기가 바뀌어도 카드가 커졌다 줄었다 하지 않고, 칸 안 스크롤도 없다. 넘치는 행은
// 쪽으로 넘긴다(진행 = 목록을 잘라서, 히스토리 = 서버 페이징). 카드는 오른쪽 칸 **아래에 붙는다**(`mt-auto`).
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
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
import { TaskActions } from '../taskmgr/TaskActions'
import { StateCell } from '../taskmgr/TaskTable'
import { TransferOrdersDialog } from './TransferOrdersDialog'
import { TaskGenDialog } from './TaskGenDialog'

const LIMIT = 20
/** 진행 행에 서는 조작 — PLC 로 가는 둘(삭제 = Delete, 완료 = Complete). 나머지는 행을 눌러 여는 상세에. */
const ROW_ACTIONS = ['cancel', 'complete'] as const
/**
 * 표 칸 높이의 재료(px) — 머리글, compact 행(StateCell 캡슐 포함). 기본값은 기본 밀도에서 잰 값이고,
 * 행이 그려지면 다시 재서 모듈에 남긴다: 빈 목록(행을 잴 수 없다)으로 보기를 바꿔도 카드 높이가 같아야 한다.
 */
let measured = { head: 20.5, row: 23.66 }
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
  // 진행 목록도 LIMIT 행씩 쪽으로 — 길어지면 카드가 자라지 않고 쪽이 는다.
  const [liveOffset, setLiveOffset] = useState(0)
  const liveMax = Math.max(0, Math.floor(Math.max(0, live.length - 1) / LIMIT) * LIMIT)
  const liveAt = Math.min(liveOffset, liveMax)
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

  const rows = view === 'live' ? live.slice(liveAt, liveAt + LIMIT) : (page?.items ?? [])

  // 표 칸 높이 = 머리글 + LIMIT × 행 높이. 행 높이는 그려진 첫 행을 재서 맞춘다(글꼴·밀도 설정을 따른다).
  const box = useRef<HTMLDivElement>(null)
  const [size, setSize] = useState(measured)
  useLayoutEffect(() => {
    const el = box.current
    if (!el) return
    const head = el.querySelector('thead')?.getBoundingClientRect().height
    const row = el.querySelector('tbody tr')?.getBoundingClientRect().height
    if (head && row && (Math.abs(head - size.head) > 0.5 || Math.abs(row - size.row) > 0.5)) {
      measured = { head, row }
      setSize(measured)
    }
  })
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
    <Card padded={false} className="mt-auto flex flex-none flex-col" data-testid="tm-card">
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

      <div
        ref={box}
        className="flex-none overflow-hidden"
        style={{ height: Math.ceil(size.head + LIMIT * size.row) }}
        data-testid="tm-body"
      >
        <DataTable
          rows={rows}
          columns={cols}
          rowKey={(t) => t.id}
          onPick={(t) => open(t.id, ids)}
          density="compact"
          loading={view === 'history' && loading && !page}
          emptyDense
          empty={view === 'live' ? '진행 중인 Task 없음' : '종결된 Task 없음'}
          // 진행 행 끝에 삭제·완료 — Task 관리 표와 같은 고정 슬롯 둘(AUTO 면 사유를 달고 비활성).
          actions={
            view === 'live'
              ? (t) => <TaskActions task={t} only={ROW_ACTIONS} row testid="tm-row-action" />
              : undefined
          }
          testid={view === 'live' ? 'tm-live-table' : 'tm-history-table'}
        />
      </div>

      <div className="flex flex-none items-center gap-2 border-t border-line-default px-2 py-1">
        {view === 'history' && error ? (
          <span className="truncate text-2xs text-fault-fg">{error}</span>
        ) : null}
        <span className="min-w-0 flex-1">
          {view === 'live' ? (
            <Pagination
              total={live.length}
              limit={LIMIT}
              offset={liveAt}
              onMove={setLiveOffset}
              testid="tm-live-pager"
            />
          ) : (
            <Pagination
              total={page?.total ?? 0}
              limit={LIMIT}
              offset={offset}
              onMove={setOffset}
              testid="tm-pager"
            />
          )}
        </span>
        <Button
          size="icon-sm"
          intent="ghost"
          aria-label="다시 읽기"
          title={view === 'history' ? '다시 읽기' : '진행 목록은 실시간(SSE)입니다'}
          loading={view === 'history' && loading}
          disabled={view !== 'history'}
          onClick={() => void load(offset)}
        >
          <RefreshCw size={13} />
        </Button>
      </div>

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
