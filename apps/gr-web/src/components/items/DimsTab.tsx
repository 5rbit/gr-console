// 품목 상세의 "Measured" 탭 — MeasureItem 기록으로 만든 치수 제안을 **칸마다 골라 적용**하고, 측정 기록과
// 바꾼 이력을 본다(`lib/items/dims.ts`, 백엔드 `registry/dims.rs`).
//
// 위 = 일괄 검토와 같은 표(`DimTable`)의 이 품목 세 줄: State 배지가 결정(초록 적용 가능 · 파랑 새 값 · 회색
// 표본 부족 · 주황 흔들림 · 빨강 차이 큼), 점이 신뢰도, 줄의 적용 단추 또는 체크 후 "선택 적용". 적용한 줄은
// 초록 "반영됨" 과 되돌리기로 남는다. 줄을 펼치면 측정 표본. 아래 = 변경 이력.
//
// 적용은 서버가 방금 계산한 제안값을 그대로 쓴다(화면이 값을 보내지 않는다). 저장하지 않은 편집이 있으면
// 적용을 막는다: 적용 뒤 상세가 새 값으로 다시 열리면서 그 편집이 사라지기 때문이다.
import { useCallback, useEffect, useState } from 'react'
import { CheckCheck, RefreshCw, Undo2 } from 'lucide-react'
import {
  TONE_LABEL,
  WINDOWS,
  applicable,
  changeLine,
  defaultPick,
  dimTone,
  dimsApi,
  fieldLabel,
  liveChange,
  safeTone,
  type DimChange,
  type DimField,
  type DimSuggestion,
  type ItemDims,
} from '../../lib/items/dims'
import { pos } from '../../lib/meas/format'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { FieldList } from '../../lib/ui/FieldList'
import { HelpTip } from '../../lib/ui/HelpTip'
import { Select } from '../../lib/ui/Select'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'
import { DIM_TONE_CLASS, DimTable, buildRows, reason, rowKey as cellKey } from './DimTable'
import { DIMS_HELP } from './DimsReviewDialog'

const errMsg = (e: unknown) => (e instanceof Error ? e.message : String(e))

export interface DimsTabProps {
  code: number
  /** 상세에 저장하지 않은 편집이 있다 — 적용을 막는다. */
  dirty: boolean
  /** 적용·되돌리기 뒤 — 부모가 목록을 다시 받는다(상세가 새 값으로 다시 열린다). */
  onApplied: () => Promise<void>
  /** 표의 제안 칩에서 온 필드 — 그 줄을 강조하고 한 번 반짝인다. */
  focus?: { field: DimField; n: number } | null
}

