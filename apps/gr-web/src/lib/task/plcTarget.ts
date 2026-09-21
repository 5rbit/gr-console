// 레지스트리 PLC 대상 — `/api/plcs`의 S7 목록에서 선택지를 만들고, 읽기·차이에 쓸 한 대를 고른다.
//
// 대상은 설정(`[[plcs]]`)에 따라 GR1·GR2·GRM처럼 늘고 준다. 선택지를 하드코드하면 GR1을 추가한 현장에서
// GR1 테이블을 읽을 길이 없다. 쓰기 전체(`all`)는 GR PLC들 → GRM 순서로 백엔드가 쓴다.
import type { PlcTarget } from './types'

/** `/api/plcs` 한 줄 중 여기서 쓰는 것 — id 는 설정에 따라 `gr1_s7` 처럼 `PlcId` 밖으로 늘 수 있어 문자열로 받는다. */
export interface PlcListRow {
  id: string
  kind: string
  name?: string
  role?: 'gr' | 'grm'
}

/** 쓰기 전용 "전체" 값 — 읽기·차이는 PLC 한 대만 본다. */
export const ALL_TARGET = 'all'

/** 대상 PLC 한 대(설정 이름 + 역할). */
export interface PlcRef {
  name: string
  role: 'gr' | 'grm'
}

/** S7 PLC만 — 이름은 `name`(없으면 `gr2_s7` → `GR2`), 역할은 `role`(없으면 이름으로 짐작). GR 먼저, GRM 뒤. */
export function s7Plcs(list: readonly PlcListRow[]): PlcRef[] {
  const out: PlcRef[] = []
  for (const p of list) {
    if (p.kind !== 's7') continue
    const name = p.name ?? p.id.replace(/_s7$/i, '').toUpperCase()
    if (!name || out.some((x) => x.name.toLowerCase() === name.toLowerCase())) continue
    const role = p.role ?? (/^grm$/i.test(name) ? 'grm' : 'gr')
    out.push({ name, role })
  }
  const gr = out.filter((p) => p.role === 'gr')
  const grm = out.filter((p) => p.role === 'grm')
  return [...gr, ...grm]
}

/** 기본 대상 — 선택된 로봇의 PLC, 없으면 첫 GR PLC, 그것도 없으면 첫 PLC. 목록이 비면 `null`. */
export function defaultTarget(plcs: readonly PlcRef[], robotPlc: string | null | undefined): string | null {
  if (robotPlc) {
    const hit = plcs.find((p) => p.name.toLowerCase() === robotPlc.toLowerCase())
    if (hit) return hit.name
  }
  return (plcs.find((p) => p.role === 'gr') ?? plcs[0])?.name ?? null
}

/** 사용자가 고른 값이 지금 목록에 없으면(설정이 바뀌었거나 아직 목록 전) 기본값으로 돌아간다. */
export function effectiveTarget(
  chosen: PlcTarget | null,
  plcs: readonly PlcRef[],
  fallback: string,
): PlcTarget {
  if (chosen === null) return fallback
  if (isWriteAll(chosen)) return plcs.length > 1 ? chosen : fallback
  return plcs.find((p) => p.name.toLowerCase() === chosen.toLowerCase())?.name ?? fallback
}

export function isWriteAll(t: PlcTarget): boolean {
  return t === ALL_TARGET || t === 'both'
}

/** 읽기·차이는 한 대 — "전체"면 기본 대상으로 본다. */
export function readTarget(t: PlcTarget, fallback: string): string {
  return isWriteAll(t) ? fallback : t
}

/** 표시 이름 — `all`은 쓰는 순서대로 이어 붙인다(`GR1 + GR2 + GRM`). */
export function targetLabel(t: PlcTarget, plcs: readonly PlcRef[]): string {
  if (t === 'both') return 'GR2 + GRM'
  if (t === ALL_TARGET) return plcs.length > 0 ? plcs.map((p) => p.name).join(' + ') : '전체'
  return t
}

/** 선택지 — PLC 하나씩 + (둘 이상이면) 전체 쓰기. */
export function targetOptions(plcs: readonly PlcRef[]): { value: PlcTarget; label: string }[] {
  const one = plcs.map((p) => ({ value: p.name, label: p.name }))
  return plcs.length > 1 ? [...one, { value: ALL_TARGET, label: '전체(쓰기)' }] : one
}
