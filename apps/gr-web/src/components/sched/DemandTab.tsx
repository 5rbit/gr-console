// 스케줄러 · 수요 — 엔진이 지금 보는 것: 스테이션 준비(PICK · DROP) · 예약 · 파생 수요 · Hand 경고 · 셀 요약.
import { useCallback, useEffect, useState } from 'react'
import { api, loadFailureText } from '../../lib/api'
import { useRegistry } from '../../lib/registry'
import { visibleInterval } from '../../lib/poll'
import CondExpr from './CondExpr'
import {
  DROP_MODE_LABEL,
  ROLE_LABEL,
  schedApi,
  type HandAlert,
  type ReadyMark,
  type ReadySnapshot,
  type SchedState,
} from '../../lib/sched'
import {
  MARK_VIEW,
  cellSummary,
  handAlertText,
  stationRows,
  waitText,
  type StationDemandRow,
} from '../../lib/sched/demandModel'
import { itemLabel } from '../../lib/items/model'
import type { Item } from '../../lib/types'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { FormDialog } from '../../lib/ui/Dialog'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'
import { ItemPicker } from '../shared/ItemPicker'

const TONE = { ok: 'text-ok-fg', warn: 'text-warn-fg', muted: 'text-content-faint' } as const
const HAND_REMOVE_TEXT =
  '재고에 반영하지 않고 Hand 를 비웁니다. 짝 Task 는 취소됩니다. 화물은 사람이 들어냅니다.'
const WAIT_HELP = 'PICK 또는 DROP 이 준비된 뒤 지난 시간(초, 1분 넘으면 m:ss)'

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e))

function Mark({ mark, why }: { mark: ReadyMark; why: string }) {
  const v = MARK_VIEW[mark]
  if (mark === 'none')
    return (
      <span className="text-content-faint" title={why || undefined}>
        —
      </span>
    )
  return (
    <StatusBadge status={v.tone} title={why || undefined}>
      {v.label}
    </StatusBadge>
  )
}

/** 스테이션 화물의 품목을 콘솔 재고로 정한다(1개) — 트래킹 OD 로 고른 후보를 버튼으로. */
function SetItemDialog({
  row,
  items,
  onClose,
}: {
  row: StationDemandRow
  items: readonly Item[]
  onClose: () => void
}) {
  const [item, setItem] = useState<number | null>(row.hints[0] ?? null)
  const [busy, setBusy] = useState(false)

  async function submit() {
    if (!item) return
    setBusy(true)
    try {
      await api.stockSet(row.id, { item_code: item, count: 1 })
      toast.ok(`Station ${row.id} ItemCode ${item}`)
      onClose()
    } catch (e) {
      toast.error(`품목 지정 실패 — ${errText(e)}`)
    } finally {
      setBusy(false)
    }
  }

  return (
    <FormDialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={`Station ${row.id} · 품목 지정`}
      size="sm"
      submitLabel="지정"
      busy={busy}
      disabledReason={item ? undefined : '품목을 고르세요'}
      onSubmit={() => void submit()}
      testid="sched-set-item"
    >
      {/* select 안의 Enter 는 브라우저가 제출하지 않는다 — 여기서 받는다. */}
      <div
        className="flex flex-col gap-2"
        onKeyDown={(e) => {
          if (e.key === 'Enter' && (e.target as HTMLElement).tagName === 'SELECT') {
            e.preventDefault()
            void submit()
          }
        }}
      >
        {row.hints.length ? (
          <div className="flex flex-wrap gap-1" data-testid="sched-set-item-hints">
            {row.hints.map((c) => {
              const it = items.find((i) => i.code === c)
              return (
                <Button
                  key={c}
                  type="button"
                  tabIndex={-1}
                  onMouseDown={(e) => e.preventDefault()}
                  size="sm"
                  intent={item === c ? 'primary' : 'outline'}
                  onClick={() => setItem(c)}
                  title={it ? itemLabel(it) : undefined}
                >
                  {c}
                  {it ? (
                    <span className="ml-1 max-w-28 truncate font-normal opacity-80">{it.name}</span>
                  ) : null}
                </Button>
              )
            })}
          </div>
        ) : null}
        <ItemPicker value={item} onChange={setItem} items={[...items]} allowNone={false} />
      </div>
    </FormDialog>
  )
}

