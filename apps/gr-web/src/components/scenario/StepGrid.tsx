// 스텝 편집 그리드 — `DataGrid<ScenarioStep>`. 값의 진실원은 부모(draft)이고 여기서는 편집 의도만 올린다.
//
// 열: # · 라벨 · 종류(select) · 대상 종류(select) · 대상 id(picker: 셀/스테이션 목록) · 품목(picker) ·
// 수량 · 대기 조건 · 후 대기(ms) · 실패 시 · 파라미터(버튼 → 다이얼로그) · 메모.
// 붙여넣기는 라벨("셀")과 원값("cell") 둘 다 받는다(`coercePaste`).
import { useMemo, useState } from 'react'
import { DataGrid, type DataGridColumn } from '../../lib/ui/datagrid/DataGrid'
import { TASK_TYPES } from '../../lib/gr/const'
import type { Cell, Item, ScenarioStep, Station, TaskType } from '../../lib/types'
import {
  ON_FAILURE_LABEL,
  TARGET_KIND_LABEL,
  WAIT_FOR_LABEL,
  paramCount,
} from '../../lib/scenario/model'
import { paramsToKv } from '../../lib/scenario/io'
import { cellText, stationText } from '../shared/TargetPicker'

export interface StepGridProps {
  steps: ScenarioStep[]
  cells: readonly Cell[]
  stations: readonly Station[]
  items: readonly Item[]
  /** `issueMap()` 결과 — `${index}:${field}` → 메시지. */
  issues: Map<string, string>
  selectedId: string | null
  onSelect: (id: string) => void
  onChange: (steps: ScenarioStep[]) => void
  onEditParams: (step: ScenarioStep) => void
  disabled?: boolean
}

const NONE = '—'
const KIND_OPTIONS = [NONE, TARGET_KIND_LABEL.cell, TARGET_KIND_LABEL.station] as const
const WAIT_OPTIONS = [WAIT_FOR_LABEL.accepted, WAIT_FOR_LABEL.completed] as const
const FAIL_OPTIONS = [ON_FAILURE_LABEL.stop, ON_FAILURE_LABEL.skip, ON_FAILURE_LABEL.retry] as const

function kindOf(v: string): 'cell' | 'station' | null {
  const s = v.trim().toLowerCase()
  if (s === 'cell' || s === TARGET_KIND_LABEL.cell) return 'cell'
  if (s === 'station' || s === TARGET_KIND_LABEL.station) return 'station'
  return null
}
function waitOf(v: string): ScenarioStep['wait_for'] | null {
  const s = v.trim().toLowerCase()
  if (s === 'accepted' || s === WAIT_FOR_LABEL.accepted) return 'accepted'
  if (s === 'completed' || s === WAIT_FOR_LABEL.completed) return 'completed'
  return null
}
function failOf(v: string): ScenarioStep['on_failure'] | null {
  const s = v.trim().toLowerCase()
  if (s === 'stop' || s === ON_FAILURE_LABEL.stop) return 'stop'
  if (s === 'skip' || s === ON_FAILURE_LABEL.skip) return 'skip'
  if (s === 'retry' || s === ON_FAILURE_LABEL.retry) return 'retry'
  return null
}
function typeOf(v: string): TaskType | null {
  const t = v.trim().toUpperCase() as TaskType
  return TASK_TYPES.includes(t) ? t : null
}
function intOf(v: string): number | null {
  const n = Number(v.trim())
  return v.trim() !== '' && Number.isFinite(n) ? Math.trunc(n) : null
}

