// 품목(타이어 코드) 레지스트리 — 콘솔 전용(PLC 테이블 없음). Excel은 품목 전용(`Items` 시트).
// 넓은 표·복제·여러 건 삭제는 `화물 규격` 화면(`components/items/ItemsPage`)이 맡고, 여기서는 그리로 간다.
import { useMemo, useState } from 'react'
import { Maximize2, Package } from 'lucide-react'
import { api } from '../../lib/api'
import { nav } from '../../lib/nav'
import { taskApi } from '../../lib/task/api'
import { Button } from '../../lib/ui/Button'
import type { Registry } from '../../lib/registry'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'
import type { Item, ItemUpsert } from '../../lib/types'
import { EMPTY_ITEM, ItemForm } from './forms'
import { RegistryToolbar, type RegistryIo } from './RegistryToolbar'

const f1 = (n: number) => (Math.round(n * 10) / 10).toString()

export function toItemUpsert(i: Item): ItemUpsert {
  return {
    code: i.code,
    name: i.name,
    count: i.count,
    inner_diameter: i.inner_diameter,
    outer_diameter: i.outer_diameter,
    lower_bead_height: i.lower_bead_height,
    upper_bead_height: i.upper_bead_height,
    height: i.height,
    deflection_factor: i.deflection_factor,
    note: i.note,
    // 서버가 준 규격을 그대로 싣는다(없으면 빼서 서버가 저장된 값을 두게).
    ...(i.spec ? { spec: i.spec } : {}),
  }
}

export function matchItem(i: Item, q: string): boolean {
  if (!q) return true
  return `${i.code} ${i.name} ${i.note}`.toLowerCase().includes(q.toLowerCase())
}

const IO: RegistryIo<Item> = {
  exportUrl: taskApi.itemsExportUrl,
  importFile: taskApi.itemsImportFile,
}

export interface ItemRegistryProps {
  reg: Registry<Item>
  q: string
}

export function ItemRegistry({ reg, q }: ItemRegistryProps) {
  const [selected, setSelected] = useState<number | null>(null)
  const [form, setForm] = useState<{
    open: boolean
    editing: boolean
    initial: ItemUpsert
    key: number
  }>({ open: false, editing: false, initial: EMPTY_ITEM, key: 0 })
  const [del, setDel] = useState(false)
  const rows = useMemo(() => reg.items.filter((i) => matchItem(i, q)), [reg.items, q])
  const sel = reg.items.find((i) => i.code === selected) ?? null

  // `priority` — 품목은 **무엇이 몇 개 있고 치수가 얼마인가**로 훑는다: 코드·이름이 1, 수량과
  // 명령에 직접 쓰는 내경·높이가 2, 나머지 치수와 비고가 3이다.
  const columns: Column<Item>[] = [
    {
      key: 'code',
      label: 'Code',
      get: (i) => i.code,
      numeric: true,
      class: 'font-mono',
      priority: 1,
    },
    { key: 'name', label: 'Name', get: (i) => i.name, priority: 1 },
    { key: 'count', label: 'Count', get: (i) => i.count, numeric: true, priority: 2 },
    {
      key: 'id',
      label: 'InnerDiameter',
      get: (i) => i.inner_diameter,
      numeric: true,
      cell: (i) => f1(i.inner_diameter),
      priority: 2,
    },
    {
      key: 'od',
      label: 'OuterDiameter',
      get: (i) => i.outer_diameter,
      numeric: true,
      cell: (i) => f1(i.outer_diameter),
      priority: 3,
    },
    {
      key: 'h',
      label: 'Height',
      get: (i) => i.height,
      numeric: true,
      cell: (i) => f1(i.height),
      priority: 2,
    },
    {
      key: 'lb',
      label: 'LowerBidHeight',
      get: (i) => i.lower_bead_height,
      numeric: true,
      cell: (i) => f1(i.lower_bead_height),
      priority: 3,
    },
    {
      key: 'ub',
      label: 'UpperBidHeight',
      get: (i) => i.upper_bead_height,
      numeric: true,
      cell: (i) => f1(i.upper_bead_height),
      priority: 3,
    },
    {
      key: 'df',
      label: 'DeflectionFactor',
      get: (i) => i.deflection_factor,
      numeric: true,
      priority: 3,
    },
    { key: 'note', label: 'Note', get: (i) => i.note, class: 'text-content-muted', priority: 3 },
  ]

  async function save(v: ItemUpsert) {
    if (form.editing) await api.itemUpdate(v.code, v)
    else await api.itemCreate(v)
    setForm((s) => ({ ...s, open: false }))
    await reg.reload()
    toast.ok(`품목 ${v.code} 저장됨`)
  }
  async function remove() {
    if (!sel) return
    try {
      await api.itemDelete(sel.code)
      toast.ok(`품목 ${sel.code} 삭제됨`)
      setSelected(null)
      await reg.reload()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e))
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="item-registry">
      <RegistryToolbar<Item>
        title="품목"
        icon={<Package size={14} />}
        what="품목"
        rows={reg.items.length}
        dirty={0}
        selected={!!sel}
        io={IO}
        reload={reg.reload}
        onAdd={() => setForm({ open: true, editing: false, initial: EMPTY_ITEM, key: Date.now() })}
        onEdit={() =>
          sel && setForm({ open: true, editing: true, initial: toItemUpsert(sel), key: Date.now() })
        }
        onDelete={() => setDel(true)}
        extra={
          <Button
            size="sm"
            intent="ghost"
            icon={<Maximize2 className="h-3.5 w-3.5" />}
            onClick={() => nav.go('items')}
            title="화물 규격 화면에서 열기 — 넓은 표 · 복제 · 여러 건 삭제"
            data-testid="item-open-page"
          >
            규격 화면
          </Button>
        }
      />
      <div className="min-h-0 flex-1 overflow-auto">
        {reg.error ? <p className="p-2 text-xs text-fault-fg">{reg.error}</p> : null}
        <DataTable
          rows={rows}
          columns={columns}
          rowKey={(i) => String(i.code)}
          selected={selected === null ? null : String(selected)}
          onPick={(i) => setSelected(i.code === selected ? null : i.code)}
          loading={reg.loading}
          empty={q ? '검색 결과 없음' : '품목 없음'}
          emptyHint={
            q
              ? '검색어를 지우거나 Code·Name 의 다른 조각으로 찾으세요.'
              : '추가 버튼이나 Excel 가져오기(Items 시트)로 타이어 코드를 등록하세요.'
          }
          testid="item-table"
        />
      </div>
      {form.open ? (
        <ItemForm
          key={form.key}
          open={form.open}
          onOpenChange={(o) => setForm((s) => ({ ...s, open: o }))}
          initial={form.initial}
          editing={form.editing}
          onSave={save}
        />
      ) : null}
      <ConfirmDialog
        open={del}
        onOpenChange={setDel}
        scope="single"
        title="품목 삭제"
        danger
        confirmLabel="삭제"
        onConfirm={() => void remove()}
      >
        <p className="text-sm">
          품목 <b>{sel?.code}</b> {sel?.name}을 지웁니다. 이 코드를 쓰는 시나리오 스텝은 제출 때
          실패합니다.
        </p>
      </ConfirmDialog>
    </div>
  )
}
