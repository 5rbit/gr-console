// 화물 규격 표 안의 측정 제안 — 등록값 열(InnerDia · Height · UpperBead) 바로 옆에 좁은 "측정" 열을 두고, 표 위 띠에서
// 선택 · 전체 · 되돌리기를 한다.
//
// 측정 열은 **색 글자 하나**다(화살표 · 바탕 없음): 글자색 = 결정(`dimTone`) — 초록 적용 가능 · 파랑 새 값 · 흐린
// 회색 표본 부족(윗첨자 = 표본 수) · 주황 흔들림 · 빨강 차이 큼. 등록값 열은 늘 검은 글자라 둘이 섞여 읽히지 않는다.
//   - 측정 값을 누르면 옆 상세 패널이 Measured 탭으로 열리고 그 필드 줄이 강조된다(표본 · 신뢰도 · 적용 · 되돌리기)
//   - Ctrl(⌘)+누르면 선택(굵은 테두리) — 띠의 "선택 적용" 으로 여러 개를 한 번에
//   - 반영된 필드는 측정 열에 초록 ↺ 하나 — 누르면 그 필드만 되돌린다
//   - 띠: 선택 적용 · 바로 적용 전체(초록 · 파랑만) · 방금 적용 되돌리기(마지막 묶음 전체) · 제안 있는 행만 · 창 · 검토 창
// 적용 · 되돌리기는 기존 API(`dimsApi.applyBulk` · `revert`) 그대로다 — 값은 서버가 그 순간의 제안으로 정한다.
import { useCallback, useEffect, useMemo, useState } from 'react'
import { Undo2 } from 'lucide-react'
import {
  DIM_LABEL,
  MIN_SAMPLES,
  TONE_LABEL,
  applicable,
  confidence,
  dimTone,
  dimsApi,
  liveChange,
  safeTone,
  signed,
  type DimChange,
  type DimField,
  type DimSuggestion,
  type DimTone,
  type ItemDims,
} from '../../lib/items/dims'
import { pos } from '../../lib/meas/format'
import { cn } from '../../lib/utils'
import { reason } from './DimTable'

/** 글자색 · 점 색 — 뜻으로만 부른다(`tokens.css`). */
const TEXT: Record<DimTone, string> = {
  ok: 'text-ok-fg',
  info: 'text-info-fg',
  thin: 'text-content-muted',
  warn: 'text-warn-fg',
  fault: 'text-fault-fg',
  muted: 'text-content-faint',
}
const DOT: Record<DimTone, string> = {
  ok: 'bg-ok',
  info: 'bg-info',
  thin: 'bg-content-muted',
  warn: 'bg-warn',
  fault: 'bg-fault',
  muted: 'bg-content-faint',
}

export const dimKey = (code: number, f: DimField | string) => `${code}:${f}`

/** 표에 겹칠 측정 제안 — 목록(`only_changes=false`) · 전체 변경 이력. `version` 이 바뀌면(표를 다시 받으면) 다시 읽는다. */
export function useDimsOverlay(on: boolean, window: number, version: unknown) {
  const [list, setList] = useState<ItemDims[]>([])
  const [changes, setChanges] = useState<DimChange[]>([])
  const [err, setErr] = useState<string | null>(null)
  const reload = useCallback(async () => {
    if (!on) return
    try {
      const [l, c] = await Promise.all([dimsApi.suggest(window, false), dimsApi.allChanges()])
      setList(l)
      setChanges(c)
      setErr(null)
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e))
    }
  }, [on, window])
  useEffect(() => {
    void reload()
  }, [reload, version])
  const byCode = useMemo(() => new Map(list.map((d) => [d.code, d])), [list])
  const sugOf = useCallback(
    (code: number, f: DimField) => byCode.get(code)?.fields.find((s) => s.field === f) ?? null,
    [byCode],
  )
  const liveOf = useCallback(
    (code: number, f: DimField) => {
      const s = sugOf(code, f)
      return s
        ? liveChange(
            changes.filter((c) => c.code === code),
            f,
            s.current,
          )
        : null
    },
    [sugOf, changes],
  )
  /** 적용할 제안 전부(품목 · 필드). */
  const all = useMemo(
    () =>
      list.flatMap((d) =>
        d.fields.filter(applicable).map((s) => ({ code: d.code, name: d.name, s })),
      ),
    [list],
  )
  return { list, changes, err, reload, sugOf, liveOf, all, byCode }
}

export type DimsOverlay = ReturnType<typeof useDimsOverlay>

