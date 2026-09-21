// gr-console 백엔드 REST + SSE 타입드 클라이언트 — **공유 계약**이다(리드 소유).
//
// 상대 경로(`/api/...`)라 dev는 Vite 프록시, prod는 같은 origin으로 동작한다.
// sh4w-web의 `api.ts`에서 fetch 래퍼(`httpError`·`getJson`·`postJson`·`putJson`·`patchJson`·`del`)를
// 가져오되 **인증 헤더·401 가드는 뺐다**(gr-console은 프록시 뒤 무인증). 파일 업로드(`postForm`)와
// 다운로드(`getBlobUrl`)를 더했다.

import { invalidateShared, shareGet } from './share'
import type {
  BeadSample,
  Cell,
  CellBulkOptions,
  CellBulkResult,
  CellUpsert,
  CompressionSuggestion,
  ConsoleInfo,
  Defaults,
  DiffRow,
  Gate,
  ImportResult,
  Item,
  ItemLevels,
  ItemSpec,
  ItemUpsert,
  MeasLogEntries,
  MeasLogSnapshot,
  PlcId,
  PlcStatus,
  Robot,
  Scenario,
  ScenarioRun,
  ScenarioUpsert,
  Station,
  StationUpsert,
  StatusEvent,
  HandView,
  StockEntry,
  StockProjected,
  TransferOrder,
  TransferOrderDetail,
  StockZ,
  Task,
  TaskPage,
  TaskQuery,
  TaskRequest,
  TaskType,
} from './types'
import type {
  LaserSnapshot,
  ParaSnapshot,
  RecordMeta,
  RecordOverview,
  RecordSession,
  RecordStart,
} from './types'

// ── fetch 래퍼 ────────────────────────────────────────────────────────────────

/** 오류 응답을 서버 사유를 실은 Error로 만든다 — 본문이 비면 상태코드만. */
export async function httpError(r: Response, method: string, path: string): Promise<Error> {
  let detail = ''
  try {
    detail = (await r.text()).trim()
  } catch {
    /* 본문 없음 */
  }
  // JSON 오류 봉투면 흔한 메시지 필드를 꺼낸다(그 외엔 원문).
  if (detail.startsWith('{')) {
    try {
      const o = JSON.parse(detail) as { message?: string; error?: string; detail?: string }
      detail = o.message ?? o.error ?? o.detail ?? detail
    } catch {
      /* 원문 유지 */
    }
  }
  const head = `${method} ${path} → ${r.status}`
  return new Error(detail ? `${detail} (${head})` : head)
}

/** GET → JSON. 동일 URL 동시 조회는 [`shareGet`]이 하나로 합친다. */
export async function getJson<T>(path: string): Promise<T> {
  return shareGet(path, async () => {
    const r = await fetch(path)
    if (!r.ok) throw await httpError(r, 'GET', path)
    return r.json() as Promise<T>
  })
}

export async function postJson<T>(path: string, body?: unknown): Promise<T> {
  invalidateShared() // 명령 뒤의 조회는 **명령 이후 값**이어야 한다 — 합치기 창을 버린다.
  const r = await fetch(path, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (!r.ok) throw await httpError(r, 'POST', path)
  return r.json() as Promise<T>
}

/** JSON 본문 PUT → JSON 응답(전체 교체 의미론). */
export async function putJson<T>(path: string, body: unknown): Promise<T> {
  invalidateShared()
  const r = await fetch(path, {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  })
  if (!r.ok) throw await httpError(r, 'PUT', path)
  return r.json() as Promise<T>
}

/** JSON 본문 PATCH → JSON 응답(부분 갱신). */
export async function patchJson<T>(path: string, body: unknown): Promise<T> {
  invalidateShared()
  const r = await fetch(path, {
    method: 'PATCH',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  })
  if (!r.ok) throw await httpError(r, 'PATCH', path)
  return r.json() as Promise<T>
}

export async function del(path: string): Promise<void> {
  invalidateShared()
  const r = await fetch(path, { method: 'DELETE' })
  if (!r.ok) throw await httpError(r, 'DELETE', path)
}

/** multipart 업로드(파일 import) → JSON 응답. content-type은 브라우저가 boundary와 함께 붙인다. */
export async function postForm<T>(path: string, form: FormData): Promise<T> {
  invalidateShared()
  const r = await fetch(path, { method: 'POST', body: form })
  if (!r.ok) throw await httpError(r, 'POST', path)
  return r.json() as Promise<T>
}

/** 파일 다운로드 — Blob을 받아 object URL로 돌려준다. 쓰고 나면 `URL.revokeObjectURL`로 놓는다. */
export async function getBlobUrl(path: string): Promise<string> {
  const r = await fetch(path)
  if (!r.ok) throw await httpError(r, 'GET', path)
  return URL.createObjectURL(await r.blob())
}

/** 쿼리스트링 — `undefined`/`null`은 뺀다. 비면 빈 문자열. */
function qs(params: Record<string, string | number | boolean | undefined | null>): string {
  const q = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) {
    if (v === undefined || v === null || v === '') continue
    q.set(k, String(v))
  }
  const s = q.toString()
  return s ? `?${s}` : ''
}

