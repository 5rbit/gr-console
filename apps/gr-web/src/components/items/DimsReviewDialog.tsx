// 측정 반영 일괄 검토 — 한 줄 = 품목 하나의 필드 하나인 표(`DimTable`)에서 줄을 골라 한 번에 적용한다.
//
// State 배지가 결정이다(초록 적용 가능 · 파랑 새 값 · 회색 표본 부족 · 주황 흔들림 · 빨강 차이 큼). 처음엔
// 초록 · 파랑 줄만 골라져 있다. 회색 · 주황 · 빨강 줄을 고르면 적용 전에 확인 창이 그 줄들을 다시 보인다.
// 줄마다 "적용" 단추도 있다(바로 적용해도 되는 줄은 곧장, 나머지는 확인 창).
// 품목마다 따로 적용되므로(서버 `apply-bulk`) 한 품목이 실패해도 나머지는 적용되고 결과를 품목별로 알린다.
// 적용한 줄은 초록 "반영됨" 과 되돌리기로 남는다 — 변경 이력에서 읽으므로 창을 다시 열어도 남는다.
import { useCallback, useEffect, useMemo, useState } from 'react'
import { CheckCheck, RefreshCw } from 'lucide-react'
import {
  MIN_SAMPLES,
  TONE_LABEL,
  WINDOWS,
  applicable,
  changeLine,
  defaultPick,
  dimTone,
  dimsApi,
  fieldLabel,
  liveChange,
  passFilter,
  safeTone,
  type DimChange,
  type DimField,
  type DimSuggestion,
  type ItemDims,
  type ReviewFilter,
} from '../../lib/items/dims'
import { pos } from '../../lib/meas/format'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { Dialog } from '../../lib/ui/Dialog'
import { HelpTip } from '../../lib/ui/HelpTip'
import { Segmented } from '../../lib/ui/Segmented'
import { Select } from '../../lib/ui/Select'
import { toast } from '../../lib/ui/toast'
import {
  DIM_TONE_CLASS,
  DimTable,
  buildRows,
  reason,
  rowKey as cellKey,
  type DimRow,
} from './DimTable'

const errMsg = (e: unknown) => (e instanceof Error ? e.message : String(e))

/** 측정 반영 표 사용법 — 도움말(?) 한 곳(상세 탭도 쓴다). */
export const DIMS_HELP = `State = 결정. 초록 적용 가능 · 파랑 새 값(등록 0) · 회색 표본 부족(${MIN_SAMPLES}개 미만) · 주황 흔들림(표본 편차 > 한도) · 빨강 차이 큼(다른 타이어를 잰 의심). 신뢰도 ●●●○○ = 표본 수 × 편차. 체크한 줄을 한 번에, 또는 줄의 적용 단추로 하나씩. 줄을 펼치면 측정 표본(중앙값에서 벗어난 값은 경고색). 값은 MeasureItem(DONE) 기록의 중앙값이고 높이 둘은 1 단 측정만 — PLC 로 가는 품목 값이라 다음 명령부터 G · Z 가 바뀝니다.`

export interface DimsReviewDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** 적용 뒤 — 부모가 목록을 다시 받는다. */
  onApplied: () => Promise<void>
}

