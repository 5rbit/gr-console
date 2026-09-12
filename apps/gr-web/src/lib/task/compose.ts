// 작성 카드의 순수 로직 — 초안 → `TaskRequest`, 파라미터 우선순위, 검증, 기본값 표의 행/편집.
//
// 백엔드 `issue::compose_from`과 같은 우선순위다: `base` ← `by[type][kind]` ← 요청 `params`.
// 화면은 이 결과를 "지금 이 값이 갈 것이다"로 보여 주고, 최종 확정은 백엔드 미리보기가 한다.
import { PARAM_LABELS } from '../gr/const'
import { robots } from '../robots'
import type { Defaults, TargetKind, TaskParams, TaskRequest, TaskType } from '../types'
import type { ComposePreview, Draft } from './types'

export type ParamKey = keyof TaskParams

export const PARAM_KEYS: readonly ParamKey[] = Object.keys(PARAM_LABELS) as ParamKey[]

/** 불리언 파라미터(나머지는 숫자). `TaskParamFields.BOOL_KEYS`와 같은 집합 — 컴포넌트를 끌어오지 않으려고 따로 둔다. */
export const BOOL_PARAMS: ReadonlySet<ParamKey> = new Set<ParamKey>([
  'lift_up_after_complete',
  'lift_up_partial',
  'measure_floor',
  'measure_item',
  'measure_sku',
  'adjust_center',
  'find_station_item',
  'avoid',
  'outbound',
  'use_drag_out',
  'use_drag_in',
])

export const EMPTY_DRAFT: Draft = {
  type: 'PICK',
  target: null,
  item_code: null,
  count: 1,
  params: {},
  note: '',
}

/** 종류별로 고를 수 있는 대상 — MEASURE는 셀만, UP은 대상이 없어도 된다. */
export function targetKindsFor(type: TaskType): TargetKind[] {
  return type === 'MEASURE' ? ['cell'] : ['cell', 'station']
}

/** 품목이 꼭 있어야 하는 종류. */
export function itemRequired(type: TaskType): boolean {
  return type === 'PICK' || type === 'DROP' || type === 'MEASURE'
}

/** 대상이 꼭 있어야 하는 종류(UP만 예외). */
export function targetRequired(type: TaskType): boolean {
  return type !== 'UP'
}

/** 초안 → 제출 본문. 위치 덮어쓰기와 시나리오 출처는 이 화면에서 쓰지 않는다. */
export function buildRequest(d: Draft): TaskRequest {
  return {
    type: d.type,
    target: d.target,
    item_code: d.item_code,
    count: Math.max(1, Math.floor(d.count || 1)),
    params: { ...d.params },
    position_override: null,
    note: d.note,
    source: null,
    robot: robots.selected,
    grip_ref: null,
  }
}

/** 제출 전 검증 — 문제가 없으면 빈 배열. */
export function validateDraft(d: Draft): string[] {
  const out: string[] = []
  if (targetRequired(d.type) && !d.target) out.push('대상(셀/스테이션)을 고르세요')
  if (d.target && !targetKindsFor(d.type).includes(d.target.kind))
    out.push(`${d.type}에는 ${d.target.kind === 'station' ? '스테이션' : '셀'}을 쓸 수 없습니다`)
  if (itemRequired(d.type) && d.item_code === null) out.push('품목을 고르세요')
  if (!Number.isFinite(d.count) || d.count < 1 || d.count > 255) out.push('수량은 1..255')
  for (const [k, v] of Object.entries(d.params)) {
    if (v === undefined) continue
    if (BOOL_PARAMS.has(k as ParamKey)) {
      if (typeof v !== 'boolean') out.push(`${PARAM_LABELS[k as ParamKey]}: 참/거짓이어야 합니다`)
    } else if (typeof v !== 'number' || !Number.isFinite(v) || v < 0) {
      out.push(`${PARAM_LABELS[k as ParamKey]}: 0 이상의 수여야 합니다`)
    }
  }
  return out
}

/** 우선순위를 적용한 파라미터 전체 — `base` ← `by[type][kind]` ← `overrides`. `null`/`undefined` 키는 건너뛴다. */
export function effectiveParams(
  defaults: Defaults | null,
  type: TaskType,
  kind: TargetKind,
  overrides: Partial<TaskParams>,
): TaskParams | null {
  if (!defaults) return null
  const out: Record<string, number | boolean> = { ...defaults.base }
  const by = (
    defaults.by as Partial<Record<string, Partial<Record<TargetKind, Partial<TaskParams>>>>>
  )[type]?.[kind]
  for (const layer of [by ?? {}, overrides]) {
    for (const [k, v] of Object.entries(layer)) {
      if (v !== undefined && v !== null) out[k] = v as number | boolean
    }
  }
  return out as unknown as TaskParams
}

