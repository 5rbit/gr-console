// 표 컬럼 정의 — `DataTable`의 계약 + **좁은 폭에서 어느 열을 접을지**의 순수 판정.
//
// 열 접기가 필요해진 이유: 화면이 도킹 존에 들어갈 수 있게 되면서 열 열두 개짜리 표가 300px 안에
// 서는 일이 생겼다. 그대로 두면 가로 스크롤이고, 가로 스크롤이 걸린 표는 **오른쪽 끝 열이 없는 것과
// 같다**(스크롤해야 보이는 값은 훑어볼 수 없다).
//
// 규약은 DataTables의 `responsivePriority`에서 가져왔다: **숫자가 작을수록 오래 남는다.** 기본값은
// 1(항상 보인다)이라, 우선순위를 안 적은 기존 표는 동작이 바뀌지 않는다.
//
// 접은 열을 **버리지는 않는다** — 행을 펼치면 라벨+값 짝으로 나온다(`priority+` 패턴). 좁아졌다고
// 값이 사라지면 그 표는 좁은 자리에서 쓸 수 없는 표가 된다.
import type { ReactNode } from 'react'

/** 컬럼 1개. `key`는 정렬·추적용 식별자이고, 값 접근은 `get`이 소유한다. */
export type Column<R> = {
  /** 식별자(정렬 키이자 리스트 키). */
  key: string
  /** 헤더에 보일 이름 — **사람의 말로**. DB 컬럼명을 그대로 쓰지 않는다. */
  label: string
  /** 정렬·기본 표시에 쓸 값. 없으면 그 열은 정렬 불가. */
  get?: (row: R) => string | number | null | undefined
  /** 셀 렌더(선택) — 배지·진행률처럼 값만으로 안 되는 것. */
  cell?: (row: R) => ReactNode
  /** 헤더 클릭 정렬 허용(기본: `get`이 있으면 true). */
  sortable?: boolean
  /** 숫자 열 — 자릿수를 맞춰 읽기 쉽게(우측 정렬 + tabular). */
  numeric?: boolean
  /** 추가 클래스(폭 등). */
  class?: string
  /**
   * 좁은 폭에서 남을 순서 — **작을수록 오래 남는다**(1 = 항상 보인다, 기본값).
   * 행을 알아보는 데 필요한 열(식별자·상태·대상)이 1, 맥락이 2, 세부가 3이다.
   */
  priority?: ColumnPriority
}

/** 열 우선순위 — 1 항상 · 2 보통 폭부터 · 3 넓을 때만. */
export type ColumnPriority = 1 | 2 | 3

/**
 * 우선순위별로 열이 살아나는 **컨테이너 폭**(px).
 *
 * 창 폭이 아니라 **표가 든 컨테이너 폭**이 기준이다(존 하나가 300px일 수 있다). 값은 실제 표에서
 * 골랐다: 필수 열 셋(식별자·상태·대상)이 280px 안에 서고, 448px이면 맥락 열 둘이 더 들어가고,
 * 672px이면 열 열두 개짜리 Task 표가 가로 스크롤 없이 다 선다.
 */
export const COLUMN_WIDTH_FOR: Record<Exclude<ColumnPriority, 1>, number> = {
  2: 448,
  3: 672,
}

/** 이 폭에서 보일 최대 우선순위. 폭을 아직 모르면(`null`) 전부 보인다 — 첫 페인트에서 열이 깜빡이지 않게. */
export function columnLevel(width: number | null): ColumnPriority {
  if (width === null) return 3
  if (width >= COLUMN_WIDTH_FOR[3]) return 3
  if (width >= COLUMN_WIDTH_FOR[2]) return 2
  return 1
}

/** 열을 보이는 것과 접힌 것으로 가른다. 순서는 원본을 지킨다(열이 자리를 바꾸면 눈이 다시 찾는다). */
export function splitColumns<R>(
  columns: readonly Column<R>[],
  level: ColumnPriority,
): { visible: Column<R>[]; hidden: Column<R>[] } {
  const visible: Column<R>[] = []
  const hidden: Column<R>[] = []
  for (const c of columns) {
    if ((c.priority ?? 1) <= level) visible.push(c)
    else hidden.push(c)
  }
  return { visible, hidden }
}
