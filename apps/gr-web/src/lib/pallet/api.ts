// 팔렛 패턴 슬라이스 API — 공유 fetch 래퍼 위에 `/api/pallet/*` 를 얹는다.
import { del, getBlobUrl, getJson, httpError, postJson, putJson } from '../api'
import { invalidateShared } from '../share'
import type { DraftSlot } from './editorModel'
import {
  planQuery,
  type DragKind,
  type EditFlowView,
  type FlowDiff,
  type ImportReport,
  type PalletPlan,
  type PalletProfile,
  type PalletProfileUpsert,
  type PalletSpec,
  type PlanParams,
  type ScreenAxes,
  type SpecPattern,
} from './model'

const flowPath = (id: string) => `/api/pallet/flows/${encodeURIComponent(id)}`

export interface FlowCreate {
  id: string
  name?: string
  drag_kind?: DragKind
  note?: string
  copy_from?: string
}

export interface FlowPatchBody {
  id?: string
  name?: string
  drag_kind?: DragKind
  note?: string
  screen_axes?: ScreenAxes
  reference?: boolean
}

export interface PatternPut {
  pattern: number
  od_min: number
  od_max: number
  note: string
  slots: DraftSlot[]
}

export interface FlowResult {
  flow: EditFlowView
  warnings?: string[]
}

export const palletApi = {
  spec: () => getJson<PalletSpec>('/api/pallet/spec'),
  seed: () => getJson<PalletSpec>('/api/pallet/spec?seed=1'),
  profiles: () => getJson<PalletProfile[]>('/api/pallet/profiles'),
  saveProfile: (p: PalletProfileUpsert) => putJson<PalletProfile>('/api/pallet/profiles', p),
  deleteProfile: (station: number) => del(`/api/pallet/profiles/${station}`),
  plan: (p: PlanParams) => getJson<PalletPlan>(`/api/pallet/plan${planQuery(p)}`),

  // ── 패턴 편집 ──
  flows: () => getJson<EditFlowView[]>('/api/pallet/flows'),
  createFlow: (b: FlowCreate) => postJson<FlowResult>('/api/pallet/flows', b),
  updateFlow: (id: string, b: FlowPatchBody) => putJson<FlowResult>(flowPath(id), b),
  deleteFlow: (id: string) => del(flowPath(id)),
  savePattern: (id: string, originalPattern: number, b: PatternPut) =>
    putJson<FlowResult & { pattern: SpecPattern }>(`${flowPath(id)}/patterns/${originalPattern}`, b),
  deletePattern: async (id: string, pattern: number) => {
    invalidateShared()
    const path = `${flowPath(id)}/patterns/${pattern}`
    const r = await fetch(path, { method: 'DELETE' })
    if (!r.ok) throw await httpError(r, 'DELETE', path)
    return (await r.json()) as FlowResult
  },
  resetFlow: (id: string, pattern?: number) =>
    postJson<FlowResult & { diff: FlowDiff }>(`${flowPath(id)}/reset${pattern === undefined ? '' : `?pattern=${pattern}`}`),
  diff: (id: string) => getJson<FlowDiff>(`${flowPath(id)}/diff`),
  exportUrl: () => getBlobUrl('/api/pallet/export.json'),
  exportJson: () => getJson<PalletSpec>('/api/pallet/export.json'),
  importJson: (doc: unknown, dryRun: boolean) =>
    postJson<ImportReport>(`/api/pallet/import?dry_run=${dryRun ? 1 : 0}`, doc),
}