/** 미리보기 → 표시 항목(라벨·값). 위치는 소수 1자리, 나머지는 그대로. */
export function previewFields(p: ComposePreview | null): { label: string; value: string }[] {
  if (!p) return []
  const t = p.task
  const pos = t.Position ?? []
  const f = (v: number | undefined) => (v === undefined ? '' : (Math.round(v * 10) / 10).toString())
  const out = [
    { label: 'X', value: f(pos[0]) },
    { label: 'Y', value: f(pos[1]) },
    { label: 'Z', value: f(pos[2]) },
    { label: 'G', value: f(pos[3]) },
    { label: 'TaskType', value: `0x${t.TaskType.toString(16).toUpperCase()}` },
    { label: '셀 Id', value: t.Cell?.Id ? String(t.Cell.Id) : '' },
    { label: '셀 위치', value: t.Cell?.Id ? (t.Cell.Position ?? []).map(f).join(' / ') : '' },
    { label: '품목', value: t.Item?.Code ? `${t.Item.Code} × ${t.Item.Count}` : '' },
    {
      label: '내경/높이',
      value: t.Item?.Code ? `${f(t.Item.InnerDiameter)} / ${f(t.Item.Height)}` : '',
    },
    { label: PARAM_LABELS.grip_height, value: String(p.params.grip_height) },
    { label: PARAM_LABELS.lift_up_height, value: String(p.params.lift_up_height) },
    { label: PARAM_LABELS.blend_up_distance, value: String(p.params.blend_up_distance) },
    { label: PARAM_LABELS.blend_down_distance, value: String(p.params.blend_down_distance) },
  ]
  const flags = PARAM_KEYS.filter((k) => BOOL_PARAMS.has(k) && p.params[k] === true).map(
    (k) => PARAM_LABELS[k],
  )
  out.push({ label: '켜진 플래그', value: flags.join(', ') })
  return out
}

// ── 기본값 표(DefaultsDialog) ─────────────────────────────────────────────────

export type DefaultsCol = 'base' | 'PICK.cell' | 'PICK.station' | 'DROP.cell' | 'DROP.station'
export const DEFAULTS_COLS: readonly { id: DefaultsCol; header: string }[] = [
  { id: 'base', header: '공통' },
  { id: 'PICK.cell', header: 'PICK · 셀' },
  { id: 'PICK.station', header: 'PICK · 스테이션' },
  { id: 'DROP.cell', header: 'DROP · 셀' },
  { id: 'DROP.station', header: 'DROP · 스테이션' },
]

export interface DefaultsRow {
  key: ParamKey
  label: string
  bool: boolean
  values: Record<DefaultsCol, number | boolean | undefined>
}

function layer(d: Defaults, col: DefaultsCol): Partial<TaskParams> {
  if (col === 'base') return d.base
  const [t, k] = col.split('.') as ['PICK' | 'DROP', TargetKind]
  return d.by[t]?.[k] ?? {}
}

/** 파라미터 키 하나가 행, 공통·PICK/DROP×셀/스테이션이 열. 부분 열의 `undefined`는 "공통 상속". */
export function defaultsRows(d: Defaults): DefaultsRow[] {
  return PARAM_KEYS.map((key) => ({
    key,
    label: PARAM_LABELS[key],
    bool: BOOL_PARAMS.has(key),
    values: Object.fromEntries(DEFAULTS_COLS.map((c) => [c.id, layer(d, c.id)[key]])) as Record<
      DefaultsCol,
      number | boolean | undefined
    >,
  }))
}

/** 셀 하나의 편집을 새 `Defaults`로. 부분 열에 빈 값이면 키를 지운다(상속). 잘못된 값은 `null`. */
export function applyDefaultsEdit(
  d: Defaults,
  key: ParamKey,
  col: DefaultsCol,
  raw: string,
): Defaults | null {
  const s = raw.trim()
  const bool = BOOL_PARAMS.has(key)
  let v: number | boolean | undefined
  if (s === '') {
    if (col === 'base') return null // 공통 값은 비울 수 없다
    v = undefined
  } else if (bool) {
    const l = s.toLowerCase()
    if (['true', '1', 'y', 'yes', '예', 'on'].includes(l)) v = true
    else if (['false', '0', 'n', 'no', '아니오', 'off'].includes(l)) v = false
    else return null
  } else {
    const n = Number(s)
    if (!Number.isFinite(n) || n < 0) return null
    v = n
  }
  const next: Defaults = {
    ...d,
    base: { ...d.base },
    by: {
      PICK: { cell: { ...d.by.PICK?.cell }, station: { ...d.by.PICK?.station } },
      DROP: { cell: { ...d.by.DROP?.cell }, station: { ...d.by.DROP?.station } },
    },
  }
  if (col === 'base') {
    ;(next.base as unknown as Record<string, number | boolean>)[key] = v as number | boolean
  } else {
    const [t, k] = col.split('.') as ['PICK' | 'DROP', TargetKind]
    const part = next.by[t][k] as Record<string, number | boolean | undefined>
    if (v === undefined) delete part[key]
    else part[key] = v
  }
  return next
}

/** 초안이 기본값과 다른가(저장 버튼 활성). */
export function defaultsChanged(a: Defaults, b: Defaults): boolean {
  return JSON.stringify({ base: a.base, by: a.by }) !== JSON.stringify({ base: b.base, by: b.by })
}
