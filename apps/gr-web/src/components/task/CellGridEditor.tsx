// 셀 그리드 편집기(엑셀형) — 레이아웃 편집 모드 우측 사이드바.
//
// 칸을 바로 고치고(입력 시작·Enter·더블클릭), 엑셀에서 범위를 복사해 붙여넣고(Ctrl+V), 행을 더하거나 지운 뒤
// "적용" 으로 한 번에 저장한다(로컬 사본 · PLC 반영은 툴바의 PLC 쓰기). 저장 전 초안도 맵에 바로 그려진다.
// 되돌리기(Ctrl+Z)는 그리드가, 편집 전체 취소는 "되돌리기" 버튼이 맡는다.
import { useEffect, useMemo, useRef, useState } from 'react'
import { Grid3x3, Plus, Save, Trash2, Undo2 } from 'lucide-react'
import { api } from '../../lib/api'
import type { Registry } from '../../lib/registry'
import { taskApi } from '../../lib/task/api'
import {
  applyCellEdit,
  boolText,
  cellToUpsert,
  diffDraft,
  draftFrom,
  isCellIdOk,
  newCellRow,
  pasteBool,
  pasteInt,
  pasteNum,
  savedKey,
  setRow,
  type DraftRow,
} from '../../lib/task/registryGrid'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataGrid, type DataGridColumn } from '../../lib/ui/datagrid/DataGrid'
import { toast } from '../../lib/ui/toast'
import type { Cell, CellUpsert } from '../../lib/types'
import { cellSummary, matchCell } from './CellRegistry'
import { cellPositionErrors } from './forms'
import { RegistryToolbar, type RegistryIo } from './RegistryToolbar'

type Row = DraftRow<CellUpsert>

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

export interface CellGridEditorProps {
  reg: Registry<Cell>
  q: string
  selectedId: number | null
  onSelect: (id: number | null) => void
  /** 저장 전 초안 — 맵 미리보기용. 편집기가 사라지면 null. */
  onDraft: (cells: Cell[] | null) => void
}

