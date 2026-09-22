// 로봇 운전 명령 보내기 — 사이드바 우클릭 메뉴와 작업 명령 머리띠의 Stop 버튼이 같이 쓴다.
import { api } from './api'
import { robotActionDone, robotActionSpec } from './robotCommandModel'
import type { Robot, RobotAction } from './types'
import { toast } from './ui/toast'

/** 명령을 보내고 결과를 토스트로 — 왕복이 1초 넘게 걸리므로(비트 펄스) 진행 토스트로 시작한다. */
export async function sendRobotAction(robot: Robot, action: RobotAction): Promise<void> {
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
