// 재고 스냅샷 · 되돌리기 — 전체 비우기 · Excel 가져오기 · 되돌리기 직전마다 백엔드가 떠 둔 재고 표 전체.
//
// 한 팝업, 두 단계다(오버레이 예산: 두 번째 모달 금지 — `docs/DESIGN.md` 5절). 목록에서 되돌리기를
// 누르면 같은 상자가 **무엇이 무엇으로 바뀌는지** 보여 주는 확인 단계로 바뀐다.
import { useEffect, useState } from 'react'
import { ArrowLeft, Undo2 } from 'lucide-react'
import { api } from '../../lib/api'
import { Badge } from '../../lib/ui/Badge'
import { Button } from '../../lib/ui/Button'
import { DataTable } from '../../lib/ui/DataTable'
import { Dialog } from '../../lib/ui/Dialog'
import { FieldList } from '../../lib/ui/FieldList'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'
import { reasonLabel, shortTime } from '../../lib/task/stockSnapshot'
import type { StockSnapshotInfo } from '../../lib/types'

export interface StockSnapshotsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** 지금 재고(표 전체, 검색과 무관) — 확인 단계의 "지금" 쪽. */
  current: { cells: number; total: number }
}

export function StockSnapshotsDialog({ open, onOpenChange, current }: StockSnapshotsDialogProps) {
  const [list, setList] = useState<StockSnapshotInfo[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [pick, setPick] = useState<StockSnapshotInfo | null>(null)
  const [busy, setBusy] = useState(false)

  function load() {
    setError(null)
    api
      .stockSnapshots(20)
      .then(setList)
      .catch((e) => setError(e instanceof Error ? e.message : String(e)))
  }
  useEffect(() => {
    if (!open) return
    setPick(null)
    setList(null)
    load()
  }, [open])

  async function restore(s: StockSnapshotInfo) {
    setBusy(true)
    try {
      const r = await api.stockRestore(s.id)
      toast.ok(
        `스냅샷 #${r.restored} 로 되돌림 — ${r.row_count}칸 · ${r.total}개 (되돌리기 전 상태는 #${r.snapshot_id})`,
      )
      onOpenChange(false)
    } catch (e) {
      toast.error(`되돌리기 실패 — ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  const columns: Column<StockSnapshotInfo>[] = [
    { key: 'id', label: 'Id', get: (s) => s.id, numeric: true, class: 'font-mono', priority: 2 },
    {
      key: 'at',
      label: 'CreatedAt',
      get: (s) => s.created_at,
      cell: (s) => <span className="tabular-nums">{shortTime(s.created_at)}</span>,
      priority: 1,
    },
    { key: 'reason', label: 'Reason', get: (s) => reasonLabel(s), priority: 1 },
    { key: 'cells', label: 'Cells', get: (s) => s.row_count, numeric: true, priority: 1 },
    { key: 'total', label: 'Total', get: (s) => s.total, numeric: true, priority: 1 },
    {
      key: 'restored',
      label: 'RestoredAt',
      get: (s) => s.restored_at ?? '',
      cell: (s) => <span className="text-2xs text-content-muted">{shortTime(s.restored_at)}</span>,
      priority: 3,
    },
  ]

  const scope = <Badge className="bg-warn-soft text-warn-fg">콘솔 데이터 전체 · 로봇 무관</Badge>

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={pick ? `스냅샷 #${pick.id} 로 되돌리기` : '재고 스냅샷'}
      meta={pick ? scope : `최근 ${list?.length ?? 0}개`}
      size="lg"
      testid="stock-snapshots"
      closeLabel={pick ? undefined : '닫기'}
      footer={
        pick ? (
          <>
            <Button
              size="sm"
              intent="ghost"
              icon={<ArrowLeft className="h-3.5 w-3.5" />}
              onClick={() => setPick(null)}
              data-autofocus
            >
              목록으로
            </Button>
            <Button
              size="sm"
              intent="danger"
              loading={busy}
              onClick={() => void restore(pick)}
              data-testid="stock-restore-execute"
            >
              되돌리기
            </Button>
          </>
        ) : undefined
      }
    >
      {pick ? (
        <div className="flex flex-col gap-2">
          <FieldList
            columns={1}
            dense
            items={[
              {
                label: '스냅샷',
                value: `#${pick.id} · ${reasonLabel(pick)} · ${shortTime(pick.created_at)}`,
              },
              { label: '지금', value: `${current.cells}칸 · ${current.total}개` },
              { label: '되돌린 뒤', value: `${pick.row_count}칸 · ${pick.total}개` },
            ]}
          />
          {/* design-lint-allow: no-panel-paragraph — 되돌리기 확인 단계의 확인 문구(대화상자 안, ConfirmDialog 대신 같은 상자) */}
          <p className="m-0 text-content-tertiary">
            지금 재고(셀·스테이션)를 이 스냅샷으로 통째로 바꿉니다. 스냅샷 뒤의 수정과 완료된
            PICK/DROP 반영분도 바뀝니다. PLC·로봇에는 쓰지 않습니다. 지금 상태는 먼저 새
            스냅샷(되돌리기 전)으로 남으므로 이 되돌리기도 다시 되돌릴 수 있습니다.
          </p>
        </div>
      ) : (
        <DataTable
          rows={list ?? []}
          columns={columns}
          rowKey={(s) => String(s.id)}
          loading={list === null && !error}
          empty={error ? '스냅샷을 읽지 못했습니다' : '스냅샷 없음'}
          emptyHint={
            error ??
            '전체 비우기 · Excel 가져오기 · 되돌리기를 하면 그 직전 재고가 여기에 남습니다.'
          }
          actions={(s) => (
            <Button
              size="sm"
              intent="ghost"
              icon={<Undo2 className="h-3.5 w-3.5" />}
              onClick={() => setPick(s)}
              data-testid="stock-restore"
            >
              되돌리기
            </Button>
          )}
        />
      )}
    </Dialog>
  )
}
