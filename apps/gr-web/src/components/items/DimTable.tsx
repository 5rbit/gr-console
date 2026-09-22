// 측정 반영 표 — 한 줄 = 품목 하나의 필드 하나. 품목 상세 Measured 탭(그 품목의 세 줄)과 일괄 검토(모든 품목)가
// 같은 표를 쓴다.
//
// 열: ☐ · Code · Name · Field · Current · Measured · Δ · n · Spread · 신뢰도 · State · 적용/↺.
// 색은 **State 배지와 Δ 글자에만** 쓴다(초록 적용 가능 · 파랑 새 값 · 회색 표본 부족 · 주황 흔들림 · 빨강 차이 큼,
// 반영된 줄은 초록 "반영됨"). 줄 순서는 사람이 볼 것부터(차이 큼 → 흔들림 → 표본 부족 → 새 값 → 적용 가능 →
// 반영됨). 줄을 펼치면 그 필드의 측정 표본(기록 · 시각 · 셀 · 값, 중앙값에서 벗어난 값은 경고색).
import { Check, Undo2 } from 'lucide-react'
import {
  DIM_LABEL,
  DIM_SOURCE,
  MIN_SAMPLES,
  TONE_LABEL,
  applicable,
  confidence,
  dimTone,
  passFilter,
  signed,
  type DimChange,
  type DimSample,
  type DimSuggestion,
  type DimTone,
  type ItemDims,
  type ReviewFilter,
} from '../../lib/items/dims'
import { pos } from '../../lib/meas/format'
import { Button } from '../../lib/ui/Button'
import { DataTable } from '../../lib/ui/DataTable'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import type { Column } from '../../lib/ui/table'
import { cn } from '../../lib/utils'

/** 톤 → 배지 상태 · 글자색. 색은 뜻으로만 부른다(`tokens.css`). */
export const DIM_TONE_CLASS: Record<DimTone, { badge: string; text: string; dot: string }> = {
  ok: { badge: 'ok', text: 'text-ok-fg', dot: 'bg-ok' },
  info: { badge: 'info', text: 'text-info-fg', dot: 'bg-info' },
  thin: { badge: 'neutral', text: 'text-content-secondary', dot: 'bg-content-muted' },
  warn: { badge: 'warn', text: 'text-warn-fg', dot: 'bg-warn' },
  fault: { badge: 'fault', text: 'text-fault-fg', dot: 'bg-fault' },
  muted: { badge: 'neutral', text: 'text-content-faint', dot: 'bg-content-faint' },
}

/** 표 한 줄. `live` = 이 필드에 지금 걸려 있는 측정 반영(되돌릴 수 있는 것). */
export interface DimRow {
  key: string
  code: number
  name: string
  s: DimSuggestion
  live: DimChange | null
}

export const rowKey = (code: number, field: string) => `${code}:${field}`

const ORDER: Record<DimTone, number> = { fault: 0, warn: 1, thin: 2, info: 3, ok: 4, muted: 5 }

/**
 * 표에 올릴 줄 — 적용할 제안이 있는 필드와(필터를 지나는 것만) 반영된 필드(전체 보기에서만).
 * `liveOf` 는 품목·필드의 지금 걸린 측정 반영.
 */
export function buildRows(
  items: readonly ItemDims[],
  filter: ReviewFilter,
  liveOf: (code: number, s: DimSuggestion) => DimChange | null,
): DimRow[] {
  const out: DimRow[] = []
  for (const d of items)
    for (const s of d.fields) {
      const live = liveOf(d.code, s)
      const show = passFilter(s, filter) || (filter === 'all' && live !== null && !applicable(s))
      if (show) out.push({ key: rowKey(d.code, s.field), code: d.code, name: d.name, s, live })
    }
  const rank = (r: DimRow) => (r.live && !applicable(r.s) ? 6 : ORDER[dimTone(r.s)])
  return out.sort((a, b) => rank(a) - rank(b) || a.code - b.code || a.key.localeCompare(b.key))
}

/** 신뢰도 점 다섯 — 채운 수 = `confidence`. */
export function ConfidenceDots({ value, tone }: { value: number; tone: DimTone }) {
  return (
    <span className="inline-flex items-center gap-0.5" aria-label={`신뢰도 ${value}/5`}>
      {[0, 1, 2, 3, 4].map((i) => (
        <span
          key={i}
          className={cn(
            'h-1.5 w-1.5 rounded-full',
            i < value ? DIM_TONE_CLASS[tone].dot : 'bg-line-default',
          )}
        />
      ))}
    </span>
  )
}

