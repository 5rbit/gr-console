// 로봇이 들고 있는 화물 — 맵 표식과 로봇 우클릭 메뉴가 같이 쓴다. 순수 함수(테스트 대상).
//
// 근거는 둘이다: 콘솔 **Hand**(PICK 완료로 들어오고 DROP 완료로 나간다, `stock` 스토어)와 PLC 상태
// (`Task.Status.HoldItem` = GRIPPER.Item.Code ≠ 0, `Gripper.ItemDetect` = 센서). 둘이 어긋나면 표식이 그 사실을
// 말하고, 고치는 것은 사람이다(서버 동기화 경고 `stock/sync.rs` 와 같은 규칙: 콘솔 Hand vs PLC HoldItem·감지).
import type { HandEntry, WebMon } from '../types'

export type CargoState =
  /** 빈 손 — 콘솔 · PLC 모두 */
  | 'none'
  /** 콘솔 Hand 와 PLC 가 둘 다 들고 있음 */
  | 'ok'
  /** PLC 는 들고 있는데 콘솔 Hand 가 비었음 → "PLC 기준으로 맞춤" */
  | 'plc_only'
  /** 콘솔 Hand 에 있는데 PLC 는 빈 그리퍼 → "Hand 비움" */
  | 'console_only'
  /** 콘솔 Hand 만 알고 PLC 상태는 모름(스트림 없음) */
  | 'unknown_plc'

export interface RobotCargo {
  state: CargoState
  /** 콘솔 Hand 품목(0 = 모름) */
  itemCode: number
  count: number
  /** PLC HoldItem / ItemDetect(모르면 null) */
  plcHold: boolean | null
  plcDetect: boolean | null
  /** 한 줄 설명(호버·메뉴 머리줄) */
  text: string
}

/** PLC 쪽 파지 신호. 레이아웃이 옛 판이라 필드가 없으면 null. */
export function plcHoldOf(wm: WebMon | null | undefined): {
  hold: boolean | null
  detect: boolean | null
} {
  if (!wm) return { hold: null, detect: null }
  const h = wm.Stat?.Task?.Status?.HoldItem
  const d = wm.Gripper?.ItemDetect
  return {
    hold: typeof h === 'boolean' ? h : typeof h === 'number' ? h !== 0 : null,
    detect: typeof d === 'boolean' ? d : null,
  }
}

export function robotCargo(
  hand: Pick<HandEntry, 'item_code' | 'count'> | null | undefined,
  plcHold: boolean | null,
  plcDetect: boolean | null,
): RobotCargo {
  const count = hand?.count ?? 0
  const itemCode = count > 0 ? (hand?.item_code ?? 0) : 0
  const console = count > 0
  // PLC 는 HoldItem 을 먼저 보고, 그게 없으면(옛 레이아웃) 센서로 판단한다.
  const plc = plcHold ?? plcDetect
  const what = console ? `${itemCode || '품목 모름'} × ${count}` : ''
  let state: CargoState
  let text: string
  if (plc === null) {
    state = console ? 'unknown_plc' : 'none'
    text = console ? `Hand ${what} (PLC 상태 모름)` : '빈 손'
  } else if (console && plc) {
    state = 'ok'
    text = `Hand ${what}`
  } else if (console) {
    state = 'console_only'
    text = `콘솔 Hand ${what} — PLC 는 빈 그리퍼`
  } else if (plc) {
    state = 'plc_only'
    text = 'PLC 는 들고 있음 — 콘솔 Hand 비어 있음'
  } else {
    state = 'none'
    text = '빈 손'
  }
  if (plcHold !== null && plcDetect !== null && plcHold !== plcDetect && state !== 'none') {
    text += ` · HoldItem ${plcHold ? 1 : 0} ≠ ItemDetect ${plcDetect ? 1 : 0}`
  }
  return { state, itemCode, count, plcHold, plcDetect, text }
}

/** 맵에 화물 원을 그릴까 — 한쪽이라도 들고 있으면 그린다(어긋남도 보여야 한다). */
export function cargoVisible(c: RobotCargo): boolean {
  return c.state !== 'none'
}

/** 어긋남(사람이 맞춰야 함) */
export function cargoMismatch(c: RobotCargo): boolean {
  return c.state === 'plc_only' || c.state === 'console_only'
}
