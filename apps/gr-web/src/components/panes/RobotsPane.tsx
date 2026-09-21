// 로봇 패널 — 명령을 보낼 로봇을 고르는 자리. 예전에는 `Sidebar.tsx` 안의 섹션이었다.
//
// 왜 떼어 냈나: 도킹이 되는 순간 "왼쪽 사이드바의 로봇 섹션"이라는 것은 없다 — 같은 목록이 왼쪽에도
// 오른쪽에도 하단에도 설 수 있어야 하고, 그러려면 섹션이 **자기 머리띠 없이** 자기 몸만 그려야 한다
// (제목·탭·닫기는 도킹 껍데기가 그린다). 사이드바는 이제 이 패널들을 얹는 존 하나다.
//
// 행 하나 = **로봇 색 막대**(정체 — 상태가 아니다) · 이름 · 모드 칩 · 상태 칩 하나(2026-09-21).
// 예전에는 글자 없는 빨간 사각(게이트) + 둥근 색 점(로봇 색)이 나란히 서서 색 점까지 상태로 읽혔다.
// 색 막대는 행 왼쪽 끝에 붙어 점 모양이 아니고, 상태는 `제출 가능`/`제출 불가` 글자로 선다
// (사유는 칩 툴팁, 어휘는 `lib/indicators`).
//
// 행 우클릭 = 그 로봇의 운전 명령(Start/Stop/Reset/Buzzer Stop/Complete/Clear, 2026-09-21). 선택과 무관하게 **우클릭한
// 행의 로봇**에 간다 — 대상은 메뉴 머리줄과 확인 대화 제목에 이름으로 선다. 규칙은 `lib/robotCommandModel`.
import { useEffect, useState } from 'react'
import { api } from '../../lib/api'
import { allStatus } from '../../lib/feeds'
import { modeName } from '../../lib/gr/const'
import { modeIndicator, robotGate } from '../../lib/indicators'
import { robotColor, robots } from '../../lib/robots'
import { density } from '../../lib/density'
import { useStore } from '../../lib/store'
import {
  ROBOT_ACTIONS,
  describeRobotAction,
  robotActionDisabled,
  robotActionDone,
  robotActionSpec,
} from '../../lib/robotCommandModel'
import type { Robot, RobotAction } from '../../lib/types'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { IndicatorChip } from '../../lib/ui/IndicatorChip'
import { ctxMenu } from '../../lib/ui/menu'
import { toast } from '../../lib/ui/toast'

/** 명령을 보내고 결과를 토스트로 — 왕복이 1초 넘게 걸리므로(비트 펄스) 진행 토스트로 시작한다. */
async function sendRobotAction(robot: Robot, action: RobotAction): Promise<void> {
  const label = robotActionSpec(action).label
  const t = toast.pending(`${robot.name} ${label} 보내는 중…`)
  try {
    const res = await api.robotCommand(robot.id, action)
    toast.resolve(t, 'ok', robotActionDone(action, robot.name, res))
  } catch (e) {
    toast.resolve(
      t,
      'error',
      `${robot.name} ${label} 실패 — ${e instanceof Error ? e.message : String(e)}`,
    )
  }
}