export function DimsTab({ code, dirty, onApplied, focus }: DimsTabProps) {
  const [win, setWin] = useState<number>(5)
  const [dims, setDims] = useState<ItemDims | null>(null)
  const [changes, setChanges] = useState<DimChange[]>([])
  const [err, setErr] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [pick, setPick] = useState<Set<DimField>>(new Set())
  const [confirm, setConfirm] = useState<DimSuggestion[] | null>(null)
  const [flash, setFlash] = useState<Set<string>>(new Set())
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const [d, c] = await Promise.all([dimsApi.item(code, win), dimsApi.changes(code)])
      setDims(d)
      setChanges(c)
      setPick(defaultPick(d))
      setErr(null)
    } catch (e) {
      setErr(errMsg(e))
    } finally {
      setLoading(false)
    }
  }, [code, win])
  useEffect(() => void load(), [load])
  useEffect(() => {
    if (!flash.size) return
    const t = setTimeout(() => setFlash(new Set()), 1400)
    return () => clearTimeout(t)
  }, [flash])

  // 표의 제안 칩 — 그 필드 줄을 반짝인다(강조는 아래 표의 selected 로 남는다)
  useEffect(() => {
    if (focus) setFlash(new Set([cellKey(code, focus.field)]))
  }, [focus, code])

  const fields = dims?.fields ?? []
  const chosen = fields.filter((s) => applicable(s) && pick.has(s.field))
  const risky = chosen.filter((s) => !safeTone(dimTone(s)))
  const safe = fields.filter((s) => safeTone(dimTone(s)))
  const blocked = dirty
    ? '저장하지 않은 편집이 있습니다 — 먼저 저장하거나 되돌리세요'
    : busy
      ? '처리 중입니다'
      : undefined

  async function apply(list: DimSuggestion[]) {
    setBusy(true)
    try {
      const r = await dimsApi.apply(
        code,
        list.map((s) => s.field),
        win,
      )
      toast.ok(
        `#${code} 측정 반영 ${r.changes.length}건 — ${list.map((s) => fieldLabel(s.field)).join(' · ')}`,
      )
      setFlash(new Set((r.applied ?? []).map((a) => cellKey(code, a.field))))
      await onApplied()
      await load()
    } catch (e) {
      toast.error(errMsg(e))
    } finally {
      setBusy(false)
    }
  }

  async function doRevert(c: DimChange) {
    setBusy(true)
    try {
      await dimsApi.revert(code, c.id)
      toast.ok(
        `#${code} ${fieldLabel(c.field)} 되돌림 — ${c.before === 0 ? '미입력' : pos(c.before)}`,
      )
      setFlash(new Set([cellKey(code, c.field as DimField)]))
      await onApplied()
      await load()
    } catch (e) {
      toast.error(errMsg(e))
    } finally {
      setBusy(false)
    }
  }

  const histCols: Column<DimChange>[] = [
    {
      key: 'at',
      label: 'At',
      get: (c) => c.at,
      cell: (c) => <span className="text-2xs">{c.at.slice(5, 19).replace('T', ' ')}</span>,
      priority: 1,
    },
    {
      key: 'field',
      label: 'Field',
      get: (c) => c.field,
      cell: (c) => fieldLabel(c.field),
      priority: 1,
    },
    {
      key: 'chg',
      label: 'Before → After',
      get: (c) => c.after,
      cell: (c) => (
        <span className={c.reverted ? 'text-content-faint line-through' : undefined}>
          {pos(c.before)} → {pos(c.after)}
        </span>
      ),
      priority: 1,
    },
    {
      key: 'src',
      label: 'Source',
      get: (c) => c.source,
      cell: (c) =>
        c.source === 'measured' ? (
          <span className="text-2xs" title={c.samples.map((s) => `${s.plc}#${s.seq}`).join(', ')}>
            측정 {c.samples.length}건
          </span>
        ) : (
          <span className="text-2xs text-content-muted">{c.note || c.source}</span>
        ),
      priority: 2,
    },
  ]

  return (
    <div className="flex flex-col gap-3" data-testid="item-dims">
      <div className="flex flex-wrap items-end gap-2">
        <Select
          dense
          label="Window"
          value={String(win)}
          onValueChange={(v) => setWin(Number(v))}
          data-testid="dims-window"
        >
          {WINDOWS.map((w) => (
            <option key={w} value={String(w)}>
              최근 {w}개 중앙값
            </option>
          ))}
        </Select>
        <Button
          size="sm"
          intent="ghost"
          icon={<RefreshCw className="h-3.5 w-3.5" />}
          loading={loading}
          onClick={() => void load()}
        >
          다시 읽기
        </Button>
        <span className="pb-1.5">
          <HelpTip title="측정 반영" text={DIMS_HELP} />
        </span>
        <span className="flex-1" />
        <Button
          size="sm"
          intent="ghost"
          disabled={!!blocked || safe.length === 0}
          title={`${TONE_LABEL.ok} · ${TONE_LABEL.info} 줄만 선택`}
          onClick={() => setPick(new Set(safe.map((s) => s.field)))}
        >
          바로 적용 줄만 ({safe.length})
        </Button>
        <Button
          size="sm"
          intent={risky.length ? 'outline' : 'primary'}
          icon={<CheckCheck className="h-3.5 w-3.5" />}
          disabled={!!blocked || chosen.length === 0}
          loading={busy}
          title={
            blocked ??
            (chosen.length === 0
              ? '적용할 줄을 체크하세요'
              : risky.length
                ? `확인이 필요한 줄 ${risky.length}개 포함 — 확인 후 적용`
                : undefined)
          }
          onClick={() => (risky.length ? setConfirm(chosen) : void apply(chosen))}
          data-testid="dims-apply"
        >
          선택 {chosen.length}줄 적용
        </Button>
      </div>
      {err ? <div className="text-xs text-fault-fg">{err}</div> : null}

      <DimTable
        rows={
          dims ? buildRows([dims], 'all', (_, s) => liveChange(changes, s.field, s.current)) : []
        }
        window={win}
        picked={new Set([...pick].map((f) => cellKey(code, f)))}
        flash={flash}
        disabled={!!blocked}
        onToggle={(r) =>
          setPick((p) => {
            const n = new Set(p)
            if (n.has(r.s.field)) n.delete(r.s.field)
            else n.add(r.s.field)
            return n
          })
        }
        onApply={(r) => (safeTone(dimTone(r.s)) ? void apply([r.s]) : setConfirm([r.s]))}
        onRevert={(_, c) => void doRevert(c)}
        single
        selected={focus ? cellKey(code, focus.field) : null}
        testid="dims-table"
      />

      {dims ? (
        <FieldList
          dense
          columns={2}
          items={[
            { label: 'StackMax (현재)', value: dims.stack_max.current || '-' },
            {
              label: 'SKU 최대 단수',
              value:
                dims.stack_max.observed_max === null
                  ? null
                  : `${dims.stack_max.observed_max}단 (표본 ${dims.stack_max.samples})`,
              missing: 'SKU 측정 없음',
              tooltip:
                '참고 — MeasureSKU 로 실제로 쌓아 잰 최대 단수. StackMax 는 Spec 탭에서 고친다',
            },
          ]}
        />
      ) : null}

      <div className="flex flex-col gap-1">
        <div className="text-2xs font-semibold text-content-muted">변경 이력</div>
        <DataTable
          rows={changes}
          columns={histCols}
          rowKey={(c) => String(c.id)}
          density="compact"
          emptyDense
          empty="측정으로 바꾼 이력 없음"
          actions={(c) =>
            !c.reverted && c.source === 'measured' ? (
              <Button
                size="icon-sm"
                intent="ghost"
                icon={<Undo2 className="h-3.5 w-3.5" />}
                aria-label={`#${c.id} 되돌리기`}
                title={`되돌리기 — ${fieldLabel(c.field)} 를 ${c.before === 0 ? '미입력' : pos(c.before)} 로`}
                disabled={!!blocked}
                onClick={() => void doRevert(c)}
              />
            ) : null
          }
          testid="dims-history"
        />
      </div>

      <ConfirmDialog
        open={confirm !== null}
        onOpenChange={(o) => !o && setConfirm(null)}
        scope="single"
        title={`#${code} 측정 반영 — 확인이 필요한 줄 포함`}
        confirmLabel="적용"
        danger
        onConfirm={() => confirm && void apply(confirm)}
      >
        <div className="flex flex-col gap-1 text-xs">
          {(confirm ?? []).map((s) => {
            const t = dimTone(s)
            return (
              <div key={s.field} className="tabular-nums">
                {changeLine(s)}
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
    </div>
  )
}
