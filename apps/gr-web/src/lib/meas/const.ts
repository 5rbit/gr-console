// 측정 모니터 상수 — Data[0..19] 인덱스 라벨(종류별)과 추세 항목. Item/Sku 는 OPCUA Measuring.Item/SKU.Data 사양과 1:1
// (빈 문자열 = 예비). Floor/Pick/수동은 이력 전용 배치로 PLC `MeasureLogAdd` 주석과 1:1.
import type { MeasRow } from './rows'

export interface Metric {
  /** 종류 제한(0 = 공통). */
  k: number
  id: string
  label: string
  fn: (r: MeasRow) => number | null
}

/** 추세 그래프 항목. 공통(편차)은 0 값을 "측정 안 함"으로 보고 뺀다. */
export const METRICS: readonly Metric[] = [
  { k: 0, id: 'dInnerDia', label: 'Delta.InnerDia (Meas − Cmd)', fn: (r) => r.dInnerDia },
  { k: 0, id: 'dHeight', label: 'Delta.Height (Meas − Cmd)', fn: (r) => r.dHeight },
  { k: 0, id: 'dZ', label: 'Delta.Z (Meas − Cmd.Z)', fn: (r) => r.dZ },
  { k: 0, id: 'dOffset', label: 'Delta.Offset', fn: (r) => r.dOffset },
  { k: 1, id: 'i1', label: '[Item] InnerDia', fn: (r) => r.data[1] ?? null },
  { k: 1, id: 'i7', label: '[Item] Torq_InnerDia', fn: (r) => r.data[7] ?? null },
  { k: 1, id: 'i10', label: '[Item] In.InnerDia', fn: (r) => r.data[10] ?? null },
  { k: 1, id: 'i15', label: '[Item] Out.InnerDia', fn: (r) => r.data[15] ?? null },
  { k: 1, id: 'i2', label: '[Item] UpperBeadHeight', fn: (r) => r.data[2] ?? null },
  { k: 1, id: 'i3', label: '[Item] TireHeight', fn: (r) => r.data[3] ?? null },
  { k: 1, id: 'i13', label: '[Item] In.OffsetX', fn: (r) => r.data[13] ?? null },
  { k: 1, id: 'i14', label: '[Item] In.OffsetY', fn: (r) => r.data[14] ?? null },
  { k: 2, id: 's18', label: '[Sku] EachHeight', fn: (r) => r.data[18] ?? null },
  { k: 2, id: 's19', label: '[Sku] StackHeight', fn: (r) => r.data[19] ?? null },
  { k: 2, id: 's17', label: '[Sku] StackCount', fn: (r) => r.data[17] ?? null },
  { k: 3, id: 'f1', label: '[Floor] FloorHeight', fn: (r) => r.data[1] ?? null },
  { k: 4, id: 'p1', label: '[Pick] LastBeadPos − PickZTarget', fn: (r) => r.data[1] ?? null },
  {
    k: 4,
    id: 'p5',
    label: '[Pick] InnerDia',
    fn: (r) => ((r.data[3] ?? 0) > 0 ? (r.data[5] ?? null) : null),
  },
  { k: 5, id: 'm1', label: '[Manual] LaserInnerDia', fn: (r) => r.data[1] ?? null },
  { k: 5, id: 'm2', label: '[Manual] TorqueInnerDia', fn: (r) => r.data[2] ?? null },
]

export function metricsFor(kind: number): Metric[] {
  return METRICS.filter((m) => m.k === 0 || !kind || m.k === kind)
}

/** `ByCode.Meas` 키 → 표시 이름(PLC 멤버 이름 그대로). 순서 = 표 열 순서. */
export const MEAS_KEY_LABEL = {
  InnerDia: 'InnerDia',
  LaserInnerDia: 'LaserInnerDia',
  TorqInnerDia: 'TorqInnerDia',
  UpperBeadHeight: 'UpperBeadHeight',
  TireHeight: 'TireHeight',
  Offset: 'Offset',
  PickBeadPos: 'PickBeadPos',
  PickInnerDia: 'PickInnerDia',
  SkuEachHeight: 'SkuEachHeight',
  SkuStackHeight: 'SkuStackHeight',
} as const

export type MeasKey = keyof typeof MEAS_KEY_LABEL
export const MEAS_KEYS = Object.keys(MEAS_KEY_LABEL) as MeasKey[]
