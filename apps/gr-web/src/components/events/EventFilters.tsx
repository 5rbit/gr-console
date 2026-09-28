// 필터 띠 한 줄 — PLC · Cat · 레벨 · code · ctx · 기간 · 검색. 글자 칸은 잠깐 멈춘 뒤에 조회한다.
import { useCallback, useEffect, useMemo, useState } from 'react'
import { ChevronDown, X } from 'lucide-react'
import type { EvtCatalog } from '../../lib/evtlog/api'
import { EVT_TYPES } from '../../lib/evtlog/evtTypeModel'
import {
  EMPTY_FILTER,
  RANGE_PRESETS,
  activeCount,
  codeSuggestions,
  localInputToMs,
  msToLocalInput,
  parseCtx,
  rangeLabel,
  type EvtFilter,
  type RangePreset,
} from '../../lib/evtlog/evtFilterModel'
import { Button } from '../../lib/ui/Button'
import { FormDialog } from '../../lib/ui/Dialog'
import { Input } from '../../lib/ui/Input'
import { Select } from '../../lib/ui/Select'
import { useDismiss } from '../../lib/ui/useDismiss'
import { cn } from '../../lib/utils'

const TEXT_DEBOUNCE_MS = 350
const CODE_LIST_ID = 'evt-code-suggest'
export const TYPE_OPTIONS: PickOption[] = EVT_TYPES.map((t) => ({ id: t.id, label: t.id }))

export interface PickOption {
  id: string
  label: string
  hint?: string
}