export function DimsReviewDialog({ open, onOpenChange, onApplied }: DimsReviewDialogProps) {
  const [win, setWin] = useState(5)
  const [list, setList] = useState<ItemDims[]>([])
  const [changes, setChanges] = useState<DimChange[]>([])
  const [loading, setLoading] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const [pick, setPick] = useState<Set<string>>(new Set())
  const [busy, setBusy] = useState(false)
  const [filter, setFilter] = useState<ReviewFilter>('all')
  const [flash, setFlash] = useState<Set<string>>(new Set())
  const [confirm, setConfirm] = useState<{ code: number; s: DimSuggestion }[] | null>(null)

  /** 목록 · 이력을 다시 받는다. `repick` 이면 선택을 기본(초록 · 파랑)으로 되돌린다. */
  const reload = useCallback(
    async (repick: boolean) => {
      setLoading(true)
      try {
        const [l, c] = await Promise.all([dimsApi.suggest(win, false), dimsApi.allChanges()])
        setList(l)
        setChanges(c)
        if (repick)
          setPick(new Set(l.flatMap((d) => [...defaultPick(d)].map((f) => cellKey(d.code, f)))))
        else
          setPick((p) => {
            // 적용되어 "같음" 이 된 칸은 선택에서 뺀다
            const ok = new Set(
              l.flatMap((d) => d.fields.filter(applicable).map((s) => cellKey(d.code, s.field))),
            )
            return new Set([...p].filter((k) => ok.has(k)))
          })
        setErr(null)
      } catch (e) {
        setErr(errMsg(e))
      } finally {
        setLoading(false)
      }
    },
    [win],
  )
  useEffect(() => {
    if (open) void reload(true)
  }, [open, reload])
  useEffect(() => {
    if (!flash.size) return
    const t = setTimeout(() => setFlash(new Set()), 1400)
    return () => clearTimeout(t)
  }, [flash])

  const rows = useMemo(
    () =>
      buildRows(list, filter, (code, s) =>
        liveChange(
          changes.filter((c) => c.code === code),
          s.field,
          s.current,
        ),
      ),
    [list, filter, changes],
  )
  const count = (f: ReviewFilter) =>
    list.reduce((n, d) => n + d.fields.filter((s) => passFilter(s, f)).length, 0)
  const chosen = list.flatMap((d) =>
    d.fields
      .filter((s) => applicable(s) && pick.has(cellKey(d.code, s.field)))
      .map((s) => ({ code: d.code, s })),
  )
  const risky = chosen.filter((r) => !safeTone(dimTone(r.s)))
  const safeAll = list.flatMap((d) =>
    d.fields.filter((s) => safeTone(dimTone(s))).map((s) => cellKey(d.code, s.field)),
  )

  const toggle = (r: DimRow) =>
    setPick((p) => {
      const n = new Set(p)
      if (n.has(r.key)) n.delete(r.key)
      else n.add(r.key)
      return n
    })
  /** 줄의 적용 단추 — 바로 적용해도 되는 줄은 곧장, 나머지는 확인 창. */
  const applyOne = (r: DimRow) =>
    safeTone(dimTone(r.s))
      ? void apply([{ code: r.code, s: r.s }])
      : setConfirm([{ code: r.code, s: r.s }])

  async function apply(sel: { code: number; s: DimSuggestion }[]) {
    const by = new Map<number, DimField[]>()
    for (const r of sel) by.set(r.code, [...(by.get(r.code) ?? []), r.s.field])
    setBusy(true)
    try {
      const res = await dimsApi.applyBulk(
        [...by].map(([code, fields]) => ({ code, fields })),
        win,
      )
      const lit = new Set(
        res.results.flatMap((r) => (r.applied ?? []).map((a) => cellKey(r.code, a.field))),
      )
      setFlash(lit)
      if (res.failed) {
        const first = res.results.find((r) => !r.ok)
        toast.error(
          `측정 반영 ${lit.size}줄 적용 · ${res.failed}개 품목 실패 (#${first?.code} ${first?.error ?? ''})`,
        )
      } else toast.ok(`측정 반영 ${lit.size}줄 적용 (${by.size}개 품목)`)
      await onApplied()
      await reload(false)
    } catch (e) {
      toast.error(errMsg(e))
    } finally {
      setBusy(false)
    }
  }

  async function revert(code: number, c: DimChange) {
    setBusy(true)
    try {
      await dimsApi.revert(code, c.id)
      setFlash(new Set([cellKey(code, c.field as DimField)]))
      toast.ok(
        `#${code} ${fieldLabel(c.field)} 되돌림 — ${c.before === 0 ? '미입력' : pos(c.before)}`,
      )
      await onApplied()
      await reload(false)
    } catch (e) {
      toast.error(errMsg(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="측정 반영 검토"
      meta={`${new Set(rows.map((r) => r.code)).size}개 품목 · 제안 ${count('all')} · 선택 ${chosen.length}`}
      size="xl"
      testid="dims-review"
      footer={
        <>
          <Button
            size="sm"
            intent="ghost"
            disabled={busy || safeAll.length === 0}
            title={`${TONE_LABEL.ok} · ${TONE_LABEL.info} 줄만 선택`}
            onClick={() => setPick(new Set(safeAll))}
            data-testid="dims-pick-safe"
          >
            바로 적용 줄만 선택 ({safeAll.length})
          </Button>
          <Button
            size="sm"
            intent="ghost"
            disabled={busy || pick.size === 0}
            onClick={() => setPick(new Set())}
          >
            선택 해제
          </Button>
          <span className="flex-1" />
          <Button size="sm" intent="ghost" onClick={() => onOpenChange(false)}>
            닫기
          </Button>
          <Button
            size="sm"
            intent={risky.length ? 'outline' : 'primary'}
            icon={<CheckCheck className="h-3.5 w-3.5" />}
            disabled={chosen.length === 0 || busy}
            loading={busy}
            title={
              chosen.length === 0
                ? '적용할 줄을 체크하세요'
                : risky.length
                  ? `확인이 필요한 줄 ${risky.length}개 포함 — 확인 후 적용`
                  : undefined
            }
            onClick={() => (risky.length ? setConfirm(chosen) : void apply(chosen))}
            data-testid="dims-review-apply"
          >
            선택 {chosen.length}줄 적용
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-2">
        <div className="flex flex-wrap items-end gap-2">
          <Select dense label="Window" value={String(win)} onValueChange={(v) => setWin(Number(v))}>
            {WINDOWS.map((w) => (
              <option key={w} value={String(w)}>
                최근 {w}개 중앙값
              </option>
            ))}
          </Select>
          <Segmented<ReviewFilter>
            ariaLabel="보기"
            value={filter}
            onChange={setFilter}
            options={[
              { id: 'all', label: '전체', badge: count('all') || '' },
              {
                id: 'safe',
                label: '바로 적용',
                badge: count('safe') || '',
                title: `${TONE_LABEL.ok} · ${TONE_LABEL.info}`,
                testid: 'dims-filter-safe',
              },
              {
                id: 'check',
                label: '확인 필요',
                badge: count('check') || '',
                title: `${TONE_LABEL.fault} · ${TONE_LABEL.warn} · ${TONE_LABEL.thin}`,
                testid: 'dims-filter-check',
              },
            ]}
          />
          <Button
            size="sm"
            intent="ghost"
            icon={<RefreshCw className="h-3.5 w-3.5" />}
            loading={loading}
            onClick={() => void reload(true)}
          >
            다시 읽기
          </Button>
          <span className="pb-1.5">
            <HelpTip title="측정 반영" text={DIMS_HELP} />
          </span>
        </div>
        {err ? <div className="text-xs text-fault-fg">{err}</div> : null}
        <div className="max-h-96 overflow-y-auto">
          <DimTable
            rows={rows}
            window={win}
            picked={pick}
            flash={flash}
            disabled={busy}
            onToggle={toggle}
            onApply={applyOne}
            onRevert={(r, c) => void revert(r.code, c)}
            testid="dims-review-table"
          />
        </div>
      </div>

      <ConfirmDialog
        open={confirm !== null}
        onOpenChange={(o) => !o && setConfirm(null)}
        scope="single"
        title="측정 반영 — 확인이 필요한 줄 포함"
        confirmLabel="적용"
        danger
        onConfirm={() => confirm && void apply(confirm)}
      >
        <div className="flex flex-col gap-1 text-xs">
          {(confirm ?? []).map(({ code, s }) => {
            const t = dimTone(s)
            return (
              <div key={cellKey(code, s.field)} className="tabular-nums">
                #{code} {changeLine(s)}
                {!safeTone(t) ? (
                  <span className={DIM_TONE_CLASS[t].text}>
                    {' '}
                    · {TONE_LABEL[t]} — {reason(s)}
                  </span>
                ) : null}
              </div>
            )
          })}
          <div className="text-content-muted">
            PLC 로 보내는 품목 값이라 다음 명령부터 G · Z 가 바뀝니다. 줄의 되돌리기로 되돌릴 수
            있습니다.
          </div>
        </div>
      </ConfirmDialog>
    </Dialog>
  )
}
