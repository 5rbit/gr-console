// Task 관리 화면 — **표가 곧 화면이다**. 머리띠(요약) · 조작 띠(검색·필터·보기) · 표 하나.
//
// 예전에는 머리띠 + 필터 띠(칩 열 개) + 목록 카드 + 종결 이력 카드 + PLC 판넬이 한 번에 서 있었다.
// 그중 눈이 실제로 읽는 것은 표 한 줄이었고, 나머지는 그 표가 설 자리를 먹었다. 그래서
//  - 고르는 자리(상태·로봇·종류·종결 포함)는 `필터` 팝오버로 접고 **걸린 것만 칩**으로 남긴다,
//  - 목록과 종결 이력은 **한 자리에서 보기 전환**(둘 다 표 하나다, 위아래로 쌓지 않는다),
//  - 상세와 PLC 뷰는 행을 누르면 열리는 **시트**로 옮겼다(PLC 뷰는 그 시트의 탭).
//
// 헤더 요약: 실행 n · 대기 n · 오늘 완료 n · 평균 소요 — 앞 둘은 스토어 집계, 뒤 둘은
// `GET /api/tasks/stats`(5초 폴, 새 종결이 생기면 즉시). 오른쪽 끝의 점은 Task SSE 연결 상태다.
//
// 대조용 `WebMon.Stat.Task`는 1초 틱으로 **각 Task 의 로봇** 상태(`allStatus`)를 읽는다 — GR1 Task 를 GR2
// 배열과 대조하면 늘 "불일치"가 뜬다. 20Hz 피드를 구독하면 표가 초당 스무 번 다시 그리므로 구독하지 않고
// 열어만 둔다. 같은 틱이 경과 시간 열도 움직인다.
import { useCallback, useEffect, useMemo, useState } from 'react'
import { History, ListChecks } from 'lucide-react'
import { allStatus, tasksFeed } from '../../lib/feeds'
import { nav } from '../../lib/nav'
import { panels } from '../../lib/panels'
import { visibleInterval } from '../../lib/poll'
import { robots } from '../../lib/robots'
import { useSse } from '../../lib/sse'
import { useStore } from '../../lib/store'
import { tasks } from '../../lib/tasks'
import { ScreenHeader, type MetaItem } from '../../lib/ui/ScreenHeader'
import { Segmented } from '../../lib/ui/Segmented'
import { StatusDot } from '../../lib/ui/StatusDot'
import { taskApi, type TaskStats } from '../../lib/task/actions'
import {
  EMPTY_FILTER,
  endedToday,
  fmtElapsed,
  isTerminal,
  matchesFilter,
  typeName,
  type PlcTaskArea,
  type TaskFilter,
} from '../../lib/task/state'
import type { Task } from '../../lib/types'
import TaskDetail, { TASK_DETAIL_PANEL } from './TaskDetail'
import { TaskFilters } from './TaskFilters'
import { TaskHistory } from './TaskHistory'
import { TaskTable } from './TaskTable'

const STATS_MS = 5000

type View = 'active' | 'history'

/**
 * 필터와 보기를 **기억한다** — 이 화면은 탭(도킹 존·사이드바)이라 다른 화면을 보면 통째로 unmount
 * 되고, 돌아오면 걸어 둔 필터가 사라져 있었다. 같은 일을 하러 다시 들어온 사람에게 같은 질문을 두
 * 번 하지 않는다(측정 모니터의 `gr-measure-sub` 와 같은 관용구).
 */
const FILTER_KEY = 'gr-task-filter'
const VIEW_KEY = 'gr-task-view'

function loadFilter(): TaskFilter {
  try {
    const raw = localStorage.getItem(FILTER_KEY)
    if (!raw) return EMPTY_FILTER
    // 저장된 모양이 지금 계약과 다를 수 있다(구버전·손으로 고친 값) — 아는 키만 받는다.
    const v = JSON.parse(raw) as Partial<TaskFilter>
    return { ...EMPTY_FILTER, ...v }
  } catch {
    return EMPTY_FILTER
  }
}

