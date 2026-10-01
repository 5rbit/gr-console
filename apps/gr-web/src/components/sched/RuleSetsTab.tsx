// 규칙 세트(옛 "시나리오") — 특정 규칙만 켜는 묶음. [적용] 하면 묶음에 든 작업 규칙 · 로봇 규칙만 켜지고 나머지는 꺼진다.
// 이미 대기열에 든 작업은 그대로 간다. 세트 편집은 체크 목록 하나(규칙 탭과 같은 Id).
import { useState } from 'react'
import { Plus, Trash2 } from 'lucide-react'
import { schedApi, type SchedState } from '../../lib/sched'
import { withPolicy } from '../../lib/sched/policyModel'
import type { RuleSet } from '../../lib/taskgen'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { FormDialog } from '../../lib/ui/Dialog'
import { ErrorBoundary } from '../../lib/ui/ErrorBoundary'
import { Input } from '../../lib/ui/Input'
import { InfoRows } from '../../lib/ui/Pair'
import { Section } from '../../lib/ui/Section'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import { Switch } from '../../lib/ui/Switch'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'
import { useSaveConfig } from './PolicyTab'

const hm = (s: string | null | undefined) => (s ? s.replace('T', ' ').slice(5, 16) : '—')

function Body({ state, reload }: { state: SchedState | null; reload: () => void }) {
  const save = useSaveConfig(reload)
  const cfg = state?.config ? withPolicy(state.config) : null
  const sets = cfg?.rule_sets ?? []
  const [edit, setEdit] = useState<RuleSet | null>(null)
  const [ask, setAsk] = useState<RuleSet | null>(null)
  const allRules = [
    ...(cfg?.rules ?? []).map((r) => ({ id: r.id, name: r.name, scope: '작업' })),
    ...(cfg?.robot_rules ?? []).map((r) => ({ id: r.id, name: r.name, scope: '로봇' })),
  ]
  const saveSets = (next: RuleSet[]) => cfg && void save({ ...cfg, rule_sets: next }, '규칙 세트')

  async function apply(s: RuleSet) {
    try {
      await schedApi.applySet(s.id)
      toast.ok(`규칙 세트 "${s.name}" 적용 — 규칙 ${s.rules.length}개만 켬`)
      reload()
    } catch (e) {
      toast.error(`적용 실패 — ${e instanceof Error ? e.message : String(e)}`)
    }
  }

  const cols: Column<RuleSet>[] = [
    { key: 'name', label: '이름', get: (s) => s.name, priority: 1 },
    {
      key: 'rules',
      label: '켜는 규칙',
      get: (s) => s.rules.join(' '),
      cell: (s) => <span className="font-mono">{s.rules.join(' ')}</span>,
      priority: 2,
    },
    { key: 'n', label: '규칙 수', get: (s) => s.rules.length, numeric: true, priority: 3 },
    {
      key: 'at',
      label: '마지막 적용',
      get: (s) => s.applied_at ?? '',
      cell: (s) => <span className="font-mono">{hm(s.applied_at)}</span>,
      priority: 3,
    },
    {
      key: 'state',
      label: '상태',
      get: (s) => (cfg?.active_set === s.id ? 1 : 0),
      cell: (s) =>
        cfg?.active_set === s.id ? (
          <StatusBadge status="ok">적용 중</StatusBadge>
        ) : (
          <StatusBadge status="neutral">대기</StatusBadge>
        ),
      priority: 1,
    },
  ]

  return (
    <div className="flex min-h-0 flex-col gap-3 text-xs" data-testid="sched-sets-tab">
      <Section
        title="규칙 세트"
        help="상황별로 켤 규칙 묶음 — 예: 입고 기본 · 측정 데이 · 야간 정리. 적용하면 묶음의 규칙만 켜지고 나머지는 꺼진다."
        right={
          <Button
            size="sm"
            intent="outline"
            icon={<Plus className="h-3.5 w-3.5" />}
            disabled={!cfg}
            onClick={() =>
              setEdit({
                id: `set-${sets.length + 1}`,
                name: '',
                rules: [],
                note: '',
                applied_at: null,
              })
            }
            data-testid="sets-add"
          >
            세트 추가
          </Button>
        }
        testid="sched-sets"
      >
        <DataTable
          rows={sets}
          columns={cols}
          rowKey={(s) => s.id}
          density="compact"
          onPick={(s) => setEdit(s)}
          selected={cfg?.active_set ?? null}
          actions={(s) => (
            <span className="flex gap-1">
              <Button size="sm" onClick={() => setAsk(s)} data-testid={`sets-apply-${s.id}`}>
                적용
              </Button>
              <Button
                size="icon-sm"
                intent="ghost"
                icon={<Trash2 className="h-3.5 w-3.5" />}
                title="삭제"
                onClick={() => saveSets(sets.filter((x) => x.id !== s.id))}
              />
            </span>
          )}
          empty="규칙 세트 없음"
          emptyHint="[세트 추가] 로 함께 켤 규칙을 고릅니다."
          testid="sched-sets-table"
        />
      </Section>
      {edit ? (
        <SetDialog
          set={edit}
          rules={allRules}
          takenIds={sets.filter((x) => x.id !== edit.id).map((x) => x.id)}
          onClose={() => setEdit(null)}
          onSave={(s) => {
            const exists = sets.some((x) => x.id === edit.id)
            setEdit(null)
            saveSets(exists ? sets.map((x) => (x.id === edit.id ? s : x)) : [...sets, s])
          }}
        />
      ) : null}
      <ConfirmDialog
        open={ask !== null}
        onOpenChange={(o) => {
          if (!o) setAsk(null)
        }}
        scope="single"
        title={`규칙 세트 적용 — ${ask?.name ?? ''}`}
        confirmLabel="적용"
        onConfirm={() => ask && void apply(ask)}
      >
        <InfoRows
          rows={[
            { label: '켜는 규칙', value: ask?.rules.join(' ') || '없음' },
            { label: '나머지', value: '모두 꺼짐 (대기열의 작업은 그대로)' },
          ]}
        />
      </ConfirmDialog>
    </div>
  )
}

