// 시나리오 JSON/CSV 입출력 — 백엔드 `scenario/io.rs`와 같은 형식.
//
// JSON: `{ schema: 1, name, description, repeat, steps[] }`. CSV 열:
// `label,type,target_kind,target_id,item_code,count,wait_for,wait_after_ms,on_failure,note,params`
// (`params`는 `k=v;k=v`). 열 순서는 헤더 이름으로 맞추므로 자유롭다. BOM 허용.
import type { ScenarioStep, ScenarioUpsert, TaskParams, TaskType, Target } from '../types'
import { TASK_TYPES } from '../gr/const'
import { newStep } from './model'

export const SCHEMA = 1

export const CSV_COLUMNS = [
  'label',
  'type',
  'target_kind',
  'target_id',
  'item_code',
  'count',
  'wait_for',
  'wait_after_ms',
  'on_failure',
  'note',
  'params',
] as const

export interface ScenarioDoc {
  schema: number
  id?: string
  name: string
  description: string
  repeat: number
  steps: ScenarioStep[]
}

// ── JSON ──────────────────────────────────────────────────────────────────────

export function toJson(s: ScenarioUpsert): string {
  const doc: ScenarioDoc = {
    schema: SCHEMA,
    ...(s.id ? { id: s.id } : {}),
    name: s.name,
    description: s.description,
    repeat: s.repeat,
    steps: s.steps,
  }
  return JSON.stringify(doc, null, 2)
}

function asRecord(v: unknown): Record<string, unknown> | null {
  return v !== null && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : null
}
function str(v: unknown, d = ''): string {
  return typeof v === 'string' ? v : v === null || v === undefined ? d : String(v)
}
function num(v: unknown, d: number): number {
  if (typeof v === 'number' && Number.isFinite(v)) return v
  if (typeof v === 'string' && v.trim() !== '') {
    const n = Number(v)
    if (Number.isFinite(n)) return n
  }
  return d
}
function optNum(v: unknown): number | null {
  if (v === null || v === undefined || v === '') return null
  const n = Number(v)
  return Number.isFinite(n) ? n : null
}

export function parseTaskType(v: unknown): TaskType | null {
  const t = str(v).trim().toUpperCase() as TaskType
  return TASK_TYPES.includes(t) ? t : null
}
export function parseWaitFor(v: unknown): ScenarioStep['wait_for'] | null {
  const s = str(v).trim().toLowerCase()
  if (s === '' || s === 'completed') return 'completed'
  if (s === 'accepted') return 'accepted'
  return null
}
export function parseOnFailure(v: unknown): ScenarioStep['on_failure'] | null {
  const s = str(v).trim().toLowerCase()
  if (s === '' || s === 'stop') return 'stop'
  if (s === 'skip' || s === 'retry') return s
  return null
}

function parseTarget(v: unknown, where: string): Target | null {
  if (v === null || v === undefined) return null
  const o = asRecord(v)
  if (!o) throw new Error(`${where}: target은 {kind,id} 객체여야 함`)
  const kind = str(o.kind).toLowerCase()
  if (kind !== 'cell' && kind !== 'station') throw new Error(`${where}: 알 수 없는 대상 종류 '${kind}'`)
  const id = num(o.id, NaN)
  if (!Number.isInteger(id)) throw new Error(`${where}: 대상 id 오류`)
  return { kind, id }
}

