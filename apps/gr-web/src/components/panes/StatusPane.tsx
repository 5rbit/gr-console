// 상태 패널 — WebMon 스냅샷의 값 줄(로봇·모드·작업·위치·알람). `Sidebar.tsx`의 상태 섹션을 뗀 것.
//
// **사이드바에서 고른 로봇**의 상태 스트림을 읽는다(`useSelectedStatus`) — 호기를 바꾸면 이 줄들이 그
// 로봇 것으로 바뀐다. 맨 윗줄에 로봇·PLC 이름을 두는 이유: 두 호기의 값이 비슷하면 지금 어느 것을 보는지
// 값만으로는 모른다.
//
// 라벨+값 짝으로만 선다(`ScreenHeader`와 같은 규칙) — 문장으로 풀어 쓰면 매번 다시 읽힌다.
// 여기에는 버튼이 없다: 이 면은 **표시**이고 조작은 화면의 도구 띠가 맡는다.
//
// 줄 수를 일곱에서 다섯으로 줄였다(2026-09-18). 측정 모니터가 제 머리에 모드·상태·Task·Step·알람을
// 한 줄로 세우면서 같은 값이 한 화면에 두 번 서게 됐고, 그중 **짝을 이루는 값끼리는 한 줄에 붙였다**
// (Work/Task + Step, 알람 + 대기열). 이 패널은 화면이 무엇이든 늘 보이는 요약이라 지우지는 않는다 —
// 측정 화면을 닫아도 "지금 저 호기가 무엇을 하고 있나"는 남아야 한다.
import { useSelectedStatus } from '../../lib/feeds'
import { modeName } from '../../lib/gr/const'
import { density } from '../../lib/density'
import { robots } from '../../lib/robots'
import { useStore } from '../../lib/store'
import { StatusDot } from '../../lib/ui/StatusDot'
import type { Status } from '../../lib/ui/status'

export default function StatusPane() {
  useStore(density)
  const feed = useSelectedStatus()

  const ev = feed.data
  const wm = ev?.webmon ?? null
  const gap = density.isCompact ? 'gap-y-0.5 py-1' : 'gap-y-1 py-2'

  return (
    <dl
      className={`grid min-h-0 flex-1 auto-rows-min grid-cols-[auto_1fr] gap-x-3 overflow-y-auto px-2 text-xs ${gap}`}
      data-testid="status-rows"
    >
      <dt className="text-content-faint">Robot</dt>
      <dd className="font-mono" data-testid="st-robot">
        {robots.current?.name ?? '기본'}
        {ev?.plc ? <span className="ml-1 text-content-faint">{ev.plc}</span> : null}
      </dd>
      <dt className="text-content-faint">Mode</dt>
      <dd className="font-mono" data-testid="st-mode">
        {wm ? modeName(wm.Mode) : '—'}
      </dd>
      {/* Work/Task 와 Step 은 늘 함께 읽는다 — 줄을 갈라 두면 눈이 위아래로 왕복한다. */}
      <dt className="text-content-faint">Task / Step</dt>
      <dd className="font-mono tabular-nums" data-testid="st-task">
        {wm ? `W${wm.Stat.Task.Now.WorkId} / T${wm.Stat.Task.Now.TaskId} · ${wm.Proc.Step.Now}` : '—'}
        {wm?.Proc.Msg ? <span className="ml-1 text-content-faint">{wm.Proc.Msg}</span> : null}
      </dd>
      <dt className="text-content-faint">Position</dt>
      <dd className="font-mono tabular-nums" data-testid="st-axis">
        {wm && wm.Axis.length >= 2
          ? `X ${wm.Axis[0].Position.toFixed(0)} · Y ${wm.Axis[1].Position.toFixed(0)}`
          : '—'}
      </dd>
      <dt className="text-content-faint">Alarm / Queue</dt>
      <dd className="flex items-center gap-2" data-testid="st-alarm">
        {wm ? (
          <>
            <StatusDot status={wm.Alarm.Fault ? 'fault' : 'neutral'} size="sm" label="Fault" />
            <StatusDot status={wm.Alarm.Warn ? 'warn' : 'neutral'} size="sm" label="Warn" />
            <span className="font-mono tabular-nums" data-testid="st-queue">
              {wm.Stat.Task.Queue.length}
            </span>
          </>
        ) : (
          '—'
        )}
      </dd>
    </dl>
  )
}

/** 머리띠 요약 — SSE 연결 점. 값이 아니라 "이 면을 믿어도 되는가"를 말한다. */
export function StatusSummary() {
  const feed = useSelectedStatus()
  const s: Status = feed.connected ? 'ok' : feed.error ? 'fault' : 'neutral'
  return (
    <StatusDot
      status={s}
      size="sm"
      label={robots.current?.name ?? 'SSE'}
      title={feed.error ?? `${robots.current?.name ?? '기본 로봇'} 상태 스트림`}
    />
  )
}
