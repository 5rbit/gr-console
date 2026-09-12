// 로봇 패널 — 명령을 보낼 로봇을 고르는 자리. 예전에는 `Sidebar.tsx` 안의 섹션이었다.
//
// 왜 떼어 냈나: 도킹이 되는 순간 "왼쪽 사이드바의 로봇 섹션"이라는 것은 없다 — 같은 목록이 왼쪽에도
// 오른쪽에도 하단에도 설 수 있어야 하고, 그러려면 섹션이 **자기 머리띠 없이** 자기 몸만 그려야 한다
// (제목·탭·닫기는 도킹 껍데기가 그린다). 사이드바는 이제 이 패널들을 얹는 존 하나다.
//
// 색은 **시맨틱 토큰**으로 부른다(`bg-surface-*` · `text-content-*` · 상태 6종). 처음 떼어 낼 때는
// 셸 크롬이 아직 `slate` + `dark:` 쌍이라 그대로 옮겼지만, 이제 `tokens.css`가 다크 층을 들고 있어
// 킷·셸 전체가 토큰을 쓴다 — 두 테마가 이 파일을 고치지 않고 따라온다.
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
        <li className="px-2 py-2 text-2xs text-content-faint">
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
              className={`flex w-full items-center gap-2 px-2 text-left hover:bg-surface-inset ${rowPad} ${on ? 'bg-accent-soft' : ''}`}
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
                className={`min-w-0 flex-1 truncate text-xs ${on ? 'font-semibold text-accent-text' : 'font-medium'}`}
              >
                {r.name}
              </span>
              {r.active_tasks ? (
                <span className="font-mono text-3xs text-content-faint" title="진행 중 Task">
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