/** 임의 JSON 객체 하나를 스텝으로 — 모르는 필드는 버리고, 빠진 필드는 기본값. */
export function stepFromObject(v: unknown, where = 'step'): ScenarioStep {
  const o = asRecord(v)
  if (!o) throw new Error(`${where}: 객체가 아님`)
  const type = parseTaskType(o.type)
  if (!type) throw new Error(`${where}: 알 수 없는 작업 종류 '${str(o.type)}'`)
  const wait_for = parseWaitFor(o.wait_for)
  if (!wait_for) throw new Error(`${where}: wait_for 오류 '${str(o.wait_for)}'`)
  const on_failure = parseOnFailure(o.on_failure)
  if (!on_failure) throw new Error(`${where}: on_failure 오류 '${str(o.on_failure)}'`)
  const params = asRecord(o.params) ?? {}
  const step = newStep({
    ...(str(o.id) ? { id: str(o.id) } : {}),
    label: str(o.label),
    type,
    target: parseTarget(o.target, where),
    item_code: optNum(o.item_code),
    count: num(o.count, 1),
    params: params as Partial<TaskParams>,
    wait_for,
    wait_after_ms: num(o.wait_after_ms, 0),
    on_failure,
    note: str(o.note),
  })
  const pallet = parsePallet(o.pallet)
  return pallet ? { ...step, pallet } : step
}

/** `pallet` — `{seq, level}` 또는 `{auto: true}`(팔렛 패턴 화면이 싣는다). 모양이 틀리면 버린다. */
function parsePallet(v: unknown): ScenarioStep['pallet'] {
  const o = asRecord(v)
  if (!o) return undefined
  if (o.auto === true) return { auto: true }
  const seq = optNum(o.seq)
  if (seq === null || !Number.isInteger(seq) || seq < 1) return undefined
  const level = optNum(o.level)
  return { seq, level: level !== null && Number.isInteger(level) && level >= 1 ? level : 1 }
}

/** 내보낸 문서 또는 `Scenario` 원본을 읽는다. id·시각은 버린다(가져오기는 새 문서). */
export function fromJson(text: string): ScenarioUpsert {
  let v: unknown
  try {
    v = JSON.parse(text.replace(/^﻿/, ''))
  } catch (e) {
    throw new Error(`JSON 파싱 실패 — ${e instanceof Error ? e.message : String(e)}`)
  }
  const o = asRecord(v)
  if (!o) throw new Error('JSON: 객체가 아님')
  const schema = num(o.schema, SCHEMA)
  if (schema > SCHEMA) throw new Error(`지원하지 않는 schema ${schema} (최대 ${SCHEMA})`)
  const stepsRaw = Array.isArray(o.steps) ? o.steps : []
  return {
    name: str(o.name),
    description: str(o.description),
    repeat: Math.max(0, Math.trunc(num(o.repeat, 1))),
    steps: stepsRaw.map((s, i) => stepFromObject(s, `스텝 ${i + 1}`)),
  }
}

// ── params k=v ────────────────────────────────────────────────────────────────

/** `{b:true,a:1}` → `a=1;b=true`(키 정렬). */
export function paramsToKv(p: Partial<TaskParams>): string {
  return Object.keys(p)
    .sort()
    .filter((k) => p[k as keyof TaskParams] !== undefined && p[k as keyof TaskParams] !== null)
    .map((k) => `${k}=${String(p[k as keyof TaskParams])}`)
    .join(';')
}

/** `a=1; b=true` → `{a:1,b:true}`. 값은 true/false → 불리언, 숫자 → 숫자, 그 외 문자열. */
export function paramsFromKv(s: string): Record<string, number | boolean | string> {
  const out: Record<string, number | boolean | string> = {}
  for (const part of s.split(';')) {
    const p = part.trim()
    if (!p) continue
    const eq = p.indexOf('=')
    if (eq < 0) throw new Error(`params: '${p}' 은(는) k=v 형식이 아님`)
    const k = p.slice(0, eq).trim()
    const v = p.slice(eq + 1).trim()
    if (!k) throw new Error(`params: '${p}' 키가 비어 있음`)
    const lv = v.toLowerCase()
    if (lv === 'true') out[k] = true
    else if (lv === 'false') out[k] = false
    else if (v !== '' && Number.isFinite(Number(v))) out[k] = Number(v)
    else out[k] = v
  }
  return out
}

// ── CSV ───────────────────────────────────────────────────────────────────────

