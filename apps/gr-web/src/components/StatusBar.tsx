// 하단 상태바 — 셸 전역 상태 한 줄(보고 있는 것·SSE 연결·레이아웃 종합·Task 집계·폴 감속·단축키).
// 메뉴바는 네비게이션만 맡고, 상태는 폭이 남는 이 한 줄이 맡는다.
//
// 왼쪽 끝이 **지금 무엇을 보고 있나**다. 단일 화면 모드에서는 화면 이름 하나지만, 도킹 모드에서는
// 한 화면에 패널이 여럿이라 이름 하나로는 못 말한다 — 그래서 보이는 패널을 늘어놓고, 그 자리를
// 누르면 배치 프리셋을 고를 수 있게 팔레트를 연다(상태 표시가 조작으로 이어지는 유일한 자리다).
import { useEffect, useState } from 'react'
import { pollFactor } from '../lib/poll'
import { palette } from '../lib/palette'
import { plcs } from '../lib/plcs'
import { tasks } from '../lib/tasks'
import { tasksFeed, useSelectedStatus } from '../lib/feeds'
import { robots } from '../lib/robots'
import { useSse } from '../lib/sse'
import { useStore } from '../lib/store'
import { StatusDot } from '../lib/ui/StatusDot'
import type { Status } from '../lib/ui/status'
import { chord } from '../lib/keys'
import { ALL_TABS } from '../lib/tabs'
import { workspace } from '../lib/workspace/store'
import { visibleLabels } from './workspace/WorkspaceShell'

export interface StatusBarProps {
  tab: string
}

function feedStatus(connected: boolean, error: string | null): Status {
  return connected ? 'ok' : error ? 'fault' : 'neutral'
}

export function StatusBar({ tab }: StatusBarProps) {
  useStore(plcs, tasks, workspace)
  // 상태 점은 **고른 로봇**의 스트림 — 사이드바에서 호기를 바꾸면 이 점도 그 로봇 것을 말한다.
  const statusFeed = useSelectedStatus()
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
      className="flex h-6 shrink-0 items-center gap-3 border-t border-line-default bg-surface-inset px-2 text-2xs text-content-muted"
      data-testid="statusbar"
    >
      {workspace.enabled ? (
        <button
          type="button"
          className="max-w-[22rem] shrink-0 truncate rounded px-1 font-medium text-content-tertiary hover:bg-surface-active"
          title="보이는 패널 — 누르면 배치·패널 명령"
          data-testid="sb-panes"
          onClick={() => palette.show('배치')}
        >
          {workspace.layout.maximized ? '최대화 · ' : ''}
          {visibleLabels().join(' · ') || tab}
        </button>
      ) : (
        <span className="shrink-0 font-medium text-content-tertiary">{tab}</span>
      )}

      <span className="flex shrink-0 items-center gap-2" data-testid="sb-feeds">
        <StatusDot
          status={feedStatus(statusFeed.connected, statusFeed.error)}
          size="sm"
          label={`상태 ${robots.current?.name ?? ''}`.trim()}
          title={
            statusFeed.error ?? `${robots.current?.name ?? '기본 로봇'} 상태 스트림 연결됨`
          }
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
        {/* 정상은 말로 반복하지 않는다 — 점이 초록이면 OK다(`docs/DESIGN.md` 4절 ④). */}
        Layout {layout === false ? '불일치' : layout === null && plcs.loaded ? '미검사' : ''}
      </span>

      <span className="shrink-0" data-testid="sb-counts">
        Task <strong className="font-medium tabular-nums">{counts.total}</strong>
        {/* 괄호 대신 가운뎃점 — 이 띠의 다른 묶음과 같은 구분자를 쓴다(눈이 구획을 한 규칙으로 읽는다). */}
        <span className="text-content-faint tabular-nums">
          {' '}
          · Active {counts.active} · Running {counts.running} · Queued {counts.queued}
        </span>
      </span>

      <span className="flex-1"></span>

      {slow > 1.05 ? (
        // 폴이 물러난 상태 — 화면이 밀리고 있다는 사실을 값 대신 여기서 말한다.
        <span
          className="shrink-0 rounded bg-warn-soft px-1 text-warn-fg tabular-nums"
          title="화면이 밀려 폴링 주기를 자동으로 늘렸습니다(메인스레드 보호). 한가해지면 원래대로 돌아옵니다."
          data-testid="sb-poll-slow"
        >
          폴 ×{slow.toFixed(1)}
        </span>
      ) : null}

      {/* 숫자키는 포커스가 버튼·입력에 없을 때만 산다. 팔레트 키는 입력 안에서도 산다. */}
      <span
        className="hidden shrink-0 sm:inline"
        title={`1~${Math.min(ALL_TABS.length, 9)} 화면 이동 — 포커스가 버튼·입력에 없을 때만`}
      >
        <kbd className="font-mono">1~{Math.min(ALL_TABS.length, 9)}</kbd> 화면 이동
      </span>
      <span className="hidden shrink-0 md:inline" title="명령 팔레트 — 패널 · 배치 · 존 · 설정">
        <kbd className="font-mono">{chord('K')}</kbd> 명령
      </span>
    </footer>
  )
}
