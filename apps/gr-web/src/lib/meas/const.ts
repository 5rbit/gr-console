// 측정 모니터 상수 — Data[0..19] 인덱스 라벨(종류별)과 추세 항목. PLC `MeasureLogAdd` 주석과 1:1.
import type { MeasRow } from './rows'

/** 측정 로그 `Data[i]` 라벨(종류별). 없는 인덱스는 예비. */
export const DATA_LABEL: Record<number, readonly string[]> = {
  1: ['Status', '내경(정합)', '상단비드높이', '타이어높이', '토크Raw1', '토크Raw2', '중심출처', '토크내경', '토크계수', '톨러런스', 'In 내경', 'In 상단비드', 'In 타이어높이', 'In 편심X', 'In 편심Y', 'Out 내경', 'Out 상단비드', 'Out 타이어높이', 'Out 편심X', 'Out 편심Y'],
  2: ['Status', '1단 하비드', '1단 상비드', '2단 하비드', '2단 상비드', '3단 하비드', '3단 상비드', '4단 하비드', '4단 상비드', '5단 하비드', '5단 상비드', '6단 하비드', '6단 상비드', '7단 하비드', '7단 상비드', '결과센서', '완료센서수', '단수', '단당높이', '스택높이'],
  3: ['Status', '바닥높이(abs)', 'Z위치', 'FLD거리'],
  4: ['Status', '마지막비드-TargetZ', 'PickZTarget', 'Valid', 'FitError', '내경', '편심X', '편심Y', '상단비드', '타이어높이'],
  5: ['Status', '레이저내경', '토크내경', '타이어높이', '편심X', '편심Y', '상단비드', 'FitError'],
}

export interface Metric {
  /** 종류 제한(0 = 공통). */
  k: number
  id: string
  label: string
  fn: (r: MeasRow) => number | null
}

/** 추세 그래프 항목. 공통(편차)은 0 값을 "측정 안 함"으로 보고 뺀다. */
export const METRICS: readonly Metric[] = [
  { k: 0, id: 'dInnerDia', label: 'Δ내경 (측정-명령)', fn: (r) => r.dInnerDia },
  { k: 0, id: 'dHeight', label: 'Δ타이어높이 (측정-명령)', fn: (r) => r.dHeight },
  { k: 0, id: 'dZ', label: 'ΔZ (측정높이-명령Z)', fn: (r) => r.dZ },
  { k: 0, id: 'dOffset', label: '편심 크기', fn: (r) => r.dOffset },
  { k: 1, id: 'i1', label: '[Item] 내경(정합)', fn: (r) => r.data[1] ?? null },
  { k: 1, id: 'i7', label: '[Item] 토크 내경', fn: (r) => r.data[7] ?? null },
  { k: 1, id: 'i10', label: '[Item] In 레이저 내경', fn: (r) => r.data[10] ?? null },
  { k: 1, id: 'i15', label: '[Item] Out 레이저 내경', fn: (r) => r.data[15] ?? null },
  { k: 1, id: 'i2', label: '[Item] 상단 비드 높이', fn: (r) => r.data[2] ?? null },
  { k: 1, id: 'i3', label: '[Item] 타이어 높이', fn: (r) => r.data[3] ?? null },
  { k: 1, id: 'i13', label: '[Item] In 편심 X', fn: (r) => r.data[13] ?? null },
  { k: 1, id: 'i14', label: '[Item] In 편심 Y', fn: (r) => r.data[14] ?? null },
  { k: 2, id: 's18', label: '[Sku] 단당 높이', fn: (r) => r.data[18] ?? null },
  { k: 2, id: 's19', label: '[Sku] 스택 높이', fn: (r) => r.data[19] ?? null },
  { k: 2, id: 's17', label: '[Sku] 단수', fn: (r) => r.data[17] ?? null },
  { k: 3, id: 'f1', label: '[Floor] 바닥 높이', fn: (r) => r.data[1] ?? null },
  { k: 4, id: 'p1', label: '[Pick] 마지막 비드 - TargetZ', fn: (r) => r.data[1] ?? null },
  { k: 4, id: 'p5', label: '[Pick] 레이저 내경', fn: (r) => ((r.data[3] ?? 0) > 0 ? (r.data[5] ?? null) : null) },
  { k: 5, id: 'm1', label: '[Manual] 레이저 내경', fn: (r) => r.data[1] ?? null },
  { k: 5, id: 'm2', label: '[Manual] 토크 내경', fn: (r) => r.data[2] ?? null },
]

export function metricsFor(kind: number): Metric[] {
  return METRICS.filter((m) => m.k === 0 || !kind || m.k === kind)
}

/** `ByCode.Meas` 키의 한글 라벨. */
export const MEAS_KEY_LABEL: Record<string, string> = {
  InnerDia: '내경(정합)',
  LaserInnerDia: '레이저 내경',
  TorqInnerDia: '토크 내경',
  UpperBeadHeight: '상단비드',
  TireHeight: '타이어높이',
  Offset: '편심',
  PickBeadPos: 'Pick 비드-Z',
  PickInnerDia: 'Pick 내경',
  SkuEachHeight: 'Sku 단당',
  SkuStackHeight: 'Sku 스택',
}
