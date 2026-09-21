// 재고 레일 탭 — 등록된 셀마다 재고(품목·개수)를 보고 고친다. 완료된 PICK/DROP 은 백엔드가 자동 반영한다.
import { useEffect, useMemo, useState } from 'react'
import { ArrowLeft, Boxes, Eraser, Pencil } from 'lucide-react'
import { api } from '../../lib/api'
import { stock as stockStore } from '../../lib/stock'
import { useStore } from '../../lib/store'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { FieldList } from '../../lib/ui/FieldList'
import { FormDialog } from '../../lib/ui/Dialog'
import { HelpTip } from '../../lib/ui/HelpTip'
import { Input } from '../../lib/ui/Input'
import { f1 } from '../../lib/meas/format'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'
import { Toolbar } from '../../lib/ui/Toolbar'
import type { Cell, Item, StockEntry } from '../../lib/types'
import { ItemPicker } from '../shared/ItemPicker'
import { EMPTY_ITEM, FormErrors, ItemFields, validateItem } from './forms'
import type { ItemUpsert } from '../../lib/types'

export interface StockEdit {
  cell: Cell
  item: number | null
  count: number
}

/**
 * 셀 하나의 재고 편집 팝업 — 레일 재고 탭과 레이아웃 정보 패널이 같이 쓴다.
 *
 * **한 팝업, 두 단계다.** 전에는 "새 품목 등록" 이 이 모달 **위에 또 모달**을 띄웠다(오버레이 예산:
 * 두 번째 모달 금지 — `docs/DESIGN.md` 5절). 겹친 팝업은 Escape 가 어느 것을 닫는지, 뒤에 깔린
 * 값이 아직 살아 있는지를 매번 다시 확인하게 만든다. 그래서 같은 상자 안에서 **본문과 바닥 띠만**
 * 갈아 끼운다(대화상자가 이미 `dirty` 확인에 쓰는 관용구와 같다): 재고 → 새 품목 → 등록하면 그
 * 품목이 골라진 채 재고로 돌아온다.
 */
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
  /** 지금 보고 있는 단계 — 재고 값이냐, 새 품목 칸이냐. */
  const [step, setStep] = useState<'stock' | 'item'>('stock')
  const [draft, setDraft] = useState<ItemUpsert>(EMPTY_ITEM)
  const [errors, setErrors] = useState<string[]>([])
  useEffect(() => {
    setV(edit)
    // 팝업이 다시 열리면 언제나 재고 단계부터 — 지난번에 품목 칸에서 닫았다고 그 자리로 열지 않는다.
    setStep('stock')
    setDraft(EMPTY_ITEM)
    setErrors([])
  }, [edit])

  async function createItem() {
    const errs = validateItem(draft)
    setErrors(errs)
    if (errs.length) return
    setBusy(true)
    try {
      const created = await api.itemCreate(draft)
      toast.ok(`품목 ${created.code} 등록`)
      onItemsChanged?.()
      setV((cur) => (cur ? { ...cur, item: created.code } : cur))
      setDraft(EMPTY_ITEM)
      setErrors([])
      setStep('stock')
    } catch (e) {
      setErrors([e instanceof Error ? e.message : String(e)])
    } finally {
      setBusy(false)
    }
  }

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

  const stockDirty = !!v && !!edit && (v.item !== edit.item || v.count !== edit.count)
  const itemDirty = step === 'item' && JSON.stringify(draft) !== JSON.stringify(EMPTY_ITEM)
  return (
    <FormDialog
      open={!!edit}
      onOpenChange={(o) => !o && onClose()}
      title={edit ? `셀 #${edit.cell.id} 재고` : ''}
      meta={step === 'item' ? '새 품목' : undefined}
      // 품목 칸은 2~3 열 격자라 좁은 상자에서는 라벨이 값보다 길어진다.
      size={step === 'item' ? 'lg' : 'sm'}
      dirty={stockDirty || itemDirty}
      busy={busy}
      submitLabel={step === 'item' ? '품목 등록' : '저장'}
      onSubmit={() => void (step === 'item' ? createItem() : save())}
      extra={
        step === 'item' ? (
          <Button
            size="sm"
            intent="ghost"
            icon={<ArrowLeft className="h-3.5 w-3.5" />}
            onClick={() => {
              setStep('stock')
              setErrors([])
            }}
            data-testid="stock-item-back"
          >
            재고로 돌아가기
          </Button>
        ) : (
          <Button
            size="sm"
            intent="outline"
            onClick={() => setStep('item')}
            data-testid="stock-new-item"
          >
            새 품목 등록
          </Button>
        )
      }
      testid="stock-edit"
    >
      {step === 'item' ? (
        <>
          <ItemFields value={draft} onChange={setDraft} />
          <FormErrors list={errors} />
        </>
      ) : v ? (
        <div className="flex flex-col gap-3">
          <ItemPicker value={v.item} onChange={(item) => setV({ ...v, item })} items={[...items]} />
          <div className="flex items-end gap-1.5">
            <Input
              label="Count"
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
            <span className="pb-2">
              <HelpTip
                title="Count"
                text="0 이면 품목도 함께 비웁니다. PICK/DROP 이 끝나면 백엔드가 이 값을 자동으로 ±합니다."
              />
            </span>
          </div>
        </div>
      ) : null}
    </FormDialog>
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
      label: 'CellId',
      get: (r) => r.cell.id,
      numeric: true,
      class: 'font-mono',
      priority: 1,
    },
    {
      key: 'pos',
      label: 'Section/Row/Col',
      get: (r) => `S${r.cell.section} R${r.cell.row} C${r.cell.col}`,
      priority: 3,
    },
    {
      key: 'count',
      label: 'Count',
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
      label: 'ItemCode',
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
      label: 'StackHeight (mm)',
      get: (r) => (itemOf(r.stock?.item_code)?.height ?? 0) * (r.stock?.count ?? 0),
      numeric: true,
      cell: (r) => {
        const it = itemOf(r.stock?.item_code)
        return it && r.stock?.count ? (
          f1(it.height * r.stock.count)
        ) : (
          <span className="text-content-faint">-</span>
        )
      },
      priority: 3,
    },
    {
      key: 'at',
      label: 'UpdatedAt',
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
      <Toolbar icon={<Boxes size={14} />} title="재고" meta={`${rows.length}칸 · 합계 ${total}개`}>
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
          empty={q ? '검색 결과 없음' : '등록된 셀 없음'}
          emptyHint={
            q
              ? '검색어를 지우거나 Id·Section·Row·Col 의 다른 조각으로 찾으세요.'
              : '셀 표에서 PLC 읽기·Excel 가져오기로 셀을 먼저 등록하세요.'
          }
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
        <div className="flex flex-col gap-2 text-xs">
          <p className="m-0">모든 셀의 재고를 비울까요?</p>
          <FieldList
            columns={2}
            dense
            labelWidth={48}
            items={[
              { label: '대상', value: `${rows.length}칸` },
              { label: '합계', value: `${total}개` },
            ]}
          />
          <p className="m-0 text-fault-fg">되돌릴 수 없습니다.</p>
        </div>
      </ConfirmDialog>
    </div>
  )
}
