// 로봇 운전 명령(사이드바 로봇 행 우클릭) — 항목·확인 여부·비활성 사유를 한곳에서 정한다.
//
// Start/Stop/Reset 은 GRM `GR[n].CMD.Command` 비트 펄스, Complete 는 PLC 가 지금 실행 중인 Task 강제 완료,
// Clear 는 이 로봇의 살아 있는 Task 전부 삭제(백엔드 `ledger/robot_cmd.rs`).
// Stop · Reset · Buzzer Stop 은 확인 없이 바로 나간다 — 멈춤을 대화 상자 뒤에 두지 않는다.
import type { Robot, RobotAction } from './types'

export interface RobotActionSpec {
  action: RobotAction
  label: string
  /** 확인 대화를 거친다 */
  confirm: boolean
  danger: boolean
}

export const ROBOT_ACTIONS: readonly RobotActionSpec[] = [
  { action: 'start', label: 'Start', confirm: true, danger: false },
  { action: 'stop', label: 'Stop', confirm: false, danger: false },
  { action: 'reset', label: 'Reset', confirm: false, danger: false },
  { action: 'buzzerstop', label: 'Buzzer Stop', confirm: false, danger: false },
  { action: 'complete', label: 'Complete', confirm: true, danger: true },
  { action: 'clear', label: 'Clear', confirm: true, danger: true },
]

export function robotActionSpec(action: RobotAction): RobotActionSpec {
  return ROBOT_ACTIONS.find((s) => s.action === action) ?? ROBOT_ACTIONS[0]
}

/** 확인 대화 본문 — 무엇이 PLC 에 가는지 그대로 말한다. */
export function describeRobotAction(action: RobotAction, name: string): string {
  switch (action) {
    case 'start':
      return `${name} 에 Start(Command.Common.Start)를 보냅니다. READY 상태면 AUTO 로 들어가 대기 중인 Task 를 실행합니다.`
    case 'stop':
      return `${name} 에 Stop(Command.Stop.Normal)을 보냅니다.`
    case 'reset':
      return `${name} 에 Reset(Command.Common.Reset)을 보냅니다.`
    case 'buzzerstop':
      return `${name} 에 Buzzer Stop(Command.Common.BuzzerStop)을 보냅니다.`
    case 'complete':
      return `${name} 가 지금 실행 중인 Task 를 강제 완료(Command.Task.Complete)합니다. 화물 상태를 먼저 확인하세요.`
    case 'clear':
      return `${name} 의 실행 중·대기 중 Task 를 모두 삭제(Command.Task.Delete)합니다.`
  }
}

/**
 * 누를 수 없는 사유 — 없으면 undefined. 운전 명령(Master Command)은 `Task.Status.Accept` 와 **무관**하게 로봇
 * 모드만 본다(Accept 는 AUTO 에서 Task 를 받을지 여부일 뿐, 2026-09-22):
 * - Start = READY 에서만 · Stop = 언제든 · Complete / Clear(취소) = AUTO 가 아닐 때 · Reset / Buzzer Stop = 언제든.
 * (Task 제출은 별도 게이트 = Accept + AUTO.) 명령 경로(OPC UA)가 없으면 보낼 수 없으니 전부 막는다.
 * 모드를 아직 모르면 Start 만 막고 나머지는 백엔드가 PLC 스냅샷으로 판정한다.
 */
export function robotActionDisabled(
  action: RobotAction,
  robot: Pick<Robot, 'cmd_ready' | 'cmd_error'>,
  mode: string | null,
): string | undefined {
  if (!robot.cmd_ready) {
    return `명령 경로 준비 안 됨${robot.cmd_error ? ` — ${robot.cmd_error}` : ''}`
  }
  if (action === 'start' && mode !== 'READY')
    return `READY 에서만 Start (지금 ${mode ?? '모드 모름'})`
  if ((action === 'complete' || action === 'clear') && mode === 'AUTO') {
    return 'AUTO 모드에서는 완료·삭제할 수 없음 — 먼저 Stop'
  }
  return undefined
}

/** 성공 토스트 문구 */
export function robotActionDone(
  action: RobotAction,
  name: string,
  res: { task?: string; deleted?: string[] },
): string {
  const label = robotActionSpec(action).label
  if (action === 'complete' && res.task) return `${name} ${label} 요청 — Task ${res.task}`
  if (action === 'clear' && res.deleted) {
    return `${name} ${label} 요청 — ${res.deleted.length}건 (${res.deleted.join(', ')})`
  }
  return `${name} ${label} 보냄`
}
