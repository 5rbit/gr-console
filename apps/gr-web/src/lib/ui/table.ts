// 표 컬럼 정의 — `DataTable`의 계약.
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
}
