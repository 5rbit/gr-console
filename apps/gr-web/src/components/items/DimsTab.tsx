// 품목 상세의 "Measured" 탭 — MeasureItem 기록으로 만든 치수 제안을 **검토하고 골라 적용**하고, 바꾼 이력을
// 보고 되돌린다(`lib/items/dims.ts`, 백엔드 `registry/dims.rs`).
//
// 적용은 서버가 방금 계산한 제안값을 그대로 쓴다(화면이 값을 보내지 않는다) — 보는 사이 새 기록이 들어와도
// "지금 표본의 중앙값"이 적용된다. 저장하지 않은 편집이 있으면 적용을 막는다: 적용 뒤 상세가 새 값으로
// 다시 열리면서 그 편집이 사라지기 때문이다.
import { useCallback, useEffect, useState } from 'react'
import { RefreshCw, Undo2 } from 'lucide-react'
import {
  DIM_LABEL,
  DIM_SOURCE,
  WINDOWS,
  applicable,
  changeLine,
  defaultPick,
  dimState,
  dimsApi,
  fieldLabel,
  signed,
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
import { Select } from '../../lib/ui/Select'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'

const errMsg = (e: unknown) => (e instanceof Error ? e.message : String(e))

export interface DimsTabProps {
  code: number
  /** 상세에 저장하지 않은 편집이 있다 — 적용을 막는다. */
  dirty: boolean
  /** 적용·되돌리기 뒤 — 부모가 목록을 다시 받는다(상세가 새 값으로 다시 열린다). */
  onApplied: () => Promise<void>
}

export function DimsTab({ code, dirty, onApplied }: DimsTabProps) {
  const [win, setWin] = useState<number>(5)
  const [dims, setDims] = useState<ItemDims | null>(null)
  const [changes, setChanges] = useState<DimChange[]>([])
  const [err, setErr] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [pick, setPick] = useState<Set<DimField>>(new Set())
  const [confirm, setConfirm] = useState(false)
  const [busy, setBusy] = useState(false)
  const [revert, setRevert] = useState<DimChange | null>(null)

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

  const toggle = (f: DimField) =>
    setPick((s) => {
      const n = new Set(s)
      if (n.has(f)) n.delete(f)
      else n.add(f)
      return n
    })
  const chosen = dims?.fields.filter((s) => pick.has(s.field) && applicable(s)) ?? []

  async function apply() {
    setBusy(true)
    try {
      const r = await dimsApi.apply(
        code,
        chosen.map((s) => s.field),
        win,
      )
      toast.ok(`#${code} 측정 반영 ${r.changes.length}건`)
      await onApplied()
    } catch (e) {
      toast.error(errMsg(e))
    } finally {
      setBusy(false)
    }
  }

  async function doRevert(c: DimChange, force: boolean) {
    try {
      await dimsApi.revert(code, c.id, force)
      toast.ok(`#${c.id} 되돌림 — ${fieldLabel(c.field)} ${pos(c.before)}`)
      await onApplied()
    } catch (e) {
      toast.error(errMsg(e))
    }
  }

  const columns: Column<DimSuggestion>[] = [
    {
      key: 'use',
      label: '',
      get: () => 0,
      cell: (s) => (
        <input
          type="checkbox"
          checked={pick.has(s.field)}
          disabled={!applicable(s)}
          aria-label={`${DIM_LABEL[s.field]} 적용`}
          title={
            applicable(s)
              ? '적용할 필드로 고른다'
              : s.suggested === null
                ? '측정 표본 없음'
                : '지금 값과 같다'
          }
          onClick={(e) => e.stopPropagation()}
          onChange={() => toggle(s.field)}
          data-testid={`dims-pick-${s.field}`}
        />
      ),
      priority: 1,
    },
    {
      key: 'field',
      label: 'Field',
      get: (s) => DIM_LABEL[s.field],
      cell: (s) => <span title={DIM_SOURCE[s.field]}>{DIM_LABEL[s.field]}</span>,
      priority: 1,
    },
    {
      key: 'cur',
      label: 'Current',
      get: (s) => s.current,
      numeric: true,
      cell: (s) => pos(s.current),
      priority: 1,
    },
    {
      key: 'sug',
      label: 'Measured',
      get: (s) => s.suggested ?? 0,
      numeric: true,
      cell: (s) =>
        s.suggested === null ? (
          <span className="text-content-faint">-</span>
        ) : (
          <b className={s.changed ? 'text-content-primary' : 'text-content-muted'}>
            {pos(s.suggested)}
          </b>
        ),
      priority: 1,
    },
    {
      key: 'delta',
      label: 'Δ',
      get: (s) => s.delta ?? 0,
      numeric: true,
      cell: (s) => (s.changed ? signed(s.delta) : <span className="text-content-faint">0.0</span>),
      priority: 1,
    },
    { key: 'n', label: 'n', get: (s) => s.n, numeric: true, priority: 2 },
    {
      key: 'spread',
      label: 'Spread',
      get: (s) => s.spread ?? 0,
      numeric: true,
      cell: (s) =>
        s.spread === null ? (
          <span className="text-content-faint">-</span>
        ) : (
          <span
            className={s.unstable ? 'text-warn-fg' : undefined}
            title={`한도 ${s.spread_limit} mm`}
          >
            {pos(s.spread)}
          </span>
        ),
      priority: 2,
    },
    {
      key: 'state',
      label: 'State',
      get: (s) => (s.outlier ? 3 : s.unstable ? 2 : s.changed ? 1 : 0),
      cell: (s) => {
        const st = dimState(s)
        return st.tone ? (
          <StatusBadge status={st.tone} title={st.title}>
            {st.text}
          </StatusBadge>
        ) : (
          <span className={st.text === '제안' ? 'text-2xs' : 'text-2xs text-content-faint'}>
            {st.text}
          </span>
        )
      },
      priority: 1,
    },
  ]

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
        <span className="flex-1" />
        <Button
          size="sm"
          intent="primary"
          disabled={dirty || chosen.length === 0 || busy}
          title={
            dirty
              ? '저장하지 않은 편집이 있습니다 — 먼저 저장하거나 되돌리세요'
              : chosen.length === 0
                ? '적용할 필드를 고르세요'
                : undefined
          }
          onClick={() => setConfirm(true)}
          data-testid="dims-apply"
        >
          선택 {chosen.length}개 적용
        </Button>
      </div>
      {err ? <div className="text-xs text-fault-fg">{err}</div> : null}

      <DataTable
        rows={dims?.fields ?? []}
        columns={columns}
        rowKey={(s) => s.field}
        loading={loading && !dims}
        density="compact"
        emptyDense
        empty="측정 기록 없음"
        rowDetail={(s) =>
          s.samples.length ? (
            <div className="flex flex-col gap-0.5 text-2xs text-content-muted tabular-nums">
              <span>{DIM_SOURCE[s.field]}</span>
              {s.samples.map((x) => (
                <span key={`${x.plc}-${x.seq}`}>
                  {x.plc}#{x.seq} · {x.at.slice(5, 19)} · Cell {x.cell_id} · <b>{pos(x.value)}</b>
                </span>
              ))}
            </div>
          ) : (
            <span className="text-2xs text-content-faint">{DIM_SOURCE[s.field]} — 표본 없음</span>
          )
        }
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
                title={`되돌리기 — ${fieldLabel(c.field)} 를 ${pos(c.before)} 로`}
                disabled={dirty}
                onClick={() => setRevert(c)}
              />
            ) : null
          }
          testid="dims-history"
        />
      </div>

      <ConfirmDialog
        open={confirm}
        onOpenChange={setConfirm}
        scope="single"
        title={`#${code} 측정 반영`}
        confirmLabel="적용"
        onConfirm={() => void apply()}
      >
        <div className="flex flex-col gap-1 text-xs">
          {chosen.map((s) => (
            <div key={s.field} className="tabular-nums">
              {changeLine(s)}
              {s.outlier ? (
                <span className="text-fault-fg"> · 차이 큼 — 재고 품목 확인</span>
              ) : s.unstable ? (
                <span className="text-warn-fg"> · 흔들림(편차 {pos(s.spread)})</span>
              ) : null}
            </div>
          ))}
          <div className="text-content-muted">
            PLC 로 보내는 품목 값이라 다음 명령부터 G · Z 가 바뀝니다. 이력에서 되돌릴 수 있습니다.
          </div>
        </div>
      </ConfirmDialog>
      <ConfirmDialog
        open={revert !== null}
        onOpenChange={(o) => !o && setRevert(null)}
        scope="single"
        title="측정 반영 되돌리기"
        confirmLabel="되돌리기"
        onConfirm={() => revert && void doRevert(revert, false)}
      >
        {revert ? (
          <div className="text-xs tabular-nums">
            {fieldLabel(revert.field)} {pos(revert.after)} → {pos(revert.before)} (변경 #{revert.id}
            )
          </div>
        ) : null}
      </ConfirmDialog>
    </div>
  )
}
