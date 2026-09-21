// 측정 반영 일괄 검토 — 적용할 제안이 있는 품목을 모아 필드 단위로 골라 한 번에 적용한다.
// 기본 선택은 "바뀌고 안정한" 필드(`defaultPick`), 흔들리는 필드는 보이되 사람이 직접 고른다.
// 한 품목씩 따로 적용되므로(서버 `apply-bulk`) 한 품목이 실패해도 나머지는 적용되고, 결과를 품목별로 알린다.
import { useCallback, useEffect, useMemo, useState } from 'react'
import { RefreshCw } from 'lucide-react'
import {
  DIM_LABEL,
  DIM_SOURCE,
  WINDOWS,
  applicable,
  defaultPick,
  dimState,
  dimsApi,
  signed,
  type DimField,
  type DimSuggestion,
  type ItemDims,
} from '../../lib/items/dims'
import { pos } from '../../lib/meas/format'
import { Button } from '../../lib/ui/Button'
import { DataTable } from '../../lib/ui/DataTable'
import { Dialog } from '../../lib/ui/Dialog'
import { Select } from '../../lib/ui/Select'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'

interface Row {
  key: string
  code: number
  name: string
  s: DimSuggestion
}

const keyOf = (code: number, f: DimField) => `${code}:${f}`
const errMsg = (e: unknown) => (e instanceof Error ? e.message : String(e))

export interface DimsReviewDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** 적용 뒤 — 부모가 목록을 다시 받는다. */
  onApplied: () => Promise<void>
}

export function DimsReviewDialog({ open, onOpenChange, onApplied }: DimsReviewDialogProps) {
  const [win, setWin] = useState(5)
  const [list, setList] = useState<ItemDims[]>([])
  const [loading, setLoading] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const [pick, setPick] = useState<Set<string>>(new Set())
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const l = await dimsApi.suggest(win)
      setList(l)
      setPick(new Set(l.flatMap((d) => [...defaultPick(d)].map((f) => keyOf(d.code, f)))))
      setErr(null)
    } catch (e) {
      setErr(errMsg(e))
    } finally {
      setLoading(false)
    }
  }, [win])
  useEffect(() => {
    if (open) void load()
  }, [open, load])

  const rows = useMemo<Row[]>(
    () =>
      list.flatMap((d) =>
        d.fields
          .filter(applicable)
          .map((s) => ({ key: keyOf(d.code, s.field), code: d.code, name: d.name, s })),
      ),
    [list],
  )
  const chosen = rows.filter((r) => pick.has(r.key))
  const toggle = (k: string) =>
    setPick((p) => {
      const n = new Set(p)
      if (n.has(k)) n.delete(k)
      else n.add(k)
      return n
    })

  async function apply() {
    const by = new Map<number, DimField[]>()
    for (const r of chosen) by.set(r.code, [...(by.get(r.code) ?? []), r.s.field])
    setBusy(true)
    try {
      const res = await dimsApi.applyBulk(
        [...by].map(([code, fields]) => ({ code, fields })),
        win,
      )
      if (res.failed) {
        const first = res.results.find((r) => !r.ok)
        toast.error(
          `측정 반영 ${res.applied}건 적용 · ${res.failed}건 실패 (#${first?.code} ${first?.error ?? ''})`,
        )
      } else toast.ok(`측정 반영 ${res.applied}개 품목 적용`)
      await onApplied()
      onOpenChange(false)
    } catch (e) {
      toast.error(errMsg(e))
    } finally {
      setBusy(false)
    }
  }

  const columns: Column<Row>[] = [
    {
      key: 'use',
      label: '',
      get: () => 0,
      cell: (r) => (
        <input
          type="checkbox"
          checked={pick.has(r.key)}
          aria-label={`${r.code} ${DIM_LABEL[r.s.field]} 적용`}
          onClick={(e) => e.stopPropagation()}
          onChange={() => toggle(r.key)}
          data-testid="dims-review-pick"
        />
      ),
      priority: 1,
    },
    {
      key: 'code',
      label: 'Code',
      get: (r) => r.code,
      numeric: true,
      class: 'font-mono',
      priority: 1,
    },
    { key: 'name', label: 'Name', get: (r) => r.name, priority: 2 },
    {
      key: 'field',
      label: 'Field',
      get: (r) => DIM_LABEL[r.s.field],
      cell: (r) => <span title={DIM_SOURCE[r.s.field]}>{DIM_LABEL[r.s.field]}</span>,
      priority: 1,
    },
    {
      key: 'cur',
      label: 'Current',
      get: (r) => r.s.current,
      numeric: true,
      cell: (r) => pos(r.s.current),
      priority: 1,
    },
    {
      key: 'sug',
      label: 'Measured',
      get: (r) => r.s.suggested ?? 0,
      numeric: true,
      cell: (r) => <b>{pos(r.s.suggested)}</b>,
      priority: 1,
    },
    {
      key: 'delta',
      label: 'Δ',
      get: (r) => r.s.delta ?? 0,
      numeric: true,
      cell: (r) => signed(r.s.delta),
      priority: 1,
    },
    { key: 'n', label: 'n', get: (r) => r.s.n, numeric: true, priority: 2 },
    {
      key: 'spread',
      label: 'Spread',
      get: (r) => r.s.spread ?? 0,
      numeric: true,
      cell: (r) => (
        <span
          className={r.s.unstable ? 'text-warn-fg' : undefined}
          title={`한도 ${r.s.spread_limit} mm`}
        >
          {pos(r.s.spread)}
        </span>
      ),
      priority: 2,
    },
    {
      key: 'state',
      label: 'State',
      get: (r) => (r.s.outlier ? 2 : r.s.unstable ? 1 : 0),
      cell: (r) => {
        const st = dimState(r.s)
        return st.tone ? (
          <StatusBadge status={st.tone} title={st.title}>
            {st.text}
          </StatusBadge>
        ) : null
      },
      priority: 1,
    },
  ]

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="측정 반영 검토"
      meta={`${list.length}개 품목 · 제안 ${rows.length} · 선택 ${chosen.length}`}
      size="xl"
      testid="dims-review"
      footer={
        <>
          <Button size="sm" intent="ghost" onClick={() => onOpenChange(false)}>
            닫기
          </Button>
          <Button
            size="sm"
            intent="primary"
            disabled={chosen.length === 0 || busy}
            loading={busy}
            title={chosen.length === 0 ? '적용할 필드를 고르세요' : undefined}
            onClick={() => void apply()}
            data-testid="dims-review-apply"
          >
            선택 {chosen.length}개 적용
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
          <Button
            size="sm"
            intent="ghost"
            icon={<RefreshCw className="h-3.5 w-3.5" />}
            loading={loading}
            onClick={() => void load()}
          >
            다시 읽기
          </Button>
          <span className="text-2xs text-content-muted">
            MeasureItem(DONE) 기록의 중앙값 · 높이 둘은 1 단 측정만 · PLC 로 가는 품목 값이라 다음
            명령부터 G · Z 가 바뀝니다
          </span>
        </div>
        {err ? <div className="text-xs text-fault-fg">{err}</div> : null}
        <DataTable
          rows={rows}
          columns={columns}
          rowKey={(r) => r.key}
          loading={loading && !list.length}
          density="compact"
          stickyHeader
          className="max-h-96 overflow-y-auto"
          empty="적용할 측정 제안 없음"
          emptyHint="MeasureItem 으로 잰 품목 중 등록값과 다른 것이 없습니다."
          testid="dims-review-table"
        />
      </div>
    </Dialog>
  )
}
