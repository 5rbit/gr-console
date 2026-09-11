// 화면 헤더의 요약 항목 — 컴포넌트 밖에 둔다(`table.ts`와 같은 관용구).

/** 라벨 + 값 짝. 문장이 아니라 항목이라 눈이 필요한 하나에 바로 꽂힌다. */
export interface MetaItem {
  label: string
  value: string
}
