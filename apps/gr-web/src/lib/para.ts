// PARA 보기 — 검색과 값 표시(순수). 화면은 `components/measure/ParaView.tsx`.
//
// 현장에서는 파라미터를 **번호(p428)** 로 부르고, 문서·알람은 이름(`LaserInnerDia_Offset`)으로 부른다.
// 그래서 검색 한 칸이 둘 다 받는다: `p428`·`428` 은 번호 일치, 그 밖은 이름·경로·주석·형식 부분 일치.
import type { ParaGroup, ParaRow, ParaScalar } from './types'

/** 한 줄이 검색어에 맞는가. 빈 검색어는 모두 맞다. */
export function matchRow(row: ParaRow, q: string): boolean {
  const s = q.trim().toLowerCase()
  if (!s) return true
  const num = /^p?(\d+)$/.exec(s)
  if (num) {
    const n = Number(num[1])
    if (row.param !== null && row.param === n) return true
    // 범위 주석(`[p50-p53]`)은 첫 번호만 `param` 으로 오므로 범위 안의 번호도 맞힌다.
    // (부분 문자열로 찾으면 `p42` 가 `p428` 에 걸린다.)
    for (const m of row.comment.matchAll(/p(\d+)\s*-\s*p?(\d+)/gi)) {
      if (n >= Number(m[1]) && n <= Number(m[2])) return true
    }
    if (!s.startsWith('p')) return row.name.toLowerCase().includes(s)
    return false
  }
  return (
    row.path.toLowerCase().includes(s) ||
    row.comment.toLowerCase().includes(s) ||
    row.ty.toLowerCase().includes(s)
  )
}

/** 검색 결과 — 그룹 이름·주석이 맞으면 그룹 전체, 아니면 맞는 값 줄만 남긴다(빈 그룹은 뺀다). */
export function filterPara(groups: readonly ParaGroup[], q: string): ParaGroup[] {
  const s = q.trim().toLowerCase()
  if (!s) return groups.slice()
  const out: ParaGroup[] = []
  for (const g of groups) {
    if (g.name.toLowerCase().includes(s) || g.comment.toLowerCase().includes(s)) {
      out.push(g)
      continue
    }
    const rows = g.rows.filter((r) => r.leaf && matchRow(r, s))
    if (rows.length) out.push({ ...g, rows })
  }
  return out
}

/** 값 줄 수(머리줄 제외). */
export function leafCount(groups: readonly ParaGroup[]): number {
  return groups.reduce((n, g) => n + g.rows.filter((r) => r.leaf).length, 0)
}

function scalar(v: ParaScalar, ty: string): string {
  if (typeof v === 'boolean') return v ? 'TRUE' : 'FALSE'
  if (typeof v === 'number') {
    if (Number.isInteger(v)) return String(v)
    // Real 은 float32 라 0.1 이 0.10000000149 로 온다 — 소수 넷째 자리에서 끊고 뒤 0 을 뺀다.
    const digits = /lreal/i.test(ty) ? 6 : 4
    return String(Number(v.toFixed(digits)))
  }
  return v
}

/** 표에 쓰는 값 문자열 — 배열은 쉼표로, 구조체 배열은 JSON 으로. */
export function formatParaValue(v: ParaRow['value'], ty: string): string {
  if (v === null || v === undefined) return '-'
  if (Array.isArray(v)) {
    return v
      .map((x) => (x !== null && typeof x === 'object' ? JSON.stringify(x) : scalar(x, ty)))
      .join(', ')
  }
  if (typeof v === 'object') return JSON.stringify(v)
  return scalar(v, ty)
}
