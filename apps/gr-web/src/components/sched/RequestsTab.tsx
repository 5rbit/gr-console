// 스케줄러 · 요청 — 상위(호스트)와 사용자의 입고 · 출고 · 이동 요청. 엔진이 정책 수요보다 먼저 돈다.
import { useCallback, useEffect, useState } from 'react'
import { ChevronDown, ChevronUp, Plus, X } from 'lucide-react'
import {
  REQUEST_KIND_LABEL,
  REQUEST_STATE_LABEL,
  requestProgress,
  schedApi,
  type RequestKind,
  type SchedState,
  type TransferRequest,
} from '../../lib/sched'
import {
  EMPTY_DRAFT,
  REQUEST_STATE_TONE,
  draftError,
  draftToRequest,
  endpointKinds,
  isLive,
  shortTime,
  sortRequests,
  targetText,
  type RequestDraft,
  type RequestFilter,
} from '../../lib/sched/requestsModel'
import { loadFailureText } from '../../lib/api'
import { visibleInterval } from '../../lib/poll'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { FormDialog } from '../../lib/ui/Dialog'
import { HelpTip } from '../../lib/ui/HelpTip'
import { Input } from '../../lib/ui/Input'
import { Segmented } from '../../lib/ui/Segmented'
import { Select } from '../../lib/ui/Select'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'

const ITEM_HELP =
  '입고: 비우면 스테이션의 콘솔 재고 품목을 씁니다(모르면 수요 탭에서 품목 지정). 출고: 비우면 가장 오래된 재고.'
const ENDPOINT_HELP =
  '비우면 Auto — 입고 From 은 준비된 PICK 스테이션, To 는 정책의 적재 셀. 출고는 그 반대.'

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e))

function AddRequestDialog({ onClose, onDone }: { onClose: () => void; onDone: () => void }) {
  const [d, setD] = useState<RequestDraft>(EMPTY_DRAFT)
  const [busy, setBusy] = useState(false)
  const k = endpointKinds(d.kind)
  const kindName = (x: 'station' | 'cell') => (x === 'station' ? 'Station' : 'Cell')
  const set = (p: Partial<RequestDraft>) => setD((o) => ({ ...o, ...p }))
  const error = draftError(d)

  async function submit() {
    setBusy(true)
    try {
      const r = await schedApi.addRequest(draftToRequest(d))
      toast.ok(`요청 ${r.id} — ${REQUEST_KIND_LABEL[r.kind]} ${r.qty}`)
      onDone()
    } catch (e) {
      toast.error(`요청 추가 실패 — ${errText(e)}`)
    } finally {
      setBusy(false)
    }
  }

  return (
    <FormDialog
      open
      onOpenChange={(o) => !o && onClose()}
      title="요청 추가"
      size="sm"
      submitLabel="추가"
      busy={busy}
      disabledReason={error ?? undefined}
      onSubmit={() => void submit()}
      testid="sched-req-add"
    >
      <div className="grid grid-cols-2 gap-2">
        <Select label="Kind" value={d.kind} onValueChange={(v) => set({ kind: v as RequestKind })}>
          {(Object.keys(REQUEST_KIND_LABEL) as RequestKind[]).map((x) => (
            <option key={x} value={x}>
              {REQUEST_KIND_LABEL[x]}
            </option>
          ))}
        </Select>
        <Input label="Qty" mono value={d.qty} onValueChange={(v) => set({ qty: v })} />
        <div className="flex items-end gap-1">
          <Input
            className="min-w-0 flex-1"
            label="ItemCode"
            mono
            placeholder="자동"
            value={d.item}
            onValueChange={(v) => set({ item: v })}
          />
          <span className="pb-2">
            <HelpTip title="ItemCode" text={ITEM_HELP} />
          </span>
        </div>
        <Input
          label="Priority"
          mono
          value={d.priority}
          onValueChange={(v) => set({ priority: v })}
        />
        <Input
          label={`From (${kindName(k.from)})`}
          mono
          placeholder={d.kind === 'move' ? '' : 'Auto'}
          value={d.from}
          onValueChange={(v) => set({ from: v })}
        />
        <div className="flex items-end gap-1">
          <Input
            className="min-w-0 flex-1"
            label={`To (${kindName(k.to)})`}
            mono
            placeholder={d.kind === 'move' ? '' : 'Auto'}
            value={d.to}
            onValueChange={(v) => set({ to: v })}
          />
          <span className="pb-2">
            <HelpTip title="From · To" text={ENDPOINT_HELP} align="right" />
          </span>
        </div>
      </div>
      <Input label="Note" value={d.note} onValueChange={(v) => set({ note: v })} />
    </FormDialog>
  )
}