function csvCell(v: string): string {
  return /[",\r\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v
}

export function toCsv(s: ScenarioUpsert): string {
  const lines = [CSV_COLUMNS.join(',')]
  for (const st of s.steps) {
    lines.push(
      [
        st.label,
        st.type,
        st.target?.kind ?? '',
        st.target ? String(st.target.id) : '',
        st.item_code === null ? '' : String(st.item_code),
        String(st.count),
        st.wait_for,
        String(st.wait_after_ms),
        st.on_failure,
        st.note,
        paramsToKv(st.params),
      ]
        .map(csvCell)
        .join(','),
    )
  }
  return lines.join('\n') + '\n'
}

/** RFC 4180 파서 — 따옴표·겹따옴표·따옴표 안 줄바꿈·CRLF. 빈 줄은 버린다. */
export function parseCsv(text: string): string[][] {
  const rows: string[][] = []
  let row: string[] = []
  let cell = ''
  let quoted = false
  const src = text.replace(/^﻿/, '')
  for (let i = 0; i < src.length; i++) {
    const ch = src[i]
    if (quoted) {
      if (ch === '"') {
        if (src[i + 1] === '"') {
          cell += '"'
          i++
        } else quoted = false
      } else cell += ch
      continue
    }
    if (ch === '"') quoted = true
    else if (ch === ',') {
      row.push(cell)
      cell = ''
    } else if (ch === '\n' || ch === '\r') {
      if (ch === '\r' && src[i + 1] === '\n') i++
      row.push(cell)
      cell = ''
      if (row.some((c) => c.trim() !== '')) rows.push(row)
      row = []
    } else cell += ch
  }
  if (cell !== '' || row.length > 0) {
    row.push(cell)
    if (row.some((c) => c.trim() !== '')) rows.push(row)
  }
  return rows
}

export function fromCsv(text: string, name: string): ScenarioUpsert {
  const rows = parseCsv(text)
  if (rows.length === 0) throw new Error('CSV: 헤더가 없음')
  const header = rows[0].map((h) => h.trim().toLowerCase())
  const col = (n: string): number => header.indexOf(n)
  if (col('type') < 0) throw new Error("CSV: 'type' 열이 없음")
  const idx = Object.fromEntries(CSV_COLUMNS.map((c) => [c, col(c)])) as Record<
    (typeof CSV_COLUMNS)[number],
    number
  >
  const steps: ScenarioStep[] = []
  rows.slice(1).forEach((r, n) => {
    const where = `CSV ${n + 2}행`
    const get = (c: (typeof CSV_COLUMNS)[number]): string => (idx[c] >= 0 ? (r[idx[c]] ?? '').trim() : '')
    const kind = get('target_kind').toLowerCase()
    const idS = get('target_id')
    let target: unknown = null
    if (kind || idS) target = { kind, id: idS === '' ? NaN : Number(idS) }
    let params: Record<string, unknown>
    try {
      params = paramsFromKv(get('params'))
    } catch (e) {
      throw new Error(`${where}: ${e instanceof Error ? e.message : String(e)}`)
    }
    const numOr = (s: string, field: string, d: number): number => {
      if (s === '') return d
      const v = Number(s)
      if (!Number.isFinite(v)) throw new Error(`${where}: ${field} 오류 '${s}'`)
      return v
    }
    const itemS = get('item_code')
    steps.push(
      stepFromObject(
        {
          label: get('label'),
          type: get('type'),
          target,
          item_code: itemS === '' ? null : numOr(itemS, 'item_code', NaN),
          count: numOr(get('count'), 'count', 1),
          wait_for: get('wait_for'),
          wait_after_ms: numOr(get('wait_after_ms'), 'wait_after_ms', 0),
          on_failure: get('on_failure'),
          note: get('note'),
          params,
        },
        where,
      ),
    )
  })
  return { name, description: '', repeat: 1, steps }
}