export default function DemandTab({
  state,
  reload,
}: {
  state: SchedState | null
  reload: () => void
}) {
  const [snap, setSnap] = useState<ReadySnapshot | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const [, setTick] = useState(0)
  const [setting, setSetting] = useState<StationDemandRow | null>(null)
  const [removing, setRemoving] = useState<HandAlert | null>(null)
  const items = useRegistry<Item>(api.items)

  const load = useCallback(() => {
    schedApi
      .ready()
      .then((s) => {
        setSnap(s)
        setErr(null)
        setTick((n) => n + 1) // 대기 시간은 값이 같아도 흐른다
      })
      .catch((e: unknown) => {
        setErr((prev) => {
          const why = loadFailureText(e, '준비 상태', false)
          if (prev === null) toast.error(why)
          return why
        })
      })
  }, [])
  useEffect(() => {
    load()
    const t = visibleInterval(load, 1000)
    return () => clearInterval(t)
  }, [load])

  const auto = !!state?.config?.auto
  const rows = stationRows(snap?.targets, state?.derived, auto)
  const cells = cellSummary(snap?.targets)
  const hands = state?.hand_alerts ?? []

  const cols: Column<StationDemandRow>[] = [
    {
      key: 'id',
      label: 'Id',
      get: (r) => r.id,
      numeric: true,
      priority: 1,
    },
    {
      key: 'role',
      label: 'Role',
      get: (r) => (r.role ? ROLE_LABEL[r.role] : ''),
      priority: 2,
      cell: (r) =>
        r.role ? (
          ROLE_LABEL[r.role]
        ) : (
          <span className="text-content-faint" title="프로파일 없음 — 정책이 보지 않는다">
            —
          </span>
        ),
    },
    {
      key: 'mode',
      label: 'DropMode',
      get: (r) => (r.dropMode ? DROP_MODE_LABEL[r.dropMode] : ''),
      priority: 3,
    },
    {
      key: 'pick',
      label: 'PICK',
      get: (r) => r.pick,
      priority: 1,
      cell: (r) => (
        <span className="flex items-center gap-2">
          <Mark mark={r.pick} why={r.pickWhy} />
          <CondExpr expr={r.pickExpr} />
        </span>
      ),
    },
    {
      key: 'drop',
      label: 'DROP',
      get: (r) => r.drop,
      priority: 1,
      cell: (r) => (
        <span className="flex items-center gap-2">
          <Mark mark={r.drop} why={r.dropWhy} />
          <CondExpr expr={r.dropExpr} />
        </span>
      ),
    },
    {
      key: 'item',
      label: 'ItemCode',
      get: (r) => r.itemCode,
      numeric: true,
      priority: 2,
      cell: (r) => (r.itemCode ? String(r.itemCode) : ''),
    },
    {
      key: 'count',
      label: 'Count',
      get: (r) => r.count,
      numeric: true,
      priority: 2,
    },
    {
      key: 'wait',
      label: 'Wait',
      help: WAIT_HELP,
      get: (r) => r.wait ?? -1,
      numeric: true,
      priority: 2,
      cell: (r) => <span className="font-mono tabular-nums">{waitText(r.wait)}</span>,
    },
    {
      key: 'holds',
      label: 'Holds',
      get: (r) => r.holds,
      priority: 3,
      cell: (r) => (
        <span className="block max-w-48 truncate text-2xs" title={r.holds}>
          {r.holds}
        </span>
      ),
    },
    {
      key: 'demand',
      label: 'Demand',
      get: (r) => r.demand?.label ?? '',
      priority: 1,
      cell: (r) =>
        r.demand ? (
          <span className={TONE[r.demand.tone]} title={r.demand.title}>
            {r.demand.label}
          </span>
        ) : (
          <span className="text-content-faint">—</span>
        ),
    },
  ]

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2" data-testid="sched-demand">
      {hands.length ? (
        <div
          className="flex flex-col gap-1 rounded border border-warn-border bg-warn-soft px-2 py-1 text-xs text-warn-fg"
          data-testid="sched-hand-alerts"
        >
          {hands.map((a) => (
            <div key={a.robot} className="flex items-center gap-2">
              <span
                className="min-w-0 flex-1 truncate"
                title={a.order ? `이송 지시 ${a.order}` : undefined}
              >
                {handAlertText(a)}
              </span>
              <Button
                size="sm"
                intent="outline"
                onClick={() => setRemoving(a)}
                data-testid={`sched-hand-remove-${a.robot}`}
              >
                Hand 정리 (사람이 제거)
              </Button>
            </div>
          ))}
        </div>
      ) : null}
      <div
        className="flex items-center gap-3 text-2xs text-content-muted"
        data-testid="sched-cells-line"
      >
        <span>
          Cells <span className="font-mono tabular-nums text-content-secondary">{cells.total}</span>
        </span>
        <span>
          PICK 준비 <span className="font-mono tabular-nums text-ok-fg">{cells.pick}</span>
        </span>
        <span>
          DROP 준비 <span className="font-mono tabular-nums text-ok-fg">{cells.drop}</span>
        </span>
        <span>
          예약 <span className="font-mono tabular-nums text-info-fg">{cells.held}</span>
        </span>
        {err ? (
          <span className="min-w-0 flex-1 truncate text-warn-fg" title={err}>
            {err}
          </span>
        ) : snap?.at ? (
          <span className="ml-auto font-mono text-content-faint">{snap.at}</span>
        ) : null}
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        <DataTable
          rows={rows}
          columns={cols}
          rowKey={(r) => String(r.id)}
          density="compact"
          loading={snap === null && !err}
          empty="스테이션 없음"
          emptyDense
          stickyHeader
          testid="sched-demand-stations"
          actions={(r) =>
            r.needItem ? (
              <Button
                size="sm"
                intent="outline"
                onClick={() => setSetting(r)}
                title={r.hints.length ? `후보 ${r.hints.join(', ')}` : undefined}
                data-testid={`sched-set-item-${r.id}`}
              >
                품목 지정
              </Button>
            ) : null
          }
        />
      </div>
      {setting ? (
        <SetItemDialog
          row={setting}
          items={items.items}
          onClose={() => {
            setSetting(null)
            load()
          }}
        />
      ) : null}
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(o) => !o && setRemoving(null)}
        scope="single"
        danger
        title={removing ? `${removing.robot_name || `R${removing.robot}`} Hand 정리` : 'Hand 정리'}
        confirmLabel="Hand 비우기"
        onConfirm={() => {
          const a = removing
          setRemoving(null)
          if (a)
            void schedApi
              .handRemove(a.robot)
              .then((r) => {
                toast.ok(
                  `${a.robot_name || `R${a.robot}`} Hand 비움${r.canceled?.length ? ` · Task ${r.canceled.length}건 취소` : ''}`,
                )
                reload()
              })
              .catch((e) => toast.error(`Hand 정리 실패 — ${errText(e)}`))
        }}
      >
        {removing ? (
          <div className="flex flex-col gap-1 text-xs">
            <div className="font-mono">{handAlertText(removing)}</div>
            <div>{HAND_REMOVE_TEXT}</div>
          </div>
        ) : null}
      </ConfirmDialog>
    </div>
  )
}
