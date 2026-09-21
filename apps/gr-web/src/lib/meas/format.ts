// 측정 화면 숫자·코드 포맷 — GrWeb(`tools/GrWeb/index.html`)의 f0/f1/f2/hex/tt/dtl/flagStr 이식.
//
// **소수 자릿수는 양(量)이 정한다, 화면이 정하지 않는다.** 같은 값이 한 판넬 안에서 `12.3`과 `12`로
// 서면 읽는 사람이 "정밀도가 다른 값인가"를 매번 의심한다(실제로 Task 상세의 Position 은 1 자리,
// Cell.Position 은 0 자리였다). 그래서 자릿수를 **세 갈래**로 못 박고 화면은 이 이름만 부른다:
//
// | 양 | 자리 | 이름 | 왜 |
// | --- | --- | --- | --- |
// | 위치·치수 (mm) | 1 | `pos` | 축 지령·셀 좌표의 분해능이 0.1 mm 다. 그 아래는 잡음이다 |
// | 편차·Δ (mm) | 2 | `delta` | 판정 기준(비드 여유·보정량)이 0.01 mm 단위로 말한다 |
// | 산포·미세값 | 3 | `fine` | σ 처럼 **0.01 아래가 실제 값**인 자리에만. 그 외에 쓰지 않는다 |
//
// 옛 이름 `f0`/`f1`/`f2` 는 다른 화면들이 부르고 있어 그대로 둔다(f1 은 1000 이상을 정수로 접는
// 목록용 규칙이라 위치 열에는 `pos` 를 쓴다).
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

/** 자리 고정 소수 — 값이 없으면 빈 칸(0 으로 채우면 없는 값이 데이터로 읽힌다). */
function fixed(v: number | null | undefined, digits: number): string {
  return v === null || v === undefined || Number.isNaN(Number(v)) ? '' : Number(v).toFixed(digits)
}

/** 위치·치수 mm — 소수 **1자리**. X/Y/Z/G · Cell.Position · 높이 · 지름. */
export function pos(v: number | null | undefined): string {
  return fixed(v, 1)
}

/** 편차·Δ mm — 소수 **2자리**. Delta.* · BeadDev · 오프셋 보정량. */
export function delta(v: number | null | undefined): string {
  return fixed(v, 2)
}

/** 산포·미세값 — 소수 **3자리**. σ 처럼 0.01 아래가 실제 값인 자리에만. */
export function fine(v: number | null | undefined): string {
  return fixed(v, 3)
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