/** 여러 개 고르기 — 버튼 + 체크 목록. 비우면 전부. */
export function EvtMultiPick({
  label,
  options,
  value,
  onChange,
  testid,
  allLabel = '전체',
}: {
  label: string
  options: readonly PickOption[]
  value: readonly string[]
  onChange: (next: string[]) => void
  testid?: string
  /** 아무것도 안 골랐을 때의 뜻(기본 `전체`). */
  allLabel?: string
}) {
  const [open, setOpen] = useState(false)
  const close = useCallback(() => setOpen(false), [])
  const root = useDismiss<HTMLDivElement>(open, close)
  const shown = value.length === 0 ? label : `${label} ${value.join(',')}`
  const toggle = (id: string) =>
    onChange(value.includes(id) ? value.filter((v) => v !== id) : [...value, id])

  return (
    <div ref={root} className="relative">
      <Button
        size="sm"
        intent={value.length ? 'outline' : 'ghost'}
        aria-expanded={open}
        title={value.length ? shown : `${label} ${allLabel}`}
        data-testid={testid}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="max-w-32 truncate">{shown}</span>
        <ChevronDown size={12} className="opacity-60" />
      </Button>
      {open ? (
        <div
          role="dialog"
          aria-label={label}
          className="absolute top-8 left-0 z-30 max-h-80 w-52 overflow-y-auto rounded-lg border border-line-default bg-surface-panel py-1 text-xs shadow-lg"
        >
          <button
            type="button"
            className={cn(
              'flex w-full items-center gap-2 px-2.5 py-1 text-left hover:bg-surface-inset',
              value.length === 0 ? 'text-accent-text' : 'text-content-tertiary',
            )}
            onClick={() => onChange([])}
          >
            {allLabel}
          </button>
          {options.map((o) => (
            <label
              key={o.id}
              className="flex cursor-pointer items-center gap-2 px-2.5 py-1 hover:bg-surface-inset"
            >
              <input
                type="checkbox"
                checked={value.includes(o.id)}
                onChange={() => toggle(o.id)}
                className="accent-accent"
              />
              <span className="font-mono">{o.label}</span>
              {o.hint ? (
                <span className="ml-auto text-3xs text-content-faint">{o.hint}</span>
              ) : null}
            </label>
          ))}
          {options.length === 0 ? (
            <div className="px-2.5 py-1 text-content-faint">목록 없음</div>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}

export interface EventFiltersProps {
  value: EvtFilter
  onChange: (next: EvtFilter) => void
  plcs: readonly PickOption[]
  catalog: EvtCatalog | null
}

export function EventFilters({ value, onChange, plcs, catalog }: EventFiltersProps) {
  // 글자 칸은 초안을 들고 있다가 멈추면 반영한다(한 글자마다 조회하지 않는다).
  const [code, setCode] = useState(value.code)
  const [ctx, setCtx] = useState(value.ctx)
  const [q, setQ] = useState(value.q)
  const [rangeOpen, setRangeOpen] = useState(false)
  const [from, setFrom] = useState('')
  const [to, setTo] = useState('')

  useEffect(() => {
    setCode(value.code)
    setCtx(value.ctx)
    setQ(value.q)
  }, [value.code, value.ctx, value.q])

  useEffect(() => {
    if (code === value.code && ctx === value.ctx && q === value.q) return
    const t = setTimeout(() => onChange({ ...value, code, ctx, q }), TEXT_DEBOUNCE_MS)
    return () => clearTimeout(t)
  }, [code, ctx, q, value, onChange])

  const suggestions = useMemo(() => codeSuggestions(code, catalog?.events ?? []), [code, catalog])
  const cats = useMemo(
    () => (catalog?.cats ?? []).map((c) => ({ id: c.name, label: c.name, hint: String(c.id) })),
    [catalog],
  )
  const ctxBad = ctx.trim() !== '' && parseCtx(ctx) === null
  const n = activeCount(value)

  function pickRange(v: string) {
    if (v === 'custom') {
      setFrom(msToLocalInput(value.from ?? Date.now() - 3_600_000))
      setTo(msToLocalInput(value.to))
      setRangeOpen(true)
      return
    }
    onChange({ ...value, range: v as RangePreset, from: null, to: null })
  }

  const fromMs = localInputToMs(from)
  const toMs = localInputToMs(to)
  const rangeWhy =
    fromMs === null && toMs === null
      ? '시작이나 끝 중 하나는 있어야 합니다'
      : (from && fromMs === null) || (to && toMs === null)
        ? '시각 형식이 잘못됐습니다'
        : fromMs !== null && toMs !== null && fromMs >= toMs
          ? '시작이 끝보다 앞이어야 합니다'
          : undefined

  return (
    <div className="flex flex-wrap items-center gap-1.5 py-1" data-testid="evt-filters">
      <EvtMultiPick
        label="PLC"
        options={plcs}
        value={value.plcs}
        onChange={(plcs) => onChange({ ...value, plcs })}
        testid="evt-f-plc"
      />
      <EvtMultiPick
        label="Type"
        options={TYPE_OPTIONS}
        value={value.types}
        onChange={(types) => onChange({ ...value, types })}
        testid="evt-f-type"
      />
      <EvtMultiPick
        label="Cat"
        options={cats}
        value={value.cats}
        onChange={(cats) => onChange({ ...value, cats })}
        testid="evt-f-cat"
      />
      <Select
        dense
        aria-label="MinLevel"
        title="MinLevel — 이 레벨 이상"
        value={value.minLvl === null ? '' : String(value.minLvl)}
        onValueChange={(v) => onChange({ ...value, minLvl: v === '' ? null : Number(v) })}
        data-testid="evt-f-lvl"
      >
        <option value="">레벨 전체</option>
        {(catalog?.levels ?? []).map((l) => (
          <option key={l.id} value={l.id}>
            ≥ {l.name}
          </option>
        ))}
      </Select>
      <Input
        dense
        mono
        className="w-36"
        aria-label="code"
        placeholder="code / NAME"
        title="코드 번호, 이벤트 이름, ErrorList 코드(F3119), 쉼표로 여럿"
        list={CODE_LIST_ID}
        value={code}
        onValueChange={setCode}
        data-testid="evt-f-code"
      />
      <datalist id={CODE_LIST_ID}>
        {suggestions.map((s) => (
          <option key={s} value={s} />
        ))}
      </datalist>
      <Input
        dense
        mono
        className="w-20"
        aria-label="WorkId"
        placeholder="WorkId"
        inputMode="numeric"
        title={
          ctxBad ? 'WorkId 는 0 이상의 정수입니다 — 조건에서 빠집니다' : 'Task WorkId (이벤트 ctx)'
        }
        aria-invalid={ctxBad || undefined}
        value={ctx}
        onValueChange={setCtx}
        data-testid="evt-f-ctx"
      />
      <Select
        dense
        aria-label="기간"
        value={value.range}
        onValueChange={pickRange}
        data-testid="evt-f-range"
      >
        {RANGE_PRESETS.map((p) => (
          <option key={p.id} value={p.id}>
            {p.label}
          </option>
        ))}
        <option value="custom">
          {value.range === 'custom' ? rangeLabel(value) : '직접 지정…'}
        </option>
      </Select>
      <Input
        dense
        className="w-40"
        aria-label="검색"
        placeholder="검색"
        value={q}
        onValueChange={setQ}
        data-testid="evt-f-q"
      />
      {n > 0 ? (
        <Button
          size="sm"
          intent="ghost"
          icon={<X size={12} />}
          title="조건 전부 해제"
          onClick={() => onChange(EMPTY_FILTER)}
          data-testid="evt-f-reset"
        >
          초기화
        </Button>
      ) : null}

      <FormDialog
        open={rangeOpen}
        onOpenChange={setRangeOpen}
        title="기간 직접 지정"
        size="sm"
        submitLabel="적용"
        disabledReason={rangeWhy}
        onSubmit={() => {
          onChange({ ...value, range: 'custom', from: fromMs, to: toMs })
          setRangeOpen(false)
        }}
        testid="evt-range"
      >
        <Input
          type="datetime-local"
          step={1}
          label="from"
          value={from}
          onValueChange={setFrom}
          hint="비우면 처음부터"
        />
        <Input
          type="datetime-local"
          step={1}
          label="to"
          value={to}
          onValueChange={setTo}
          hint="비우면 지금까지"
        />
      </FormDialog>
    </div>
  )
}