export interface DimTableProps {
  rows: readonly DimRow[]
  window: number
  picked: ReadonlySet<string>
  flash?: ReadonlySet<string>
  disabled?: boolean
  onToggle: (r: DimRow) => void
  onApply: (r: DimRow) => void
  onRevert: (r: DimRow, c: DimChange) => void
  /** 한 품목만 보일 때(상세 탭) — Code · Name 열을 뺀다. */
  single?: boolean
  /** 강조할 줄(표의 제안 칩에서 온 필드) — 행 선택 표시로 남는다. */
  selected?: string | null
  testid?: string
}

export function DimTable({
  rows,
  window,
  picked,
  flash,
  disabled,
  onToggle,
  onApply,
  onRevert,
  single,
  selected,
  testid = 'dims-table',
}: DimTableProps) {
  const done = (r: DimRow) => !!r.live && !applicable(r.s)
  const columns: Column<DimRow>[] = [
    {
      key: 'use',
      label: '',
      priority: 1,
      cell: (r) =>
        done(r) ? null : (
          <input
            type="checkbox"
            checked={picked.has(r.key)}
            disabled={disabled}
            aria-label={`${r.code} ${DIM_LABEL[r.s.field]} 선택`}
            onClick={(e) => e.stopPropagation()}
            onChange={() => onToggle(r)}
            data-testid={`dims-pick-${r.key}`}
          />
        ),
    },
    ...(single
      ? []
      : [
          {
            key: 'code',
            label: 'Code',
            priority: 1 as const,
            numeric: true,
            class: 'font-mono',
            get: (r: DimRow) => r.code,
          },
          { key: 'name', label: 'Name', priority: 2 as const, get: (r: DimRow) => r.name },
        ]),
    {
      key: 'field',
      label: 'Field',
      priority: 1,
      get: (r) => DIM_LABEL[r.s.field],
      cell: (r) => <span title={DIM_SOURCE[r.s.field]}>{DIM_LABEL[r.s.field]}</span>,
    },
    {
      key: 'cur',
      label: 'Current (mm)',
      priority: 1,
      numeric: true,
      get: (r) => (done(r) ? r.live!.before : r.s.current),
      cell: (r) => {
        const v = done(r) ? r.live!.before : r.s.current
        return v === 0 ? <span className="text-content-faint">—</span> : pos(v)
      },
    },
    {
      key: 'sug',
      label: 'Measured (mm)',
      priority: 1,
      numeric: true,
      get: (r) => r.s.suggested ?? 0,
      cell: (r) =>
        r.s.suggested === null ? (
          <span className="text-content-faint">—</span>
        ) : (
          <span className="font-medium">{pos(done(r) ? r.live!.after : r.s.suggested)}</span>
        ),
    },
    {
      key: 'delta',
      label: 'Δ (mm)',
      priority: 1,
      numeric: true,
      get: (r) => Math.abs(r.s.delta ?? 0),
      cell: (r) => {
        if (done(r))
          return <span className="text-ok-fg">{signed(r.live!.after - r.live!.before)}</span>
        const t = dimTone(r.s)
        return (
          <span className={DIM_TONE_CLASS[t].text}>
            {r.s.current === 0 ? 'new' : signed(r.s.delta)}
          </span>
        )
      },
    },
    // 좁은 상세 패널(한 품목)에서는 n · Spread 를 빼고 신뢰도 툴팁 · 펼친 줄로 보인다
    ...(single
      ? []
      : ([
          {
            key: 'n',
            label: 'n',
            priority: 2,
            numeric: true,
            get: (r) => r.s.n,
            cell: (r) => (
              <span
                className={r.s.n < MIN_SAMPLES ? 'text-content-faint' : undefined}
                title={`최근 ${window}개 창 중 ${r.s.n}개 · ${MIN_SAMPLES}개 미만이면 표본 부족`}
              >
                {r.s.n}
              </span>
            ),
          },
          {
            key: 'spread',
            label: 'Spread (mm)',
            priority: 2,
            numeric: true,
            get: (r) => r.s.spread ?? 0,
            cell: (r) =>
              r.s.spread === null ? (
                <span className="text-content-faint">—</span>
              ) : (
                <span
                  className={r.s.unstable ? 'text-warn-fg' : undefined}
                  title={`표본 편차(최대 − 최소) · 한도 ${r.s.spread_limit}`}
                >
                  {pos(r.s.spread)}
                  <span className="text-content-faint"> / {r.s.spread_limit}</span>
                </span>
              ),
          },
        ] as Column<DimRow>[])),
    {
      key: 'conf',
      label: '신뢰도',
      priority: 2,
      get: (r) => confidence(r.s, window),
      cell: (r) => (
        <span
          title={`표본 ${r.s.n} / 창 ${window}${r.s.n < MIN_SAMPLES ? ` (${MIN_SAMPLES}개 미만)` : ''} · 편차 ${r.s.spread === null ? '—' : pos(r.s.spread)} / 한도 ${r.s.spread_limit}`}
        >
          <ConfidenceDots value={confidence(r.s, window)} tone={dimTone(r.s)} />
        </span>
      ),
    },
    {
      key: 'state',
      label: 'State',
      priority: 1,
      get: (r) => (done(r) ? 6 : ORDER[dimTone(r.s)]),
      // 방금 바뀐 줄은 이 칸이 한 번 반짝인다(표에 행 클래스 자리가 없다)
      cell: (r) => (
        <span className={cn('inline-block rounded', flash?.has(r.key) && 'ds-flash')}>
          {done(r) ? (
            <StatusBadge status="ok" title={`변경 #${r.live!.id} · ${r.live!.at.slice(5, 16)}`}>
              반영됨
            </StatusBadge>
          ) : (
            <StatusBadge status={DIM_TONE_CLASS[dimTone(r.s)].badge} title={reason(r.s)}>
              {TONE_LABEL[dimTone(r.s)]}
            </StatusBadge>
          )}
        </span>
      ),
    },
  ]
  return (
    <DataTable
      rows={[...rows]}
      columns={columns}
      rowKey={(r) => r.key}
      selected={selected ?? null}
      density="compact"
      stickyHeader
      empty="적용할 측정 제안 없음"
      emptyHint="MeasureItem 으로 재면 제안이 생깁니다."
      rowDetail={(r) => <SampleList s={r.s} />}
      actions={(r) =>
        done(r) ? (
          <Button
            size={single ? 'icon-sm' : 'sm'}
            intent="ghost"
            icon={<Undo2 className="h-3.5 w-3.5" />}
            disabled={disabled}
            title={`되돌리기 — ${r.live!.before === 0 ? '미입력' : pos(r.live!.before)} 로`}
            onClick={() => onRevert(r, r.live!)}
            aria-label={`${DIM_LABEL[r.s.field]} 되돌리기`}
            data-testid={`dims-revert-${r.key}`}
          >
            {single ? null : '되돌리기'}
          </Button>
        ) : applicable(r.s) ? (
          <Button
            size={single ? 'icon-sm' : 'sm'}
            intent="ghost"
            icon={<Check className="h-3.5 w-3.5" />}
            disabled={disabled}
            title={`${DIM_LABEL[r.s.field]} 만 적용${reason(r.s) ? ` — ${reason(r.s)}` : ''}`}
            onClick={() => onApply(r)}
            aria-label={`${DIM_LABEL[r.s.field]} 적용`}
            data-testid={`dims-apply-${r.key}`}
          >
            {single ? null : '적용'}
          </Button>
        ) : null
      }
      testid={testid}
    />
  )
}