export default function RequestsTab(_props: { state: SchedState | null; reload: () => void }) {
  const [filter, setFilter] = useState<RequestFilter>('open')
  const [rows, setRows] = useState<TransferRequest[] | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const [adding, setAdding] = useState(false)
  const [canceling, setCanceling] = useState<TransferRequest | null>(null)

  const load = useCallback(() => {
    schedApi
      .requests(filter)
      .then((r) => {
        setRows(sortRequests(r ?? []))
        setErr(null)
      })
      .catch((e: unknown) => {
        setErr((prev) => {
          const why = loadFailureText(e, '요청 목록', false)
          if (prev === null) toast.error(why)
          return why
        })
      })
  }, [filter])
  useEffect(() => {
    load()
    const t = visibleInterval(load, 2000)
    return () => clearInterval(t)
  }, [load])

  const bump = (r: TransferRequest, delta: number) =>
    void schedApi
      .patchRequest(r.id, { priority: r.priority + delta })
      .then(load)
      .catch((e) => toast.error(`우선순위 변경 실패 — ${errText(e)}`))

  const cols: Column<TransferRequest>[] = [
    {
      key: 'id',
      label: 'Id',
      get: (r) => r.id,
      priority: 1,
      cell: (r) => (
        <span className="font-mono" title={r.ext_ref ? `ext_ref ${r.ext_ref}` : undefined}>
          {r.id}
        </span>
      ),
    },
    {
      key: 'source',
      label: 'Source',
      get: (r) => r.source,
      priority: 2,
      cell: (r) => (
        <span className={r.source === 'host' ? 'text-info-fg' : 'text-content-muted'}>
          {r.source}
        </span>
      ),
    },
    { key: 'kind', label: 'Kind', get: (r) => REQUEST_KIND_LABEL[r.kind] ?? r.kind, priority: 1 },
    {
      key: 'item',
      label: 'ItemCode',
      get: (r) => r.item_code ?? 0,
      numeric: true,
      priority: 1,
      cell: (r) =>
        r.item_code ? String(r.item_code) : <span className="text-content-faint">자동</span>,
    },
    {
      key: 'qty',
      label: 'Qty',
      get: (r) => r.qty,
      numeric: true,
      priority: 1,
      cell: (r) => <span className="font-mono tabular-nums">{requestProgress(r)}</span>,
    },
    {
      key: 'from',
      label: 'From',
      get: (r) => targetText(r.from),
      priority: 2,
      cell: (r) => <span className={r.from ? '' : 'text-content-faint'}>{targetText(r.from)}</span>,
    },
    {
      key: 'to',
      label: 'To',
      get: (r) => targetText(r.to),
      priority: 2,
      cell: (r) => <span className={r.to ? '' : 'text-content-faint'}>{targetText(r.to)}</span>,
    },
    { key: 'prio', label: 'Priority', get: (r) => r.priority, numeric: true, priority: 1 },
    {
      key: 'state',
      label: 'State',
      get: (r) => r.state,
      priority: 1,
      cell: (r) => (
        <StatusBadge status={REQUEST_STATE_TONE[r.state]} title={r.reason ?? undefined}>
          {REQUEST_STATE_LABEL[r.state] ?? r.state}
        </StatusBadge>
      ),
    },
    {
      key: 'reason',
      label: 'Reason',
      sortable: false,
      priority: 3,
      cell: (r) => (
        <span
          className="block max-w-56 truncate text-2xs text-warn-fg"
          title={[r.reason, r.note].filter(Boolean).join('\n')}
        >
          {r.reason ?? (r.note ? <span className="text-content-faint">{r.note}</span> : '')}
        </span>
      ),
    },
    {
      key: 'created',
      label: 'Created',
      get: (r) => r.created_at,
      priority: 3,
      cell: (r) => (
        <span className="font-mono tabular-nums text-content-muted" title={r.created_at}>
          {shortTime(r.created_at)}
        </span>
      ),
    },
  ]

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2" data-testid="sched-requests">
      <div className="flex flex-wrap items-center gap-2">
        <Segmented
          ariaLabel="요청 범위"
          value={filter}
          onChange={setFilter}
          options={[
            { id: 'open', label: '열림', testid: 'sched-req-open' },
            { id: 'all', label: '전체', testid: 'sched-req-all' },
          ]}
        />
        {err ? (
          <span className="min-w-0 flex-1 truncate text-2xs text-warn-fg" title={err}>
            {err}
          </span>
        ) : (
          <span className="flex-1" />
        )}
        <Button
          size="sm"
          intent="outline"
          icon={<Plus className="h-3.5 w-3.5" />}
          onClick={() => setAdding(true)}
          data-testid="sched-req-add-open"
        >
          요청 추가
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        <DataTable
          rows={rows ?? []}
          columns={cols}
          rowKey={(r) => r.id}
          density="compact"
          loading={rows === null && !err}
          empty={filter === 'open' ? '열린 요청 없음' : '요청 없음'}
          emptyDense
          stickyHeader
          testid="sched-req-table"
          actions={(r) =>
            isLive(r) ? (
              <>
                <Button
                  size="icon-sm"
                  intent="ghost"
                  icon={<ChevronUp className="h-3.5 w-3.5" />}
                  title="Priority +1"
                  onClick={() => bump(r, 1)}
                />
                <Button
                  size="icon-sm"
                  intent="ghost"
                  icon={<ChevronDown className="h-3.5 w-3.5" />}
                  title="Priority −1"
                  onClick={() => bump(r, -1)}
                />
                <Button
                  size="icon-sm"
                  intent="ghost"
                  icon={<X className="h-3.5 w-3.5" />}
                  title="취소"
                  onClick={() => setCanceling(r)}
                />
              </>
            ) : null
          }
        />
      </div>
      {adding ? (
        <AddRequestDialog
          onClose={() => setAdding(false)}
          onDone={() => {
            setAdding(false)
            load()
          }}
        />
      ) : null}
      <ConfirmDialog
        open={canceling !== null}
        onOpenChange={(o) => !o && setCanceling(null)}
        scope="single"
        danger
        title="요청 취소"
        confirmLabel="취소하기"
        cancelLabel="닫기"
        onConfirm={() => {
          const r = canceling
          setCanceling(null)
          if (r)
            void schedApi
              .cancelRequest(r.id)
              .then(() => {
                toast.ok(`요청 ${r.id} 취소`)
                load()
              })
              .catch((e) => toast.error(`취소 실패 — ${errText(e)}`))
        }}
      >
        {canceling ? (
          <div className="flex flex-col gap-1 text-xs">
            <div>
              {canceling.id} · {REQUEST_KIND_LABEL[canceling.kind]} {requestProgress(canceling)}
            </div>
            <div className="text-content-faint">
              남은 수만 취소합니다. 진행 중인 지시는 끝까지 갑니다.
            </div>
          </div>
        ) : null}
      </ConfirmDialog>
    </div>
  )
}
