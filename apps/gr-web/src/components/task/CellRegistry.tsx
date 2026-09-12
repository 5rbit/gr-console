// 셀 레지스트리 — 로컬 사본 표 + 툴바(PLC 읽기/쓰기/차이, Excel). 행 상태 배지: PLC 동일 / 로컬 수정 / 로컬.
import { useMemo, useState } from 'react'
import { Grid3x3 } from 'lucide-react'
import { api } from '../../lib/api'
import { taskApi } from '../../lib/task/api'
import type { Registry } from '../../lib/registry'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'
import type { Cell, CellUpsert } from '../../lib/types'
import { CellForm, EMPTY_CELL } from './forms'
import { RegistryToolbar, type RegistryIo } from './RegistryToolbar'

/** 행 출처 배지 — 로컬 사본이 PLC와 어떤 관계인지. */
export function RowBadge({ source, dirty }: { source: 'plc' | 'local'; dirty: boolean }) {
  if (dirty) return <StatusBadge status="warn">로컬 수정</StatusBadge>
  if (source === 'plc') return <StatusBadge status="ok">PLC 동일</StatusBadge>
  return <StatusBadge status="info">로컬</StatusBadge>
}

const f1 = (n: number) => (Math.round(n * 10) / 10).toString()
export const cellSummary = (c: Cell): string =>
  `S${c.section} R${c.row} C${c.col} ${c.use ? '' : '(미사용)'} · ${c.position.map(f1).join('/')} · ${f1(c.length)}×${f1(c.width)}`

export function toCellUpsert(c: Cell): CellUpsert {
  return {
    id: c.id,
    use: c.use,
    blend_use: c.blend_use,
    section: c.section,
    row: c.row,
    col: c.col,
    length: c.length,
    width: c.width,
    position: [...c.position] as [number, number, number],
  }
}

export function matchCell(c: Cell, q: string): boolean {
  if (!q) return true
  const s = `${c.id} s${c.section} r${c.row} c${c.col} ${c.source}`.toLowerCase()
  return s.includes(q.toLowerCase())
}

const IO: RegistryIo<Cell> = {
  plc: {
    import: taskApi.cellsImport,
    push: taskApi.cellsPush,
    diff: taskApi.cellsDiff,
    summarize: cellSummary,
  },
  exportUrl: taskApi.cellsExportUrl,
  importFile: taskApi.cellsImportFile,
}

export interface CellRegistryProps {
  reg: Registry<Cell>
  q: string
  /** 바깥(레이아웃 편집 맵)이 선택을 쥘 때. */
  selectedId?: number | null
  onSelect?: (id: number | null) => void
  /** 좁은 사이드바 — 핵심 열만, 툴바 아이콘만. */
  compact?: boolean
}

