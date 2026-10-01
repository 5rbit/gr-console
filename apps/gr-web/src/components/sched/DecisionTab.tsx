// 결정 탭 — 이번 판정의 후보 · 예정 큐 · 냉각 · Hand 정리 · 파생 규칙(요청 → 정책 → 사용자).
import { useState } from 'react'
import { Trash2 } from 'lucide-react'
import { metricsLine, ruleState, taskgenApi, type GenItem } from '../../lib/taskgen'
import {
  schedApi,
  type Cooldown,
  type DerivedRuleStatus,
  type HandAlert,
  type RuleOrigin,
  type SchedState,
} from '../../lib/sched'
import {
  ORIGIN_LABEL,
  ORIGIN_ORDER,
  normalizeState,
  originCounts,
  skippedLine,
  sortDerived,
  termsSummary,
} from '../../lib/sched/decisionModel'
import { fmtAt } from '../../lib/sched/historyModel'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { ErrorBoundary } from '../../lib/ui/ErrorBoundary'
import { Section } from '../../lib/ui/Section'
import { Segmented } from '../../lib/ui/Segmented'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'
import { CandidateTable, OriginChip } from './dialogs'

const TONE = { ok: 'text-ok-fg', warn: 'text-warn-fg', muted: 'text-content-faint' } as const

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e))

type OriginFilter = 'all' | RuleOrigin

