// 측정 기록 `Data[0..19]` → 사람이 읽는 줄. 전에는 전부 "Value (mm)" 소수 2자리라 Status 10 · DiagFlags
// 257 · StackCount 6.00 이 길이처럼 보였다. 항목마다 **형식**(mm · 단 · 상태 · 비트 · 예/아니오)을 붙인다.
// 인덱스 정의는 GR2 `MeasureLogAdd` 머리 주석과 `MEAS_ITEM_*` / `MEAS_SKU_*` 상수를 따른다. 순수 함수 — 테스트 대상.
import { STATUS } from '../gr/const'
import { delta } from './format'

export type DataFmt =
  'mm' | 'count' | 'status' | 'skuDiag' | 'bool' | 'factor' | 'code' | 'reserved'

interface FieldSpec {
  label: string
  fmt: DataFmt
}

const R: FieldSpec = { label: 'Reserved', fmt: 'reserved' }
const mm = (label: string): FieldSpec => ({ label, fmt: 'mm' })
const ST: FieldSpec = { label: 'Status', fmt: 'status' }

/** 종류(Kind)별 Data 칸. 1 Item · 2 Sku · 3 Floor · 4 Pick · 5 Manual. */
export const DATA_FIELDS: Record<number, readonly FieldSpec[]> = {
  1: [
    ST,
    mm('InnerDia'),
    mm('UpperBeadHeight'),
    mm('TireHeight'),
    R,
    R,
    R,
    mm('Torq_InnerDia'),
    { label: 'Torq_Factor', fmt: 'factor' },
    R,
    mm('In.InnerDia'),
    mm('In.UpperBeadHeight'),
    mm('In.TireHeight'),
    mm('In.OffsetX'),
    mm('In.OffsetY'),
    mm('Out.InnerDia'),
    mm('Out.UpperBeadHeight'),
    mm('Out.TireHeight'),
    mm('Out.OffsetX'),
    mm('Out.OffsetY'),
  ],
  2: [
    ST,
    ...[1, 2, 3, 4, 5, 6, 7].flatMap((n) => [
      mm(`Stack[${n}].LowerBead`),
      mm(`Stack[${n}].UpperBead`),
    ]),
    { label: 'DiagFlags', fmt: 'skuDiag' },
    mm('LayerOffsetMax'),
    { label: 'StackCount', fmt: 'count' },
    mm('EachHeight'),
    mm('StackHeight'),
  ],
  3: [ST, mm('FloorHeight'), mm('Z.Position'), mm('FLD.Distance')],
  4: [
    ST,
    mm('LastBeadPos − Target'),
    mm('PickZTarget'),
    { label: 'Valid', fmt: 'bool' },
    { label: 'FitError', fmt: 'code' },
    mm('InnerDia'),
    mm('OffsetX'),
    mm('OffsetY'),
    mm('UpperBeadHeight'),
    mm('TireHeight'),
  ],
  5: [
    ST,
    mm('LaserInnerDia'),
    mm('TorqueInnerDia'),
    mm('TireHeight'),
    mm('OffsetX'),
    mm('OffsetY'),
    mm('UpperBeadHeight'),
    { label: 'FitError', fmt: 'code' },
  ],
}

/** 측정 상태 → 뜻(한 줄). 이름은 `STATUS`, 여기는 판단에 필요한 말. */
export const STATUS_TEXT: Record<number, string> = {
  0: '없음',
  1: '측정 중',
  2: '정상',
  3: '오류',
  4: '레이저 · 토크 내경 차이 큼',
  5: '방향별 단수 불일치 (무효)',
  6: '방향별 이탈 높이 편차 초과 (무효)',
  7: '단별 편심 초과 — 삐뚤게 쌓임 (무효)',
  8: '다 빠져나가기 전 리프트 끝 (무효)',
  9: '이탈 경계 없음 (무효)',
  10: '명령 단수와 측정 단수 다름 (무효)',
}

/** SKU `DiagFlags` 비트 — `FB_MeasureSku_V2` 스택 정렬 진단. `bad` = 무효 원인, 아니면 경고. */
export const SKU_DIAG_BITS: readonly { bit: number; text: string; bad: boolean }[] = [
  { bit: 0, text: '방향별 단수 불일치', bad: true },
  { bit: 1, text: '방향별 이탈 높이 편차', bad: true },
  { bit: 2, text: '단별 비드 높이 편차', bad: false },
  { bit: 3, text: 'TBR 비평면', bad: false },
  { bit: 4, text: '단별 편심 (삐뚤게 쌓임)', bad: true },
  { bit: 5, text: '비드 구간 원 맞춤 실패', bad: false },
  { bit: 6, text: '다 빠져나가기 전 리프트 끝', bad: true },
  { bit: 7, text: '이탈 경계 없음', bad: true },
  { bit: 8, text: '명령 단수 불일치', bad: true },
]

export function skuDiagLabels(flags: number): string[] {
  const out: string[] = []
  for (const b of SKU_DIAG_BITS)
    if (flags & (1 << b.bit)) out.push(`X${b.bit} ${b.text}${b.bad ? '' : ' (경고)'}`)
  return out
}

export interface DataLine {
  i: number
  label: string
  /** 원래 값(Real). */
  value: number
  /** 화면 글 — 형식이 붙은 값. */
  text: string
  /** 단위 칸('' = 없음). */
  unit: string
  /** 판단이 필요한 값 — 무효 상태·무효 비트. */
  bad: boolean
}

function line(i: number, spec: FieldSpec, v: number): DataLine {
  const base = { i, label: spec.label, value: v, unit: '', bad: false }
  switch (spec.fmt) {
    case 'mm':
      return { ...base, text: delta(v), unit: 'mm' }
    case 'count':
      return { ...base, text: String(Math.round(v)), unit: '단' }
    case 'status': {
      const n = Math.round(v)
      const name = STATUS[n] ?? String(n)
      const t = STATUS_TEXT[n]
      return { ...base, text: t ? `${name} — ${t}` : name, bad: n >= 3 }
    }
    case 'skuDiag': {
      const n = Math.round(v)
      const labels = skuDiagLabels(n)
      const bad = SKU_DIAG_BITS.some((b) => b.bad && n & (1 << b.bit))
      return { ...base, text: labels.length ? labels.join(' · ') : '없음', bad }
    }
    case 'bool':
      return { ...base, text: v ? '예' : '아니오' }
    case 'factor':
      return { ...base, text: v.toFixed(3) }
    case 'code':
      return { ...base, text: String(Math.round(v)), bad: Math.round(v) !== 0 }
    default:
      return { ...base, text: delta(v) }
  }
}

/**
 * 고른 기록의 Data 줄. 예비 칸은 값이 있을 때만, SKU 단 칸은 잰 단수(`StackCount`)를 넘고 0 이면 뺀다.
 */
export function dataLines(kind: number, data: readonly number[]): DataLine[] {
  const specs = DATA_FIELDS[kind] ?? []
  const count = kind === 2 ? Math.round(Number(data[17] ?? 0)) : 0
  const out: DataLine[] = []
  data.forEach((raw, i) => {
    const v = Number(raw)
    const spec = specs[i] ?? R
    if (spec.fmt === 'reserved' && !v) return
    if (kind === 2 && i >= 1 && i <= 14 && !v && Math.ceil(i / 2) > count) return
    out.push(line(i, spec, v))
  })
  return out
}
