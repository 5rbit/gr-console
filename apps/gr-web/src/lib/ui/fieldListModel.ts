// 값 목록 항목 — `FieldList.tsx`의 계약. Svelte 컴포넌트의 `<script>`에서는 타입을 export할 수
// 없어 컴포넌트 밖에 둔다(`table.ts`와 같은 관용구).

import type { Status } from './status'

export interface FieldItem {
  /** 라벨(왼쪽 열). */
  label: string
  /** 값. `null`·`undefined`·`''`은 "로봇이 안 냈다"로 그린다(글로 적지 않는다). */
  value?: unknown
  /** 상태색을 입힌다 — 판정이 붙은 값만. 색은 `tokens/status.css`의 여섯. */
  status?: Status
  /** 라벨 설명 — 화면에 풀어 쓰지 않고 여기로 내린다(UX 규칙 ③: 설명은 툴팁, 화면은 값). */
  tooltip?: string
  /** 아직 안 옴 — 스켈레톤 바. */
  loading?: boolean
  /** 값이 없는 **이유**. 없으면 기본 문구. */
  missing?: string
  /** 값 글꼴 — 기본은 **sans + tabular-nums**. `true`면 mono.
   *
   *  UX 규칙 ⑩: 한글 라벨과 숫자가 한 줄에 섞이는 화면에서 mono는 자릿수만 맞추고 글자 리듬을
   *  깨뜨린다. mono는 **식별자**에만 남긴다 — 비트 이름(`operation_enabled`),
   *  OD 인덱스(`0x607A`), 해시, 로그 타깃(`mxcore::fusion`). */
  mono?: boolean
  /** 값이 남은 열을 다 쓴다(경로·문장). */
  wide?: boolean
}
