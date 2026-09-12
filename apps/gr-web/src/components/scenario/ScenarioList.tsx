// 시나리오 목록 레일 — 이름 · 스텝 수 · 반복 · 갱신. 새로 만들기 / 복제 / 삭제(확인).
import { useState } from 'react'
import { Copy, Plus, Trash2 } from 'lucide-react'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import { Toolbar } from '../../lib/ui/Toolbar'
import type { Column } from '../../lib/ui/table'
import type { Scenario } from '../../lib/types'

export interface ScenarioListProps {
  items: Scenario[]
  loading: boolean
  selectedId: string | null
  /** 지금 도는 시나리오 id(배지). */
  runningId: string | null
  onSelect: (id: string) => void
  onNew: () => void
  onDuplicate: (s: Scenario) => void
  onDelete: (s: Scenario) => void
}

function fmtTime(s: string): string {
  const d = new Date(s)
  if (Number.isNaN(d.getTime())) return s
  const p = (n: number) => String(n).padStart(2, '0')
  return `${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}

export function ScenarioList({ items, loading, selectedId, runningId, onSelect, onNew, onDuplicate, onDelete }: ScenarioListProps) {
  const [confirm, setConfirm] = useState<Scenario | null>(null)
  const selected = items.find((s) => s.id === selectedId) ?? null

  const columns: Column<Scenario>[] = [
    {
      key: 'name',
      label: '이름',
      get: (s) => s.name,
      cell: (s) => (
        <span className="flex min-w-0 items-center gap-1.5">
          <span className="truncate">{s.name}</span>
          {s.id === runningId ? <StatusBadge status="ok">실행 중</StatusBadge> : null}
        </span>
      ),
    },
    { key: 'steps', label: '스텝', get: (s) => s.steps.length, numeric: true, priority: 2 },
    {
      key: 'repeat',
      label: '반복',
      get: (s) => s.repeat,
      cell: (s) => (s.repeat === 0 ? '∞' : String(s.repeat)),
      numeric: true,
      priority: 3,
    },
    {
      key: 'updated',
      label: '갱신',
      get: (s) => s.updated_at,
      cell: (s) => <span className="text-slate-500">{fmtTime(s.updated_at)}</span>,
      priority: 2,
    },
  ]

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="scenario-list">
      <Toolbar title="시나리오" meta={`${items.length}건`} dense>
        <Button size="sm" intent="primary" icon={<Plus size={13} />} onClick={onNew} data-testid="scenario-new">
          새로 만들기
        </Button>
        <Button size="sm" icon={<Copy size={13} />} disabled={!selected} title="선택한 시나리오를 복제해 편집" onClick={() => selected && onDuplicate(selected)}>
          복제
        </Button>
        <Button size="sm" intent="ghost" icon={<Trash2 size={13} />} disabled={!selected} onClick={() => selected && setConfirm(selected)} data-testid="scenario-delete">
          삭제
        </Button>
      </Toolbar>
      <div className="min-h-0 flex-1 overflow-auto">
        <DataTable<Scenario>
          rows={items}
          columns={columns}
          rowKey={(s) => s.id}
          selected={selectedId}
          onPick={(s) => onSelect(s.id)}
          loading={loading}
          empty="시나리오 없음"
          emptyHint="'새로 만들기' 또는 JSON/CSV 가져오기로 시작하세요."
          testid="scenario-table"
        />
      </div>
      <ConfirmDialog open={confirm !== null} onOpenChange={(o) => !o && setConfirm(null)} scope="single" title="시나리오 삭제" danger confirmLabel="삭제" onConfirm={() => confirm && onDelete(confirm)}>
        <p className="m-0">
          <b>{confirm?.name}</b> (스텝 {confirm?.steps.length ?? 0}개)를 삭제합니다. 되돌릴 수 없습니다.
        </p>
      </ConfirmDialog>
    </div>
  )
}
