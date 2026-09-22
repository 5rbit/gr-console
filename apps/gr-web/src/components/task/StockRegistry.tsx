// 재고 레일 탭 — 등록된 셀마다 재고(품목·개수)를 보고 고친다. 완료된 PICK/DROP 은 백엔드가 자동 반영한다.
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
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
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import { menuItems, type MenuEntry } from '../../lib/task/menuEntries'
import type { Cell, Item, Station, StockEntry } from '../../lib/types'
import { stationAsCell } from '../../lib/task/stationCell'
import { taskApi } from '../../lib/task/api'
import { SHEET_ACCEPT, importOutcome, outcomeSummary } from '../../lib/task/importPreview'
import type { FileImportResult } from '../../lib/task/types'
import { Segmented } from '../../lib/ui/Segmented'
import { ImportDialog } from './ImportDialog'
import { ItemPicker } from '../shared/ItemPicker'
import { EMPTY_ITEM, FormErrors, ItemFields, validateItem } from './forms'
import type { ItemUpsert } from '../../lib/types'

/** 재고 자리 이름 — 스테이션(Id > 2000)도 재고를 가진다(컨베이어 화물 코드). */
function placeName(id: number): string {
  return `${id > 2000 ? '스테이션' : '셀'} #${id}`
}

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
      toast.ok(`${placeName(v.cell.id)} 재고 ${v.count}`)
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
      title={edit ? `${placeName(edit.cell.id)} 재고` : ''}
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
  /** 스테이션 행 — ConvNo(셀이면 없음). */
  conv?: number
}

export interface StockRegistryProps {
  cells: readonly Cell[]
  /** 스테이션 — 컨베이어 화물 코드도 재고라 셀 뒤에 같은 표로 싣는다(코드 지정 = 재고 편집). */
  stations?: readonly Station[]
  items: readonly Item[]
  q: string
  onItemsChanged?: () => void
  /** 고른 셀(= 맵 선택) — 행을 누르면 맵이 그 셀을 강조한다. 고치는 일은 연필 버튼. */
  selectedId?: number | null
  onSelect?: (id: number | null) => void
  /** 레일이 넘기는 머리줄 조작(표 종류 토글 + 검색)과 ⋯ 보기 항목. */
  lead?: ReactNode
  menuExtra?: MenuEntry[]
}

