// 상태 패널 — WebMon 스냅샷의 값 줄(모드·작업·스텝·알람·대기열). `Sidebar.tsx`의 상태 섹션을 뗀 것.
//
// 라벨+값 짝으로만 선다(`ScreenHeader`와 같은 규칙) — 문장으로 풀어 쓰면 매번 다시 읽힌다.
// 여기에는 버튼이 없다: 이 면은 **표시**이고 조작은 화면의 도구 띠가 맡는다.
import { statusFeed } from '../../lib/feeds'
import { modeName } from '../../lib/gr/const'
import { density } from '../../lib/density'
import { useSse } from '../../lib/sse'
import { useStore } from '../../lib/store'
import { StatusDot } from '../../lib/ui/StatusDot'
import type { Status } from '../../lib/ui/status'

export default function StatusPane() {
  useStore(density)
  useSse(statusFeed)

  const wm = statusFeed.data?.webmon ?? null
  const gap = density.isCompact ? 'gap-y-0.5 py-1' : 'gap-y-1 py-2'

  return (
    <dl
      className={`grid min-h-0 flex-1 auto-rows-min grid-cols-[auto_1fr] gap-x-3 overflow-y-auto px-2 text-xs ${gap}`}
      data-testid="status-rows"
    >
      <dt className="text-content-faint">모드</dt>
      <dd className="font-mono" data-testid="st-mode">
        {wm ? modeName(wm.Mode) : '—'}
      </dd>
      <dt className="text-content-faint">작업</dt>
      <dd className="font-mono tabular-nums" data-testid="st-task">
        {wm ? `W${wm.Stat.Task.Now.WorkId} / T${wm.Stat.Task.Now.TaskId}` : '—'}
      </dd>
      <dt className="text-content-faint">스텝</dt>
      <dd className="font-mono tabular-nums" data-testid="st-step">
        {wm ? `${wm.Proc.Step.Now}` : '—'}
        {wm?.Proc.Msg ? <span className="ml-1 text-content-faint">{wm.Proc.Msg}</span> : null}
      </dd>
      <dt className="text-content-faint">알람</dt>
      <dd className="flex items-center gap-2" data-testid="st-alarm">
        {wm ? (
          <>
            <StatusDot status={wm.Alarm.Fault ? 'fault' : 'neutral'} size="sm" label="Fault" />
            <StatusDot status={wm.Alarm.Warn ? 'warn' : 'neutral'} size="sm" label="Warn" />
          </>
        ) : (
          '—'
        )}
      </dd>
      <dt className="text-content-faint">대기열</dt>
      <dd className="font-mono tabular-nums" data-testid="st-queue">
        {wm ? wm.Stat.Task.Queue.length : '—'}
      </dd>
    </dl>
  )
}

/** 머리띠 요약 — SSE 연결 점. 값이 아니라 "이 면을 믿어도 되는가"를 말한다. */
export function StatusSummary() {
  useSse(statusFeed)
  const s: Status = statusFeed.connected ? 'ok' : statusFeed.error ? 'fault' : 'neutral'
  return <StatusDot status={s} size="sm" label="SSE" title={statusFeed.error ?? '상태 스트림'} />
}