/** 한 칸의 편집을 스텝에 적용한다(모르는 값은 무시 — 붙여넣기가 엉뚱한 값을 심지 않게). */
export function applyEdit(step: ScenarioStep, colId: string, value: string): ScenarioStep {
  switch (colId) {
    case 'label':
      return { ...step, label: value }
    case 'note':
      return { ...step, note: value }
    case 'type': {
      const t = typeOf(value)
      return t ? { ...step, type: t } : step
    }
    case 'target_kind': {
      if (value.trim() === '' || value.trim() === NONE) return { ...step, target: null }
      const k = kindOf(value)
      if (!k) return step
      return { ...step, target: { kind: k, id: step.target?.kind === k ? step.target.id : 0 } }
    }
    case 'target_id': {
      if (value.trim() === '') return { ...step, target: null }
      const id = intOf(value)
      if (id === null) return step
      return { ...step, target: { kind: step.target?.kind ?? 'cell', id } }
    }
    case 'item_code': {
      if (value.trim() === '') return { ...step, item_code: null }
      const n = intOf(value)
      return n === null ? step : { ...step, item_code: n }
    }
    case 'count': {
      const n = intOf(value)
      return n === null ? step : { ...step, count: Math.max(1, n) }
    }
    case 'wait_for': {
      const w = waitOf(value)
      return w ? { ...step, wait_for: w } : step
    }
    case 'wait_after_ms': {
      const n = intOf(value)
      return n === null ? step : { ...step, wait_after_ms: Math.max(0, n) }
    }
    case 'on_failure': {
      const f = failOf(value)
      return f ? { ...step, on_failure: f } : step
    }
    default:
      return step
  }
}

interface PickOption {
  value: string
  text: string
}

/** 참조 칸의 선택 팝업 — 검색칸 + 목록. Enter는 첫 항목, Esc/포커스 이탈은 취소(그리드가 닫는다). */
function PickList({
  options,
  value,
  allowNone,
  onPick,
}: {
  options: PickOption[]
  value: string
  allowNone: boolean
  onPick: (v: string) => void
}) {
  const [q, setQ] = useState('')
  const needle = q.trim().toLowerCase()
  const shown = needle ? options.filter((o) => o.text.toLowerCase().includes(needle)) : options
  const rowCls = (active: boolean) =>
    `block w-full truncate rounded px-2 py-1 text-left text-xs hover:bg-surface-inset ${
      active ? 'bg-accent-soft text-accent-text' : ''
    }`
  return (
    <div className="absolute top-0 left-0 z-40 w-72 rounded-md border border-line-default bg-surface-panel p-1 shadow-lg">
      <input
        tabIndex={0}
        className="mb-1 h-7 w-full rounded border border-line-strong bg-transparent px-2 text-xs outline-none focus:border-focus"
        placeholder="검색 — Enter로 첫 항목 선택"
        value={q}
        onChange={(e) => setQ(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault()
            if (shown[0]) onPick(shown[0].value)
          }
        }}
      />
      <div className="max-h-56 overflow-auto">
        {allowNone ? (
          <button
            type="button"
            className={rowCls(value === '')}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => onPick('')}
          >
            (없음)
          </button>
        ) : null}
        {shown.length === 0 ? (
          <div className="px-2 py-1 text-xs text-content-faint">해당 없음</div>
        ) : null}
        {shown.slice(0, 300).map((o) => (
          <button
            key={o.value}
            type="button"
            className={rowCls(o.value === value)}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => onPick(o.value)}
          >
            {o.text}
          </button>
        ))}
      </div>
    </div>
  )
}

