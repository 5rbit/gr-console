// 후보 표 — 결정 탭과 시뮬레이션 대화상자가 같이 쓴다(Rule · Origin · Robot · 대상 · Score · Area · 생성/대기).
import { breakdownText, type GenCandidate } from '../../../lib/taskgen'
import type { DerivedRuleStatus, RuleOrigin } from '../../../lib/sched'
import { ORIGIN_LABEL, candTargets, originOf } from '../../../lib/sched/decisionModel'
import { Badge } from '../../../lib/ui/Badge'
import { DataTable } from '../../../lib/ui/DataTable'
import type { Column } from '../../../lib/ui/table'

const ORIGIN_TONE: Record<RuleOrigin, string> = {
  request: 'bg-accent-soft text-accent-text',
  policy: 'bg-surface-inset text-content-secondary',
  user: 'bg-surface-inset text-content-faint',
}

export function OriginChip({ origin }: { origin: RuleOrigin }) {
  return <Badge className={ORIGIN_TONE[origin] ?? ''}>{ORIGIN_LABEL[origin] ?? origin}</Badge>
}

export function CandidateTable({
  rows,
  derived,
  empty = '후보 없음',
  testid,
}: {
  rows: readonly GenCandidate[]
  derived: readonly DerivedRuleStatus[]
  empty?: string
  testid?: string
}) {
  const cols: Column<GenCandidate>[] = [
    { key: 'rule', label: 'Rule', get: (c) => c.rule_name, priority: 1 },
    {
      key: 'origin',
      label: 'Origin',
      get: (c) => originOf(c.rule_id, derived),
      priority: 2,
      cell: (c) => <OriginChip origin={originOf(c.rule_id, derived)} />,
    },
    { key: 'robot', label: 'Robot', get: (c) => c.robot_name, priority: 1 },
    {
      key: 'targets',
      label: 'PICK → DROP',
      get: (c) => candTargets(c),
      priority: 2,
      cell: (c) => <span className="font-mono text-2xs">{candTargets(c)}</span>,
    },
    {
      key: 'score',
      label: 'Score',
      get: (c) => c.score,
      numeric: true,
      priority: 1,
      cell: (c) => (
        <span title={breakdownText(c)} className="font-mono tabular-nums">
          {c.score.toFixed(1)}
        </span>
      ),
    },
    {
      key: 'area',
      label: 'Area X (mm)',
      get: (c) => c.area?.lo ?? 0,
      numeric: true,
      priority: 3,
      cell: (c) =>
        c.area ? (
          <span className="font-mono tabular-nums">
            {c.area.lo.toFixed(0)}..{c.area.hi.toFixed(0)}
          </span>
        ) : (
          '—'
        ),
    },
    {
      key: 'state',
      label: 'State',
      get: (c) => (c.reason ? 1 : 0),
      priority: 1,
      cell: (c) =>
        c.reason ? (
          <span className="flex min-w-0 items-center gap-1" title={c.reason}>
            <span className="text-warn-fg">대기</span>
            <span className="truncate text-2xs text-content-muted">{c.reason}</span>
          </span>
        ) : (
          <span className="text-ok-fg">생성</span>
        ),
    },
  ]
  return (
    <DataTable
      rows={[...rows]}
      columns={cols}
      rowKey={(c) => `${c.rule_id}-${c.robot}`}
      density="compact"
      emptyDense
      empty={empty}
      testid={testid}
    />
  )
}
