// 트레이스 파형 색 — `tokens.css` 가 가리키는 진실원.
//
// 브랜드 팔레트(면·글자·판정 여섯)와 **일부러 갈라 둔다**. 파형은 값이 아니라 **계열을 구분하는**
// 색이라, 초록이 정상이고 빨강이 고장이라는 뜻을 싣지 않는다. 32 채널을 한 화면에 그리는 자리라
// 색 여덟으로는 모자라고, 그렇다고 색을 서른둘로 늘리면 인접한 두 색을 사람이 못 가른다 —
// **색 8 × 파선 4 = 32**로 쪼갠다. 파선은 흑백 인쇄·색각 이상에서도 남는 축이다.
//
// 값은 라이트(`#ffffff` 패널)와 다크(`#1a1a16` 패널) 양쪽에서 읽히는 중간 톤이다. 테마마다 값을
// 갈지 않는 이유: 같은 세션을 두 사람이 다른 테마로 보며 "세 번째 파란 선"이라고 말할 수 있어야 한다.

/** 계열 색 여덟 — 색상환에서 고르게 떨어뜨렸다. */
export const TRACE_COLORS: readonly string[] = [
  '#2a7be4', // 파랑
  '#009b4d', // 초록
  '#f2a900', // 골드
  '#d92d20', // 빨강
  '#7c4dd6', // 보라
  '#e4571b', // 주황
  '#12a3a3', // 청록
  '#c63c8f', // 자홍
]

/** 파선 넷 — `setLineDash` 인자 그대로(빈 배열 = 실선). */
export const TRACE_DASHES: readonly (readonly number[])[] = [[], [6, 3], [2, 3], [9, 3, 2, 3]]

/** 채널 순번의 색. 8을 넘으면 처음부터 다시 돈다(그때는 파선이 갈라 준다). */
export function traceColor(i: number): string {
  const n = TRACE_COLORS.length
  return TRACE_COLORS[((i % n) + n) % n] ?? TRACE_COLORS[0]
}

/** 채널 순번의 파선 — 색이 한 바퀴 돌 때마다 다음 파선으로 넘어간다. */
export function traceDash(i: number): readonly number[] {
  const c = TRACE_COLORS.length
  const d = TRACE_DASHES.length
  const k = Math.floor(Math.abs(i) / c) % d
  return TRACE_DASHES[k] ?? TRACE_DASHES[0]
}

/** 색+파선이 겹치기 시작하는 채널 수 — 32(= `LNK_TraceCfg` 의 채널 상한). */
export const TRACE_DISTINCT = TRACE_COLORS.length * TRACE_DASHES.length
