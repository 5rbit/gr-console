// 시나리오 목록 레일 — 이름 · 스텝 수 · 반복 · 갱신. 늘 보이는 조작은 `새로 만들기` 하나고,
// 복제·삭제는 **고른 것에만 뜻이 있으므로** `⋯`로 옮겼다(비활성이면 사유를 단다). 삭제는 확인을 거친다.
import { useState } from 'react'
import { Plus } from 'lucide-react'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { StatusDot } from '../../lib/ui/StatusDot'
import { Toolbar } from '../../lib/ui/Toolbar'
import type { Column } from '../../lib/ui/table'
import type { Scenario } from '../../lib/types'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import type { MenuItem } from '../../lib/ui/menu'

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

export function ScenarioList({
  items,
  loading,
  selectedId,
  runningId,
  onSelect,
  onNew,
  onDuplicate,
  onDelete,
}: ScenarioListProps) {
  const [confirm, setConfirm] = useState<Scenario | null>(null)
  const selected = items.find((s) => s.id === selectedId) ?? null

  const menuFor = (s: Scenario | null): MenuItem[] => [
    {
      label: '복제해서 편집',
      disabled: s ? undefined : '먼저 시나리오를 고르세요',
      run: () => s && onDuplicate(s),
    },
    {
      label: '삭제…',
      danger: true,
      disabled: s ? undefined : '먼저 시나리오를 고르세요',
      run: () => s && setConfirm(s),
    },
  ]

  const columns: Column<Scenario>[] = [
    {
      key: 'name',
      label: 'Name',
      get: (s) => s.name,
      cell: (s) => (
        <span className="flex min-w-0 items-center gap-1.5">
          {/* 점 자리는 항상 잡아 둔다 — 실행 중인 행만 이름이 밀리면 세로로 훑을 수 없다. */}
          <span className="inline-flex w-2.5 shrink-0 justify-center">
            {s.id === runningId ? <StatusDot status="ok" size="sm" title="실행 중" /> : null}
          </span>
          <span className="truncate">{s.name}</span>
        </span>
      ),
    },
    { key: 'steps', label: 'Steps', get: (s) => s.steps.length, numeric: true, priority: 2 },
    {
      key: 'repeat',
      label: 'Repeat',
      get: (s) => s.repeat,
      cell: (s) => (s.repeat === 0 ? '∞' : String(s.repeat)),
      numeric: true,
      priority: 3,
    },
    {
      key: 'updated',
      label: 'UpdatedAt',
      get: (s) => s.updated_at,
      cell: (s) => <span className="text-content-muted">{fmtTime(s.updated_at)}</span>,
      priority: 2,
    },
  ]

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="scenario-list">
      <Toolbar title="시나리오" meta={`${items.length}건`} dense>
        <Button size="sm" icon={<Plus size={13} />} onClick={onNew} data-testid="scenario-new">
          새로 만들기
        </Button>
        {/* 고른 시나리오가 없으면 손잡이째 잠근다 — 열어 봐야 죽은 항목 둘이고, 사유는 손잡이가
            말한다(`docs/DESIGN.md` 4절 ⑥). */}
        <OverflowMenu
          items={menuFor(selected)}
          testid="scenario-list-more"
          title="복제 · 삭제"
          disabledReason={selected ? undefined : '먼저 시나리오를 고르세요'}
        />
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
      <ConfirmDialog
        open={confirm !== null}
        onOpenChange={(o) => !o && setConfirm(null)}
        scope="single"
        title="시나리오 삭제"
        danger
        confirmLabel="삭제"
        onConfirm={() => confirm && onDelete(confirm)}
      >
        <p className="m-0">
          <b>{confirm?.name}</b> (스텝 {confirm?.steps.length ?? 0}개)를 삭제합니다. 되돌릴 수
          없습니다.
        </p>
      </ConfirmDialog>
    </div>
  )
}
