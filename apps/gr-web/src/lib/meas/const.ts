// 측정 모니터 상수 — Data[0..19] 인덱스 라벨(종류별)과 추세 항목. Item/Sku 는 OPCUA Measuring.Item/SKU.Data 사양과 1:1
// (빈 문자열 = 예비). Floor/Pick/수동은 이력 전용 배치로 PLC `MeasureLogAdd` 주석과 1:1.
import type { MeasRow } from './rows'

/** 측정 로그 `Data[i]` 라벨(종류별). 없는 인덱스는 예비. */
export const DATA_LABEL: Record<number, readonly string[]> = {
  1: [
    'Status',
    '내경',
    '상비드 높이',
    '타이어 높이',
    '',
    '',
    '',
    '토크 내경',
    '토크 계수',
    '',
    'Entry 내경',
    'Entry 상비드',
    'Entry 높이',
    'Entry 편심X',
    'Entry 편심Y',
    'Exit 내경',
    'Exit 상비드',
    'Exit 높이',
    'Exit 편심X',
    'Exit 편심Y',
  ],
  2: [
    'Status',
    'Stack#1 하비드',
    'Stack#1 상비드',
    'Stack#2 하비드',
    'Stack#2 상비드',
    'Stack#3 하비드',
    'Stack#3 상비드',
    'Stack#4 하비드',
    'Stack#4 상비드',
    'Stack#5 하비드',
    'Stack#5 상비드',
    'Stack#6 하비드',
    'Stack#6 상비드',
    'Stack#7 하비드',
    'Stack#7 상비드',
    '',
    '',
    '단수',
    '단당 높이',
    '전체 높이',
  ],
  3: ['Status', '바닥높이(abs)', 'Z위치', 'FLD거리'],
  4: [
    'Status',
    '마지막비드-TargetZ',
    'PickZTarget',
    'Valid',
    'FitError',
    '내경',
    '편심X',
    '편심Y',
    '상단비드',
    '타이어높이',
  ],
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
  { k: 1, id: 'i10', label: '[Item] Entry 내경', fn: (r) => r.data[10] ?? null },
  { k: 1, id: 'i15', label: '[Item] Exit 내경', fn: (r) => r.data[15] ?? null },
  { k: 1, id: 'i2', label: '[Item] 상단 비드 높이', fn: (r) => r.data[2] ?? null },
  { k: 1, id: 'i3', label: '[Item] 타이어 높이', fn: (r) => r.data[3] ?? null },
  { k: 1, id: 'i13', label: '[Item] Entry 편심 X', fn: (r) => r.data[13] ?? null },
  { k: 1, id: 'i14', label: '[Item] Entry 편심 Y', fn: (r) => r.data[14] ?? null },
  { k: 2, id: 's18', label: '[Sku] 단당 높이', fn: (r) => r.data[18] ?? null },
  { k: 2, id: 's19', label: '[Sku] 전체 높이', fn: (r) => r.data[19] ?? null },
  { k: 2, id: 's17', label: '[Sku] 단수', fn: (r) => r.data[17] ?? null },
  { k: 3, id: 'f1', label: '[Floor] 바닥 높이', fn: (r) => r.data[1] ?? null },
  { k: 4, id: 'p1', label: '[Pick] 마지막 비드 - TargetZ', fn: (r) => r.data[1] ?? null },
  {
    k: 4,
    id: 'p5',
    label: '[Pick] 레이저 내경',
    fn: (r) => ((r.data[3] ?? 0) > 0 ? (r.data[5] ?? null) : null),
  },
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