export default function RobotsPane() {
  useStore(robots, density, allStatus)
  useEffect(() => robots.start(), [])
  // 로봇마다 모드를 보이려면 각자의 상태 스트림이 필요하다(맵과 같은 스토어 — 이미 열려 있으면 공유).
  useEffect(() => allStatus.start(), [])

  const rowPad = density.isCompact ? 'py-0.5' : 'py-1.5'
  const [pending, setPending] = useState<{ robot: Robot; action: RobotAction } | null>(null)

  const openMenu = (e: React.MouseEvent, r: Robot, mode: string | null) => {
    ctxMenu.show(e, [
      { label: `${r.name} 운전 명령` },
      ...ROBOT_ACTIONS.map((s) => ({
        label: s.label,
        danger: s.danger,
        disabled: robotActionDisabled(s.action, r, mode),
        testid: `robot-cmd-${r.id}-${s.action}`,
        run: () =>
          s.confirm
            ? setPending({ robot: r, action: s.action })
            : void sendRobotAction(r, s.action),
      })),
    ])
  }

  return (
    <>
      <ul
        className="min-h-0 flex-1 overflow-y-auto"
        data-testid="robot-list"
        role="radiogroup"
        aria-label="명령을 보낼 로봇"
      >
        {robots.list.length === 0 ? (
          <li className="px-2 py-2 text-2xs text-content-faint">
            {robots.error ? `로봇 목록 조회 실패 — ${robots.error}` : '로봇 목록 없음'}
          </li>
        ) : null}
        {robots.list.map((r) => {
          const on = r.id === robots.selected
          const gate = robotGate(r)
          const wm = allStatus.get(r.id)?.webmon ?? null
          const modeText = wm ? modeName(wm.Mode) : null
          const mode = modeIndicator(modeText)
          return (
            <li key={r.id}>
              <button
                type="button"
                role="radio"
                aria-checked={on}
                className={`relative flex w-full items-center gap-1.5 pr-2 pl-3 text-left ${rowPad} ${
                  on ? 'bg-accent-soft ring-1 ring-accent ring-inset' : 'hover:bg-surface-inset'
                }`}
                data-testid={`robot-${r.id}`}
                data-selected={on ? 'true' : 'false'}
                title={`${r.name}${on ? ' (선택됨)' : ''} · ${r.opcua_root} · DST ${r.dst} · Plc ${r.plc}`}
                onClick={() => robots.select(r.id)}
                onContextMenu={(e) => openMenu(e, r, modeText)}
              >
                {/* 로봇 색 — 맵의 작업 테두리 색. 상태가 아니라 **정체**라 점이 아닌 행 끝 막대로 둔다. */}
                <span
                  className="absolute inset-y-0 left-0 w-1"
                  style={{ background: robotColor(r.id) }}
                  aria-hidden="true"
                  data-testid={`robot-color-${r.id}`}
                />
                <span
                  className={`min-w-0 flex-1 truncate text-xs ${on ? 'font-semibold text-accent-text' : 'font-medium'}`}
                >
                  {r.name}
                </span>
                {r.active_tasks ? (
                  <span className="font-mono text-2xs text-content-muted" title="진행 중 Task 수">
                    T{r.active_tasks}
                  </span>
                ) : null}
                {mode ? <IndicatorChip ind={mode} bare data-testid={`robot-mode-${r.id}`} /> : null}
                <IndicatorChip ind={gate} data-testid={`robot-gate-${r.id}`} />
              </button>
            </li>
          )
        })}
      </ul>
      {pending ? (
        <ConfirmDialog
          open
          onOpenChange={(o) => {
            if (!o) setPending(null)
          }}
          scope="single-robot"
          title={`${robotActionSpec(pending.action).label} — ${pending.robot.name}`}
          danger={robotActionSpec(pending.action).danger}
          confirmLabel={robotActionSpec(pending.action).label}
          onConfirm={() => void sendRobotAction(pending.robot, pending.action)}
        >
          {/* design-lint-allow: no-panel-paragraph — 대화상자의 확인 문구다(실 로봇에 가는 명령을 말한다) */}
          <p className="m-0 text-content-primary">
            {describeRobotAction(pending.action, pending.robot.name)}
          </p>
        </ConfirmDialog>
      ) : null}
    </>
  )
}

/**
 * 머리띠 요약 — 껍데기가 제목 옆에 붙인다. **컴포넌트**로 내는 이유는 구독이다: 함수로 내면
 * 껍데기가 로봇 스토어를 구독하지 않아 선택이 바뀌어도 숫자가 그 자리에 굳는다.
 */
export function RobotsSummary() {
  useStore(robots)
  return (
    <span className="tabular-nums">
      {robots.list.length}
      {robots.current ? ` · ${robots.current.name}` : ''}
    </span>
  )
}
