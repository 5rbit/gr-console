// Task 관리 화면 — 진행 목록(SSE 스토어) · 필터 · 상세 드로어 · 조작 · 종결 이력(서버 페이징) · PLC 뷰.
//
// 헤더 요약: 실행 n · 대기 n · 오늘 완료 n · 평균 소요 — 앞 둘은 스토어 집계, 뒤 둘은
// `GET /api/tasks/stats`(5초 폴, 새 종결이 생기면 즉시). 오른쪽 끝의 점은 Task SSE 연결 상태다.
//
// 대조용 `WebMon.Stat.Task`는 1초 틱으로 `statusFeed.data`를 읽는다 — 20Hz 피드를 구독하면 표가
// 초당 스무 번 다시 그린다. 같은 틱이 경과 시간 열도 움직인다.
import { useCallback, useEffect, useMemo, useState } from 'react'
import { Cpu, ListChecks } from 'lucide-react'
import { statusFeed, tasksFeed } from '../../lib/feeds'
import { nav } from '../../lib/nav'
import { panels } from '../../lib/panels'
import { visibleInterval } from '../../lib/poll'
import { useSse } from '../../lib/sse'
import { useStore } from '../../lib/store'
import { tasks } from '../../lib/tasks'
import { Button } from '../../lib/ui/Button'
import { Card } from '../../lib/ui/Card'
import { ScreenHeader, type MetaItem } from '../../lib/ui/ScreenHeader'
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
import { PlcView } from './PlcView'
import TaskDetail, { TASK_DETAIL_PANEL } from './TaskDetail'
import { TaskFilters } from './TaskFilters'
import { TaskHistory } from './TaskHistory'
import { TaskTable } from './TaskTable'

const STATS_MS = 5000

export default function TaskManager() {
  useStore(tasks, nav)
  useSse(tasksFeed)
  useEffect(() => tasks.start(), [])
  // 상태 피드는 **구독하지 않고** 열어만 둔다(대조용 스냅샷을 1초 틱으로 읽는다).
  useEffect(() => statusFeed.start(), [])

  const [now, setNow] = useState(() => Date.now())
  const [area, setArea] = useState<PlcTaskArea | null>(
    () => statusFeed.data?.webmon.Stat.Task ?? null,
  )
  const [stats, setStats] = useState<TaskStats | null>(null)
  const [filter, setFilter] = useState<TaskFilter>(EMPTY_FILTER)
  const [plcView, setPlcView] = useState(false)
  const [selected, setSelected] = useState<string | null>(null)

  useEffect(() => {
    const t = visibleInterval(() => {
      setNow(Date.now())
      setArea(statusFeed.data?.webmon.Stat.Task ?? null)
    }, 1000)
    return () => clearInterval(t)
  }, [])

  const list = tasks.list
  const counts = tasks.counts
  const terminalCount = useMemo(() => list.filter((t) => isTerminal(t.state)).length, [list])

  // 통계 — 주기 폴 + 종결 건수가 바뀌면 즉시.
  useEffect(() => {
    let alive = true
    const tick = () =>
      taskApi
        .stats()
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
  }, [terminalCount])

  const openDetail = useCallback((id: string) => {
    setSelected(id)
    const t = tasks.get(id)
    panels.open({
      id: TASK_DETAIL_PANEL,
      mode: 'sidebar',
      title: t ? `Task #${t.seq}` : 'Task 상세',
      component: TaskDetail,
      props: { id },
    })
  }, [])

  // 다른 화면이 지목한 Task(`nav.goTask`) — 신호를 소비하며 연다.
  useEffect(() => {
    const id = nav.consumeTaskId()
    if (id) openDetail(id)
  }, [nav.taskId, openDetail])

  const rows = useMemo(() => list.filter((t) => matchesFilter(t, filter, typeName)), [list, filter])

  const completedToday =
    stats?.completed_today ??
    list.filter((t) => t.state === 'completed' && endedToday(t, now)).length
  const items: MetaItem[] = [
    { label: '실행', value: String(counts.running) },
    { label: '대기', value: String(counts.queued) },
    { label: '오늘 완료', value: String(completedToday) },
  ]
  if (stats?.rejected_today) items.push({ label: '오늘 거부', value: String(stats.rejected_today) })
  if (stats?.avg_complete_secs != null)
    items.push({ label: '평균 소요', value: fmtElapsed(stats.avg_complete_secs) })

  const feedStatus = tasksFeed.connected ? 'ok' : tasksFeed.error ? 'fault' : 'neutral'
  const selectedTask = selected ? tasks.get(selected) : null

  const onPick = (t: Task) => openDetail(t.id)
  const onPickKey = (workId: number, taskId: number) => {
    const t = list.find((x) => x.work_id === workId && x.task_id === taskId)
    if (t) openDetail(t.id)
  }

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="screen-taskmgr">
      <ScreenHeader
        title="Task 관리"
        icon={<ListChecks className="h-4 w-4" />}
        items={items}
        trailing={
          <span className="flex items-center gap-2">
            <Button
              size="sm"
              intent={plcView ? 'primary' : 'outline'}
              active={plcView}
              icon={<Cpu size={13} />}
              aria-pressed={plcView}
              data-testid="toggle-plc-view"
              onClick={() => setPlcView(!plcView)}
            >
              PLC 뷰
            </Button>
            <StatusDot
              status={feedStatus}
              size="sm"
              label="SSE"
              title={
                tasksFeed.error ??
                (tasksFeed.connected ? 'Task 스트림 연결됨' : 'Task 스트림 연결 중')
              }
            />
          </span>
        }
      />
      <TaskFilters value={filter} onChange={setFilter} counts={counts} />
      <div className="flex min-h-0 flex-1">
        <div className="flex min-w-0 flex-1 flex-col gap-3 overflow-auto p-3">
          <Card padded={false}>
            <div
              className="flex items-center gap-2 border-b border-slate-200 px-3 py-1.5 dark:border-slate-700"
              data-testid="task-active"
            >
              <span className="text-xs font-semibold">Task 목록</span>
              <span className="text-2xs text-slate-400 tabular-nums">
                {rows.length}건{rows.length !== list.length ? ` / 전체 ${list.length}건` : ''}
              </span>
              {filter.states.length === 0 && !filter.includeTerminal ? (
                <span className="text-2xs text-slate-400">
                  진행 중만 — 종결은 아래 이력 또는 '종결 포함'
                </span>
              ) : null}
            </div>
            <TaskTable
              rows={rows}
              area={area}
              now={now}
              selected={selected}
              onPick={onPick}
              loading={!tasksFeed.lastAt && list.length === 0}
              emptyHint={
                filter !== EMPTY_FILTER
                  ? '필터에 맞는 Task가 없습니다.'
                  : '작업 명령 화면에서 Task를 제출하면 여기에 나타납니다.'
              }
            />
          </Card>
          <TaskHistory selected={selected} onPick={onPick} refreshKey={terminalCount} now={now} />
        </div>
        {plcView ? (
          <aside
            className="w-[22rem] shrink-0 overflow-auto border-l border-line-default bg-surface-panel"
            data-testid="plc-view-pane"
          >
            <PlcView
              onPickKey={onPickKey}
              highlight={
                selectedTask
                  ? { work_id: selectedTask.work_id, task_id: selectedTask.task_id }
                  : null
              }
            />
          </aside>
        ) : null}
      </div>
    </div>
  )
}