export function CellGridEditor({ reg, q, selectedId, onSelect, onDraft }: CellGridEditorProps) {
  const saved = reg.items
  // 초안이 어느 저장본에서 나왔는지 — 첫 로드·PLC 읽기로 목록이 바뀌어도 "전부 삭제" 로 오인하지 않게 이것과 비교한다.
  const [base, setBase] = useState<readonly Cell[]>(saved)
  const savedById = useMemo(() => new Map(saved.map((c) => [c.id, c])), [saved])
  const original = useMemo(
    () => new Map(base.map((c) => [savedKey(c.id), cellToUpsert(c)])),
    [base],
  )
  const [rows, setRows] = useState<Row[]>(() => draftFrom(saved.map(cellToUpsert)))
  const [selKey, setSelKey] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [confirm, setConfirm] = useState(false)
  const [stale, setStale] = useState(false)
  const justSaved = useRef(false)
  const diff = useMemo(
    () =>
      diffDraft(
        rows,
        base.map((c) => c.id),
      ),
    [rows, base],
  )
  const pending = diff.upserts.length + diff.deletes.length
  const pendingRef = useRef(pending)
  pendingRef.current = pending

  // 저장본이 바뀌면(PLC 읽기·Excel·적용 뒤) 편집 중이 아닐 때만 초안을 새로 받는다.
  useEffect(() => {
    if (saved === base) return
    if (pendingRef.current === 0 || justSaved.current) {
      setBase(saved)
      setRows(draftFrom(saved.map(cellToUpsert)))
      setStale(false)
      justSaved.current = false
    } else {
      setStale(true)
    }
  }, [saved])

  // 맵 미리보기 — 수정·신규 행은 점선(로컬 수정)으로 보인다.
  useEffect(() => {
    onDraft(
      rows.map((r) => {
        const s = savedById.get(r.value.id)
        return {
          ...r.value,
          source: s?.source ?? 'local',
          dirty: r.state !== 'same' || (s?.dirty ?? true),
          updated_at: s?.updated_at ?? '',
        }
      }),
    )
  }, [rows, savedById, onDraft])
  useEffect(() => () => onDraft(null), [onDraft])

  // 맵에서 고른 셀 → 행 선택
  useEffect(() => {
    if (selectedId === null) return
    const r = rows.find((x) => x.value.id === selectedId)
    if (r) setSelKey(r.key)
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 선택 변경에만 반응
  }, [selectedId])

  const dup = useMemo(() => new Set(diff.dupIds), [diff.dupIds])
  const idError = (c: CellUpsert) =>
    dup.has(c.id) ? `id ${c.id} 중복` : isCellIdOk(c.id) ? null : '셀 id 는 1..1000'
  const sectionError = (c: CellUpsert) => ([1, 2, 3].includes(c.section) ? null : '구역은 1..3')
  const posError = (c: CellUpsert, axis: 'X' | 'Y' | 'Z') =>
    cellPositionErrors(c.position).find((e) => e.includes(`위치 ${axis}`)) ?? null
  const errorCount = rows.filter(
    (r) =>
      idError(r.value) || sectionError(r.value) || cellPositionErrors(r.value.position).length > 0,
  ).length

  const shown = useMemo(() => {
    const needle = q.trim()
    if (!needle) return rows
    return rows.filter((r) =>
      matchCell({ ...r.value, source: 'local', dirty: false, updated_at: '' }, needle),
    )
  }, [rows, q])

  const stateLabel = (r: Row) =>
    r.state === 'added'
      ? '신규'
      : r.state === 'changed'
        ? '수정'
        : savedById.get(r.value.id)?.dirty
          ? '로컬'
          : 'PLC'
  const num = (
    id: string,
    header: string,
    get: (c: CellUpsert) => number,
    extra: Partial<DataGridColumn<Row>> = {},
  ): DataGridColumn<Row> => ({
    id,
    header,
    width: '4.5rem',
    align: 'right',
    mono: true,
    editor: 'number',
    decimals: 0,
    text: (r) => String(get(r.value)),
    coercePaste: (v) => pasteNum(v),
    ...extra,
  })
  const columns: DataGridColumn<Row>[] = [
    {
      id: 'state',
      header: '',
      width: '2.5rem',
      editor: 'none',
      sticky: true,
      text: stateLabel,
      cellClass: (r) =>
        r.state === 'added'
          ? 'font-semibold text-ok-fg'
          : r.state === 'changed'
            ? 'font-semibold text-warn-fg'
            : 'text-content-faint',
    },
    num('id', 'Id', (c) => c.id, {
      width: '3.25rem',
      sticky: true,
      invalid: (r) => idError(r.value),
      coercePaste: pasteInt,
    }),
    {
      id: 'section',
      header: '구역',
      width: '2.75rem',
      align: 'center',
      editor: 'select',
      options: ['1', '2', '3'],
      text: (r) => String(r.value.section),
      invalid: (r) => sectionError(r.value),
      coercePaste: pasteInt,
    },
    num('row', '행(X)', (c) => c.row, { width: '3rem', coercePaste: pasteInt }),
    num('col', '열(Y)', (c) => c.col, { width: '3rem', coercePaste: pasteInt }),
    num('x', 'X', (c) => c.position[0], { decimals: 1, invalid: (r) => posError(r.value, 'X') }),
    num('y', 'Y', (c) => c.position[1], { decimals: 1, invalid: (r) => posError(r.value, 'Y') }),
    num('z', 'Z', (c) => c.position[2], { decimals: 1, invalid: (r) => posError(r.value, 'Z') }),
    {
      id: 'use',
      header: '사용',
      width: '2.75rem',
      align: 'center',
      editor: 'select',
      options: ['Y', 'N'],
      text: (r) => boolText(r.value.use),
      coercePaste: pasteBool,
    },
    num('length', '길이', (c) => c.length, { width: '3.75rem' }),
    num('width', '폭', (c) => c.width, { width: '3.75rem' }),
  ]

  function applyOne(cur: Row[], key: string, col: string, value: string): Row[] {
    const r = cur.find((x) => x.key === key)
    if (!r) return cur
    const next = applyCellEdit(r.value, col, value)
    return next ? setRow(cur, key, next, original) : cur
  }
  const edit = (key: string, col: string, value: string) =>
    setRows((cur) => applyOne(cur, key, col, value))
  const editMany = (ups: { rowId: string; colId: string; value: string }[]) =>
    setRows((cur) => ups.reduce((acc, u) => applyOne(acc, u.rowId, u.colId, u.value), cur))

  function addRow() {
    const r = newCellRow(rows)
    if (!r) {
      toast.warn('남은 셀 id 가 없습니다 (1..1000)')
      return
    }
    setRows((cur) => [...cur, r])
    setSelKey(r.key)
    onSelect(r.value.id)
  }
  function removeSelected() {
    if (!selKey) return
    setRows((cur) => cur.filter((r) => r.key !== selKey))
    setSelKey(null)
    onSelect(null)
  }
  function revert() {
    setBase(saved)
    setRows(draftFrom(saved.map(cellToUpsert)))
    setStale(false)
  }
  async function save() {
    setBusy(true)
    try {
      for (const id of diff.deletes) await api.cellDelete(id)
      if (diff.upserts.length) await api.cellsBulk(diff.upserts, null)
      justSaved.current = true
      await reg.reload()
      toast.ok(
        `셀 적용 — 수정 ${diff.changed} · 신규 ${diff.added} · 삭제 ${diff.deletes.length} (로컬 사본, PLC 반영은 PLC 쓰기)`,
      )
    } catch (e) {
      toast.error(`셀 적용 실패 — ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="cell-grid-editor">
      <RegistryToolbar<Cell>
        compact
        hideCrud
        title="셀"
        icon={<Grid3x3 size={14} />}
        what="셀"
        rows={saved.length}
        dirty={saved.filter((c) => c.dirty).length}
        selected={false}
        io={IO}
        onAdd={addRow}
        onEdit={() => undefined}
        onDelete={removeSelected}
        reload={reg.reload}
      />
      <div className="flex h-10 flex-none items-center gap-1 border-b border-line-default px-2">
        <Button
          size="sm"
          intent="ghost"
          icon={<Plus className="h-3.5 w-3.5" />}
          onClick={addRow}
          data-testid="grid-add"
        >
          행 추가
        </Button>
        <Button
          size="sm"
          intent="ghost"
          icon={<Trash2 className="h-3.5 w-3.5" />}
          disabled={!selKey}
          onClick={removeSelected}
          data-testid="grid-delete"
        >
          행 삭제
        </Button>
        <span
          className="min-w-0 flex-1 truncate px-1 text-2xs text-content-muted tabular-nums"
          data-testid="grid-status"
        >
          {pending
            ? `수정 ${diff.changed} · 신규 ${diff.added} · 삭제 ${diff.deletes.length}`
            : '칸을 바로 고치거나 엑셀에서 붙여넣기'}
          {errorCount ? <span className="ml-1 text-fault-fg">· 오류 {errorCount}</span> : null}
        </span>
        <Button
          size="sm"
          intent="ghost"
          icon={<Undo2 className="h-3.5 w-3.5" />}
          disabled={!pending || busy}
          onClick={revert}
          data-testid="grid-revert"
        >
          되돌리기
        </Button>
        <Button
          size="sm"
          intent="primary"
          icon={<Save className="h-3.5 w-3.5" />}
          disabled={!pending || errorCount > 0 || busy}
          loading={busy}
          onClick={() => setConfirm(true)}
          data-testid="grid-apply"
        >
          적용{pending ? ` ${pending}` : ''}
        </Button>
      </div>
      {stale ? (
        <div className="flex h-8 flex-none items-center border-b border-warn bg-warn-soft px-3 text-2xs text-warn-fg">
          저장본이 바뀌었습니다(PLC 읽기·가져오기). 편집을 적용하거나 되돌리기로 새로 받으세요.
        </div>
      ) : null}
      <div className="min-h-0 flex-1">
        <DataGrid<Row>
          rows={shown}
          columns={columns}
          rowId={(r) => r.key}
          persistKey="layout-cell-grid"
          layoutFixed
          zebra
          maxHeight="calc(100vh - 15rem)"
          empty="셀이 없습니다 — 행 추가 또는 엑셀에서 붙여넣기"
          rowClass={(r) => (r.key === selKey ? 'bg-accent-soft' : '')}
          onrowclick={(r) => {
            setSelKey(r.key)
            onSelect(r.value.id)
          }}
          onedit={edit}
          oneditmany={editMany}
          onpasteskipped={(n) => toast.warn(`붙여넣기 ${n}칸은 형식이 맞지 않아 건너뛰었습니다`)}
          cellTestId={(r, col) => `cellgrid-${r.value.id}-${col.id}`}
        />
      </div>
      <ConfirmDialog
        open={confirm}
        onOpenChange={setConfirm}
        scope="single"
        title="셀 편집 적용"
        confirmLabel="적용"
        onConfirm={() => void save()}
      >
        <p className="text-xs">
          수정 {diff.changed} · 신규 {diff.added} · 삭제 {diff.deletes.length} 건을 로컬 사본에
          저장합니다. PLC 반영은 툴바의 PLC 쓰기로 합니다.
        </p>
      </ConfirmDialog>
    </div>
  )
}