/** 파일 하나를 `file` 필드로 싣는 FormData. */
function fileForm(file: File): FormData {
  const fd = new FormData()
  fd.append('file', file, file.name)
  return fd
}

/** SSE 스트림 URL — `lib/feeds.ts`의 피드가 이 주소로 붙는다. */
export const STREAM_URL = {
  /** 기본 로봇 상태. 로봇별은 [`statusStreamUrl`]. */
  status: '/api/status/stream',
  tasks: '/api/tasks/stream',
  runs: '/api/scenarios/runs/stream',
  stock: '/api/stock/stream',
} as const

/** 로봇 한 대의 상태 스트림 — `robot` 이 없으면 기본 로봇(백엔드 `robots[0]`). */
export function statusStreamUrl(robot?: number | null): string {
  return `${STREAM_URL.status}${qs({ robot })}`
}

// ── API ───────────────────────────────────────────────────────────────────────

export const api = {
  // 콘솔·PLC
  consoleInfo: () => getJson<ConsoleInfo>('/api/console/info'),
  plcs: () => getJson<PlcStatus[]>('/api/plcs'),
  plcCheck: (id: PlcId) => postJson<PlcStatus>(`/api/plcs/${id}/check`),
  plcReconnect: (id: PlcId) => postJson<PlcStatus>(`/api/plcs/${id}/reconnect`),

  // 상태 — 모두 로봇별(`robot` 없으면 기본 로봇)
  statusNow: (robot?: number | null) => getJson<StatusEvent>(`/api/status${qs({ robot })}`),
  statusStream: (robot?: number | null): EventSource => new EventSource(statusStreamUrl(robot)),

  // 측정 로그 — 로봇마다 따로 쌓인다
  measlogSnapshot: (robot?: number | null) =>
    getJson<MeasLogSnapshot>(`/api/measlog/snapshot${qs({ robot })}`),
  measlogEntries: (
    since?: number,
    kind?: number,
    code?: number,
    limit?: number,
    robot?: number | null,
  ) => getJson<MeasLogEntries>(`/api/measlog/entries${qs({ since, kind, code, limit, robot })}`),
  measlogReload: (robot?: number | null) =>
    postJson<MeasLogSnapshot>(`/api/measlog/reload${qs({ robot })}`),
  measlogCsvUrl: (kind?: number, code?: number, robot?: number | null): string =>
    `/api/measlog/export.csv${qs({ kind, code, robot })}`,
  laser: (robot?: number | null) => getJson<LaserSnapshot>(`/api/laser${qs({ robot })}`),
  laserZCal: (enable: boolean, robot?: number | null) =>
    postJson<{ ok: boolean; enable: boolean }>(`/api/laser/zcal${qs({ robot })}`, { enable }),
  laserReset: (robot?: number | null) =>
    postJson<{ ok: boolean }>(`/api/laser/reset${qs({ robot })}`),
  /** 로봇 PLC 의 PARA DB(계약 주석 포함, 읽기 전용) */
  para: (robot?: number | null) => getJson<ParaSnapshot>(`/api/para${qs({ robot })}`),

  // 측정 기록 (현장 시험)
  record: () => getJson<RecordOverview>('/api/record'),
  recordStart: (body: RecordStart) => postJson<RecordMeta>('/api/record/start', body),
  recordStop: () => postJson<RecordMeta>('/api/record/stop'),
  recordMark: (text: string) => postJson<{ ok: boolean }>('/api/record/mark', { text }),
  recordSession: (id: string, max?: number) =>
    getJson<RecordSession>(`/api/record/${encodeURIComponent(id)}${qs({ max })}`),
  recordDelete: (id: string) => del(`/api/record/${encodeURIComponent(id)}`),
  recordCsvUrl: (id: string): string => `/api/record/${encodeURIComponent(id)}/export.csv`,

  // 품목
  items: () => getJson<Item[]>('/api/items'),
  itemCreate: (body: ItemUpsert) => postJson<Item>('/api/items', body),
  itemUpdate: (code: number, body: ItemUpsert) => putJson<Item>(`/api/items/${code}`, body),
  itemDelete: (code: number) => del(`/api/items/${code}`),
  /**
   * 하중(Above) 기준 비드 곡선을 `preview` 개 스택의 단별 보기로 — 설정·유효(측정/보간/계산)·절대 비드·
   * 공칭·측정·편차·집는 높이 + 최근 SKU 표본 이력.
   */
  itemLevels: (code: number, robot?: number | null, preview?: number | null) =>
    getJson<ItemLevels>(`/api/items/${code}/levels${qs({ robot, preview })}`),
  /** 최신 SKU 측정을 규격에 바로 넣는다(서버가 검증·저장) — `fields` = beads · compression */
  itemLevelsApplyMeasured: (code: number, robot?: number | null, fields?: string) =>
    postJson<{
      code: number
      applied: unknown[]
      spec: ItemSpec
      suggestion: CompressionSuggestion
    }>(`/api/items/${code}/levels/apply-measured${qs({ robot, fields })}`, {}),
  /** SKU 측정 표본 이력(최신 순, 반영 여부·거부 사유 포함) */
  itemBeadSamples: (code: number, robot?: number | null, limit?: number, all?: boolean) =>
    getJson<{
      code: number
      plc: string | null
      limit: number
      auto_apply_measured: boolean
      samples: BeadSample[]
    }>(`/api/items/${code}/bead-samples${qs({ robot, limit, all })}`),
  /** 옛 표본을 손으로 규격에 넣는다(자동 반영 스위치가 꺼져 있어도) */
  itemBeadSampleApply: (code: number, seq: number, robot?: number | null) =>
    postJson<{
      code: number
      plc: string
      seq: number
      applied: boolean
      reason: string
      spec: ItemSpec
      samples: BeadSample[]
    }>(`/api/items/${code}/bead-samples/${seq}/apply${qs({ robot, force: true })}`, {}),
  /** 체크한 품목들에 단수 Max / 팔레트 Max 를 한 번에(전부 검증 뒤 적용) */
  itemsBulkSpec: (body: { codes: number[]; stack_max?: number; pallet_max?: number }) =>
    postJson<{ updated: number }>('/api/items/bulk-spec', body),

  // 셀
  cells: () => getJson<Cell[]>('/api/cells'),
  cellCreate: (body: CellUpsert) => postJson<Cell>('/api/cells', body),
  cellUpdate: (id: number, body: CellUpsert) => putJson<Cell>(`/api/cells/${id}`, body),
  cellDelete: (id: number) => del(`/api/cells/${id}`),
  /**
   * 레이아웃 편집기·그리드 — 여러 셀 한 번에(로컬 사본). 기본 모드 `append` 는 **아무것도 지우지 않고**
   * 충돌(id 중복·겹침)만 건너뛴다. 줄마다 결과와 사유가 `rows` 로 돌아온다.
   */
  cellsBulk: (rows: CellUpsert[], opts: CellBulkOptions = {}) =>
    postJson<CellBulkResult>(
      `/api/cells/bulk${qs({
        mode: opts.mode,
        replace_section: opts.section ?? undefined,
        auto_id: opts.autoId ? 1 : undefined,
        diameter: opts.diameter,
      })}`,
      rows,
    ),
  cellsImport: (plc: PlcId) => postJson<ImportResult>(`/api/cells/import${qs({ plc })}`),
  cellsPush: (plc: PlcId) => postJson<ImportResult>(`/api/cells/push${qs({ plc })}`),
  cellsDiff: (plc: PlcId) => getJson<DiffRow<Cell>[]>(`/api/cells/diff${qs({ plc })}`),
  cellsExportUrl: (): string => '/api/cells/export.xlsx',
  cellsImportFile: (file: File, dryRun = false) =>
    postForm<ImportResult>(`/api/cells/import-file${qs({ dry_run: dryRun })}`, fileForm(file)),

  // 스테이션
  stations: () => getJson<Station[]>('/api/stations'),
  stationCreate: (body: StationUpsert) => postJson<Station>('/api/stations', body),
  stationUpdate: (id: number, body: StationUpsert) => putJson<Station>(`/api/stations/${id}`, body),
  stationDelete: (id: number) => del(`/api/stations/${id}`),
  stationsImport: (plc: PlcId) => postJson<ImportResult>(`/api/stations/import${qs({ plc })}`),
  stationsPush: (plc: PlcId) => postJson<ImportResult>(`/api/stations/push${qs({ plc })}`),
  stationsDiff: (plc: PlcId) => getJson<DiffRow<Station>[]>(`/api/stations/diff${qs({ plc })}`),
  stationsExportUrl: (): string => '/api/stations/export.xlsx',
  stationsImportFile: (file: File, dryRun = false) =>
    postForm<ImportResult>(`/api/stations/import-file${qs({ dry_run: dryRun })}`, fileForm(file)),

  // 레지스트리 전체(품목+셀+스테이션 한 파일)
  registryExportUrl: (): string => '/api/registry/export.xlsx',
  registryImportFile: (file: File, dryRun = false) =>
    postForm<ImportResult>(`/api/registry/import-file${qs({ dry_run: dryRun })}`, fileForm(file)),

  // 기본 파라미터
  defaults: () => getJson<Defaults>('/api/defaults'),
  defaultsSave: (body: Defaults) => putJson<Defaults>('/api/defaults', body),

  // Task
  tasks: (q: TaskQuery = {}) =>
    getJson<TaskPage>(
      `/api/tasks${qs({
        robot: q.robot,
        state: Array.isArray(q.state) ? q.state.join(',') : q.state,
        type: q.type,
        origin: q.origin,
        since: q.since,
        q: q.q,
        limit: q.limit,
        offset: q.offset,
      })}`,
    ),
  task: (id: string) => getJson<Task>(`/api/tasks/${id}`),
  /** 생성(+`submit`이면 즉시 PLC로 제출). */
  taskCreate: (req: TaskRequest, submit = false) =>
    postJson<Task>(`/api/tasks${qs({ submit })}`, req),
  taskSubmit: (id: string) => postJson<Task>(`/api/tasks/${id}/submit`),
  taskCancel: (id: string) => postJson<Task>(`/api/tasks/${id}/cancel`),
  taskComplete: (id: string) => postJson<Task>(`/api/tasks/${id}/complete`),
  taskResubmit: (id: string) => postJson<Task>(`/api/tasks/${id}/resubmit`),
  taskGate: (robot?: number | null) =>
    getJson<Gate>(`/api/tasks/gate${qs({ robot: robot ?? undefined })}`),
  /** GRM 뒤 로봇 목록 + 명령 경로·게이트 상태 */
  robots: () => getJson<Robot[]>('/api/robots'),
  tasksStream: (): EventSource => new EventSource(STREAM_URL.tasks),

  // 재고(셀별 화물) — 콘솔 소유, 완료된 PICK/DROP 으로 자동 갱신
  stock: () => getJson<StockEntry[]>('/api/stock'),
  stockSet: (cell: number, body: { item_code: number; count: number; note?: string }) =>
    putJson<StockEntry>(`/api/stock/${cell}`, body),
  stockDelete: (cell: number) => del(`/api/stock/${cell}`),
  stockClear: () => postJson<{ removed: number }>('/api/stock/clear'),
  /** 로봇별 Hand(표 + 예상) */
  stockHands: () => getJson<HandView[]>('/api/stock/hands'),
  /** 진행 중 PICK/DROP 을 반영한 셀 재고·Hand */
  stockProjected: () => getJson<StockProjected>('/api/stock/projected'),
  /** 이송 지시 목록(최신 먼저) */
  transferOrders: (
    q: { robot?: number | null; state?: string; cell?: number; limit?: number } = {},
  ) =>
    getJson<{ items: TransferOrder[]; total: number }>(
      `/api/transfer-orders${qs({ robot: q.robot ?? undefined, state: q.state, cell: q.cell, limit: q.limit })}`,
    ),
  transferOrder: (id: string) =>
    getJson<TransferOrderDetail>(`/api/transfer-orders/${encodeURIComponent(id)}`),
  stockZ: (type: TaskType, cell: number, item?: number | null, count = 1) =>
    getJson<StockZ>(`/api/stock/z${qs({ type, cell, item: item ?? undefined, count })}`),

  // 시나리오
  scenarios: () => getJson<Scenario[]>('/api/scenarios'),
  scenario: (id: string) => getJson<Scenario>(`/api/scenarios/${id}`),
  /** `id`가 있으면 교체(PUT), 없으면 생성(POST). */
  scenarioSave: (body: ScenarioUpsert) =>
    body.id
      ? putJson<Scenario>(`/api/scenarios/${body.id}`, body)
      : postJson<Scenario>('/api/scenarios', body),
  scenarioDelete: (id: string) => del(`/api/scenarios/${id}`),
  scenarioImportFile: (file: File) => postForm<Scenario>('/api/scenarios/import', fileForm(file)),
  scenarioExportUrl: (id: string, format: 'json' | 'xlsx' = 'json'): string =>
    `/api/scenarios/${id}/export${qs({ format })}`,
  scenarioRun: (id: string, opts: { repeat?: number } = {}) =>
    postJson<ScenarioRun>(`/api/scenarios/${id}/run`, opts),
  scenarioPause: () => postJson<ScenarioRun>('/api/scenarios/run/pause'),
  scenarioResume: () => postJson<ScenarioRun>('/api/scenarios/run/resume'),
  scenarioStop: () => postJson<ScenarioRun>('/api/scenarios/run/stop'),
  /** 지금 도는 실행(없으면 `null`). */
  scenarioRunNow: () => getJson<ScenarioRun | null>('/api/scenarios/run'),
  runsStream: (): EventSource => new EventSource(STREAM_URL.runs),
}
