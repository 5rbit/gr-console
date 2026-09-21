// 로봇 문맥 — **어느 호기에 대고 하는 일인가**를 화면·메시지가 같은 규칙으로 말하게 하는 순수 모듈.
//
// 왜 따로 두나: 현장에서 사고가 났다. 사이드바 선택은 GR1(목록의 첫 호기)인데 작업자는 눈앞의
// GR2 를 보며 제출을 눌렀고, 돌아온 말은 `제출 실패 — PLC Task.Status.Accept = FALSE; 로봇이 AUTO
// 모드가 아님` 이었다. **어느 로봇 이야기인지 아무 데도 없었다.** 제출도 게이트도 GR1 을 겨냥했지만
// 화면의 다른 자리들은 GR2 값을 보이고 있었다.
//
// 그래서 셋을 한곳에 모은다: 칩 모델(이름·PLC·색) · 기본 선택 규칙 · 메시지 앞 이름 붙이기.
// 스토어(`robots.ts`)와 컴포넌트(`components/shared/RobotChip.tsx`)가 이것을 쓰고, 이 파일은
// DOM·fetch 를 모르므로 규칙을 테스트로 못 박을 수 있다.

/** 색을 고를 때 필요한 최소한. */
export interface RobotIdent {
  id: number
  name: string
  plc: string
}

/** 기본 선택 규칙이 보는 최소한 — 스토어의 `Robot` 이 구조적으로 이것을 만족한다. */
export interface RobotUsable extends RobotIdent {
  plc_connected: boolean
  gate: { can_submit: boolean }
}

/** 로봇에 할당된 색 — 맵 작업 테두리·로봇 십자·사이드바 견본·로봇 칩이 같은 색을 쓴다. */
export const ROBOT_COLORS: Readonly<Record<number, string>> = { 1: '#0284c7', 2: '#ea580c' }
const FALLBACK_COLORS = ['#7c3aed', '#db2777', '#0d9488']

export function robotColor(id: number | null | undefined): string {
  if (id === null || id === undefined) return FALLBACK_COLORS[0]
  return ROBOT_COLORS[id] ?? FALLBACK_COLORS[Math.abs(id) % FALLBACK_COLORS.length]
}

/** 칩 하나가 그리는 것 — 색 점 + 이름 + 그 로봇의 상태 PLC. */
export interface RobotChipModel {
  id: number | null
  name: string
  plc: string | null
  color: string
}

/** 로봇을 아직 못 받았을 때(목록 조회 전·설정에 없음) 쓰는 칩. */
export const UNKNOWN_ROBOT: RobotChipModel = { id: null, name: '로봇 미확인', plc: null, color: robotColor(null) }

/**
 * 칩 모델을 만든다.
 *
 * `robot` 이 없으면 `fallback`(백엔드가 응답에 실어 준 이름·PLC)을 쓴다 — 미리보기·확인 창은
 * 응답이 겨냥한 로봇을 말해야지 그 사이에 바뀐 사이드바 선택을 말하면 안 된다.
 */
export function robotChip(
  robot: RobotIdent | null | undefined,
  fallback?: { name?: string | null; plc?: string | null; id?: number | null } | null,
): RobotChipModel {
  if (robot) return { id: robot.id, name: robot.name, plc: robot.plc || null, color: robotColor(robot.id) }
  const name = fallback?.name?.trim()
  if (name) return { id: fallback?.id ?? null, name, plc: fallback?.plc?.trim() || null, color: robotColor(fallback?.id ?? null) }
  return UNKNOWN_ROBOT
}

/**
 * 칩에 PLC 를 따로 적을까 — 모르거나 이름과 같으면(현장 설정은 `name = "GR2"`, `plc = "GR2"`)
 * 적지 않는다. 같은 낱말이 두 번 서면 두 개의 다른 것처럼 읽힌다.
 */
export function plcShown(chip: RobotChipModel): string | null {
  if (!chip.plc) return null
  return chip.plc.toLowerCase() === chip.name.toLowerCase() ? null : chip.plc
}