export function StepGrid({
  steps,
  cells,
  stations,
  items,
  issues,
  selectedId,
  onSelect,
  onChange,
  onEditParams,
  disabled = false,
}: StepGridProps) {
  const index = useMemo(() => new Map(steps.map((s, i) => [s.id, i])), [steps])
  const idx = (s: ScenarioStep): number => index.get(s.id) ?? -1
  const err = (s: ScenarioStep, field: string): string | null =>
    issues.get(`${idx(s)}:${field}`) ?? null

  const cellOptions = useMemo<PickOption[]>(
    () => cells.map((c) => ({ value: String(c.id), text: cellText(c) })),
    [cells],
  )
  const stationOptions = useMemo<PickOption[]>(
    () => stations.map((s) => ({ value: String(s.id), text: stationText(s) })),
    [stations],
  )
  const itemOptions = useMemo<PickOption[]>(
    () => items.map((i) => ({ value: String(i.code), text: `${i.code} · ${i.name}` })),
    [items],
  )
  const cellById = useMemo(() => new Map(cells.map((c) => [c.id, c])), [cells])
  const stationById = useMemo(() => new Map(stations.map((s) => [s.id, s])), [stations])
  const itemByCode = useMemo(() => new Map(items.map((i) => [i.code, i])), [items])

  const columns = useMemo<DataGridColumn<ScenarioStep>[]>(
    () => [
      {
        id: '#',
        header: '#',
        width: '2.5rem',
        align: 'right',
        editor: 'none',
        text: (s) => String(idx(s) + 1),
        sortValue: (s) => idx(s),
      },
      { id: 'label', header: '라벨', width: '9rem', editor: 'text', text: (s) => s.label },
      {
        id: 'type',
        header: '종류',
        width: '6.5rem',
        editor: 'select',
        options: TASK_TYPES,
        text: (s) => s.type,
        invalid: (s) => err(s, 'type'),
        coercePaste: (v) => typeOf(v),
      },
      {
        id: 'target_kind',
        header: '대상 종류',
        width: '6rem',
        editor: 'select',
        options: KIND_OPTIONS,
        text: (s) => (s.target ? TARGET_KIND_LABEL[s.target.kind] : NONE),
        invalid: (s) => err(s, 'target'),
        coercePaste: (v) => (v.trim() === '' || v.trim() === NONE ? NONE : (kindOf(v) ?? null)),
      },
      {
        id: 'target_id',
        header: '대상',
        width: '11rem',
        editor: 'picker',
        nowrap: true,
        text: (s) => (s.target ? String(s.target.id) : ''),
        title: (s) => {
          if (!s.target) return ''
          if (s.target.kind === 'cell') {
            const c = cellById.get(s.target.id)
            return c ? cellText(c) : `셀 ${s.target.id} (레지스트리에 없음)`
          }
          const st = stationById.get(s.target.id)
          return st ? stationText(st) : `스테이션 ${s.target.id} (레지스트리에 없음)`
        },
        invalid: (s) => err(s, 'target'),
        coercePaste: (v) => (v.trim() === '' ? '' : intOf(v) === null ? null : String(intOf(v))),
      },
      {
        id: 'item_code',
        header: '품목',
        width: '10rem',
        editor: 'picker',
        nowrap: true,
        text: (s) => (s.item_code === null ? '' : String(s.item_code)),
        title: (s) =>
          s.item_code === null
            ? ''
            : (itemByCode.get(s.item_code)?.name ?? `품목 ${s.item_code} (레지스트리에 없음)`),
        invalid: (s) => err(s, 'item_code'),
        coercePaste: (v) => (v.trim() === '' ? '' : intOf(v) === null ? null : String(intOf(v))),
      },
      {
        id: 'count',
        header: '수량',
        width: '4rem',
        align: 'right',
        editor: 'number',
        decimals: 0,
        text: (s) => String(s.count),
        invalid: (s) => err(s, 'count'),
        coercePaste: (v) => (intOf(v) === null ? null : String(intOf(v))),
      },
      {
        id: 'wait_for',
        header: '대기 조건',
        width: '5.5rem',
        editor: 'select',
        options: WAIT_OPTIONS,
        text: (s) => WAIT_FOR_LABEL[s.wait_for],
        coercePaste: (v) => (waitOf(v) ? WAIT_FOR_LABEL[waitOf(v)!] : null),
      },
      {
        id: 'wait_after_ms',
        header: '후 대기',
        headerSub: 'ms',
        width: '5.5rem',
        align: 'right',
        editor: 'number',
        decimals: 0,
        text: (s) => String(s.wait_after_ms),
        invalid: (s) => err(s, 'wait_after_ms'),
        coercePaste: (v) => (intOf(v) === null ? null : String(intOf(v))),
      },
      {
        id: 'on_failure',
        header: '실패 시',
        width: '5.5rem',
        editor: 'select',
        options: FAIL_OPTIONS,
        text: (s) => ON_FAILURE_LABEL[s.on_failure],
        coercePaste: (v) => (failOf(v) ? ON_FAILURE_LABEL[failOf(v)!] : null),
      },
      {
        id: 'params',
        header: '파라미터',
        width: '7rem',
        editor: 'none',
        text: (s) => paramsToKv(s.params),
        title: (s) => paramsToKv(s.params) || '기본값 사용',
        invalid: (s) => err(s, 'params'),
      },
      { id: 'note', header: '메모', width: '12rem', editor: 'text', text: (s) => s.note },
    ],
    // eslint-disable-next-line react-hooks/exhaustive-deps -- idx/err는 index/issues에서만 파생
    [index, issues, cellById, stationById, itemByCode],
  )

  function edit(rowId: string, colId: string, value: string) {
    let changed = false
    const next = steps.map((s) => {
      if (s.id !== rowId) return s
      const n = applyEdit(s, colId, value)
      if (n !== s) changed = true
      return n
    })
    if (changed) onChange(next)
  }
  function editMany(updates: { rowId: string; colId: string; value: string }[]) {
    let next = steps
    let changed = false
    for (const u of updates) {
      next = next.map((s) => {
        if (s.id !== u.rowId) return s
        const n = applyEdit(s, u.colId, u.value)
        if (n !== s) changed = true
        return n
      })
    }
    if (changed) onChange(next)
  }

  return (
    <DataGrid<ScenarioStep>
      rows={steps}
      columns={columns}
      rowId={(s) => s.id}
      persistKey="scenario-steps"
      readOnlyAll={disabled}
      empty="스텝이 없습니다 — '행 추가'로 시작하세요"
      layoutFixed
      rowClass={(s) => (s.id === selectedId ? 'bg-accent-soft' : '')}
      onrowclick={(s) => onSelect(s.id)}
      onrowdblclick={(s) => onEditParams(s)}
      onedit={edit}
      oneditmany={editMany}
      cellTestId={(s, col) => `step-${idx(s)}-${col.id}`}
      cell={({ row, col }) => {
        if (col.id === 'params') {
          const n = paramCount(row)
          return (
            <button
              type="button"
              className="truncate text-2xs text-accent-text underline decoration-dotted underline-offset-2 hover:text-accent"
              title="파라미터 덮어쓰기 편집"
              onClick={() => onEditParams(row)}
            >
              {n > 0 ? `${n}개 override` : '기본값'}
            </button>
          )
        }
        if (col.id === 'target_id' && row.target) {
          return (
            <span className="min-w-0 truncate" title={col.title?.(row)}>
              <span className="font-mono">{row.target.id}</span>
              <span className="ml-1 text-content-faint">
                {col.title?.(row)?.replace(/^#\d+ · /, '')}
              </span>
            </span>
          )
        }
        if (col.id === 'item_code' && row.item_code !== null) {
          const it = itemByCode.get(row.item_code)
          return (
            <span className="min-w-0 truncate">
              <span className="font-mono">{row.item_code}</span>
              {it ? <span className="ml-1 text-content-faint">{it.name}</span> : null}
            </span>
          )
        }
        return (
          <span className={`min-w-0 ${col.nowrap ? 'truncate' : 'break-all'}`}>
            {col.text(row) || ' '}
          </span>
        )
      }}
      picker={({ row, col, value, choose }) => {
        if (col.id === 'item_code')
          return <PickList options={itemOptions} value={value} allowNone onPick={choose} />
        const kind = row.target?.kind ?? 'cell'
        return (
          <PickList
            options={kind === 'cell' ? cellOptions : stationOptions}
            value={value}
            allowNone
            onPick={choose}
          />
        )
      }}
    />
  )
}
