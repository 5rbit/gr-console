// 작업 명령 슬라이스 로컬 타입 — 공유 `types.ts`에 없는 응답 모양만 여기 둔다.
import type { PlcTask, TaskParams, TaskType, Target } from '../types'

/** `POST /api/cells/push` / `/api/stations/push` 응답 — 쓰기 후 재읽기 검증 결과. */
export interface PushResult {
  plc: string
  db: string
  written_bytes: number
  count: number
  verified: boolean
  mismatch_at?: number | null
  writes: number
  /** `plc=both`일 때 PLC별 결과. */
  results?: PushResult[]
}

/** `POST /api/issue/compose` — 제출 전 미리보기(PLC PascalCase 태스크 + 확정 파라미터 + 경고). */
export interface ComposePreview {
  task: PlcTask
  params: TaskParams
  warnings: string[]
}

/** 파일 가져오기 응답(`ImportResult` + 미리보기용 부가 필드). */
export interface FileImportResult {
  imported: number
  updated: number
  removed: number
  skipped: number
  errors: { row: number; sheet?: string; message: string }[]
  dry_run?: boolean
  counts?: { cells: number; stations: number; items: number }
}

/** PLC 쓰기/읽기 대상 — 백엔드 `?plc=` 값. */
export type PlcTarget = 'GR2' | 'GRM' | 'both'

/** 작성 카드의 초안 — `TaskRequest`가 되기 전의 폼 상태. */
export interface Draft {
  type: TaskType
  target: Target | null
  item_code: number | null
  count: number
  params: Partial<TaskParams>
  note: string
}