/** 톤이 확인을 요구하는 이유(없으면 빈 문자열). */
export function reason(s: DimSuggestion): string {
  const t = dimTone(s)
  if (t === 'fault')
    return `등록값과 ${Math.abs(s.delta ?? 0).toFixed(1)} mm 다름(한도 ${s.outlier_limit}) — 재고 품목이 맞는지 확인`
  if (t === 'thin') return `표본 ${s.n}개 — ${MIN_SAMPLES}개 이상이면 바로 적용 대상`
  if (t === 'warn') return `표본 편차 ${pos(s.spread)} > 한도 ${s.spread_limit}`
  return ''
}

/** 펼친 줄 — 그 필드의 측정 표본. 중앙값에서 한도의 절반 넘게 벗어난 값은 경고색. */
function SampleList({ s }: { s: DimSuggestion }) {
  const cols: Column<DimSample>[] = [
    {
      key: 'rec',
      label: 'Record',
      priority: 1,
      class: 'font-mono',
      get: (x) => `${x.plc}#${x.seq}`,
    },
    { key: 'at', label: 'At', priority: 1, get: (x) => x.at.slice(5, 19) },
    { key: 'cell', label: 'Cell', priority: 2, numeric: true, get: (x) => x.cell_id },
    {
      key: 'v',
      label: `${DIM_LABEL[s.field]} (mm)`,
      priority: 1,
      numeric: true,
      get: (x) => x.value,
      cell: (x) => {
        const off = s.suggested !== null && Math.abs(x.value - s.suggested) > s.spread_limit / 2
        return <span className={off ? 'text-warn-fg' : undefined}>{pos(x.value)}</span>
      },
    },
  ]
  return (
    <div className="flex flex-col gap-1">
      <span className="text-2xs text-content-muted">{DIM_SOURCE[s.field]}</span>
      <DataTable
        rows={s.samples}
        columns={cols}
        rowKey={(x) => `${x.plc}#${x.seq}`}
        density="compact"
        emptyDense
        empty="표본 없음"
      />
    </div>
  )
}