function loadView(): View {
  try {
    return localStorage.getItem(VIEW_KEY) === 'history' ? 'history' : 'active'
  } catch {
    return 'active'
  }
}

function save(key: string, value: string): void {
  try {
    localStorage.setItem(key, value)
  } catch {
    /* 저장 못 해도 동작 */
  }
}

export default function TaskManager() {
  useStore(tasks, nav, robots)
  useSse(tasksFeed)
  useEffect(() => tasks.start(), [])
  // 상태 피드는 **구독하지 않고** 열어만 둔다(대조용 스냅샷을 1초 틱으로 읽는다).
  useEffect(() => allStatus.start(), [])

  const [now, setNow] = useState(() => Date.now())
  const [stats, setStats] = useState<TaskStats | null>(null)
  const [filter, setFilterState] = useState<TaskFilter>(loadFilter)
  const [view, setViewState] = useState<View>(loadView)
  const [selected, setSelected] = useState<string | null>(null)
  const robot = robots.selected

  useEffect(() => {
    const t = visibleInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(t)
  }, [])

  const setFilter = useCallback((f: TaskFilter) => {
    setFilterState(f)
    save(FILTER_KEY, JSON.stringify(f))
  }, [])
  const setView = useCallback((v: View) => {
    setViewState(v)
    save(VIEW_KEY, v)
  }, [])

  // `now` 가 바뀔 때만 새 함수 — 표는 1초에 한 번 대조를 다시 계산한다.
  const areaFor = useCallback(
    (t: Task): PlcTaskArea | null => allStatus.ofPlc(t.plc_name)?.webmon.Stat.Task ?? null,
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 1초 틱이 새로 읽는 시점이다
    [now],
  )

  const list = tasks.list
  const counts = tasks.counts
  const terminalCount = useMemo(() => list.filter((t) => isTerminal(t.state)).length, [list])

  // 통계 — 주기 폴 + 종결 건수가 바뀌면 즉시.
  useEffect(() => {
    let alive = true
    setStats(null)
    const tick = () =>
      taskApi
        .stats(robot)
        .then((s) => {
          if (alive) setStats(s)
        })
        .catch(() => {})
    void tick()
    const t = visibleInterval(() => void tick(), STATS_MS)
    return () => {
      alive = false
      clearInterval(t)
    }
  }, [terminalCount, robot])

  const rows = useMemo(() => list.filter((t) => matchesFilter(t, filter, typeName)), [list, filter])
  // **값으로** 본다 — 기억해 둔 필터를 되살리면 객체가 `EMPTY_FILTER` 와 같은 참조일 수 없다
  // (참조로 보면 필터를 안 걸었는데도 "필터에 맞는 Task 가 없습니다"라고 말한다).
  const hasFilter =
    filter.states.length > 0 ||
    filter.type !== '' ||
    filter.q !== '' ||
    filter.robot !== '' ||
    filter.includeTerminal !== EMPTY_FILTER.includeTerminal
  // 시트의 `←`/`→`가 따라갈 순서 — **지금 표에 보이는 것** 그대로다.
  const rowIds = useMemo(() => rows.map((t) => t.id), [rows])

  // 상세 시트 — 같은 패널 id라 다시 열면 자리를 옮기는 대신 내용이 갈린다(제목도 따라간다).
  const openDetail = useCallback((id: string, ids: readonly string[]) => {
    setSelected(id)
    const t = tasks.get(id)
    panels.open({
      id: TASK_DETAIL_PANEL,
      mode: 'sidebar',
      title: t ? `Task #${t.seq}` : 'Task 상세',
      component: TaskDetail,
      props: { id, ids, onNavigate: (next: string) => openDetail(next, ids) },
    })
  }, [])

  // 다른 화면이 지목한 Task(`nav.goTask`) — 신호를 소비하며 연다.
  useEffect(() => {
    const id = nav.consumeTaskId()
    if (id) openDetail(id, rowIds)
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 지목 신호에만 반응한다(목록이 갈릴 때마다 열지 않게)
  }, [nav.taskId, openDetail])

  const current = robots.current
  // 통계는 고른 로봇 것 — 로봇이 둘 이상이면 라벨에 이름을 붙여 목록(전체)과 섞어 읽지 않게 한다.
  const tag = robots.multi && current ? ` · ${current.name}` : ''
  const mine = tag && current ? tasks.countsFor(current.plc) : counts
  const completedToday =
    stats?.completed_today ??
    list.filter(
      (t) =>
        t.state === 'completed' &&
        endedToday(t, now) &&
        (!tag || !t.plc_name || t.plc_name === current?.plc),
    ).length
  const items: MetaItem[] = [
    // 실행·대기도 통계처럼 **고른 로봇** 것 — 예전에는 두 로봇 합이 "오늘 완료 · GR2" 옆에 섞여 섰다.
    { label: `실행${tag}`, value: String(mine.running) },
    { label: `대기${tag}`, value: String(mine.queued) },
    { label: `오늘 완료${tag}`, value: String(completedToday) },
  ]
  if (stats?.rejected_today)
    items.push({ label: `오늘 거부${tag}`, value: String(stats.rejected_today) })
  if (stats?.avg_complete_secs != null)
    items.push({ label: `평균 소요${tag}`, value: fmtElapsed(stats.avg_complete_secs) })

  const feedStatus = tasksFeed.connected ? 'ok' : tasksFeed.error ? 'fault' : 'neutral'
  const onPick = (t: Task) => openDetail(t.id, rowIds)

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="screen-taskmgr">
      <ScreenHeader
        title="Task 관리"
        icon={<ListChecks className="h-4 w-4" />}
        items={items}
        trailing={
          <StatusDot
            status={feedStatus}
            size="sm"
            label="SSE"
            title={
              tasksFeed.error ??
              (tasksFeed.connected ? 'Task 스트림 연결됨' : 'Task 스트림 연결 중')
            }
          />
        }
      />
      <TaskFilters
        value={filter}
        onChange={setFilter}
        counts={counts}
        disabled={view === 'history'}
        meta={
          view === 'active'
            ? `${rows.length}건${rows.length !== list.length ? ` / 전체 ${list.length}건` : ''}`
            : null
        }
      >
        {/* 진행 목록과 종결 이력은 **같은 열의 같은 표**다 — 위아래로 쌓지 않고 한 자리를 나눠 쓴다. */}
        <Segmented
          value={view}
          onChange={setView}
          ariaLabel="보기"
          options={[
            { id: 'active', label: '진행', icon: <ListChecks size={12} />, testid: 'view-active' },
            {
              id: 'history',
              label: '종결 이력',
              icon: <History size={12} />,
              testid: 'view-history',
            },
          ]}
        />
      </TaskFilters>

      <div className="min-h-0 flex-1 overflow-auto">
        {view === 'active' ? (
          <TaskTable
            rows={rows}
            area={null}
            areaFor={areaFor}
            now={now}
            selected={selected}
            onPick={onPick}
            rowActions
            loading={!tasksFeed.lastAt && list.length === 0}
            emptyHint={
              hasFilter
                ? '필터에 맞는 Task가 없습니다 — 칩을 지우거나 종결 이력을 보세요.'
                : '작업 명령 화면에서 Task를 제출하면 여기에 나타납니다.'
            }
          />
        ) : (
          <TaskHistory
            selected={selected}
            onPick={(t, ids) => openDetail(t.id, ids)}
            refreshKey={terminalCount}
            now={now}
          />
        )}
      </div>
    </div>
  )
}