export interface MeasuredCellProps {
  code: number
  s: DimSuggestion | null
  live: DimChange | null
  window: number
  picked: boolean
  flash: boolean
  disabled?: boolean
  /** 누름 — `multi` = Ctrl/⌘ 를 누르고 눌렀다(여러 개 고르기). 아니면 상세 패널로 자세히. */
  onPick: (multi: boolean) => void
  onRevert: (c: DimChange) => void
}

/**
 * 측정 열(등록값 열 옆 "측정")의 칸 — 등록값 열 바로 옆에 **색 글자 하나**만 둔다(화살표 · 바탕 없음).
 * 글자색 = 결정, 표본 부족이면 흐린 글자 + 윗첨자 표본 수, 반영된 필드는 초록 ↺ 하나. 할 일 없으면 빈 칸.
 */
export function MeasuredCell({
  code,
  s,
  live,
  window,
  picked,
  flash,
  disabled,
  onPick,
  onRevert,
}: MeasuredCellProps) {
  if (s && applicable(s)) {
    const t = dimTone(s)
    return (
      <button
        type="button"
        className={cn(
          'rounded-sm px-0.5 font-mono tabular-nums outline-offset-1 hover:underline',
          TEXT[t],
          picked && 'outline-2 outline-focus',
          flash && 'ds-flash',
        )}
        title={[
          `${DIM_LABEL[s.field]} 측정 제안 — ${TONE_LABEL[t]}`,
          `등록 ${s.current === 0 ? '미입력' : pos(s.current)} · 제안 ${pos(s.suggested)} (${s.current === 0 ? 'new' : signed(s.delta)})`,
          `표본 ${s.n} / 창 ${window} · 편차 ${s.spread === null ? '—' : pos(s.spread)} / ${s.spread_limit} · 신뢰도 ${confidence(s, window)}/5`,
          reason(s),
          '누르면 옆 패널에 자세히 · Ctrl+누르면 여러 개 선택',
        ]
          .filter(Boolean)
          .join('\n')}
        aria-pressed={picked}
        aria-label={`${code} ${DIM_LABEL[s.field]} 제안 ${pos(s.suggested)}`}
        disabled={disabled}
        onClick={(e) => {
          e.stopPropagation()
          onPick(e.ctrlKey || e.metaKey)
        }}
        data-testid={`dim-chip-${dimKey(code, s.field)}`}
        data-tone={t}
      >
        {pos(s.suggested)}
        {t === 'thin' ? <sup className="text-content-faint">{s.n}</sup> : null}
      </button>
    )
  }
  if (live)
    return (
      <button
        type="button"
        className={cn(
          'rounded-sm p-0.5 text-ok-fg hover:bg-surface-hover disabled:opacity-50',
          flash && 'ds-flash',
        )}
        title={`측정 반영됨 ${live.before === 0 ? '미입력' : pos(live.before)} → ${pos(live.after)} (${live.at.slice(5, 16)}) — 누르면 되돌림`}
        aria-label={`${code} ${live.field} 되돌리기`}
        disabled={disabled}
        onClick={(e) => {
          e.stopPropagation()
          onRevert(live)
        }}
        data-testid={`dim-live-${dimKey(code, live.field)}`}
      >
        <Undo2 className="h-3 w-3" />
      </button>
    )
  return null
}

/** 품목 한 줄의 제안 요약 — 톤 점 + 개수(짧게). 뜻은 툴팁. */
export function DimsSummary({ d }: { d: ItemDims | undefined }) {
  if (!d) return null
  const tones = d.fields.filter(applicable).map(dimTone)
  if (!tones.length) return null
  const order: DimTone[] = ['fault', 'warn', 'thin', 'info', 'ok']
  return (
    <span className="inline-flex items-center gap-1 text-2xs tabular-nums">
      {order
        .filter((t) => tones.includes(t))
        .map((t) => (
          <span
            key={t}
            className={cn('inline-flex items-center gap-0.5', TEXT[t])}
            title={`${TONE_LABEL[t]} ${tones.filter((x) => x === t).length}`}
          >
            <span className={cn('h-1.5 w-1.5 rounded-full', DOT[t])} />
            {tones.filter((x) => x === t).length}
          </span>
        ))}
    </span>
  )
}

/** 적용 확인 창에 보일 한 줄. */
export function confirmLine(code: number, s: DimSuggestion): string {
  const t = dimTone(s)
  return `#${code} ${DIM_LABEL[s.field]} ${s.current === 0 ? '미입력' : pos(s.current)} → ${pos(s.suggested)}${safeTone(t) ? '' : ` · ${TONE_LABEL[t]}${t === 'thin' ? ` (표본 ${s.n} < ${MIN_SAMPLES})` : ''}`}`
}
