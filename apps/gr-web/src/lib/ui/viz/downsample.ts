// min-max 엔벨로프 다운샘플 — 파형의 점 수를 화면 픽셀 수준으로 묶는다. 차트가 hand-rolled SVG라
// 점 하나가 곧 path 좌표이고, 문자열 생성·파서·레이아웃 비용이 점 수에 그대로 붙는다.
//
// 평균으로 줄이면 스파이크가 사라지므로, 버킷마다 min·max 두 점을 그 버킷의 시각에 배출한다
// (오실로스코프의 표준 기법).

/** 다운샘플 결과 — 원본과 같은 (t, v) 쌍 구조. */
export interface Enveloped {
  t: number[]
  v: (number | null)[]
}

/**
 * `budget`점 이하로 줄인다. 원본이 이미 예산 안이면 **그대로 돌려준다**(복사도 하지 않는다).
 *
 * 결측(`null`)은 버킷 안에서 무시하고, 버킷 전체가 결측이면 `null` 한 점을 배출해 선을 끊는다
 * (gap을 메우면 없는 데이터를 그린 것이 된다).
 */
export function envelope(t: number[], v: (number | null)[], budget: number): Enveloped {
  const n = Math.min(t.length, v.length)
  if (n <= budget || budget < 4) return { t, v }
  // 버킷당 2점을 배출하므로 버킷 수는 예산의 절반.
  const buckets = Math.max(2, Math.floor(budget / 2))
  const size = n / buckets
  const outT: number[] = []
  const outV: (number | null)[] = []
  for (let b = 0; b < buckets; b++) {
    const lo = Math.floor(b * size)
    const hi = Math.min(n, Math.floor((b + 1) * size))
    if (hi <= lo) continue
    let min = Infinity
    let max = -Infinity
    let minI = -1
    let maxI = -1
    for (let i = lo; i < hi; i++) {
      const x = v[i]
      if (x === null || !Number.isFinite(x)) continue
      if (x < min) {
        min = x
        minI = i
      }
      if (x > max) {
        max = x
        maxI = i
      }
    }
    if (minI < 0) {
      // 버킷 전체가 결측 — 한 점만 null로 배출해 선을 끊는다.
      outT.push(t[lo])
      outV.push(null)
      continue
    }
    // **원본 순서를 지킨다** — min이 먼저 온 버킷에서 max를 먼저 배출하면 파형이 지그재그로 뒤집힌다.
    const [firstI, secondI] = minI <= maxI ? [minI, maxI] : [maxI, minI]
    outT.push(t[firstI])
    outV.push(v[firstI])
    if (secondI !== firstI) {
      outT.push(t[secondI])
      outV.push(v[secondI])
    }
  }
  return { t: outT, v: outV }
}

/** 화면 폭에서 점 예산을 정한다 — 픽셀당 2점이면 육안으로 원본과 구분되지 않는다. */
export function budgetFor(widthPx: number): number {
  return Math.max(256, Math.round(widthPx * 2))
}
