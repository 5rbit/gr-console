// gr-console 백엔드 REST + SSE 타입드 클라이언트 — **공유 계약**이다(리드 소유).
//
// 상대 경로(`/api/...`)라 dev는 Vite 프록시, prod는 같은 origin으로 동작한다.
// sh4w-web의 `api.ts`에서 fetch 래퍼(`httpError`·`getJson`·`postJson`·`putJson`·`patchJson`·`del`)를
// 가져오되 **인증 헤더·401 가드는 뺐다**(gr-console은 프록시 뒤 무인증). 파일 업로드(`postForm`)와
// 다운로드(`getBlobUrl`)를 더했다.

import { invalidateShared, shareGet } from './share'
import type {
  Cell,
  CellUpsert,
  ConsoleInfo,
  Defaults,
  DiffRow,
  Gate,
  ImportResult,
  Item,
  ItemUpsert,
  MeasLogEntries,
  MeasLogSnapshot,
  PlcId,
  PlcStatus,
  Scenario,
  ScenarioRun,
  ScenarioUpsert,
  Station,
  StationUpsert,
  StatusEvent,
  Task,
  TaskPage,
  TaskQuery,
  TaskRequest,
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
  status: '/api/status/stream',
  tasks: '/api/tasks/stream',
  runs: '/api/scenarios/runs/stream',
} as const

// ── API ───────────────────────────────────────────────────────────────────────

export const api = {
  // 콘솔·PLC
  consoleInfo: () => getJson<ConsoleInfo>('/api/console/info'),
  plcs: () => getJson<PlcStatus[]>('/api/plcs'),
  plcCheck: (id: PlcId) => postJson<PlcStatus>(`/api/plcs/${id}/check`),
  plcReconnect: (id: PlcId) => postJson<PlcStatus>(`/api/plcs/${id}/reconnect`),

  // 상태
  statusNow: () => getJson<StatusEvent>('/api/status'),
  statusStream: (): EventSource => new EventSource(STREAM_URL.status),

  // 측정 로그
  measlogSnapshot: () => getJson<MeasLogSnapshot>('/api/measlog/snapshot'),
  measlogEntries: (since?: number, kind?: number, code?: number, limit?: number) =>
    getJson<MeasLogEntries>(`/api/measlog/entries${qs({ since, kind, code, limit })}`),
  measlogReload: () => postJson<MeasLogSnapshot>('/api/measlog/reload'),
  measlogCsvUrl: (kind?: number, code?: number): string =>
    `/api/measlog/export.csv${qs({ kind, code })}`,

  // 품목
  items: () => getJson<Item[]>('/api/items'),
  itemCreate: (body: ItemUpsert) => postJson<Item>('/api/items', body),
  itemUpdate: (code: number, body: ItemUpsert) => putJson<Item>(`/api/items/${code}`, body),
  itemDelete: (code: number) => del(`/api/items/${code}`),

  // 셀
  cells: () => getJson<Cell[]>('/api/cells'),
  cellCreate: (body: CellUpsert) => postJson<Cell>('/api/cells', body),
  cellUpdate: (id: number, body: CellUpsert) => putJson<Cell>(`/api/cells/${id}`, body),
  cellDelete: (id: number) => del(`/api/cells/${id}`),
  cellsImport: (plc: PlcId) => postJson<ImportResult>(`/api/cells/import${qs({ plc })}`),
  cellsPush: (plc: PlcId) => postJson<ImportResult>(`/api/cells/push${qs({ plc })}`),
  cellsDiff: (plc: PlcId) => getJson<DiffRow<Cell>[]>(`/api/cells/diff${qs({ plc })}`),
  cellsExportUrl: (): string => '/api/cells/export.xlsx',
  cellsImportFile: (file: File, dryRun = false) =>
    postForm<ImportResult>(`/api/cells/import-file${qs({ dry_run: dryRun })}`, fileForm(file)),

  // 스테이션
  stations: () => getJson<Station[]>('/api/stations'),
  stationCreate: (body: StationUpsert) => postJson<Station>('/api/stations', body),
  stationUpdate: (id: number, body: StationUpsert) =>
    putJson<Station>(`/api/stations/${id}`, body),
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
        state: Array.isArray(q.state) ? q.state.join(',') : q.state,
        type: q.type,
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
  taskGate: () => getJson<Gate>('/api/tasks/gate'),
  tasksStream: (): EventSource => new EventSource(STREAM_URL.tasks),

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
