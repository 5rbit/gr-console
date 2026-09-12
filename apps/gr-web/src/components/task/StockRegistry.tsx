// 재고 레일 탭 — 등록된 셀마다 재고(품목·개수)를 보고 고친다. 완료된 PICK/DROP 은 백엔드가 자동 반영한다.
import { useEffect, useMemo, useState } from 'react'
import { Boxes, Eraser, Pencil } from 'lucide-react'
import { api } from '../../lib/api'
import { stock as stockStore } from '../../lib/stock'
import { useStore } from '../../lib/store'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { Input } from '../../lib/ui/Input'
import { Modal } from '../../lib/ui/Modal'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'
import { Toolbar } from '../../lib/ui/Toolbar'
import type { Cell, Item, StockEntry } from '../../lib/types'
import { ItemPicker } from '../shared/ItemPicker'
import { EMPTY_ITEM, ItemForm } from './forms'
import type { ItemUpsert } from '../../lib/types'

export interface StockEdit {
  cell: Cell
  item: number | null
  count: number
}

/** 셀 하나의 재고 편집 모달 — 레일 재고 탭과 레이아웃 정보 패널이 같이 쓴다. */
export function StockEditDialog({
  edit,
  items,
  onClose,
  onItemsChanged,
}: {
  edit: StockEdit | null
  items: readonly Item[]
  onClose: () => void
  /** 품목을 새로 등록했을 때(목록 다시 받기). */
  onItemsChanged?: () => void
}) {
  const [v, setV] = useState<StockEdit | null>(edit)
  const [busy, setBusy] = useState(false)
  const [newItem, setNewItem] = useState(false)
  async function createItem(it: ItemUpsert) {
    const created = await api.itemCreate(it)
    toast.ok(`품목 ${created.code} 등록`)
    onItemsChanged?.()
    setV((cur) => (cur ? { ...cur, item: created.code } : cur))
  }
  useEffect(() => setV(edit), [edit])
  async function save() {
    if (!v) return
    setBusy(true)
    try {
      await api.stockSet(v.cell.id, {
        item_code: v.item ?? 0,
        count: Math.max(0, Math.floor(v.count)),
      })
      toast.ok(`셀 #${v.cell.id} 재고 ${v.count}`)
      onClose()
    } catch (e) {
      toast.error(`재고 저장 실패 — ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setBusy(false)
    }
  }
  return (
    <Modal
      open={!!edit}
      onOpenChange={(o) => !o && onClose()}
      title={edit ? `셀 #${edit.cell.id} 재고` : ''}
    >
      {v ? (
        <div className="flex flex-col gap-3">
          <div className="flex items-end gap-2">
            <ItemPicker
              value={v.item}
              onChange={(item) => setV({ ...v, item })}
              items={[...items]}
            />
            <Button
              size="sm"
              intent="outline"
              onClick={() => setNewItem(true)}
              data-testid="stock-new-item"
            >
              새 품목 등록
            </Button>
          </div>
          <Input
            label="개수"
            type="number"
            mono
            min={0}
            max={99}
            step="1"
            className="w-24"
            value={String(v.count)}
            onValueChange={(s) => setV({ ...v, count: Number(s) })}
            data-testid="stock-count"
          />
          <p className="text-2xs text-content-muted">
            개수 0 이면 품목도 비웁니다. PICK/DROP 완료 시 백엔드가 자동으로 ±수량 합니다.
          </p>
          <div className="flex justify-end gap-2">
            <Button intent="ghost" onClick={onClose}>
              취소
            </Button>
            <Button
              intent="primary"
              loading={busy}
              onClick={() => void save()}
              data-testid="stock-save"
            >
              저장
            </Button>
          </div>
        </div>
      ) : null}
      <ItemForm
        open={newItem}
        onOpenChange={setNewItem}
        initial={EMPTY_ITEM}
        editing={false}
        onSave={createItem}
      />
    </Modal>
  )
}

interface Row {
  cell: Cell
  stock: StockEntry | null
}

export interface StockRegistryProps {
  cells: readonly Cell[]
  items: readonly Item[]
  q: string
  onItemsChanged?: () => void
}

export function StockRegistry({ cells, items, q, onItemsChanged }: StockRegistryProps) {
  useStore(stockStore)
  const [edit, setEdit] = useState<StockEdit | null>(null)
  const [clearAll, setClearAll] = useState(false)
  const version = stockStore.getSnapshot()
  const rows = useMemo<Row[]>(() => {
    const needle = q.toLowerCase()
    return cells
      .filter(
        (c) =>
          !needle ||
          `${c.id} s${c.section} r${c.row} c${c.col} ${stockStore.get(c.id)?.item_code ?? ''}`
            .toLowerCase()
            .includes(needle),
      )
      .map((c) => ({ cell: c, stock: stockStore.get(c.id) }))
    // eslint-disable-next-line react-hooks/exhaustive-deps -- version 이 스토어 변경을 대표한다
  }, [cells, q, version])
  const total = rows.reduce((a, r) => a + (r.stock?.count ?? 0), 0)
  const itemOf = (code?: number) => (code ? items.find((i) => i.code === code) : undefined)
  const openEdit = (r: Row) =>
    setEdit({ cell: r.cell, item: r.stock?.item_code || null, count: r.stock?.count ?? 0 })

  // `priority` — 재고 표를 훑는 이유는 **어느 셀에 몇 개 있나**다: 셀·수량이 1, 품목과 갱신
  // 시각이 2, 좌표·높이·출처가 3이다(`docs/DESIGN.md` 4절).
  const columns: Column<Row>[] = [
    {
      key: 'id',
      label: '셀',
      get: (r) => r.cell.id,
      numeric: true,
      class: 'font-mono',
      priority: 1,
    },
    {
      key: 'pos',
      label: '구역/행/열',
      get: (r) => `S${r.cell.section} R${r.cell.row} C${r.cell.col}`,
      priority: 3,
    },
    {
      key: 'count',
      label: '재고',
      get: (r) => r.stock?.count ?? 0,
      numeric: true,
      cell: (r) =>
        r.stock?.count ? (
          <b className="tabular-nums">{r.stock.count}</b>
        ) : (
          <span className="text-content-faint">0</span>
        ),
      priority: 1,
    },
    {
      key: 'item',
      label: '품목',
      get: (r) => r.stock?.item_code ?? 0,
      cell: (r) =>
        r.stock?.item_code ? (
          `${r.stock.item_code} ${itemOf(r.stock.item_code)?.name ?? ''}`
        ) : (
          <span className="text-content-faint">-</span>
        ),
      priority: 2,
    },
    {
      key: 'h',
      label: '적재 높이',
      get: (r) => (itemOf(r.stock?.item_code)?.height ?? 0) * (r.stock?.count ?? 0),
      numeric: true,
      cell: (r) => {
        const it = itemOf(r.stock?.item_code)
        return it && r.stock?.count ? (
          `${(it.height * r.stock.count).toFixed(0)} mm`
        ) : (
          <span className="text-content-faint">-</span>
        )
      },
      priority: 3,
    },
    {
      key: 'at',
      label: '갱신',
      get: (r) => r.stock?.updated_at ?? '',
      cell: (r) => (
        <span className="text-2xs text-content-muted">
          {r.stock?.updated_at?.slice(5, 19).replace('T', ' ') ?? ''}
        </span>
      ),
      priority: 2,
    },
    {
      key: 'src',
      label: '',
      get: () => '',
      cell: (r) => (r.cell.dirty ? <StatusBadge status="warn">셀 로컬 수정</StatusBadge> : null),
      priority: 3,
    },
  ]

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="stock-registry">
      <Toolbar
        icon={<Boxes size={14} />}
        title="재고"
        meta={`${rows.length}칸 · 합계 ${total}개 · 완료된 PICK/DROP 자동 반영`}
      >
        <span className="flex-1" />
        <Button
          size="sm"
          intent="ghost"
          icon={<Eraser className="h-3.5 w-3.5" />}
          onClick={() => setClearAll(true)}
          disabled={total === 0}
        >
          전체 비우기
        </Button>
      </Toolbar>
      <div className="min-h-0 flex-1 overflow-auto">
        <DataTable
          rows={rows}
          columns={columns}
          rowKey={(r) => String(r.cell.id)}
          empty="등록된 셀이 없습니다"
          onPick={openEdit}
          actions={(r) => (
            <Button
              size="icon-sm"
              intent="ghost"
              icon={<Pencil className="h-3.5 w-3.5" />}
              title="재고 편집"
              onClick={() => openEdit(r)}
            />
          )}
        />
      </div>
      <StockEditDialog
        edit={edit}
        items={items}
        onClose={() => setEdit(null)}
        onItemsChanged={onItemsChanged}
      />
      <ConfirmDialog
        open={clearAll}
        onOpenChange={setClearAll}
        scope="single"
        danger
        title="재고 전체 비우기"
        confirmLabel="비우기"
        onConfirm={() => void api.stockClear().then((r) => toast.ok(`${r.removed}칸 비움`))}
      >
        <p className="text-xs">모든 셀의 재고를 0 으로 만듭니다. 되돌릴 수 없습니다.</p>
      </ConfirmDialog>
    </div>
  )
}
