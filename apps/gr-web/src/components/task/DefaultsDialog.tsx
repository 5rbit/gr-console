// 기본값 다이얼로그 — 파라미터 키가 행, `공통 | PICK·셀 | PICK·스테이션 | DROP·셀 | DROP·스테이션`이 열인
// DataGrid. 부분 열의 빈 칸은 "공통 상속"이다. 제어형 초안이라 저장 전엔 서버에 아무것도 가지 않는다.
import { useEffect, useMemo, useState } from 'react'
import { Save } from 'lucide-react'
import { api } from '../../lib/api'
import {
  DEFAULTS_COLS,
  applyDefaultsEdit,
  defaultsChanged,
  defaultsRows,
  type DefaultsCol,
  type DefaultsRow,
  type ParamKey,
} from '../../lib/task/compose'
import { Button } from '../../lib/ui/Button'
import { DataGrid } from '../../lib/ui/datagrid/DataGrid'
import type { DataGridColumn } from '../../lib/ui/datagrid/types'
import { Modal } from '../../lib/ui/Modal'
import { toast } from '../../lib/ui/toast'
import type { Defaults } from '../../lib/types'

export interface DefaultsDialogProps {
  open: boolean
  onOpenChange: (o: boolean) => void
  defaults: Defaults | null
  /** 저장 성공 — 부모가 새 값을 받는다. */
  onSaved: (d: Defaults) => void
}

const fmt = (v: number | boolean | undefined): string => (v === undefined ? '' : String(v))

export function DefaultsDialog({ open, onOpenChange, defaults, onSaved }: DefaultsDialogProps) {
  const [draft, setDraft] = useState<Defaults | null>(defaults)
  const [saving, setSaving] = useState(false)
  useEffect(() => {
    if (open) setDraft(defaults)
  }, [open, defaults])

  const rows = useMemo(() => (draft ? defaultsRows(draft) : []), [draft])
  const changed = !!draft && !!defaults && defaultsChanged(draft, defaults)

  const columns: DataGridColumn<DefaultsRow>[] = [
    { id: 'label', header: '파라미터', width: '12rem', sticky: true, editor: 'none', text: (r) => r.label, title: (r) => r.key, sortable: true },
    ...DEFAULTS_COLS.map(
      (c): DataGridColumn<DefaultsRow> => ({
        id: c.id,
        header: c.header,
        headerSub: c.id === 'base' ? '' : '비면 공통 상속',
        width: '8rem',
        align: 'right',
        mono: true,
        editor: (r) => (r.bool ? 'select' : 'number'),
        options: (r) => (r.bool ? (c.id === 'base' ? ['true', 'false'] : ['', 'true', 'false']) : []),
        decimals: 0,
        text: (r) => fmt(r.values[c.id]),
        cellClass: (r) => (c.id !== 'base' && r.values[c.id] === undefined ? 'text-slate-300 dark:text-slate-600' : ''),
        title: (r) => (c.id !== 'base' && r.values[c.id] === undefined ? `공통 값 ${fmt(r.values.base)} 상속` : ''),
        coercePaste: (v, r) => {
          const s = v.trim()
          if (s === '') return c.id === 'base' ? null : ''
          if (r.bool) return ['true', 'false', '1', '0'].includes(s.toLowerCase()) ? s : null
          return Number.isFinite(Number(s)) ? s : null
        },
      }),
    ),
  ]

  function edit(rowId: string, colId: string, value: string) {
    if (!draft || colId === 'label') return
    const next = applyDefaultsEdit(draft, rowId as ParamKey, colId as DefaultsCol, value)
    if (!next) {
      toast.warn(`${rowId}: '${value}'은(는) 이 칸에 넣을 수 없습니다`)
      return
    }
    setDraft(next)
  }

  async function save() {
    if (!draft) return
    setSaving(true)
    try {
      const saved = await api.defaultsSave(draft)
      onSaved(saved)
      toast.ok(`기본값 저장됨 (v${saved.version})`)
      onOpenChange(false)
    } catch (e) {
      toast.error(`기본값 저장 실패 — ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setSaving(false)
    }
  }

  return (
    <Modal open={open} onOpenChange={onOpenChange} title="작업 파라미터 기본값" wide>
      <div className="flex flex-col gap-2" data-testid="defaults-dialog">
        <p className="text-xs text-slate-500">
          우선순위: 공통 ← 종류·대상별 ← 작성 카드의 덮어쓰기. 셀을 더블클릭하거나 타이핑해 고치고, 붙여넣기(엑셀)도 됩니다.
          {defaults ? ` 현재 v${defaults.version}` : ''}
        </p>
        {draft ? (
          <DataGrid<DefaultsRow>
            rows={rows}
            columns={columns}
            rowId={(r) => r.key}
            onedit={edit}
            oneditmany={(ups) => {
              let cur = draft
              for (const u of ups) {
                if (u.colId === 'label') continue
                const next = applyDefaultsEdit(cur, u.rowId as ParamKey, u.colId as DefaultsCol, u.value)
                if (next) cur = next
              }
              setDraft(cur)
            }}
            onpasteskipped={(n) => toast.warn(`붙여넣기 ${n}칸 거부됨(형식 불일치)`)}
            zebra
            layoutFixed
            maxHeight="55vh"
            persistKey="gr.defaults"
            cellTestId={(r, c) => `def-${r.key}-${c.id}`}
          />
        ) : (
          <p className="text-xs text-slate-400">기본값을 아직 못 받았습니다.</p>
        )}
        <div className="flex items-center justify-end gap-2">
          <Button size="sm" intent="ghost" disabled={!changed} onClick={() => setDraft(defaults)}>
            되돌리기
          </Button>
          <Button size="sm" intent="primary" icon={<Save className="h-3.5 w-3.5" />} disabled={!changed} loading={saving} onClick={() => void save()} data-testid="defaults-save">
            저장
          </Button>
        </div>
      </div>
    </Modal>
  )
}
