// 계층 데이터 뷰어의 계약과 순수 헬퍼 — `JsonView.tsx` · `JsonViewRow.tsx`가 함께 쓴다.
//
// 규칙은 표와 같다: **한 줄 = 한 값**, 열이 자리를 지킨다(들여쓰기 14px 고정 · 행 높이 20px 고정).
// 값의 종류는 색이 말한다 — 문자열=그린 · 숫자=블루 · 불리언=골드 · null=회색 이탤릭.

/** 들여쓰기 한 단(px) — 고정이라 깊이가 눈금이 된다. */
export const IND = 14
/** 행 높이(px) — 고정이라 스크롤 위치가 개수로 읽힌다. */
export const ROW = 20

export type JsonNode = unknown

export function typeOf(v: unknown): string {
  return v === null ? 'null' : Array.isArray(v) ? 'array' : typeof v
}

export function isBranch(v: unknown): v is Record<string, unknown> {
  return v !== null && typeof v === 'object'
}

/** 값 종류 → 클래스. 상태색 여섯이 아니라 **종류**를 말하는 별개 축이다. */
export const VAL_CLASS: Record<string, string> = {
  string: 'text-ok-fg',
  number: 'text-info-fg',
  boolean: 'text-warn-fg',
  null: 'text-content-faint italic',
  undefined: 'text-content-faint italic',
}

/** 접힌 노드는 **개수를 말한다** — 무엇이 숨었는지 모르게 두지 않는다. */
export function preview(v: unknown): string {
  const t = typeOf(v)
  if (t === 'array') return `[…] ${(v as unknown[]).length} items`
  if (t === 'object') return `{…} ${Object.keys(v as object).length} keys`
  return ''
}

/** 검색 — 키·값 어느 쪽이 맞아도 걸리고, 가지는 자식이 맞으면 함께 걸린다. */
export function matches(v: unknown, key: string, q: string): boolean {
  if (!q) return true
  const n = q.toLowerCase()
  if (String(key).toLowerCase().includes(n)) return true
  if (isBranch(v)) return Object.entries(v).some(([k, x]) => matches(x, k, q))
  return String(v).toLowerCase().includes(n)
}

export interface FlatRow {
  path: string
  value: unknown
}

/** 잎만 모아 `root.a.b` 경로로 편다 — 표 뷰와 변경 추적이 같은 목록을 본다. */
export function flatten(v: unknown, path: string, out: FlatRow[] = []): FlatRow[] {
  if (isBranch(v)) {
    Object.entries(v).forEach(([k, x]) => flatten(x, `${path}.${k}`, out))
    return out
  }
  out.push({ path, value: v })
  return out
}

export function fmtValue(v: unknown): string {
  return typeof v === 'string' ? `"${v}"` : String(v)
}

/** `defaultDepth`까지 펼친 초기 집합. */
export function seedOpen(value: unknown, rootLabel: string, defaultDepth: number): Set<string> {
  const s = new Set<string>()
  const walk = (v: unknown, path: string, d: number) => {
    if (!isBranch(v) || d > defaultDepth) return
    s.add(path)
    Object.entries(v).forEach(([k, x]) => walk(x, `${path}.${k}`, d + 1))
  }
  walk(value, rootLabel, 0)
  return s
}

/** 검색 중에는 일치한 가지를 전부 펼친다 — 접힌 채로는 찾은 것이 안 보인다. */
export function expandForFilter(
  open: Set<string>,
  value: unknown,
  rootLabel: string,
  filter: string,
): Set<string> {
  if (!filter) return open
  const s = new Set(open)
  const walk = (v: unknown, path: string) => {
    if (!isBranch(v)) return
    Object.entries(v).forEach(([k, x]) => {
      if (matches(x, k, filter)) {
        s.add(path)
        walk(x, `${path}.${k}`)
      }
    })
  }
  walk(value, rootLabel)
  return s
}