function SetDialog({
  set,
  rules,
  takenIds,
  onClose,
  onSave,
}: {
  set: RuleSet
  rules: { id: string; name: string; scope: string }[]
  takenIds: readonly string[]
  onClose: () => void
  onSave: (s: RuleSet) => void
}) {
  const [s, setS] = useState<RuleSet>(set)
  const why = !s.id.trim()
    ? 'Id 가 필요합니다'
    : takenIds.includes(s.id)
      ? `Id ${s.id} 가 이미 있습니다`
      : !s.name.trim()
        ? '이름이 필요합니다'
        : undefined
  const toggle = (id: string, on: boolean) =>
    setS({ ...s, rules: on ? [...s.rules, id] : s.rules.filter((x) => x !== id) })
  return (
    <FormDialog
      open
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
      title="규칙 세트"
      size="sm"
      submitLabel="저장"
      disabledReason={why}
      onSubmit={() => onSave(s)}
      testid="rule-set"
    >
      <div className="flex flex-col gap-2 text-xs">
        <div className="grid grid-cols-2 gap-2">
          <Input label="Id" mono value={s.id} onValueChange={(x) => setS({ ...s, id: x.trim() })} />
          <Input
            label="이름"
            value={s.name}
            onValueChange={(x) => setS({ ...s, name: x })}
            data-testid="rule-set-name"
          />
        </div>
        <div className="flex flex-col gap-1 rounded border border-line-default p-2">
          {rules.length === 0 ? (
            <span className="text-content-faint">규칙이 없습니다 — 규칙 탭에서 먼저 만드세요</span>
          ) : null}
          {rules.map((r) => (
            <Switch
              key={r.id}
              inline
              label={`${r.id} · ${r.name} (${r.scope})`}
              checked={s.rules.includes(r.id)}
              onCheckedChange={(v) => toggle(r.id, v)}
              testid={`rule-set-${r.id}`}
            />
          ))}
        </div>
      </div>
    </FormDialog>
  )
}

export default function RuleSetsTab(props: { state: SchedState | null; reload: () => void }) {
  return (
    <ErrorBoundary label="규칙 세트">
      <Body {...props} />
    </ErrorBoundary>
  )
}
