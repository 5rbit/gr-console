// 스테이션 신호 조건식 — `CVOK & Req & ItemExist & CVNO 0 & Comp 0` 를 항목별 충족 여부와 함께(색은 화면이 입힌다).
import type { Cond } from '../sched'

export interface CondToken {
  /** `Req` · 꺼져 있어야 하는 항목은 `CVNO 0`. */
  text: string
  ok: boolean
}

export function condTokens(expr: readonly Cond[] | undefined | null): CondToken[] {
  return (expr ?? []).map((c) => ({ text: c.neg ? `${c.name} 0` : c.name, ok: c.ok }))
}

/** 한 줄 글자(툴팁 · 좁은 칸) — 안 된 항목 뒤에 (미충족). */
export function condText(expr: readonly Cond[] | undefined | null): string {
  return condTokens(expr)
    .map((t) => (t.ok ? t.text : `${t.text}(미충족)`))
    .join(' & ')
}

/** 모두 충족인가(항목이 없으면 false — 신호를 모른다). */
export function condAll(expr: readonly Cond[] | undefined | null): boolean {
  return !!expr?.length && expr.every((c) => c.ok)
}