export function StockRegistry({
  cells,
  stations = [],
  items,
  q,
  onItemsChanged,
  selectedId,
  onSelect,
  lead,
  menuExtra = [],
}: StockRegistryProps) {
  useStore(stockStore)
  const [selLocal, setSelLocal] = useState<number | null>(null)
  const selected = selectedId !== undefined ? selectedId : selLocal
  const pick = (id: number) => {
    const next = id === selected ? null : id
    if (selectedId === undefined) setSelLocal(next)
    onSelect?.(next)
  }
  // 맵에서 고른 셀이 표 밖(스크롤 아래)에 있으면 선택이 안 보인다 — 선택 행을 끌어온다.
  const box = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (selected === null) return
    box.current
      ?.querySelector('[data-testid="dt-row"].bg-accent-soft')
      ?.scrollIntoView({ block: 'nearest' })
  }, [selected])
  const [edit, setEdit] = useState<StockEdit | null>(null)
  const [clearAll, setClearAll] = useState(false)
  // Excel 가져오기 — 파일 → 미리보기(dry-run) → 적용. 방식(병합/교체)을 바꾸면 같은 파일로 미리보기를 다시 한다.
  const fileRef = useRef<HTMLInputElement>(null)
  const exportRef = useRef<HTMLAnchorElement>(null)
  const [mode, setMode] = useState<'merge' | 'replace'>('merge')
  const [imp, setImp] = useState<{
    open: boolean
    file: File | null
    preview: FileImportResult | null
    error: string | null
    applying: boolean
  }>({ open: false, file: null, preview: null, error: null, applying: false })
  async function previewFile(file: File, m = mode) {
    setImp({ open: true, file, preview: null, error: null, applying: false })
    try {
      const preview = await taskApi.stockImportFile(file, true, m)
      setImp((s) => (s.file === file ? { ...s, preview } : s))
    } catch (e) {
      setImp((s) => ({ ...s, error: e instanceof Error ? e.message : String(e) }))
    }
  }
  async function applyFile() {
    if (!imp.file) return
    setImp((s) => ({ ...s, applying: true }))
    try {
      const r = await taskApi.stockImportFile(imp.file, false, mode)
      const head = outcomeSummary('재고', importOutcome(r))
      if (r.errors.length) toast.warn(`${head} · 오류 ${r.errors.length} (${r.errors[0].message})`)
      else toast.ok(head)
      setImp({ open: false, file: null, preview: null, error: null, applying: false })
    } catch (e) {
      setImp((s) => ({ ...s, applying: false, error: e instanceof Error ? e.message : String(e) }))
    }
  }
  const menu: MenuEntry[] = [
    { label: 'Excel' },
    {
      label: 'Excel 가져오기…',
      testid: 'stock-import',
      run: () => {
        setImp({ open: true, file: null, preview: null, error: null, applying: false })
        fileRef.current?.click()
      },
    },
    { label: 'Excel 내보내기', testid: 'stock-export', run: () => exportRef.current?.click() },
    ...menuExtra,
  ]
  const version = stockStore.getSnapshot()
  const rows = useMemo<Row[]>(() => {
    const needle = q.toLowerCase()
    const all: Row[] = [
      ...cells.map((c) => ({ cell: c, stock: stockStore.get(c.id) })),
      ...stations.map((s) => ({
        cell: stationAsCell(s),
        stock: stockStore.get(s.id),
        conv: s.conv_no,
      })),
    ]
    return all.filter(
      (r) =>
        !needle ||
        `${r.cell.id} s${r.cell.section} r${r.cell.row} c${r.cell.col} ${r.conv ? `cv${r.conv} station` : ''} ${r.stock?.item_code ?? ''}`
          .toLowerCase()
          .includes(needle),
    )
    // eslint-disable-next-line react-hooks/exhaustive-deps -- version 이 스토어 변경을 대표한다
  }, [cells, stations, q, version])
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
      get: (r) =>
        r.conv !== undefined
          ? `Station · CV ${r.conv}`
          : `S${r.cell.section} R${r.cell.row} C${r.cell.col}`,
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
      <Toolbar
        icon={<Boxes size={14} />}
        title="재고"
        lead={lead}
        dense
        meta={`${rows.length}칸 · 합계 ${total}개`}
      >
        <Button
          size="sm"
          intent="ghost"
          icon={<Eraser className="h-3.5 w-3.5" />}
          onClick={() => setClearAll(true)}
          disabled={total === 0}
        >
          전체 비우기
        </Button>
        <OverflowMenu items={menuItems(menu)} title="Excel · 보기" testid="stock-more" />
        <input
          ref={fileRef}
          type="file"
          accept={SHEET_ACCEPT}
          className="hidden"
          aria-hidden="true"
          onChange={(e) => {
            const f = e.target.files?.[0]
            e.target.value = ''
            if (f) void previewFile(f)
          }}
        />
        <a
          ref={exportRef}
          href={taskApi.stockExportUrl}
          download
          className="hidden"
          aria-hidden="true"
        />
      </Toolbar>
      <div className="min-h-0 flex-1 overflow-auto" ref={box}>
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
          selected={selected === null ? null : String(selected)}
          onPick={(r) => pick(r.cell.id)}
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
      <ImportDialog
        open={imp.open}
        onOpenChange={(o) => !o && setImp((s) => ({ ...s, open: false }))}
        what="재고"
        file={imp.file}
        preview={imp.preview}
        error={imp.error}
        applying={imp.applying}
        onPick={() => fileRef.current?.click()}
        onFile={(f) => void previewFile(f)}
        onApply={() => void applyFile()}
        scopeLabel="콘솔 재고에 적용"
        help={[
          {
            title: '파일',
            body: 'Stock 시트(없으면 첫 시트)의 CellId · ItemCode · Count · Note 열. Excel 내보내기 파일을 그대로 고쳐 올리면 됩니다. 셀과 스테이션(컨베이어 화물 코드) 모두 받습니다. Count 0 = 그 자리 비움.',
          },
          {
            title: '병합 / 교체',
            body: '병합 = 파일에 있는 자리만 바꿉니다. 교체 = 파일이 재고 전체라고 보고 파일에 없는 자리는 비웁니다(오류 난 줄의 자리는 남김).',
          },
          {
            title: '검사',
            body: '등록된 셀·스테이션인지, 파일 안 중복, 등록된 품목인지, 품목 StackMax 를 넘는지. 오류 줄은 건너뛰고 나머지만 적용합니다. PLC 에는 쓰지 않습니다.',
          },
        ]}
        extra={
          <div className="flex items-center gap-2">
            <span className="text-content-muted">적용 방식</span>
            <Segmented<'merge' | 'replace'>
              ariaLabel="적용 방식"
              value={mode}
              onChange={(m) => {
                setMode(m)
                if (imp.file) void previewFile(imp.file, m)
              }}
              options={[
                { id: 'merge', label: '병합', testid: 'stock-mode-merge' },
                { id: 'replace', label: '교체', testid: 'stock-mode-replace' },
              ]}
            />
            <span className="text-2xs text-content-faint">
              {mode === 'merge' ? '파일에 있는 자리만 바꿈' : '파일에 없는 자리는 비움'}
            </span>
          </div>
        }
      />
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
