// 로봇 한 대의 운전 모드 이름(`WebMon.Mode` → `modeName`) — 버튼 잠금(AUTO)용.
//
// `allStatus` 는 상태 프레임마다 알리므로 그대로 `useStore` 하면 표의 행마다 초당 여러 번 다시 그린다.
// 여기서는 **모드 이름(문자열)** 을 스냅샷으로 내 `useSyncExternalStore` 가 값이 바뀔 때만 다시 그리게 한다.
import { useEffect, useSyncExternalStore } from 'react'
import { allStatus } from './feeds'
import { modeName } from './gr/const'

function modeOf(plc: string | null | undefined): string | null {
  const wm = allStatus.ofPlc(plc)?.webmon
  return wm ? modeName(wm.Mode) : null
}

/** 상태 PLC 이름(`Task.plc_name`)의 로봇 모드 — 상태를 아직 못 받았으면 null. */
export function useRobotModeOfPlc(plc: string | null | undefined): string | null {
  useEffect(() => allStatus.start(), [])
  const get = () => modeOf(plc)
  return useSyncExternalStore(allStatus.subscribe, get, get)
}
