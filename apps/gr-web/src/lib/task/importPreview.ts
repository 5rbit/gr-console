// Excel 가져오기 미리보기의 **읽는 법** — 응답 숫자를 줄 결과 넷으로 세우고, 적용을 막을 사유를 정한다.
//
// 숫자 자체는 서버가 센다. 여기서 하는 일은 두 가지다.
//
// ① **옛 이름을 가린다.** 파일 가져오기 응답은 `added|updated|unchanged|skipped` 넷을 주지만,
//    PLC 읽기 응답(`/api/cells/import`)은 아직 `imported`·`skipped`(= 값이 같아 건너뜀) 둘뿐이다.
//    같은 다이얼로그가 둘을 받으므로 "skipped 가 무슨 뜻인가"를 한 자리에서 정한다 — 화면 곳곳에서
//    `r.unchanged ?? r.skipped` 를 되풀이하면 한 군데는 반드시 반대로 읽는다.
//
// ② **적용을 막을 때 이유를 만든다.** 회색 버튼은 고장으로 읽힌다(`docs/DESIGN.md` 4절 ⑥) —
//    "적용할 행이 없습니다"와 "모두 같은 값입니다"와 "모든 행에 오류가 있습니다"는 운전자가 할 일이
//    서로 다르다.
import type { HelpSection } from '../ui/HelpTip'
import type { FileImportResult } from './types'

/** 가져오기가 받는 파일 — 백엔드 `registry::xlsx::parse_file` 과 같은 목록. */
export const SHEET_ACCEPT = '.xlsx,.xlsm,.xls,.csv'

/** 끌어다 놓은 파일이 표 파일인가 — 아니면 서버까지 가지 않고 그 자리에서 말한다. */
export function isSheetFile(name: string): boolean {
  return /\.(xlsx|xlsm|xls|csv|txt)$/i.test(name.trim())
}

/** 미리보기 머리의 시트별 줄 수 — 0 인 표는 빼서 품목 파일에 `셀 0 · 스테이션 0` 이 서지 않게. */
export function sheetCountsText(c: FileImportResult['counts']): string {
  if (!c) return ''
  const parts: string[] = []
  if (c.cells) parts.push(`셀 ${c.cells}`)
  if (c.stations) parts.push(`스테이션 ${c.stations}`)
  if (c.items) parts.push(`품목 ${c.items}`)
  if (c.item_profiles) parts.push(`비드 ${c.item_profiles}줄`)
  if (c.stock) parts.push(`재고 ${c.stock}줄`)
  return parts.join(' · ')
}

/** 가져오기 다이얼로그의 `?` — 모든 레지스트리에 공통. */
export const IMPORT_SCOPE_HELP: HelpSection[] = [
  {
    title: '적용 범위',
    body: '오류 행은 건너뛰고 나머지만 로컬 사본에 적용합니다. PLC 에는 쓰지 않습니다 — 적용 뒤 PLC 쓰기로 반영하세요.',
  },
]

/** 화물 규격 가져오기의 `?` — 양식의 두 시트와 채울 열. 정본은 `docs/item-spec-z.md` "Excel" 절. */
export const ITEMS_EXCEL_HELP: HelpSection[] = [
  {
    title: 'Items 시트',
    body: '한 줄 = 품목 하나. Code 는 꼭 채우고 Name 도 채우세요(비우면 이름 없는 품목). 나머지 열은 비우면 기본값(Count 1, 치수 0, StackMax·PalletMax 0 = 제한 없음)입니다. StackMax · PalletMax · WeightKg · PickBeadOffset · Compression 은 열을 아예 지우면 이미 있는 품목의 그 값을 그대로 둡니다.',
  },
  {
    title: 'ItemBeadProfile 시트(선택)',
    body: '한 줄 = 잰 스택 하나의 한 단(Code · Stack · Level 키, 셀 바닥 기준 mm). 줄이 하나라도 있으면 Items 시트에 있는 코드의 비드 프로파일은 이 시트가 정본입니다(그 코드의 줄이 없으면 프로파일이 비워집니다). 머리글만 있으면 없는 시트로 보고 이미 잰 비드를 그대로 둡니다.',
  },
  ...IMPORT_SCOPE_HELP,
]

