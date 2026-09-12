// 로봇 패널 — 명령을 보낼 로봇을 고르는 자리. 예전에는 `Sidebar.tsx` 안의 섹션이었다.
//
// 왜 떼어 냈나: 도킹이 되는 순간 "왼쪽 사이드바의 로봇 섹션"이라는 것은 없다 — 같은 목록이 왼쪽에도
// 오른쪽에도 하단에도 설 수 있어야 하고, 그러려면 섹션이 **자기 머리띠 없이** 자기 몸만 그려야 한다
// (제목·탭·닫기는 도킹 껍데기가 그린다). 사이드바는 이제 이 패널들을 얹는 존 하나다.
//
// 클래스는 사이드바에 있던 것을 그대로 옮겼다(슬레이트 + `dark:`) — 이 변경은 **자리 이동**이고
// 색을 다시 고르는 일이 아니다. 셸 크롬(App·PanelHost)이 아직 다크를 들고 있어 여기서만 시맨틱
// 토큰으로 갈아타면 다크에서 이 패널만 하얗게 뜬다.
import { useEffect } from 'react'
import { robotColor, robots } from '../../lib/robots'
import { density } from '../../lib/density'
import { useStore } from '../../lib/store'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import { StatusDot } from '../../lib/ui/StatusDot'
import { robotTone } from '../shared/RobotPicker'

export default function RobotsPane() {
  useStore(robots, density)
  useEffect(() => robots.start(), [])

  const rowPad = density.isCompact ? 'py-0.5' : 'py-1.5'

  return (
    <ul
      className="min-h-0 flex-1 overflow-y-auto"
      data-testid="robot-list"
      role="radiogroup"
      aria-label="명령을 보낼 로봇"
    >
      {robots.list.length === 0 ? (
        <li className="px-2 py-2 text-2xs text-slate-400">
          {robots.error ? `로봇 목록 조회 실패 — ${robots.error}` : '로봇 목록 없음'}
        </li>
      ) : null}
      {robots.list.map((r) => {
        const on = r.id === robots.selected
        return (
          <li key={r.id}>
            <button
              type="button"
              role="radio"
              aria-checked={on}
              className={`flex w-full items-center gap-2 px-2 text-left hover:bg-slate-100 dark:hover:bg-slate-800 ${rowPad} ${on ? 'bg-indigo-50 dark:bg-indigo-950' : ''}`}
              data-testid={`robot-${r.id}`}
              title={`${r.opcua_root} · DST ${r.dst} · 상태 PLC ${r.plc}${r.gate.can_submit ? '' : ` · 게이트 닫힘: ${r.gate.reasons.join('; ')}`}`}
              onClick={() => robots.select(r.id)}
            >
              <StatusDot status={robotTone(r)} size="sm" />
              <span
                className="h-2.5 w-2.5 flex-none rounded-sm"
                style={{ background: robotColor(r.id) }}
                title="맵에서 이 로봇의 작업 테두리 색"
              />
              <span
                className={`min-w-0 flex-1 truncate text-xs ${on ? 'font-semibold text-indigo-700 dark:text-indigo-300' : 'font-medium'}`}
              >
                {r.name}
              </span>
              {r.active_tasks ? (
                <span className="font-mono text-3xs text-slate-400" title="진행 중 Task">
                  {r.active_tasks}
                </span>
              ) : null}
              {on ? <StatusBadge status="info">선택</StatusBadge> : null}
            </button>
          </li>
        )
      })}
    </ul>
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
