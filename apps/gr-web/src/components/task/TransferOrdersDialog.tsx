// 이송 이력 — PICK/DROP 한 짝 = 이송 지시(TO-YYMMDD-NNNN). 재고가 어디서 어디로, 어느 Task 로 움직였는지를
// 나중에 되짚는 자리다. 목록은 최신 먼저, 행을 펼치면 상태 이력과 재고 변경(셀·Hand)이 나온다.
import { useEffect, useState } from 'react'
import { api } from '../../lib/api'
import type { StockChange, Target, TransferOrder, TransferOrderDetail } from '../../lib/types'
import { DataTable } from '../../lib/ui/DataTable'
import { Dialog } from '../../lib/ui/Dialog'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import type { Status } from '../../lib/ui/status'
import type { Column } from '../../lib/ui/table'

export const ORDER_STATE_LABEL: Record<TransferOrder['state'], string> = {
  planned: '예정',
  picking: '집는 중',
  in_hand: 'Hand',
  dropping: '놓는 중',
  done: '완료',
  failed: '실패',
  aborted: '중단',
}

const ORDER_TONE: Record<TransferOrder['state'], Status> = {
  planned: 'neutral',
  picking: 'info',
  in_hand: 'warn',
  dropping: 'info',
  done: 'ok',
  failed: 'fault',
  aborted: 'neutral',
}

function where(t: Target | null): string {
  return t ? `${t.kind === 'station' ? 'Station' : 'Cell'} ${t.id}` : '?'
}

function time(s: string | null | undefined): string {
  return s ? s.replace('T', ' ').slice(5, 19) : ''
}

function change(c: StockChange): string {
  const what = c.kind === 'hand' ? `Hand ${c.key}` : `Cell ${c.key}`
  return `${time(c.at)} ${what}: ${c.item_before}×${c.count_before} → ${c.item_after}×${c.count_after} (${c.reason})`
}

function OrderDetail({ id }: { id: string }) {
  const [d, setD] = useState<TransferOrderDetail | null>(null)
  const [err, setErr] = useState<string | null>(null)
  useEffect(() => {
    let live = true
    api
      .transferOrder(id)
      .then((x) => live && setD(x))
      .catch((e) => live && setErr(e instanceof Error ? e.message : String(e)))
    return () => {
      live = false
    }
  }, [id])
  if (err) return <span className="block text-2xs text-fault-fg">{err}</span>
  if (!d) return <span className="block text-2xs text-content-faint">읽는 중…</span>
  return (
    <div className="flex flex-col gap-1 text-2xs text-content-secondary">
      {d.order.history.map((h, i) => (
        <span key={i}>
          {time(h.at)} {h.from ? `${ORDER_STATE_LABEL[h.from]} → ` : ''}
          {ORDER_STATE_LABEL[h.to]}
          {h.note ? ` — ${h.note}` : ''}
        </span>
      ))}
      {d.tasks.map((t) => (
        <span key={t.id} className="font-mono">
          Task #{t.seq} {t.work_id}/{t.task_id} {t.state}
        </span>
      ))}
      {d.stock_changes.length ? (
        d.stock_changes.map((c) => (
          <span key={c.id} className="font-mono">
            {change(c)}
          </span>
        ))
      ) : (
        <span className="text-content-faint">재고 변경 없음</span>
      )}
    </div>
  )
}

export function TransferOrdersDialog({
  open,
  onOpenChange,
  robot,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** 이 로봇의 지시만(없으면 전체). */
  robot: number | null
}) {
  const [rows, setRows] = useState<TransferOrder[]>([])
  const [total, setTotal] = useState(0)
  const [loading, setLoading] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  useEffect(() => {
    if (!open) return
    let live = true
    setLoading(true)
    api
      .transferOrders({ robot, limit: 100 })
      .then((r) => {
        if (!live) return
        setRows(r.items)
        setTotal(r.total)
        setErr(null)
      })
      .catch((e) => live && setErr(e instanceof Error ? e.message : String(e)))
      .finally(() => live && setLoading(false))
    return () => {
      live = false
    }
  }, [open, robot])

  const cols: Column<TransferOrder>[] = [
    {
      key: 'id',
      label: 'TransferOrder',
      get: (o) => o.seq,
      cell: (o) => <span className="font-mono">{o.id}</span>,
      priority: 1,
    },
    {
      key: 'state',
      label: 'State',
      get: (o) => o.state,
      cell: (o) => (
        <StatusBadge status={ORDER_TONE[o.state]} dot={false}>
          {ORDER_STATE_LABEL[o.state]}
        </StatusBadge>
      ),
      priority: 1,
    },
    {
      key: 'item',
      label: 'Item',
      get: (o) => o.item_code,
      cell: (o) => `${o.item_code} ×${o.count}`,
      priority: 1,
    },
    {
      key: 'route',
      label: 'Route',
      sortable: false,
      cell: (o) => `${where(o.from)} → ${where(o.to)}`,
      priority: 2,
    },
    { key: 'plc', label: 'PLC', get: (o) => o.plc, priority: 3 },
    {
      key: 'at',
      label: 'UpdatedAt',
      get: (o) => o.updated_at,
      cell: (o) => time(o.updated_at),
      priority: 2,
    },
  ]

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="이송 이력"
      meta={`${rows.length}/${total}건`}
      size="lg"
      closeLabel="닫기"
      testid="to-dialog"
    >
      {err ? <span className="mb-2 block text-2xs text-fault-fg">{err}</span> : null}
      <DataTable
        rows={rows}
        columns={cols}
        rowKey={(o) => o.id}
        density="compact"
        stickyHeader
        loading={loading && !rows.length}
        emptyDense
        empty="이송 지시 없음"
        rowDetail={(o) => <OrderDetail id={o.id} />}
        testid="to-table"
      />
    </Dialog>
  )
}
