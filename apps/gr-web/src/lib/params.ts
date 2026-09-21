// 스케줄링·생성 파라미터 — 백엔드 `params.rs` 한 곳. 단위·범위·기본값·출처·도움말은 서버 `spec` 에서 온다.
import { getJson, postJson, putJson } from './api'

export interface ParamSpec {
  key: string
  group: string
  unit: string
  min: number | null
  max: number | null
  locked: boolean
  source: string | null
  help: string
}

export type ParamValues = Record<string, unknown>

export interface PlcAnticol {
  robot: number
  name: string
  machine_id: number | null
  x_length_front: number | null
  x_length_rear: number | null
  margin_default: number | null
  margin_pos: number | null
  margin_avoid: number | null
  separation: number | null
}

export interface ParamsState {
  version: number
  params: ParamValues
  defaults: ParamValues
  spec: ParamSpec[]
  plc: PlcAnticol[]
  config: { echo_timeout_ms: number }
  robots: { id: number; name: string }[]
}

export interface ParamsHistory {
  version: number
  saved_at: string
  saved_by: string
  changes: { key: string; old: unknown; new: unknown }[]
}

export const paramsApi = {
  get: () => getJson<ParamsState>('/api/params'),
  save: (params: ParamValues, by = 'console') =>
    putJson<{ version: number; changes: { key: string }[] }>('/api/params', { params, by }),
  reset: (keys: string[] = [], by = 'console') =>
    postJson<{ version: number; changes: number }>('/api/params/reset', { keys, by }),
  history: (limit = 50) => getJson<ParamsHistory[]>(`/api/params/history?limit=${limit}`),
}

/** 값 → 편집 문자열(맵은 `1:200, 2:150`, 없음은 빈 칸). */
export function toText(v: unknown): string {
  if (v === null || v === undefined) return ''
  if (typeof v === 'object')
    return Object.entries(v as Record<string, unknown>)
      .map(([k, x]) => `${k}:${String(x)}`)
      .join(', ')
  return String(v)
}

/**
 * 편집 문자열 → 값(기본값의 모양을 따른다). 범위는 스펙으로 본다 — 서버도 같은 검사를 한다.
 * 잘못되면 사유.
 */
export function fromText(
  text: string,
  spec: ParamSpec,
  def: unknown,
): { ok: unknown } | { error: string } {
  const t = text.trim()
  const range = (n: number) =>
    (spec.min !== null && n < spec.min) || (spec.max !== null && n > spec.max)
      ? `${spec.key}: ${spec.min ?? ''}..${spec.max ?? ''} ${spec.unit}`
      : null
  if (typeof def === 'boolean') return { ok: t === 'true' }
  if (def !== null && typeof def === 'object') {
    const out: Record<string, number> = {}
    for (const part of t
      .split(',')
      .map((p) => p.trim())
      .filter(Boolean)) {
      const [k, v] = part.split(':').map((x) => x.trim())
      const n = Number(v)
      if (!/^\d+$/.test(k ?? '') || v === undefined || !Number.isFinite(n))
        return { error: `'${part}' — id:값` }
      const bad = range(n)
      if (bad) return { error: bad }
      out[k] = n
    }
    return { ok: out }
  }
  if (t === '' && (def === null || spec.key === 'echo_timeout_ms')) return { ok: null }
  const n = Number(t)
  if (t === '' || !Number.isFinite(n)) return { error: `${spec.key}: 숫자` }
  const bad = range(n)
  return bad ? { error: bad } : { ok: n }
}
