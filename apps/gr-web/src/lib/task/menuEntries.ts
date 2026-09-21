// 조건부 메뉴 항목 — `조건 && {…}` 로 적은 목록을 `MenuItem[]` 로 줄인다.
//
// 그리는 일은 킷의 `lib/ui/OverflowMenu` 하나가 맡는다(예전에는 `components/task/Overflow.tsx` 가
// 같은 버튼을 다른 앵커 규칙으로 한 벌 더 갖고 있었다 — 같은 `⋯` 가 화면마다 다른 자리에 열렸다).
// 여기 남는 것은 **목록을 거르는 순수 함수** 하나뿐이다.
import type { MenuItem } from '../ui/menu'

/** 넘침 메뉴에 넣을 항목. `null`/`false`는 그 자리에서 빠진다(조건부 항목을 삼항 없이 적게). */
export type MenuEntry = MenuItem | null | false | undefined

export function menuItems(entries: readonly MenuEntry[]): MenuItem[] {
  return entries.filter((e): e is MenuItem => !!e)
}
