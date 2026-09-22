// 작업 명령 슬라이스의 API — 공유 `api.ts`의 fetch 래퍼 위에 이 화면만 쓰는 경로를 얹는다.
//
// 공유 `api`에도 `cellsPush`/`cellsDiff` 등이 있지만 `PlcId`(gr2_s7) 하나만 받고 `force`가 없어
// 여기서는 백엔드 이름(GR2/GRM/both)과 강제 쓰기를 그대로 싣는 버전을 둔다.
import { getJson, postForm, postJson } from '../api'
import type { Cell, DiffRow, Station, TaskRequest } from '../types'
import type {
  ComposePreview,
  FileImportResult,
  PlcTarget,
  PushResult,
  StationLive,
  StationOffsetRow,
} from './types'

function q(params: Record<string, string | number | boolean | undefined>): string {
  const s = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) {
    if (v === undefined || v === '') continue
    s.set(k, typeof v === 'boolean' ? (v ? '1' : '0') : String(v))
  }
  const t = s.toString()
  return t ? `?${t}` : ''
}

function fileForm(file: File): FormData {
  const fd = new FormData()
  fd.append('file', file, file.name)
  return fd
}

export const taskApi = {
  /** 스테이션마다 지금 GRM 트래킹으로 계산한 보정. */
  stationOffsets: () => getJson<StationOffsetRow[]>('/api/stations/offsets'),
  stationLive: () => getJson<StationLive[]>('/api/stations/live'),
  // 셀
  cellsImport: (plc: PlcTarget) => postJson<FileImportResult>(`/api/cells/import${q({ plc })}`),
  cellsPush: (plc: PlcTarget, force = false) =>
    postJson<PushResult>(`/api/cells/push${q({ plc, force })}`),
  cellsDiff: (plc: PlcTarget) => getJson<DiffRow<Cell>[]>(`/api/cells/diff${q({ plc })}`),
  cellsExportUrl: '/api/cells/export.xlsx',
  cellsImportFile: (file: File, dryRun: boolean) =>
    postForm<FileImportResult>(`/api/cells/import-file${q({ dry_run: dryRun })}`, fileForm(file)),

  // 스테이션
  stationsImport: (plc: PlcTarget) =>
    postJson<FileImportResult>(`/api/stations/import${q({ plc })}`),
  stationsPush: (plc: PlcTarget, force = false) =>
    postJson<PushResult>(`/api/stations/push${q({ plc, force })}`),
  stationsDiff: (plc: PlcTarget) => getJson<DiffRow<Station>[]>(`/api/stations/diff${q({ plc })}`),
  stationsExportUrl: '/api/stations/export.xlsx',
  stationsImportFile: (file: File, dryRun: boolean) =>
    postForm<FileImportResult>(
      `/api/stations/import-file${q({ dry_run: dryRun })}`,
      fileForm(file),
    ),

  // 품목(화물 규격) — Items 시트만. 레지스트리 전체 파일을 넣어도 Items 시트를 이름으로 고른다.
  itemsExportUrl: '/api/items/export.xlsx',
  /** 빈 양식(`Items` + `ItemBeadProfile` 머리글만) — 품목이 하나도 없을 때 시작하는 자리. */
  itemsTemplateUrl: '/api/items/template.xlsx',
  itemsImportFile: (file: File, dryRun: boolean) =>
    postForm<FileImportResult>(`/api/items/import-file${q({ dry_run: dryRun })}`, fileForm(file)),

  // 레지스트리 전체(품목 시트 포함)
  registryExportUrl: '/api/registry/export.xlsx',
  /** 재고만(`Stock` 시트, 셀·스테이션). 통합 파일에도 같은 시트가 있다. */
  stockExportUrl: '/api/stock/export.xlsx',
  stockImportFile: (file: File, dryRun: boolean, mode: 'merge' | 'replace') =>
    postForm<FileImportResult>(
      `/api/stock/import-file${q({ dry_run: dryRun, mode })}`,
      fileForm(file),
    ),
  registryImportFile: (file: File, dryRun: boolean) =>
    postForm<FileImportResult>(
      `/api/registry/import-file${q({ dry_run: dryRun })}`,
      fileForm(file),
    ),

  // 작성 미리보기
  compose: (req: TaskRequest, stock?: number | null) =>
    postJson<ComposePreview>(
      `/api/issue/compose${stock === null || stock === undefined ? '' : `?stock=${stock}`}`,
      req,
    ),
}