/** 줄 하나에 붙는 결과 넷 — 백엔드 `registry::xlsx::ApplyCounts` 와 같은 말. */
export interface ImportOutcome {
  added: number
  updated: number
  /** 파일 값이 저장된 것과 같아 쓰지 않은 줄. */
  unchanged: number
  /** 적용하지 못한 줄(오류와 1:1). */
  skipped: number
  /** 파일에 없어 지워질 자리(재고 교체 가져오기). 다른 가져오기는 0. */
  removed: number
  /** 실제로 쓰이는 줄 — 이 수가 0 이면 적용할 것이 없다. */
  applicable: number
  problems: number
}

export function importOutcome(r: FileImportResult): ImportOutcome {
  // 옛 응답에는 `unchanged` 가 없고 그 뜻을 `skipped` 가 지고 있었다.
  const legacy = r.unchanged === undefined
  const added = r.added ?? r.imported
  const unchanged = legacy ? r.skipped : (r.unchanged ?? 0)
  const skipped = legacy ? 0 : r.skipped
  return {
    added,
    updated: r.updated,
    unchanged,
    skipped,
    removed: r.removed ?? 0,
    applicable: added + r.updated + (r.removed ?? 0),
    problems: r.errors.length,
  }
}

export type OutcomeKey = 'added' | 'updated' | 'unchanged' | 'skipped' | 'removed'

/** 미리보기 머리줄의 라벨+값 짝(`lib/ui/StatRow`). 0 도 보인다 — 없는 칸은 "안 셌다"로 읽힌다. */
export function outcomeStats(
  o: ImportOutcome,
): { key: OutcomeKey; label: string; value: number }[] {
  return [
    { key: 'added', label: '추가', value: o.added },
    { key: 'updated', label: '갱신', value: o.updated },
    { key: 'unchanged', label: '동일', value: o.unchanged },
    { key: 'skipped', label: '건너뜀', value: o.skipped },
    // 지울 것이 있을 때만(재고 교체) — 다른 가져오기에는 이 칸이 없다.
    ...(o.removed > 0 ? [{ key: 'removed' as const, label: '삭제', value: o.removed }] : []),
  ]
}

/** 한 줄 요약 — 0 인 결과는 빼고 붙인다(`품목 12건 추가 · 3건 갱신`). */
export function outcomeSummary(what: string, o: ImportOutcome): string {
  const parts = outcomeStats(o)
    .filter((s) => s.value > 0)
    .map((s) => `${s.value}건 ${s.label}`)
  return parts.length === 0 ? `${what} 바뀐 것 없음` : `${what} ${parts.join(' · ')}`
}

/**
 * 적용을 막을 사유 — `null` 이면 누를 수 있다.
 * `file: false` = 아직 파일이 없다, `failed` = 미리보기 요청 자체가 실패했다(파일을 못 읽음 등).
 */
export function applyBlockedReason(
  preview: FileImportResult | null,
  state: { file?: boolean; failed?: boolean } = {},
): string | null {
  if (state.file === false) return '먼저 파일을 고르세요'
  if (!preview)
    return state.failed
      ? '미리보기에 실패했습니다 — 오류를 보고 파일을 다시 고르세요'
      : '미리보기를 기다리는 중입니다'
  const o = importOutcome(preview)
  if (o.applicable > 0) return null
  if (o.unchanged > 0 && o.skipped === 0) return '파일의 값이 이미 저장된 것과 같습니다'
  if (o.skipped > 0) return '모든 행에 오류가 있습니다 — 아래 표의 줄을 고쳐 다시 올리세요'
  return '적용할 행이 없습니다'
}

/** 문제 행 표의 한 줄 — 줄·시트·사유. */
export interface ProblemRow {
  key: string
  row: number
  sheet: string
  message: string
}

/** 오류를 표로 — 같은 줄이 두 번 나와도 키가 겹치지 않게 순번을 붙인다. */
export function problemRows(r: FileImportResult | null): ProblemRow[] {
  return (r?.errors ?? []).map((e, i) => ({
    key: `${i}:${e.sheet ?? ''}:${e.row}`,
    row: e.row,
    sheet: e.sheet ?? '',
    message: e.message,
  }))
}
