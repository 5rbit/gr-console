// 로거 설정(`EVTLOG.Cfg`) 편집 — CatMask 비트 · MaxPerScan 범위 · 바뀐 칸만 PUT.
import type { EvtCfg, EvtCfgPatch } from './api'

export const MAX_PER_SCAN_MIN = 1
export const MAX_PER_SCAN_MAX = 200

/** bit n = 카테고리 id n. 32 비트 부호 없는 값으로 유지한다(bit 31 이 음수로 뒤집히지 않게). */
export function hasBit(mask: number, bit: number): boolean {
  if (bit < 0 || bit > 31) return false
  return ((mask >>> bit) & 1) === 1
}

export function setBit(mask: number, bit: number, on: boolean): number {
  if (bit < 0 || bit > 31) return mask >>> 0
  const m = (1 << bit) >>> 0
  return (on ? mask | m : mask & ~m) >>> 0
}

export interface CfgDraft {
  minLevel: number
  catMask: number
  maxPerScan: string
}

export function draftOf(c: EvtCfg): CfgDraft {
  return { minLevel: c.min_level, catMask: c.cat_mask >>> 0, maxPerScan: String(c.max_per_scan) }
}

/** MaxPerScan 칸 판정 — 잘못이면 사유. */
export function maxPerScanError(s: string): string | undefined {
  const t = s.trim()
  if (!/^\d+$/.test(t)) return 'MaxPerScan 은 정수입니다'
  const n = Number(t)
  if (n < MAX_PER_SCAN_MIN || n > MAX_PER_SCAN_MAX)
    return `MaxPerScan 은 ${MAX_PER_SCAN_MIN}~${MAX_PER_SCAN_MAX} 입니다`
  return undefined
}

/** 원본과 다른 칸만 — 바뀐 것이 없으면 빈 객체. */
export function cfgPatch(orig: EvtCfg, d: CfgDraft): EvtCfgPatch {
  const p: EvtCfgPatch = {}
  if (d.minLevel !== orig.min_level) p.min_level = d.minLevel
  if (d.catMask >>> 0 !== orig.cat_mask >>> 0) p.cat_mask = d.catMask >>> 0
  const n = Number(d.maxPerScan.trim())
  if (!maxPerScanError(d.maxPerScan) && n !== orig.max_per_scan) p.max_per_scan = n
  return p
}
