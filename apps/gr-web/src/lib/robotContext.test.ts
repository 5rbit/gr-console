import { describe, expect, it } from 'vitest'
import {
  UNKNOWN_ROBOT,
  gateFor,
  pickDefaultRobot,
  plcShown,
  robotChip,
  robotColor,
  robotFailure,
  robotLabel,
  withRobot,
  withRobotChip,
  type RobotUsable,
} from './robotContext'

const r = (
  id: number,
  over: Partial<RobotUsable> = {},
): RobotUsable => ({
  id,
  name: `GR${id}`,
  plc: `GR${id}_PLC`,
  plc_connected: true,
  gate: { can_submit: true },
  ...over,
})

describe('robotChip — 칩 모델(이름·PLC·색)', () => {
  it('로봇이 있으면 그 이름·PLC·색', () => {
    expect(robotChip(r(2))).toEqual({ id: 2, name: 'GR2', plc: 'GR2_PLC', color: robotColor(2) })
    // 맵·사이드바와 같은 색 — 호기마다 다르다.
    expect(robotChip(r(1)).color).not.toBe(robotChip(r(2)).color)
  })

  it('로봇이 없으면 응답이 실어 준 이름·PLC 로(선택 상태로 짐작하지 않는다)', () => {
    const c = robotChip(null, { name: 'GR1', plc: 'GR1_PLC' })
    expect(c.name).toBe('GR1')
    expect(c.plc).toBe('GR1_PLC')
  })

  it('아무것도 모르면 "로봇 미확인" — 빈 칩이 기본 로봇인 척하지 않는다', () => {
    expect(robotChip(null)).toBe(UNKNOWN_ROBOT)
    expect(robotChip(undefined, { name: '  ' })).toBe(UNKNOWN_ROBOT)
  })

  it('표시: 이름 · PLC, PLC 가 이름과 같으면 한 번만', () => {
    expect(robotLabel(robotChip(r(2)))).toBe('GR2 · GR2_PLC')
    const same = robotChip({ id: 2, name: 'GR2', plc: 'GR2' })
    expect(plcShown(same)).toBeNull()
    expect(robotLabel(same)).toBe('GR2')
    expect(robotLabel(UNKNOWN_ROBOT)).toBe('로봇 미확인')
  })
})

describe('pickDefaultRobot — 막 열었을 때 고를 로봇', () => {
  it('지난 선택이 목록에 있으면 그것', () => {
    expect(pickDefaultRobot([r(1), r(2)], 2)).toBe(2)
  })

  it('지난 선택이 없거나 사라졌으면 쓸 수 있는 첫 호기(S7 연결 + 게이트 열림)', () => {
    // 사고 재현: GR1 이 목록 첫 줄이지만 AUTO 가 아니라 게이트가 닫혀 있다 — 말없이 GR1 을 고르지 않는다.
    const list = [r(1, { gate: { can_submit: false } }), r(2)]
    expect(pickDefaultRobot(list, null)).toBe(2)
    expect(pickDefaultRobot(list, 7)).toBe(2)
    expect(pickDefaultRobot([r(1, { plc_connected: false }), r(2)], null)).toBe(2)
  })

  it('쓸 수 있는 호기가 없으면 첫 호기, 목록이 비면 null', () => {
    const down = [r(1, { plc_connected: false }), r(2, { gate: { can_submit: false } })]
    expect(pickDefaultRobot(down, null)).toBe(1)
    expect(pickDefaultRobot([], 2)).toBeNull()
  })

  it('지난 선택은 막혀 있어도 그대로 — 작업자가 고른 것을 몰래 바꾸지 않는다', () => {
    expect(pickDefaultRobot([r(1), r(2, { gate: { can_submit: false } })], 2)).toBe(2)
  })
})

describe('withRobot — 메시지 앞 로봇 이름', () => {
  it('거부 사유에 이름을 단다', () => {
    expect(withRobot('GR1', 'Task.Status.Accept = FALSE')).toBe('GR1: Task.Status.Accept = FALSE')
    expect(withRobotChip(robotChip(r(1)), '제출 실패 — 409')).toBe('GR1: 제출 실패 — 409')
  })

  it('이미 달린 이름은 두 번 달지 않는다(백엔드가 먼저 달아 보낸다)', () => {
    expect(withRobot('GR1', 'GR1: AUTO 모드가 아님')).toBe('GR1: AUTO 모드가 아님')
    // 다른 호기로 시작하는 문구는 그 호기의 것이 아니다 — 제 이름을 앞에 단다.
    expect(withRobot('GR1', 'GR12: x')).toBe('GR1: GR12: x')
  })

  it('실패 한 줄: 백엔드가 단 이름은 앞에 한 번만(사고 문구 재현)', () => {
    const backend = 'GR1: Task.Status.Accept = FALSE; GR1: AUTO 모드가 아님 (409)'
    expect(robotFailure('GR1', '제출 실패', backend)).toBe(
      'GR1: 제출 실패 — Task.Status.Accept = FALSE; AUTO 모드가 아님 (409)',
    )
    // 이름 없는 오류(네트워크 등)에도 대상이 선다.
    expect(robotFailure('GR2', '쓰기 실패', 'fetch failed')).toBe('GR2: 쓰기 실패 — fetch failed')
    // 다른 호기의 사유는 그대로 남긴다.
    expect(robotFailure('GR1', '제출 실패', 'GR2: x')).toBe('GR1: 제출 실패 — GR2: x')
    expect(robotFailure(null, '제출 실패', 'x')).toBe('제출 실패 — x')
  })

  it('이름을 모르면 메시지를 그대로', () => {
    expect(withRobot(null, 'x')).toBe('x')
    expect(withRobot('  ', 'x')).toBe('x')
  })
})

describe('gateFor — 게이트는 고른 로봇의 것만', () => {
  const g1 = { can_submit: true, reasons: [], robot: 'GR1' }

  it('선택과 같은 로봇의 게이트는 그대로', () => {
    expect(gateFor(g1, 'GR1')).toBe(g1)
  })

  it('선택을 바꾼 직후 남은 옛 로봇 게이트는 버린다 — GR2 칩 옆에 GR1 의 "제출 가능"이 서지 않게', () => {
    expect(gateFor(g1, 'GR2')).toBeNull()
  })

  it('로봇 이름이 없는 옛 응답·아직 못 받음', () => {
    const old = { can_submit: false, reasons: ['x'] }
    expect(gateFor(old, 'GR2')).toBe(old)
    expect(gateFor(null, 'GR2')).toBeNull()
  })
})
