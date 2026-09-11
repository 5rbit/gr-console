// 측정 화면 숫자·코드 포맷 — GrWeb(`tools/GrWeb/index.html`)의 f0/f1/f2/hex/tt/dtl/flagStr 이식.
import { TASK_TYPE_OF_CODE } from '../gr/const'
import type { PlcTask } from '../types'

export function f0(v: number | null | undefined): string {
  return v === null || v === undefined || Number.isNaN(v) ? '' : Number(v).toFixed(0)
}

/** 큰 값(≥1000)은 정수, 그 외 소수 1자리. */
export function f1(v: number | null | undefined): string {
  if (v === null || v === undefined || Number.isNaN(v)) return ''
  return Math.abs(v) >= 1000 ? Number(v).toFixed(0) : Number(v).toFixed(1)
}

export function f2(v: number | null | undefined): string {
  return v === null || v === undefined || Number.isNaN(v) ? '' : Number(v).toFixed(2)
}

export function hex(v: number): string {
  return '0x' + Number(v).toString(16).toUpperCase().padStart(2, '0')
}

/** TaskType 코드 → 이름(모르면 16진수, 0은 '-'). */
export function tt(code: number | null | undefined): string {
  if (!code) return '-'
  return TASK_TYPE_OF_CODE[code] ?? hex(code)
}

/** DTL 문자열을 초 단위까지만. */
export function dtl(s: unknown): string {
  return String(s ?? '')
    .replace('T', ' ')
    .replace(/\.\d{3,}.*$/, '')
}

/** 작업 플래그 요약(F=바닥측정 I=품목측정 S=SKU C=중심보정 A=회피 L=완료후상승 O=출고). */
export function flagStr(t: Partial<PlcTask> | null | undefined): string {
  if (!t) return '-'
  const s =
    (t.MeasureFloor ? 'F' : '') +
    (t.MeasureItem ? 'I' : '') +
    (t.MeasureSku ? 'S' : '') +
    (t.AdjustCenter ? 'C' : '') +
    (t.Avoid ? 'A' : '') +
    (t.LiftUpAfterComplete ? 'L' : '') +
    (t.Outbound ? 'O' : '')
  return s || '-'
}
