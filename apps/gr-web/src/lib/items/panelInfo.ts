// 화물 규격 상세 패널의 **글**을 화면에서 떼어 낸 곳 — 칩 한 줄과 `?` 팝오버의 내용.
//
// 패널 본문에 설명 문단을 쌓으면(예전에는 표 위에 네 문단이었다) 매번 같은 글을 다시 읽어 넘기고
// 정작 고칠 표는 접힌 자리로 밀린다. 그래서 규칙은 하나다:
//
// - 표·열 머리가 이미 말하는 것은 **지운다**,
// - 값이 있는 것(최신 측정 · ByCode · 표본 수)은 **칩 한 조각**(라벨+값)으로 띠에 세우고,
// - 규칙·공식·용어 풀이는 `?` 뒤 팝오버로 **한 번 읽고 닫게** 한다.
//
// 여기 있는 것은 전부 순수 함수다(DOM 없음) — 글이 값에 맞게 나오는지는 `panelInfo.test.ts`가 본다.
// 필드 이름은 PLC/데이터 이름 그대로(영문), 설명만 한국어.
import type { CompressionSuggestion, ItemLevels, ItemSpec, Trend } from '../types'
import { compressionOf, pickBeadOffset, round1 } from './levelsModel'

/** 팝오버 한 칸 — 소제목 + 본문. */
export interface HelpSection {
  title: string
  body: string
}

/** 띠에 서는 칩 하나 — 라벨(영문 이름)+값. `title`은 마우스를 얹었을 때의 전체 값. */
export interface Chip {
  label: string
  value: string
  title: string
}

/** 라벨+값 한 줄 — 팝오버 안의 표. */
export interface InfoRow {
  label: string
  value: string
}

const MD_HM = /^(\d{4})-(\d{2})-(\d{2})[ T](\d{2}):(\d{2})/

/** `2026-09-18 13:59:30.834` → `09-18 13:59` (알아볼 수 없으면 앞 16자). */
export function shortAt(at: string): string {
  const m = MD_HM.exec((at ?? '').trim())
  return m ? `${m[2]}-${m[3]} ${m[4]}:${m[5]}` : (at ?? '').trim().slice(0, 16)
}

/** 측정 그대로의 시각(초까지) — 팝오버에서 쓴다. */
export function fullAt(at: string): string {
  return (at ?? '').trim().replace('T', ' ').slice(0, 19) || '—'
}

type Measured = NonNullable<ItemLevels['measured']>
type Summary = NonNullable<ItemLevels['measured_summary']>

/** 최신 SKU 측정 칩 — `GR2 #41 · 09-18 13:59 · n=3`. 없으면 null(칩을 세우지 않는다). */
export function measuredChip(m: Measured | null | undefined): Chip | null {
  if (!m) return null
  return {
    label: m.plc,
    value: `#${m.seq} · ${shortAt(m.at)} · n=${m.count}`,
    title: [
      `최신 SKU 측정 ${m.plc} Seq ${m.seq}`,
      fullAt(m.at),
      `TotalCount ${m.count}`,
      `EachHeight ${round1(m.each_height)}`,
      `TotalHeight ${round1(m.stack_height)}`,
    ].join(' · '),
  }
}

/** 최신 측정의 값 — 팝오버 표(단위는 라벨이 아니라 이름에 붙지 않는다 — 전부 mm). */
export function measuredRows(m: Measured | null | undefined): InfoRow[] {
  if (!m) return []
  return [
    { label: 'Seq', value: `${m.plc} #${m.seq}` },
    { label: 'At', value: fullAt(m.at) },
    { label: 'TotalCount', value: String(m.count) },
    { label: 'EachHeight', value: String(round1(m.each_height)) },
    { label: 'TotalHeight', value: String(round1(m.stack_height)) },
  ]
}

const trendText = (t: Trend | null | undefined): string =>
  t && t.Count ? `${round1(t.Ema)} (avg ${round1(t.Avg)}, n=${t.Count})` : '—'

/** ByCode 추세 칩 — `ByCode n=15`. */
export function byCodeChip(s: Summary | null | undefined): Chip | null {
  if (!s) return null
  return {
    label: 'ByCode',
    value: `${s.plc} n=${s.count}`,
    title: `MEASLOG.ByCode ${s.plc} · 표본 ${s.count} · 마지막 ${fullAt(s.last_time)}`,
  }
}

/** ByCode 추세 표 — Ema (avg, n). */
export function byCodeRows(s: Summary | null | undefined): InfoRow[] {
  if (!s) return []
  return [
    { label: 'UpperBead', value: trendText(s.upper_bead) },
    { label: 'SkuEachHeight', value: trendText(s.each_height) },
    { label: 'SkuStackHeight', value: trendText(s.stack_height) },
    { label: 'PickBeadPos', value: trendText(s.pick_bead_pos) },
  ]
}

/** 지금 보는 스택이 잰 값인가 환산값인가 — 칩 하나로. */
export function profileChip(count: number, stored: boolean): Chip {
  return {
    label: `n=${count}`,
    value: stored ? '잰 값' : '환산',
    title: stored
      ? `스택 ${count} 은(는) 통째로 잰 프로파일입니다 — 값을 그대로 씁니다`
      : `스택 ${count} 은(는) 잰 적이 없어 다른 크기의 프로파일에서 환산한 값입니다 — 고치면 손입력(manual)으로 저장됩니다`,
  }
}

