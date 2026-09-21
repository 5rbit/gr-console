// 작업 명령 슬라이스 로컬 타입 — 공유 `types.ts`에 없는 응답 모양만 여기 둔다.
import type { MoveOpts } from './moveMode'
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
  /** 막지 않는 경고 — 지금은 바닥 Z ≤ 0 셀(쓰기는 그대로 끝난다). */
  warnings?: string[]
  /** `plc=both`일 때 PLC별 결과. */
  results?: PushResult[]
}

/** `POST /api/issue/compose` — 제출 전 미리보기(PLC PascalCase 태스크 + 확정 파라미터 + 경고). */
export interface ComposePreview {
  task: PlcTask
  params: TaskParams
  warnings: string[]
  /** 이 작성이 겨냥한 로봇과 그 상태 PLC — 백엔드 `issue::Composed`. 확인 창이 대상을 짐작하지 않는다. */
  robot?: string
  plc?: string
  /** 스테이션 대상 PICK/DROP/MEASURE 일 때만 — GRM 트래킹 보정(`docs/station-offset.md`). */
  station_offset?: StationOffsetAudit | null
  /** 스택 Z 근거(셀 대상 PICK/DROP/MEASURE, 위치 덮어쓰기 없을 때) — 백엔드 `registry::spec::StackZ`. */
  stack_z?: StackZAudit | null
  /** 셀 단수 Max 검사(품목 단수 Max > 0) — 백엔드 `issue::StackLimit`. */
  stack_limit?: StackLimitAudit | null
}

export interface StackZAudit {
  z: number
  z_source: 'profile' | 'curve' | 'computed'
  /** 잡는 타이어의 단 */
  level: number
  below: number
  /** 잡는 타이어 위에 얹힌 타이어 수(눌림 계산 기준) */
  above: number
  base: number
  grip: number
  /** 실제로 쓴 그립 기준 — `pick_bead` 를 시켰어도 잰 비드가 없으면 `mid` 로 내려간다 */
  grip_ref: 'mid' | 'pick_bead'
  /** 쓴 눌림양(mm/개) */
  compression: number
  /** 그립 기준이 된 상부 비드(타이어 바닥 기준, 눌림 반영) */
  upper_bead: number | null
  pick_bead_offset: number | null
  /** 표에서 쓴 값들(`stack_height[2]=470`) */
  used: string[]
  /** 제한(클램프)이 걸린 곳 */
  warnings: string[]
}

export interface StackLimitAudit {
  stack_max: number
  stock: number | null
  count_after: number
  over_limit: boolean
  ignored: boolean
  /** 있으면 제출이 거부된다(409) */
  blocked: string | null
}

/** 백엔드 `issue::station_offset::StationOffsetAudit` — 미리보기와 원장(`Task.station_offset`)에 같은 모양. */
export interface StationOffsetAudit {
  station_id: number
  /** PLC 슬롯 = id MOD 100 (범위 밖이면 0) */
  slot: number
  rotate_type: number
  /** GRM 측정 OD(`Tracking.Now.OutterDiameter`) — 없으면 0 */
  od: number
  /** 실제로 보정에 쓴 외경(측정값 또는 등록 품목 스펙). 예전 원장 기록에는 없다. */
  od_used?: number
  /** 그 외경의 출처 — 측정 / 등록 품목 스펙 폴백 / 없음 */
  od_source?: 'tracking' | 'item_spec' | 'none'
  now_tx: number
  now_ty: number
  tx_applied: number
  ty_applied: number
  base_xy: [number, number]
  final_xy: [number, number]
  /** `OPCUA`(fast) | `STATION`(slow) | null(스냅샷 없음) */
  source: string | null
  snapshot_at: string | null
  age_ms: number | null
  mode: 'auto' | 'override' | 'off'
  warnings: string[]
  /** 있으면 제출이 거부된다(409). */
  blocked: string | null
  gr2_expected_xy: [number, number] | null
  gr2_margin: number | null
}

/** 파일 가져오기 응답(`ImportResult` + 미리보기용 부가 필드).
 *
 *  파일 가져오기는 줄마다 `added|updated|unchanged|skipped` 하나가 붙는다(셀 일괄 API와 같은 말).
 *  `added`·`unchanged` 가 **없는 응답은 PLC 읽기**(`/api/cells/import`)다 — 거기서는 옛 이름
 *  `imported`·`skipped`(= 동일)만 온다. 둘을 가르는 일은 `lib/task/importPreview.ts`가 한다. */
export interface FileImportResult {
  imported: number
  /** `imported`와 같은 수(줄 결과 이름) — 파일 가져오기에만 있다. */
  added?: number
  updated: number
  removed: number
  /** 파일 가져오기: **적용 못 한 줄**(오류와 1:1). PLC 읽기: 값이 같아 건너뛴 줄. */
  skipped: number
  /** 파일의 값이 저장된 것과 같아 쓰지 않은 줄 — 파일 가져오기에만 있다. */
  unchanged?: number
  errors: { row: number; sheet?: string; message: string }[]
  /** 막지 않는 경고 — 지금은 바닥 Z ≤ 0 셀(가져오기는 그대로 끝난다). */
  warnings?: string[]
  dry_run?: boolean
  counts?: { cells: number; stations: number; items: number; item_profiles?: number }
}

/** PLC 쓰기/읽기 대상 — 백엔드 `?plc=` 값. 설정 이름(`GR1`·`GR2`·`GRM` …) 또는 쓰기 전체 `all`
 *  (옛 `both` = 상태 PLC + GRM 도 받는다). 선택지는 `lib/task/plcTarget.ts`가 `/api/plcs`에서 만든다. */
export type PlcTarget = string

/** 작성 카드의 초안 — `TaskRequest`가 되기 전의 폼 상태. */
export interface Draft {
  type: TaskType
  target: Target | null
  item_code: number | null
  count: number
  params: Partial<TaskParams>
  note: string
  /** 스테이션 보정 — 없거나 `true` 면 켜짐, `false` 면 요청에 `station_offset: 'off'`. */
  station_offset?: boolean
  /** 단수 Max 무시 — `true` 면 요청에 `ignore_stack_max: true`. */
  ignore_stack_max?: boolean
  /** MOVE 방식(없으면 `top`) — 요청 `params.move_mode`. */
  move?: MoveOpts | null
}
