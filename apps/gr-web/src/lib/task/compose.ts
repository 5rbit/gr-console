// 작성 카드의 순수 로직 — 초안 → `TaskRequest`, 파라미터 우선순위, 검증, 기본값 표의 행/편집.
//
// 백엔드 `issue::compose_from`과 같은 우선순위다: `base` ← `by[type][kind]` ← 상황(`situations`) ← 요청 `params`.
// 기본값 표에서 빈 칸 = 그 아래 층 값을 쓴다(상속). 표는 그 상속값을 흐리게 보여 준다(`inheritedOf`).
// 화면은 이 결과를 "지금 이 값이 갈 것이다"로 보여 주고, 최종 확정은 백엔드 미리보기가 한다.
import { PARAM_LABELS } from '../gr/const'
import { robots } from '../robots'
import type { Defaults, TargetKind, TaskParams, TaskRequest, TaskType, Situation } from '../types'
import { moveErrors, moveOf, moveParams, sentItem } from './moveMode'
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
    item_code: sentItem(d),
    count: Math.max(1, Math.floor(d.count || 1)),
    params: { ...d.params, ...(d.type === 'MOVE' ? moveParams(moveOf(d)) : {}) },
    position_override: null,
    note: d.note,
    source: null,
    robot: robots.selected,
    grip_ref: null,
    // 기본(켜짐)은 싣지 않는다 — 요청 JSON 이 예전과 같게.
    ...(d.station_offset === false ? { station_offset: 'off' as const } : {}),
    ...(d.ignore_stack_max ? { ignore_stack_max: true } : {}),
    ...(d.multi_pick && d.target?.kind === 'station' && (d.type === 'PICK' || d.type === 'DROP')
      ? { multi_pick: true }
      : {}),
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
  if (d.type === 'MOVE') out.push(...moveErrors(moveOf(d)))
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
  situations: readonly Situation[] = [],
): TaskParams | null {
  if (!defaults) return null
  const out: Record<string, number | boolean> = { ...defaults.base }
  const by = (
    defaults.by as Partial<Record<string, Partial<Record<TargetKind, Partial<TaskParams>>>>>
  )[type]?.[kind]
  // 공통 ← 종류·대상별 ← 상황별(서버가 고른 것, 적용 순서) ← 덮어쓰기 — 백엔드 `compose_from` 과 같은 순서.
  const sit = situations.map((k) => defaults.situations?.[k] ?? {})
  for (const layer of [by ?? {}, ...sit, overrides]) {
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
    { label: 'Cell.Id', value: t.Cell?.Id ? String(t.Cell.Id) : '' },
    { label: 'Cell.Position', value: t.Cell?.Id ? (t.Cell.Position ?? []).map(f).join(' / ') : '' },
    { label: 'Item (Code × Count)', value: t.Item?.Code ? `${t.Item.Code} × ${t.Item.Count}` : '' },
    {
      label: 'InnerDiameter / Height',
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
  out.push({ label: 'Flags', value: flags.join(', ') })
  return out
}

// ── 기본값 표(DefaultsDialog) ─────────────────────────────────────────────────

/** 상황 키 — 적용 순서(뒤가 이긴다). 백엔드 `registry::SITUATIONS`. */
export const SITUATIONS: readonly Situation[] = [
  'measure_item',
  'measure_sku',
  'pallet_station',
  'multi_pick',
]
export const SITUATION_LABEL: Record<Situation, string> = {
  measure_item: 'Measure Item',
  measure_sku: 'Measure SKU',
  pallet_station: 'Pallet Station',
  multi_pick: 'Multi-Pick',
}

export type DefaultsCol =
  'base' | 'PICK.cell' | 'PICK.station' | 'DROP.cell' | 'DROP.station' | `sit.${Situation}`
export const DEFAULTS_COLS: readonly { id: DefaultsCol; header: string }[] = [
  { id: 'base', header: 'Base' },
  { id: 'PICK.cell', header: 'PICK · Cell' },
  { id: 'PICK.station', header: 'PICK · Station' },
  { id: 'DROP.cell', header: 'DROP · Cell' },
  { id: 'DROP.station', header: 'DROP · Station' },
  ...SITUATIONS.map((k) => ({ id: `sit.${k}` as DefaultsCol, header: SITUATION_LABEL[k] })),
]

/** 상황 열인가 — 이 열의 빈 칸은 "아래 층 상속"(공통·종류별). */
export function isSituationCol(col: DefaultsCol): col is `sit.${Situation}` {
  return col.startsWith('sit.')
}

export interface DefaultsRow {
  key: ParamKey
  label: string
  bool: boolean
  values: Record<DefaultsCol, number | boolean | undefined>
}

function layer(d: Defaults, col: DefaultsCol): Partial<TaskParams> {
  if (col === 'base') return d.base
  if (isSituationCol(col)) return d.situations?.[col.slice(4) as Situation] ?? {}
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
    situations: Object.fromEntries(SITUATIONS.map((k) => [k, { ...d.situations?.[k] }])) as Record<
      Situation,
      Partial<TaskParams>
    >,
  }
  if (col === 'base') {
    ;(next.base as unknown as Record<string, number | boolean>)[key] = v as number | boolean
  } else if (isSituationCol(col)) {
    const part = next.situations![col.slice(4) as Situation] as Record<
      string,
      number | boolean | undefined
    >
    if (v === undefined) delete part[key]
    else part[key] = v
  } else {
    const [t, k] = col.split('.') as ['PICK' | 'DROP', TargetKind]
    const part = next.by[t][k] as Record<string, number | boolean | undefined>
    if (v === undefined) delete part[key]
    else part[key] = v
  }
  return next
}

/** 열이 어디에 걸리는가 — 헤더 부제. 백엔드 `issue::situations_for` 와 같은 판정. */
export const DEFAULTS_COL_SCOPE: Record<DefaultsCol, string> = {
  base: '모든 작업',
  'PICK.cell': '',
  'PICK.station': '',
  'DROP.cell': '',
  'DROP.station': '',
  'sit.measure_item': 'MEASURE',
  'sit.measure_sku': 'MEASURE · SKU',
  'sit.pallet_station': '팔렛 슬롯 작업',
  'sit.multi_pick': 'Station PICK/DROP',
}

/** 상황 열 아래에 깔리는 층 — 그 상황이 걸리는 작업들의 종류·대상 열(없으면 Base). */
const SITUATION_BELOW: Record<Situation, readonly DefaultsCol[]> = {
  measure_item: ['base'],
  measure_sku: ['base'],
  // 팔렛 슬롯은 스테이션 PICK/DROP(MEASURE 스테이션은 종류별 층이 없어 Base)
  pallet_station: ['PICK.station', 'DROP.station', 'base'],
  multi_pick: ['PICK.station', 'DROP.station'],
}

/** 한 작업에 함께 걸릴 수 있는, 적용 순서상 **아래** 상황들(MEASURE 스테이션 팔렛 · 팔렛 + Multi-Pick 등). */
const SITUATION_STACK_BELOW: Record<Situation, readonly Situation[]> = {
  measure_item: [],
  measure_sku: ['measure_item'],
  pallet_station: ['measure_item', 'measure_sku'],
  multi_pick: ['pallet_station'],
}

/** 빈 칸이 실제로 쓰는 값 하나 — `from` 은 그 값을 준 열. */
export interface InheritedValue {
  from: DefaultsCol
  value: number | boolean
}

/** 종류·대상 열까지 적용한 값(상황 제외) — `col` 이 `base` 면 Base 값. */
function resolvedBelowSituations(d: Defaults, key: ParamKey, col: DefaultsCol): InheritedValue {
  const own = col === 'base' ? undefined : layer(d, col)[key]
  if (own !== undefined) return { from: col, value: own }
  return { from: 'base', value: d.base[key] }
}

/**
 * 이 칸을 비워 두면 쓰일 값들. 종류·대상 열은 Base 하나, 상황 열은 그 상황이 걸리는 작업마다 하나
 * (값이 같으면 하나로 합친다). Base 열은 상속이 없어 빈 배열.
 */
export function inheritedOf(d: Defaults, key: ParamKey, col: DefaultsCol): InheritedValue[] {
  if (col === 'base') return []
  if (!isSituationCol(col)) return [{ from: 'base', value: d.base[key] }]
  const out: InheritedValue[] = []
  for (const below of SITUATION_BELOW[col.slice(4) as Situation]) {
    const v = resolvedBelowSituations(d, key, below)
    if (!out.some((o) => o.from === v.from)) out.push(v)
  }
  return out
}

/** 상속값 한 칸 표시 — 모두 같으면 값 하나, 다르면 `55 / 40`(어느 열에서 왔는지는 툴팁). */
export function inheritedText(vals: readonly InheritedValue[]): string {
  if (vals.length === 0) return ''
  return [...new Set(vals.map((v) => String(v.value)))].join(' / ')
}

export function colHeader(col: DefaultsCol): string {
  return DEFAULTS_COLS.find((c) => c.id === col)?.header ?? col
}

/** 적어 둔 값이 비웠을 때 쓰일 값과 **모두** 같은가 — 지워도 결과가 같은 칸. */
export function isRedundant(d: Defaults, key: ParamKey, col: DefaultsCol): boolean {
  if (col === 'base') return false
  const own = layer(d, col)[key]
  if (own === undefined) return false
  // 같이 걸릴 수 있는 아래 상황이 같은 키를 쥐고 있으면, 지웠을 때 그 값이 올라온다 — 지우지 않는다.
  if (isSituationCol(col)) {
    const under = SITUATION_STACK_BELOW[col.slice(4) as Situation]
    if (under.some((s) => d.situations?.[s]?.[key] !== undefined)) return false
  }
  const inh = inheritedOf(d, key, col)
  return inh.length > 0 && inh.every((v) => v.value === own)
}

/** 지워도 결과가 같은 칸 목록(표 순서). 종류·대상 열을 먼저 지우면 상황 열의 판정이 바뀔 수 있어 한 번에 본다. */
export function redundantCells(d: Defaults): { key: ParamKey; col: DefaultsCol }[] {
  const out: { key: ParamKey; col: DefaultsCol }[] = []
  for (const key of PARAM_KEYS) {
    for (const c of DEFAULTS_COLS) {
      if (isRedundant(d, key, c.id)) out.push({ key, col: c.id })
    }
  }
  return out
}

/**
 * 지워도 결과가 같은 칸을 비운다. 한 칸을 지우면 다른 칸의 상속값이 바뀔 수 있으므로(예: PICK·Station 을
 * 지우면 Multi-Pick 이 Base 를 보게 된다) 더 지울 것이 없을 때까지 반복한다 — 어떤 작업의 최종 값도 바뀌지 않는다.
 */
export function pruneRedundant(d: Defaults): { next: Defaults; removed: number } {
  let cur = d
  let removed = 0
  for (;;) {
    const hit = redundantCells(cur)[0]
    if (!hit) return { next: cur, removed }
    const next = applyDefaultsEdit(cur, hit.key, hit.col, '')
    if (!next) return { next: cur, removed }
    cur = next
    removed++
  }
}

/** 초안이 기본값과 다른가(저장 버튼 활성). */
export function defaultsChanged(a: Defaults, b: Defaults): boolean {
  const pick = (d: Defaults) =>
    JSON.stringify({
      base: d.base,
      by: d.by,
      sit: SITUATIONS.map((k) => d.situations?.[k] ?? {}),
    })
  return pick(a) !== pick(b)
}