/** 이 프로파일을 채운 표본 — `GR2 #41`. */
export function sourceChip(plc: string | undefined, seq: number | undefined, at?: string): Chip | null {
  if (!seq) return null
  return {
    label: 'Sample',
    value: `${plc || '?'} #${seq}`,
    title: `이 프로파일을 채운 SKU 표본 ${plc || '?'} #${seq}${at ? ` · ${fullAt(at)}` : ''}`,
  }
}

/** `?` 팝오버 본문 — 표 위에서 걷어 낸 문단 넷이 여기로 왔다. */
export function beadsHelp(spec: ItemSpec, suggestion: CompressionSuggestion | null): HelpSection[] {
  const offset = round1(pickBeadOffset(spec))
  const comp = round1(compressionOf(spec))
  const out: HelpSection[] = [
    {
      title: '정본',
      body: '잰 스택 크기별 절대 프로파일입니다. 값은 모두 셀 바닥 기준(mm)이고, 같은 크기의 스택을 집을 때 환산 없이 그대로 쓰입니다.',
    },
    {
      title: '환산',
      body: '잰 적 없는 크기는 다른 크기의 프로파일에서 환산해 흐리게 보입니다. 칸을 고치면 그 줄은 manual 이 되어 다음 SKU 측정이 덮지 않습니다.',
    },
    { title: 'Above', body: '이 타이어 위에 얹힌 개수 — 파생 곡선의 키입니다.' },
    {
      title: 'PickZ',
      body: `AbsUpperBead − PickBeadOffset ${offset} (셀 바닥 기준). 그 크기·하중의 비드를 아직 모르면 그립점이 타이어 중간(Height/2)으로 내려가 mid 가 붙습니다.`,
    },
    {
      title: 'Compression',
      body: `${comp} mm/개 — 잰 적 없는 크기의 단 높이를 위한 대체값입니다.${
        suggestion && suggestion.value !== null
          ? ` 측정 제안 ${round1(suggestion.value)} mm/개 (${suggestion.method ?? '?'}, n=${suggestion.samples}, 잰 스택 ${suggestion.measured_count}단).`
          : ''
      }`,
    },
    {
      title: 'Source',
      body: 'measured = 측정 그대로 · manual = 손으로 고침 · interpolated = 이웃 점 보간 · computed = Compression 모형.',
    },
  ]
  return out
}

/** Spec 탭 — 묶음 셋의 `?` 본문. */
export const SPEC_HELP: Record<'plc' | 'console' | 'pick', HelpSection[]> = {
  plc: [
    { title: 'LGR_Stock_Item', body: '작업 명령을 낼 때 PLC 로 그대로 가는 품목 필드입니다.' },
    {
      title: 'DeflectionFactor',
      body: 'PLC 로만 전달합니다 — 콘솔의 Z 계산에는 쓰지 않습니다.',
    },
  ],
  console: [
    {
      title: '콘솔 전용',
      body: 'PLC 로 가지 않습니다 — 재고 한도 · DROP 제출 검사 · Z 규칙에만 씁니다.',
    },
    { title: 'StackMax · PalletMax', body: '0 = 제한 없음. StackMax 는 셀 재고와 DROP 을 막습니다.' },
  ],
  pick: [
    {
      title: 'PICK Z',
      body: '상부 비드(눌림 반영) − PickBeadOffset 입니다.',
    },
    {
      title: '눌림',
      body: 'n 개 스택의 k 단 타이어 위에는 n − k 개가 얹혀 있어 높이 · 상부 비드가 그만큼 내려갑니다. 집는 높이는 그 눌린 상부 비드에서 PickBeadOffset 만큼 아래입니다.',
    },
    {
      title: 'PLC 보정',
      body: 'PLC 는 하강 중 본 비드로 한 번 더 보정합니다 (PARA Task.Pick_BeadZOffset p973, 기본 20 mm) — 두 값을 함께 맞추세요.',
    },
  ],
}

/** Usage 탭 · 목록 표의 열 풀이 — `?`·열 머리 툴팁에 그대로 나간다. */
export const USAGE_HELP: HelpSection[] = [
  { title: '재고 칸', body: '이 코드를 들고 있는 셀과 그 개수입니다. StackMax 를 넘은 칸은 경고색입니다.' },
  { title: '시나리오 스텝', body: '이 코드를 쓰는 스텝입니다 — 코드를 지우면 그 스텝은 제출 때 실패합니다.' },
]

/** 목록 표 일괄 편집 줄의 `?`. */
export const BULK_HELP: HelpSection[] = [
  { title: '일괄 편집', body: '체크한 품목의 StackMax · PalletMax 만 바꿉니다. 빈칸은 그대로 두고, 0 은 제한 없음입니다.' },
]

/**
 * 측정값이 없을 때 띠에 서는 **상태 한 조각**(문단이 아니다) — 자세한 사정은 `title` 로 간다.
 * 값이 있으면 null 이고, 그 자리에는 측정 칩이 대신 선다.
 */
export function levelsState(
  err: string | null,
  loading: boolean,
  has: boolean,
  robotName: string,
): { text: string; title: string } | null {
  if (err) return { text: '측정값 실패', title: `측정값 조회 실패 — ${err}` }
  if (has) return null
  if (loading) return { text: '받는 중…', title: '측정값을 받고 있습니다' }
  return { text: '측정 없음', title: `${robotName} 에 이 코드의 SKU 측정 기록이 없습니다` }
}