export function CellRegistry({ reg, q, selectedId, onSelect, compact = false }: CellRegistryProps) {
  const [selLocal, setSelLocal] = useState<number | null>(null)
  const selected = selectedId !== undefined ? selectedId : selLocal
  const setSelected = (id: number | null) => {
    if (selectedId === undefined) setSelLocal(id)
    onSelect?.(id)
  }
  const [form, setForm] = useState<{
    open: boolean
    editing: boolean
    initial: CellUpsert
    key: number
  }>({ open: false, editing: false, initial: EMPTY_CELL, key: 0 })
  const [del, setDel] = useState(false)
  const rows = useMemo(() => reg.items.filter((c) => matchCell(c, q)), [reg.items, q])
  const sel = reg.items.find((c) => c.id === selected) ?? null
  const dirty = reg.items.filter((c) => c.dirty).length

  const columns: Column<Cell>[] = [
    { key: 'id', label: 'Id', get: (c) => c.id, numeric: true, class: 'font-mono' },
    {
      key: 'state',
      label: '상태',
      get: (c) => (c.dirty ? 1 : c.source === 'plc' ? 0 : 2),
      cell: (c) => <RowBadge source={c.source} dirty={c.dirty} />,
    },
    {
      key: 'use',
      label: '사용',
      get: (c) => (c.use ? 1 : 0),
      cell: (c) => (c.use ? 'Y' : <span className="text-slate-400">N</span>),
    },
    {
      key: 'blend',
      label: '블렌드',
      get: (c) => (c.blend_use ? 1 : 0),
      cell: (c) => (c.blend_use ? 'Y' : <span className="text-slate-400">N</span>),
    },
    { key: 'section', label: '구역', get: (c) => c.section, numeric: true },
    { key: 'row', label: '행', get: (c) => c.row, numeric: true },
    { key: 'col', label: '열', get: (c) => c.col, numeric: true },
    {
      key: 'x',
      label: 'X',
      get: (c) => c.position[0],
      numeric: true,
      cell: (c) => f1(c.position[0]),
    },
    {
      key: 'y',
      label: 'Y',
      get: (c) => c.position[1],
      numeric: true,
      cell: (c) => f1(c.position[1]),
    },
    {
      key: 'z',
      label: 'Z',
      get: (c) => c.position[2],
      numeric: true,
      cell: (c) => f1(c.position[2]),
    },
    { key: 'len', label: '길이', get: (c) => c.length, numeric: true, cell: (c) => f1(c.length) },
    { key: 'wid', label: '폭', get: (c) => c.width, numeric: true, cell: (c) => f1(c.width) },
  ]

  const COMPACT_KEYS = ['id', 'state', 'section', 'row', 'col', 'x', 'y', 'z']
  const shown = compact ? columns.filter((c) => COMPACT_KEYS.includes(c.key)) : columns

  async function save(v: CellUpsert) {
    if (form.editing) await api.cellUpdate(v.id, v)
    else await api.cellCreate(v)
    setForm((s) => ({ ...s, open: false }))
    await reg.reload()
    toast.ok(`셀 #${v.id} 저장됨 (로컬)`)
  }
  async function remove() {
    if (!sel) return
    try {
      await api.cellDelete(sel.id)
      toast.ok(`셀 #${sel.id} 삭제됨 (로컬)`)
      setSelected(null)
      await reg.reload()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e))
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="cell-registry">
      <RegistryToolbar<Cell>
        compact={compact}
        title="셀"
        icon={<Grid3x3 size={14} />}
        what="셀"
        rows={reg.items.length}
        dirty={dirty}
        selected={!!sel}
        io={IO}
        reload={reg.reload}
        onAdd={() => setForm({ open: true, editing: false, initial: EMPTY_CELL, key: Date.now() })}
        onEdit={() =>
          sel && setForm({ open: true, editing: true, initial: toCellUpsert(sel), key: Date.now() })
        }
        onDelete={() => setDel(true)}
      />
      <div className="min-h-0 flex-1 overflow-auto">
        {reg.error ? <p className="p-2 text-xs text-red-600">{reg.error}</p> : null}
        <DataTable
          rows={rows}
          columns={shown}
          rowKey={(c) => String(c.id)}
          selected={selected === null ? null : String(selected)}
          onPick={(c) => setSelected(c.id === selected ? null : c.id)}
          loading={reg.loading}
          empty={q ? '검색 결과 없음' : '셀 없음'}
          emptyHint={
            q
              ? undefined
              : 'PLC 읽기로 GR2 CELL 테이블을 가져오거나 추가/Excel 가져오기로 만드세요.'
          }
          testid="cell-table"
        />
      </div>
      {form.open ? (
        <CellForm
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
        title="셀 삭제"
        danger
        confirmLabel="삭제"
        onConfirm={() => void remove()}
      >
        <p className="text-sm">
          로컬 셀 <b>#{sel?.id}</b>을 지웁니다. PLC 테이블은 다음 <b>PLC 쓰기</b> 때 바뀝니다.
        </p>
        {sel ? <p className="mt-1 text-xs text-slate-500">{cellSummary(sel)}</p> : null}
      </ConfirmDialog>
    </div>
  )
}