/** 한 줄 표시 — `GR2 · GR2_PLC`(PLC 를 모르거나 이름과 같으면 이름만). */
export function robotLabel(chip: RobotChipModel): string {
  const plc = plcShown(chip)
  return plc ? `${chip.name} · ${plc}` : chip.name
}

/**
 * 막 열었을 때 고를 로봇.
 *
 * 1. 지난번 선택(localStorage)이 지금 목록에 있으면 그것 — 작업자가 마지막으로 보던 호기다.
 * 2. 없으면 **쓸 수 있는** 첫 호기(S7 연결 + 게이트에 막는 사유 없음).
 * 3. 그것도 없으면 첫 호기. 목록이 비면 `null`.
 *
 * 2번이 있는 이유: 목록의 첫 호기를 말없이 고르면, 그 호기가 꺼져 있어도 제출 버튼은 그쪽을
 * 겨냥한다. 작업자는 눈앞의 로봇을 보고 있는데 명령은 다른 호기로 간다 — 이번 사고가 그것이다.
 */
export function pickDefaultRobot(list: readonly RobotUsable[], remembered: number | null): number | null {
  if (list.length === 0) return null
  if (remembered !== null && list.some((r) => r.id === remembered)) return remembered
  const usable = list.find((r) => r.plc_connected && r.gate.can_submit)
  return (usable ?? list[0]).id
}

/** 게이트 응답 중 여기서 보는 것 — `lib/types.ts` 의 `Gate` 가 구조적으로 만족한다. */
export interface GateLike {
  can_submit: boolean
  reasons: string[]
  robot?: string
}

/**
 * 이 게이트가 **지금 고른 로봇의 것**일 때만 돌려준다.
 *
 * 선택을 GR1 → GR2 로 바꾸면 새 조회가 돌아오기 전 1초 동안 옛 GR1 게이트가 남는다 — 그 사이
 * 머리띠는 `GR2` 칩 옆에 GR1 의 `제출 가능` 을 그리고, 제출 버튼도 그 값으로 풀린다. 응답이 제
 * 로봇 이름을 싣게 한 까닭이 이것이다. 이름이 없는 옛 응답은 판정하지 못하므로 그대로 믿는다.
 */
export function gateFor<G extends GateLike>(gate: G | null, robot: string | null | undefined): G | null {
  if (!gate) return null
  if (gate.robot && robot && gate.robot !== robot) return null
  return gate
}

/**
 * 메시지 앞에 로봇 이름을 단다(이미 달려 있으면 그대로). 백엔드 `ledger::ops::with_robot` 의 짝.
 *
 * 토스트·확인 문구·거부 사유가 전부 이 한 규칙을 지나야 `GR1: …` 이 한 모양으로 선다.
 */
export function withRobot(robot: string | null | undefined, msg: string): string {
  const name = robot?.trim()
  if (!name) return msg
  if (msg.startsWith(`${name}:`)) return msg
  return `${name}: ${msg}`
}

/**
 * 실패 한 줄 — `GR1: 제출 실패 — 에코 대기 중인 제출이 있음`.
 *
 * 백엔드 거부는 이미 사유마다 `GR1: …` 을 달아 온다. 그 위에 그대로 이름을 또 달면
 * `GR1: 제출 실패 — GR1: …` 으로 같은 이름이 두 번 선다 — 앞에 한 번만 두고 본문에서는 걷는다.
 * 다른 호기 이름으로 시작하는 사유는 걷지 않는다(그것은 그 호기의 말이다).
 */
export function robotFailure(robot: string | null | undefined, what: string, msg: string): string {
  const name = robot?.trim()
  if (!name) return `${what} — ${msg}`
  const pre = `${name}: `
  const body = msg
    .split('; ')
    .map((p) => (p.startsWith(pre) ? p.slice(pre.length) : p))
    .join('; ')
  return `${name}: ${what} — ${body}`
}

/** 칩으로 메시지에 이름을 단다 — `GR2 · GR2_PLC` 가 아니라 **이름만** 붙는다(메시지는 짧아야 한다). */
export function withRobotChip(chip: RobotChipModel | null | undefined, msg: string): string {
  return withRobot(chip?.name, msg)
}
