// 화물 규격(품목) 화면의 순수 로직 — 검색·재고 사용 집계·복제 코드·검증.
//
// 검증은 백엔드 `validate_item`과 같은 규칙이다(서버 400을 기다리지 않게). 규칙이 바뀌면 두 곳을 함께 고친다.
import type { Item, ItemUpsert, StockEntry } from '../types'

/** 코드·이름·비고 검색(대소문자 무시). */
export function matchesItem(i: Pick<Item, 'code' | 'name' | 'note'>, q: string): boolean {
  const needle = q.trim().toLowerCase()
  if (!needle) return true
  return `${i.code} ${i.name} ${i.note}`.toLowerCase().includes(needle)
}

/** 코드 하나를 쓰는 재고 — 칸 수와 총 개수. */
export interface ItemUsage {
  cells: number
  count: number
}

/** 품목 코드 → 재고 사용. 비었거나(코드 0) 개수가 0인 칸은 쓰는 것으로 치지 않는다. */
export function stockUsage(stock: Iterable<StockEntry>): Map<number, ItemUsage> {
  const out = new Map<number, ItemUsage>()
  for (const s of stock) {
    if (!s.item_code || s.count <= 0) continue
    const u = out.get(s.item_code) ?? { cells: 0, count: 0 }
    out.set(s.item_code, { cells: u.cells + 1, count: u.count + s.count })
  }
  return out
}

/** 복제 초안 — 코드는 가장 큰 코드 + 1, 이름에 `(복사)`. */
export function duplicateItem(src: ItemUpsert, existing: readonly { code: number }[]): ItemUpsert {
  const next = existing.reduce((m, i) => Math.max(m, i.code), src.code) + 1
  return { ...src, code: next, name: src.name ? `${src.name} (복사)` : '' }
}

/** 삭제 확인에 쓸 사용 중인 코드들(코드 오름차순). */
export function usedCodes(
  codes: readonly number[],
  usage: ReadonlyMap<number, ItemUsage>,
): { code: number; usage: ItemUsage }[] {
  return [...codes]
    .sort((a, b) => a - b)
    .flatMap((code) => {
      const u = usage.get(code)
      return u ? [{ code, usage: u }] : []
    })
}

const DIMS: [keyof ItemUpsert, string][] = [
  ['inner_diameter', 'InnerDiameter'],
  ['outer_diameter', 'OuterDiameter'],
  ['lower_bead_height', 'LowerBidHeight'],
  ['upper_bead_height', 'UpperBidHeight'],
  ['height', 'Height'],
]

/** 백엔드 `validate_item`과 같은 규칙. 빈 배열이면 통과. */
export function itemErrors(i: ItemUpsert): string[] {
  const out: string[] = []
  if (!Number.isInteger(i.code) || i.code < 1) out.push('Code: 1 이상의 정수여야 합니다')
  if (!Number.isInteger(i.count) || i.count < 1 || i.count > 255) out.push('Count: 1..255')
  for (const [k, label] of DIMS) {
    const v = i[k] as number
    if (!Number.isFinite(v) || v < 0) out.push(`${label}: 0 이상의 숫자여야 합니다`)
  }
  if (!Number.isFinite(i.deflection_factor)) out.push('DeflectionFactor: 숫자여야 합니다')
  // 0 은 "미입력" — 둘 다 입력됐을 때만 비교한다.
  if (i.inner_diameter > 0 && i.outer_diameter > 0 && i.inner_diameter >= i.outer_diameter)
    out.push('InnerDiameter 는 OuterDiameter 보다 작아야 합니다')
  return out
}

type ItemLabelSource = Pick<Item, 'code' | 'name' | 'inner_diameter' | 'outer_diameter' | 'height' | 'note'>

/** 드롭다운 한 줄 — `코드 · 이름 · ID · OD · H · 비고`. 0(미입력) 치수와 빈 이름·비고는 뺀다. */
export function itemLabel(i: ItemLabelSource): string {
  const num = (v: number) => String(Math.round(v * 10) / 10)
  const dims = (
    [
      ['ID', i.inner_diameter],
      ['OD', i.outer_diameter],
      ['H', i.height],
    ] as const
  )
    .filter(([, v]) => v > 0)
    .map(([k, v]) => `${k} ${num(v)}`)
  return [String(i.code), i.name, ...dims, i.note.trim()].filter(Boolean).join(' · ')
}
