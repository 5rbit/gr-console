// 종결 이력 — 서버 페이징(`GET /api/tasks?state=terminal&offset=&limit=50`). 스토어는 최근 하루치만
// 들고 있어 그 너머는 여기서만 보인다. 새 종결이 생기면(`refreshKey`) 첫 페이지를 다시 받는다.
import { useCallback, useEffect, useState } from 'react'
import { History, RefreshCw } from 'lucide-react'
import { api } from '../../lib/api'
import { Button } from '../../lib/ui/Button'
import { Card } from '../../lib/ui/Card'
import { DataTable } from '../../lib/ui/DataTable'
import { Pagination } from '../../lib/ui/Pagination'
import type { Column } from '../../lib/ui/table'
import { ORIGIN_LABEL, elapsed, fmtElapsed, fmtTime, typeName } from '../../lib/task/state'
import type { Task, TaskPage } from '../../lib/types'
import { AckCell, StateCell, targetLabel } from './TaskTable'

const LIMIT = 50

export interface TaskHistoryProps {
  selected: string | null
  onPick: (task: Task) => void
  /** 바뀌면 첫 페이지를 다시 받는다(스토어의 종결 건수). */
  refreshKey: number
  now: number
}

export function TaskHistory({ selected, onPick, refreshKey, now }: TaskHistoryProps) {
  const [offset, setOffset] = useState(0)
  const [page, setPage] = useState<TaskPage | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async (off: number) => {
    setLoading(true)
    try {
      setPage(await api.tasks({ state: 'terminal', offset: off, limit: LIMIT }))
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void load(offset)
  }, [load, offset])

  // 새 종결 — 첫 페이지를 보고 있을 때만 조용히 갱신(다른 페이지를 읽는 중이면 밀리지 않게).
  useEffect(() => {
    if (offset === 0) void load(0)
  }, [refreshKey, load, offset])

  const columns: Column<Task>[] = [
    { key: 'seq', label: '#', get: (t) => t.seq, numeric: true, class: 'w-12' },
    {
      key: 'state',
      label: '상태',
      get: (t) => t.state,
      cell: (t) => <StateCell task={t} area={null} />,
    },
    { key: 'type', label: '종류', get: (t) => typeName(t.plc_task?.TaskType) },
    { key: 'target', label: '대상', get: (t) => targetLabel(t) },
    { key: 'item', label: '품목', get: (t) => t.plc_task?.Item?.Code || null, numeric: true },
    {
      key: 'ack',
      label: 'Ack',
      get: (t) => t.ack?.code ?? null,
      cell: (t) => <AckCell task={t} />,
      numeric: true,
    },
    { key: 'origin', label: '출처', get: (t) => ORIGIN_LABEL[t.origin] },
    {
      key: 'ended',
      label: '종결',
      get: (t) => t.ended_at,
      cell: (t) => <span className="tabular-nums">{fmtTime(t.ended_at, now)}</span>,
    },
    {
      key: 'elapsed',
      label: '소요',
      get: (t) => elapsed(t, now),
      cell: (t) => <span className="tabular-nums">{fmtElapsed(elapsed(t, now))}</span>,
      numeric: true,
    },
    {
      key: 'reason',
      label: '사유',
      get: (t) => t.error ?? (t.ack && !t.ack.accepted ? t.ack.reason : null),
      cell: (t) => {
        const r = t.error ?? (t.ack && !t.ack.accepted ? t.ack.reason : '')
        return r ? (
          <span className="block max-w-64 truncate text-slate-500" title={r}>
            {r}
          </span>
        ) : (
          <span className="text-slate-300">—</span>
        )
      },
    },
  ]

  return (
    <Card padded={false} className="flex min-h-0 flex-col">
      <div
        className="flex items-center gap-2 border-b border-slate-200 px-3 py-1.5 dark:border-slate-700"
        data-testid="task-history"
      >
        <History size={14} className="text-slate-400" />
        <span className="text-xs font-semibold">종결 이력</span>
        {error ? (
          <span className="truncate text-[11px] text-red-600 dark:text-red-400">{error}</span>
        ) : null}
        <span className="flex-1" />
        <Button
          size="icon-sm"
          intent="ghost"
          aria-label="다시 읽기"
          title="다시 읽기"
          loading={loading}
          onClick={() => void load(offset)}
        >
          <RefreshCw size={13} />
        </Button>
      </div>
      <div className="min-h-0 overflow-auto">
        <DataTable
          rows={page?.items ?? []}
          columns={columns}
          rowKey={(t) => t.id}
          onPick={onPick}
          selected={selected}
          loading={loading && !page}
          empty="종결된 Task 없음"
          emptyHint="완료·취소·거부·실패한 Task가 여기에 쌓입니다."
          testid="history-table"
        />
      </div>
      {page && page.total > 0 ? (
        <div className="border-t border-slate-200 px-3 py-1.5 dark:border-slate-700">
          <Pagination
            total={page.total}
            limit={LIMIT}
            offset={offset}
            onMove={setOffset}
            testid="history-pager"
          />
        </div>
      ) : null}
    </Card>
  )
}
