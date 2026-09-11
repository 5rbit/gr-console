// 작업 파라미터 폼 — `Partial<TaskParams>`를 그룹(Grip·Lift·Blend·Creep·Measure·Drag)별로 편집한다.
//
// `partial`이면 **덮어쓰기 폼**이다: 키마다 체크박스가 있고, 안 켠 키는 `value`에 없다(기본값 `base`가
// 흐리게 보인다). 시나리오 스텝·PICK/DROP 별 기본값처럼 "기본에서 이것만 바꾼다"를 표현하는 자리다.
// `partial`이 아니면 전 키가 항상 편집 가능하고, `value`에 없는 키는 `base` 값을 보인다.
import { Input } from '../../lib/ui/Input'
import { Switch } from '../../lib/ui/Switch'
import { PARAM_LABELS } from '../../lib/gr/const'
import type { TaskParams } from '../../lib/types'

type ParamKey = keyof TaskParams

export interface TaskParamFieldsProps {
  value: Partial<TaskParams>
  /** 기본값 — `value`에 없는 키의 표시값. */
  base?: TaskParams | null
  onChange: (next: Partial<TaskParams>) => void
  /** 덮어쓰기 폼(키별 체크박스). */
  partial?: boolean
  disabled?: boolean
}

interface Group {
  label: string
  keys: ParamKey[]
}

/** 표시 순서 — 현장에서 함께 만지는 것끼리 묶는다. */
export const PARAM_GROUPS: Group[] = [
  { label: 'Grip', keys: ['grip_height', 'pre_grip_delta', 'grip_back_delta', 'adjust_center', 'find_station_item'] },
  { label: 'Lift', keys: ['lift_up_height', 'lift_up_after_complete', 'lift_up_partial', 'avoid', 'outbound'] },
  { label: 'Blend', keys: ['blend_up_distance', 'blend_down_distance'] },
  { label: 'Creep', keys: ['lift_up_creep_distance', 'lift_down_creep_distance'] },
  { label: 'Measure', keys: ['measure_floor', 'measure_item', 'measure_sku'] },
  {
    label: 'Drag',
    keys: [
      'use_drag_out',
      'drag_out_height',
      'drag_out_dist',
      'drag_out_dir',
      'use_drag_in',
      'drag_in_height',
      'drag_in_dist',
      'drag_in_dir',
    ],
  },
]

/** 불리언 파라미터 집합 — 나머지는 숫자다. */
export const BOOL_KEYS: ReadonlySet<ParamKey> = new Set<ParamKey>([
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

function setKey(v: Partial<TaskParams>, k: ParamKey, x: number | boolean): Partial<TaskParams> {
  return Object.assign({}, v, { [k]: x })
}

function dropKey(v: Partial<TaskParams>, k: ParamKey): Partial<TaskParams> {
  const next = { ...v }
  delete next[k]
  return next
}

export function TaskParamFields({
  value,
  base = null,
  onChange,
  partial = false,
  disabled = false,
}: TaskParamFieldsProps) {
  const has = (k: ParamKey): boolean => value[k] !== undefined
  const shown = (k: ParamKey): number | boolean => {
    const v = value[k] ?? base?.[k]
    return v ?? (BOOL_KEYS.has(k) ? false : 0)
  }

  return (
    <div className="flex flex-col gap-3" data-testid="task-params">
      {PARAM_GROUPS.map((g) => (
        <fieldset key={g.label} className="flex flex-col gap-1.5">
          <legend className="mb-1 text-[11px] font-semibold text-slate-500">{g.label}</legend>
          <div className="grid grid-cols-1 gap-x-3 gap-y-1.5 sm:grid-cols-2">
            {g.keys.map((k) => {
              const active = !partial || has(k)
              const cur = shown(k)
              const label = PARAM_LABELS[k]
              const rowDisabled = disabled || !active
              return (
                <div
                  key={k}
                  className={`flex items-center gap-2 ${active ? '' : 'opacity-50'}`}
                  data-testid={`param-${k}`}
                >
                  {partial ? (
                    <input
                      type="checkbox"
                      className="shrink-0"
                      aria-label={`${label} 덮어쓰기`}
                      checked={has(k)}
                      disabled={disabled}
                      onChange={(e) =>
                        onChange(e.currentTarget.checked ? setKey(value, k, cur) : dropKey(value, k))
                      }
                    />
                  ) : null}
                  {BOOL_KEYS.has(k) ? (
                    <div className="flex flex-1 items-center justify-between gap-2">
                      <span className="text-xs">{label}</span>
                      <Switch
                        checked={cur === true}
                        label={label}
                        disabled={rowDisabled}
                        onCheckedChange={(next) => onChange(setKey(value, k, next))}
                      />
                    </div>
                  ) : (
                    <Input
                      className="flex-1"
                      label={label}
                      type="number"
                      mono
                      step="any"
                      value={String(cur)}
                      disabled={rowDisabled}
                      onValueChange={(s) => {
                        const n = Number(s)
                        if (s.trim() !== '' && Number.isFinite(n)) onChange(setKey(value, k, n))
                      }}
                    />
                  )}
                </div>
              )
            })}
          </div>
        </fieldset>
      ))}
    </div>
  )
}
