// 하단 상태바 — 셸 전역 상태 한 줄(활성 화면·SSE 연결·레이아웃 종합·Task 집계·폴 감속·단축키).
// 메뉴바는 네비게이션만 맡고, 상태는 폭이 남는 이 한 줄이 맡는다.
import { useEffect, useState } from 'react'
import { pollFactor } from '../lib/poll'
import { plcs } from '../lib/plcs'
import { tasks } from '../lib/tasks'
import { statusFeed, tasksFeed } from '../lib/feeds'
import { useSse } from '../lib/sse'
import { useStore } from '../lib/store'
import { StatusDot } from '../lib/ui/StatusDot'
import type { Status } from '../lib/ui/status'

export interface StatusBarProps {
  tab: string
}

function feedStatus(connected: boolean, error: string | null): Status {
  return connected ? 'ok' : error ? 'fault' : 'neutral'
}

export function StatusBar({ tab }: StatusBarProps) {
  useStore(plcs, tasks)
  useSse(statusFeed)
  useSse(tasksFeed)
  useEffect(() => plcs.start(), [])
  useEffect(() => tasks.start(), [])

  // 폴 감속 표시 — 메인스레드가 밀리면 폴이 스스로 물러난다(`lib/poll`). 감속 중일 때만 뜬다.
  const [slow, setSlow] = useState(1)
  useEffect(() => {
    const t = setInterval(() => setSlow(pollFactor()), 1000)
    return () => clearInterval(t)
  }, [])

  const layout = plcs.layoutOk
  const counts = tasks.counts

  return (
    <footer
      className="flex h-6 shrink-0 items-center gap-3 border-t border-slate-200 bg-slate-100/70 px-2 text-[11px] text-slate-500 dark:border-slate-700 dark:bg-slate-800/50 dark:text-slate-400"
      data-testid="statusbar"
    >
      <span className="shrink-0 font-medium text-slate-600 dark:text-slate-300">{tab}</span>

      <span className="flex shrink-0 items-center gap-2" data-testid="sb-feeds">
        <StatusDot
          status={feedStatus(statusFeed.connected, statusFeed.error)}
          size="sm"
          label="상태"
          title={statusFeed.error ?? '상태 스트림 연결됨'}
        />
        <StatusDot
          status={feedStatus(tasksFeed.connected, tasksFeed.error)}
          size="sm"
          label="Task"
          title={tasksFeed.error ?? 'Task 스트림 연결됨'}
        />
      </span>

      <span className="flex shrink-0 items-center gap-1" data-testid="sb-layout">
        <StatusDot
          status={layout === true ? 'ok' : layout === false ? 'fault' : 'neutral'}
          size="sm"
        />
        레이아웃{' '}
        {layout === true ? 'OK' : layout === false ? '불일치' : plcs.loaded ? '미검사' : '—'}
      </span>

      <span className="shrink-0" data-testid="sb-counts">
        Task <strong className="font-medium tabular-nums">{counts.total}</strong>
        <span className="text-slate-400">
          (진행 {counts.active} · 실행 {counts.running} · 대기 {counts.queued})
        </span>
      </span>

      <span className="flex-1"></span>

      {slow > 1.05 ? (
        // 폴이 물러난 상태 — 화면이 밀리고 있다는 사실을 값 대신 여기서 말한다.
        <span
          className="shrink-0 rounded bg-amber-100 px-1 text-amber-700 tabular-nums dark:bg-amber-500/15 dark:text-amber-300"
          title="화면이 밀려 폴링 주기를 자동으로 늘렸습니다(메인스레드 보호). 한가해지면 원래대로 돌아옵니다."
          data-testid="sb-poll-slow"
        >
          폴 ×{slow.toFixed(1)}
        </span>
      ) : null}

      {/* 숫자키는 포커스가 버튼·입력에 없을 때만 산다. */}
      <span
        className="hidden shrink-0 sm:inline"
        title="1~4 화면 이동 — 포커스가 버튼·입력에 없을 때만"
      >
        <kbd className="font-mono">1~4</kbd> 화면 이동
      </span>
    </footer>
  )
}
