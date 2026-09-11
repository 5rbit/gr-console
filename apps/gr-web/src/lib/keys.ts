// 단축키 표기·판정의 플랫폼 구분 — 맥은 ⌘, 윈도우/리눅스는 Ctrl. 표기와 판정을 한 곳에서 낸다.
//
// 판정은 언제나 둘 다 받는다(맥에서 외장 윈도우 키보드를 쓰는 경우) — 플랫폼이 가르는 것은 안내 문구다.

/** 이 브라우저가 애플 플랫폼인가. SSR·비브라우저에서는 `false`. */
export const IS_MAC =
  typeof navigator !== 'undefined' &&
  // `userAgentData.platform`이 최신 규격, `platform`은 폴백(비표준·deprecated지만 아직 유일한 보편 신호).
  /mac|iphone|ipad/i.test(
    (navigator as { userAgentData?: { platform?: string } }).userAgentData?.platform ??
      navigator.platform ??
      navigator.userAgent,
  )

/** 주 수식키 표기 — 맥 `⌘`, 그 외 `Ctrl`. */
export const MOD_KEY = IS_MAC ? '⌘' : 'Ctrl'

/** 수식키 + 글자 조합 표기(`⌘K` / `Ctrl+K`) — 맥은 붙여 쓰고 그 외는 `+`로 잇는 것이 각 플랫폼 관례다. */
export function chord(key: string): string {
  return IS_MAC ? `${MOD_KEY}${key.toUpperCase()}` : `${MOD_KEY}+${key.toUpperCase()}`
}

/** 이 이벤트가 주 수식키를 눌렀는가 — **양쪽 다 받는다**(맥에서 윈도우 키보드를 쓰는 경우 대비). */
export function hasMod(e: KeyboardEvent | MouseEvent): boolean {
  return e.metaKey || e.ctrlKey
}