function DecisionBody({ state: raw, reload }: { state: SchedState | null; reload: () => void }) {
  const state = normalizeState(raw)
  const [filter, setFilter] = useState<OriginFilter>('all')
  const [hand, setHand] = useState<HandAlert | null>(null)

  if (!state) return <span className="text-xs text-content-faint">읽는 중…</span>

  const auto = !!state.config?.auto
  const derived = sortDerived(state.derived)
  const counts = originCounts(derived)
  const shown = filter === 'all' ? derived : derived.filter((d) => d.origin === filter)
  const skip = skippedLine(state.skipped)

  const queueCols: Column<GenItem>[] = [
    { key: 'rule', label: 'Rule', get: (g) => g.rule_name, priority: 1 },
    { key: 'robot', label: 'Robot', get: (g) => g.robot, numeric: true, priority: 1 },
    {
      key: 'next',
      label: 'Next',
      get: (g) => g.next,
      priority: 1,
      cell: (g) => (
        <span className="font-mono tabular-nums">
          {g.steps[g.next]?.type ?? '-'} {Math.min(g.next + 1, g.steps.length)}/{g.steps.length}
        </span>
      ),
    },
    {
      key: 'order',
      label: 'TransferOrder',
      get: (g) => g.transfer_order_id ?? '',
      priority: 2,
      cell: (g) => <span className="font-mono text-2xs">{g.transfer_order_id ?? '—'}</span>,
    },
    {
      key: 'note',
      label: 'Note',
      sortable: false,
      priority: 3,
      cell: (g) => (
        <span className="truncate text-2xs text-content-muted" title={g.note ?? ''}>
          {g.note ?? ''}
        </span>
      ),
    },
  ]

  const coolCols: Column<Cooldown>[] = [
    {
      key: 'key',
      label: 'Key',
      get: (c) => c.key,
      priority: 1,
      cell: (c) => <span className="font-mono text-2xs">{c.key}</span>,
    },
    { key: 'fails', label: 'Fails', get: (c) => c.fails, numeric: true, priority: 1 },
    {
      key: 'until',
      label: 'Until',
      get: (c) => c.until,
      priority: 1,
      cell: (c) => (
        <span className="font-mono tabular-nums" title={c.until}>
          {fmtAt(c.until)}
        </span>
      ),
    },
    {
      key: 'reason',
      label: 'LastReason',
      sortable: false,
      priority: 2,
      cell: (c) => (
        <span className="truncate text-2xs text-content-muted" title={c.last_reason}>
          {c.last_reason}
        </span>
      ),
    },
  ]

  const handCols: Column<HandAlert>[] = [
    { key: 'robot', label: 'Robot', get: (h) => h.robot_name || String(h.robot), priority: 1 },
    { key: 'item', label: 'ItemCode', get: (h) => h.item_code, numeric: true, priority: 1 },
    { key: 'count', label: 'Count', get: (h) => h.count, numeric: true, priority: 1 },
    {
      key: 'order',
      label: 'TransferOrder',
      get: (h) => h.order ?? '',
      priority: 3,
      cell: (h) => <span className="font-mono text-2xs">{h.order ?? '—'}</span>,
    },
    {
      key: 'why',
      label: 'Why',
      sortable: false,
      priority: 2,
      cell: (h) => (
        <span className="truncate text-2xs text-warn-fg" title={h.why}>
          {h.why}
        </span>
      ),
    },
  ]

  const derivedCols: Column<DerivedRuleStatus>[] = [
    {
      key: 'origin',
      label: 'Origin',
      get: (d) => ORIGIN_ORDER.indexOf(d.origin),
      priority: 1,
      cell: (d) => <OriginChip origin={d.origin} />,
    },
    {
      key: 'name',
      label: 'Name',
      get: (d) => d.name || d.rule_id,
      priority: 1,
      cell: (d) => <span title={d.rule_id}>{d.name || d.rule_id}</span>,
    },
    {
      key: 'state',
      label: 'State',
      sortable: false,
      priority: 1,
      cell: (d) => {
        const s = ruleState(d, auto)
        return (
          <span className={TONE[s.tone]} title={s.title}>
            {s.label}
          </span>
        )
      },
    },
    {
      key: 'station',
      label: 'Station',
      get: (d) => d.station ?? '',
      numeric: true,
      priority: 2,
      cell: (d) => <span className="font-mono tabular-nums">{d.station ?? '—'}</span>,
    },
    {
      key: 'terms',
      label: '조건',
      get: (d) => termsSummary(d.terms).label,
      priority: 1,
      cell: (d) => {
        const t = termsSummary(d.terms)
        return (
          <span
            className={`font-mono tabular-nums ${t.ok ? 'text-content-faint' : 'text-warn-fg'}`}
            title={t.title}
          >
            {t.label}
          </span>
        )
      },
    },
    { key: 'made', label: 'Generated', get: (d) => d.generated, numeric: true, priority: 3 },
  ]

  const termsDetail = (d: DerivedRuleStatus) =>
    d.terms?.length ? (
      <div className="grid grid-cols-[auto_1fr_auto] gap-x-3 gap-y-0.5 text-2xs">
        {d.terms.map((t, i) => (
          <div key={i} className="contents">
            <span className="text-content-muted">{t.label}</span>
            <span className="font-mono tabular-nums">{t.value}</span>
            <span className={t.ok ? 'text-ok-fg' : 'text-warn-fg'}>{t.ok ? '충족' : '아직'}</span>
          </div>
        ))}
        {d.inputs ? <span className="col-span-3 text-content-faint">{d.inputs}</span> : null}
      </div>
    ) : (
      <span className="text-2xs text-content-faint">{d.inputs || '조건 없음'}</span>
    )

  const genCount = state.candidates.filter((c) => !c.reason).length

  return (
    <div className="flex min-h-0 flex-col gap-3 text-xs" data-testid="sched-decision">
      <span
        className="truncate font-mono text-2xs tabular-nums text-content-faint"
        data-testid="sched-metrics"
      >
        {metricsLine(state.metrics)}
        {state.note ? <span className="ml-2 text-warn-fg">{state.note}</span> : null}
      </span>

      {state.hand_alerts.length ? (
        <Section title="Hand 정리" right={`${state.hand_alerts.length}`} first testid="sched-hand">
          <DataTable
            rows={state.hand_alerts}
            columns={handCols}
            rowKey={(h) => String(h.robot)}
            density="compact"
            emptyDense
            actions={(h) => (
              <Button type="button" size="sm" intent="outline" onClick={() => setHand(h)}>
                화물 제거
              </Button>
            )}
          />
        </Section>
      ) : null}

      <Section
        title="후보"
        right={`생성 ${genCount} / ${state.candidates.length}`}
        first={!state.hand_alerts.length}
        testid="sched-candidates-section"
      >
        <div className="flex flex-col gap-1">
          {skip ? (
            <span className="truncate text-2xs text-warn-fg" title={skip.title}>
              {skip.text}
            </span>
          ) : null}
          <CandidateTable
            rows={state.candidates}
            derived={state.derived}
            empty="후보 없음"
            testid="sched-candidates"
          />
        </div>
      </Section>

      <Section title="예정" right={`${state.queue.length}`}>
        <DataTable
          rows={state.queue}
          columns={queueCols}
          rowKey={(g) => g.id}
          density="compact"
          emptyDense
          empty="예정 없음"
          testid="sched-queue"
          actions={(g) =>
            g.next === 0 ? (
              <Button
                type="button"
                size="icon-sm"
                intent="ghost"
                icon={<Trash2 className="h-3.5 w-3.5" />}
                title="예정 지우기 (안 보냄)"
                onClick={() =>
                  void taskgenApi
                    .removeQueued(g.id)
                    .then(reload)
                    .catch((e: unknown) => toast.error(`예정 지우기 실패 — ${errText(e)}`))
                }
              />
            ) : null
          }
        />
      </Section>

      {state.cooldowns.length ? (
        <Section title="냉각" right={`${state.cooldowns.length}`}>
          <DataTable
            rows={state.cooldowns}
            columns={coolCols}
            rowKey={(c) => c.key}
            density="compact"
            emptyDense
            testid="sched-cooldowns"
            actions={(c) => (
              <Button
                type="button"
                size="sm"
                intent="ghost"
                onClick={() =>
                  void schedApi
                    .clearCooldown(c.key)
                    .then(() => {
                      toast.ok(`냉각 풀기 — ${c.key}`)
                      reload()
                    })
                    .catch((e: unknown) => toast.error(`냉각 풀기 실패 — ${errText(e)}`))
                }
              >
                풀기
              </Button>
            )}
          />
        </Section>
      ) : null}

      <Section
        title="수요"
        right={
          <Segmented<OriginFilter>
            value={filter}
            onChange={setFilter}
            ariaLabel="Origin"
            options={[
              { id: 'all', label: '전체', badge: derived.length },
              ...ORIGIN_ORDER.map((o) => ({ id: o, label: ORIGIN_LABEL[o], badge: counts[o] })),
            ]}
          />
        }
      >
        <DataTable
          rows={shown}
          columns={derivedCols}
          rowKey={(d) => d.rule_id}
          rowDetail={termsDetail}
          density="compact"
          emptyDense
          empty="파생 규칙 없음"
          testid="sched-derived"
        />
      </Section>

      <ConfirmDialog
        open={!!hand}
        onOpenChange={(o) => {
          if (!o) setHand(null)
        }}
        scope="single-robot"
        danger
        title="Hand 정리"
        confirmLabel="제거함"
        onConfirm={() => {
          const h = hand
          if (!h) return
          void schedApi
            .handRemove(h.robot)
            .then((r) => {
              toast.ok(`${h.robot_name || h.robot}: Hand 정리 — Task 취소 ${r.canceled.length}건`)
              reload()
            })
            .catch((e: unknown) => toast.error(`Hand 정리 실패 — ${errText(e)}`))
        }}
      >
        {hand ? (
          <div className="flex flex-col gap-1 text-xs">
            <span>
              {hand.robot_name || hand.robot} Hand 의 화물(ItemCode {hand.item_code} × {hand.count}
              )을 사람이 들어냈습니까?
            </span>
            <span className="text-content-faint">
              재고에 넣지 않고 짝 Task 를 취소하고 이송 지시를 중단합니다.
            </span>
          </div>
        ) : null}
      </ConfirmDialog>
    </div>
  )
}

export default function DecisionTab(props: { state: SchedState | null; reload: () => void }) {
  return (
    <ErrorBoundary label="결정">
      <DecisionBody {...props} />
    </ErrorBoundary>
  )
}
