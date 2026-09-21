// 종결 이력 — 서버 페이징(`GET /api/tasks?state=terminal&offset=&limit=50`). 스토어는 최근 하루치만
// 들고 있어 그 너머는 여기서만 보인다. 새 종결이 생기면(`refreshKey`) 첫 페이지를 다시 받는다.
//
// 진행 목록과 **한 자리를 나눠 쓰므로** 여기에는 머리줄이 없다(어느 보기인지는 위 띠의 세그먼트가
// 말한다). 남은 띠는 아래 하나 — 쪽 이동과 다시 읽기.
import { useCallback, useEffect, useMemo, useState } from 'react'
import { RefreshCw } from 'lucide-react'
import { api } from '../../lib/api'
import { Button } from '../../lib/ui/Button'
import { DataTable } from '../../lib/ui/DataTable'
import { Pagination } from '../../lib/ui/Pagination'
import type { Task, TaskPage } from '../../lib/types'
import { taskColumns } from './TaskTable'

const LIMIT = 50

export interface TaskHistoryProps {
  selected: string | null
  /** 행 클릭 — 두 번째 인자는 **이 쪽에 보이는 순서**다(시트의 `←`/`→`가 그대로 따라간다). */
  onPick: (task: Task, ids: readonly string[]) => void
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

  // 진행 목록과 **같은 열**이다(`taskColumns`) — 종결 뒤에도 같은 자리에서 같은 값을 읽는다.
  const columns = useMemo(() => taskColumns(null, now), [now])
  const rows = page?.items ?? []
  const ids = useMemo(() => rows.map((t) => t.id), [rows])

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="task-history">
      <div className="min-h-0 flex-1 overflow-auto">
        <DataTable
          rows={rows}
          columns={columns}
          rowKey={(t) => t.id}
          onPick={(t) => onPick(t, ids)}
          selected={selected}
          loading={loading && !page}
          empty="종결된 Task 없음"
          emptyHint="완료·취소·거부·실패한 Task가 여기에 쌓입니다."
          testid="history-table"
          fit
        />
      </div>
      <div className="flex flex-none items-center gap-2 border-t border-line-default bg-surface-inset px-2 py-1">
        {error ? <span className="truncate text-2xs text-fault-fg">{error}</span> : null}
        {page && page.total > 0 ? (
          <Pagination
            total={page.total}
            limit={LIMIT}
            offset={offset}
            onMove={setOffset}
            testid="history-pager"
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
    </div>
  )
}
