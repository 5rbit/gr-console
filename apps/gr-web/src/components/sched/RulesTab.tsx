// 규칙 탭 — 작업 규칙과 로봇 규칙을 **한 표**로(설계 7판). 칸은 일곱: 트리거 · 출발 · 도착 · 조건 · 로봇 · 제한 · 우선.
// 로봇 규칙은 같은 표의 "범위 = 로봇" 줄이다(담당 구역 · 품목 내경 전담 · 우선 가산). 모든 셀 · 스테이션에 도는 공통 정책은
// 아래 섹션(옛 정책 탭). 요청에서 파생된 규칙은 여기 없다 — 요청 탭과 작업 표가 그 진행을 보인다.
import { useMemo, useState } from 'react'
import { Plus, Trash2 } from 'lucide-react'
import { robots } from '../../lib/robots'
import type { SchedState } from '../../lib/sched'
import { withPolicy } from '../../lib/sched/policyModel'
import { useStore } from '../../lib/store'
import {
  condShort,
  fromShort,
  limitShort,
  newRobotRule,
  newRule,
  robotRuleText,
  toShort,
  triggerShort,
  type GenRule,
  type RobotRule,
  type RobotRuleKind,
} from '../../lib/taskgen'
import { Button } from '../../lib/ui/Button'
import { DataTable } from '../../lib/ui/DataTable'
import { FormDialog } from '../../lib/ui/Dialog'
import { ErrorBoundary } from '../../lib/ui/ErrorBoundary'
import { Input } from '../../lib/ui/Input'
import { Section } from '../../lib/ui/Section'
import { Segmented } from '../../lib/ui/Segmented'
import { Select } from '../../lib/ui/Select'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import { Switch } from '../../lib/ui/Switch'
import type { Column } from '../../lib/ui/table'
import { RuleDialog } from './dialogs'
import { PolicySection, useSaveConfig } from './PolicyTab'

type Scope = 'all' | 'work' | 'robot'

/** 표 한 줄 — 작업 규칙 또는 로봇 규칙. */
type Row = { kind: 'work'; rule: GenRule } | { kind: 'robot'; rule: RobotRule }

const STATE_LABEL: Record<string, { text: string; tone: 'ok' | 'warn' | 'neutral' | 'info' }> = {
  ready: { text: '곧 생성', tone: 'ok' },
  queued: { text: '대기열', tone: 'info' },
  busy: { text: '진행 중', tone: 'info' },
  waiting: { text: '기다림', tone: 'warn' },
  skipped: { text: '못 만듦', tone: 'warn' },
  cooldown: { text: '냉각', tone: 'warn' },
  limited: { text: '한도', tone: 'warn' },
  idle: { text: '조건 대기', tone: 'neutral' },
  off: { text: '꺼짐', tone: 'neutral' },
}

function Body({ state, reload }: { state: SchedState | null; reload: () => void }) {
  useStore(robots)
  const save = useSaveConfig(reload)
  const cfg = state?.config ? withPolicy(state.config) : null
  const [scope, setScope] = useState<Scope>('all')
  const [editWork, setEditWork] = useState<GenRule | null>(null)
  const [editRobot, setEditRobot] = useState<RobotRule | null>(null)
  const rules = cfg?.rules ?? []
  const robotRules = cfg?.robot_rules ?? []
  const status = useMemo(() => new Map((state?.derived ?? []).map((s) => [s.rule_id, s])), [state])
  const robotName = (id: number) => robots.list.find((r) => r.id === id)?.name ?? `#${id}`
  const allIds = [...rules.map((r) => r.id), ...robotRules.map((r) => r.id)]

  const saveWork = (next: GenRule[]) => cfg && void save({ ...cfg, rules: next }, '규칙')
  const saveRobot = (next: RobotRule[]) =>
    cfg && void save({ ...cfg, robot_rules: next }, '로봇 규칙')

  const rows: Row[] = [
    ...(scope !== 'robot' ? rules.map((rule) => ({ kind: 'work' as const, rule })) : []),
    ...(scope !== 'work' ? robotRules.map((rule) => ({ kind: 'robot' as const, rule })) : []),
  ]

  const cols: Column<Row>[] = [
    {
      key: 'on',
      label: '켬',
      sortable: false,
      priority: 1,
      cell: (r) => (
        <Switch
          checked={r.rule.enabled}
          title={r.rule.enabled ? '켜짐' : '꺼짐'}
          onCheckedChange={(v) =>
            r.kind === 'work'
              ? saveWork(rules.map((x) => (x.id === r.rule.id ? { ...x, enabled: v } : x)))
              : saveRobot(robotRules.map((x) => (x.id === r.rule.id ? { ...x, enabled: v } : x)))
          }
        />
      ),
    },
    {
      key: 'id',
      label: 'Id',
      get: (r) => r.rule.id,
      cell: (r) => <span className="font-mono">{r.rule.id}</span>,
      priority: 1,
    },
    { key: 'name', label: '이름', get: (r) => r.rule.name, priority: 1 },
    { key: 'scope', label: '범위', get: (r) => (r.kind === 'work' ? '작업' : '로봇'), priority: 2 },
    {
      key: 'trig',
      label: '트리거',
      get: (r) => (r.kind === 'work' ? triggerShort(r.rule.trigger) : '—'),
      priority: 2,
    },
    {
      key: 'from',
      label: '출발',
      get: (r) => (r.kind === 'work' ? fromShort(r.rule.action) : robotRuleText(r.rule.kind)),
      cell: (r) =>
        r.kind === 'work' ? (
          <span className="font-mono">{fromShort(r.rule.action)}</span>
        ) : (
          <span className="font-mono text-content-secondary">{robotRuleText(r.rule.kind)}</span>
        ),
      priority: 1,
    },
    {
      key: 'to',
      label: '도착',
      get: (r) => (r.kind === 'work' ? toShort(r.rule.action) : ''),
      cell: (r) => (
        <span className="font-mono">{r.kind === 'work' ? toShort(r.rule.action) : ''}</span>
      ),
      priority: 1,
    },
    {
      key: 'cond',
      label: '조건',
      get: (r) => (r.kind === 'work' ? condShort(r.rule.cond) : ''),
      priority: 3,
    },
    {
      key: 'robot',
      label: '로봇',
      get: (r) =>
        r.kind === 'work'
          ? r.rule.robots.length
            ? r.rule.robots.map(robotName).join(' · ')
            : '로봇 규칙 따름'
          : robotName(r.rule.robot),
      priority: 2,
    },
    {
      key: 'limit',
      label: '제한',
      get: (r) => (r.kind === 'work' ? limitShort(r.rule) : ''),
      priority: 3,
    },
    {
      key: 'prio',
      label: '우선',
      get: (r) =>
        r.kind === 'work' ? r.rule.priority : r.rule.kind.kind === 'bonus' ? r.rule.kind.bonus : 0,
      numeric: true,
      priority: 3,
    },
    {
      key: 'made',
      label: '생성',
      get: (r) => (r.kind === 'work' ? (status.get(r.rule.id)?.generated ?? 0) : 0),
      cell: (r) => (
        <span className="font-mono">
          {r.kind === 'work' ? (status.get(r.rule.id)?.generated ?? 0) : ''}
        </span>
      ),
      numeric: true,
      priority: 3,
    },
    {
      key: 'state',
      label: '상태',
      get: (r) =>
        r.kind === 'work' ? (status.get(r.rule.id)?.state ?? '') : r.rule.enabled ? 'on' : 'off',
      cell: (r) => {
        if (r.kind === 'robot')
          return r.rule.enabled ? (
            <StatusBadge status="ok">동작</StatusBadge>
          ) : (
            <StatusBadge status="neutral">꺼짐</StatusBadge>
          )
        const s = status.get(r.rule.id)
        const l = STATE_LABEL[s?.state ?? (r.rule.enabled ? 'idle' : 'off')] ?? {
          text: s?.state ?? '-',
          tone: 'neutral' as const,
        }
        return (
          <span title={s?.reason ?? undefined}>
            <StatusBadge status={l.tone}>{l.text}</StatusBadge>
          </span>
        )
      },
      priority: 1,
    },
  ]

  return (
    <div className="flex min-h-0 flex-col gap-3 text-xs" data-testid="sched-rules-tab">
      <Section
        title="규칙"
        help="작업 규칙은 언제 · 어디서 · 어디로 작업을 만들지, 로봇 규칙은 그 작업을 어느 로봇이 받을지 정한다. 줄을 누르면 고친다."
        right={
          <span className="flex items-center gap-1">
            <Segmented<Scope>
              ariaLabel="규칙 범위"
              value={scope}
              onChange={setScope}
              options={[
                { id: 'all', label: '전체', badge: rules.length + robotRules.length || '' },
                { id: 'work', label: '작업 규칙', badge: rules.length || '' },
                { id: 'robot', label: '로봇 규칙', badge: robotRules.length || '' },
              ]}
            />
            <Button
              size="sm"
              intent="outline"
              icon={<Plus className="h-3.5 w-3.5" />}
              onClick={() => setEditWork(newRule(rules))}
              disabled={!cfg}
              data-testid="rules-add-work"
            >
              작업 규칙
            </Button>
            <Button
              size="sm"
              intent="outline"
              icon={<Plus className="h-3.5 w-3.5" />}
              onClick={() =>
                setEditRobot(
                  newRobotRule(
                    [...rules, ...robotRules],
                    robots.selected ?? robots.list[0]?.id ?? 1,
                  ),
                )
              }
              disabled={!cfg}
              data-testid="rules-add-robot"
            >
              로봇 규칙
            </Button>
          </span>
        }
        testid="sched-rules"
      >
        <DataTable
          rows={rows}
          columns={cols}
          rowKey={(r) => `${r.kind}:${r.rule.id}`}
          density="compact"
          fit
          onPick={(r) => (r.kind === 'work' ? setEditWork(r.rule) : setEditRobot(r.rule))}
          rowAttrs={(r) => ({ className: r.kind === 'robot' ? 'bg-surface-app' : undefined })}
          actions={(r) => (
            <Button
              type="button"
              size="icon-sm"
              intent="ghost"
              icon={<Trash2 className="h-3.5 w-3.5" />}
              title="삭제"
              onClick={() =>
                r.kind === 'work'
                  ? saveWork(rules.filter((x) => x.id !== r.rule.id))
                  : saveRobot(robotRules.filter((x) => x.id !== r.rule.id))
              }
            />
          )}
          empty="규칙 없음"
          emptyHint="[작업 규칙] 으로 언제 · 어디서 · 어디로 만들지, [로봇 규칙] 으로 어느 로봇이 받을지 정합니다."
          testid="sched-rules-table"
        />
      </Section>
      {cfg ? <PolicySection cfg={cfg} saveConfig={save} /> : null}
      {editWork ? (
        <RuleDialog
          key={editWork.id}
          rule={editWork}
          takenIds={allIds.filter((x) => x !== editWork.id)}
          onClose={() => setEditWork(null)}
          onSave={(r) => {
            const exists = rules.some((x) => x.id === editWork.id)
            setEditWork(null)
            saveWork(exists ? rules.map((x) => (x.id === editWork.id ? r : x)) : [...rules, r])
          }}
        />
      ) : null}
      {editRobot ? (
        <RobotRuleDialog
          rule={editRobot}
          takenIds={allIds.filter((x) => x !== editRobot.id)}
          onClose={() => setEditRobot(null)}
          onSave={(r) => {
            const exists = robotRules.some((x) => x.id === editRobot.id)
            setEditRobot(null)
            saveRobot(
              exists ? robotRules.map((x) => (x.id === editRobot.id ? r : x)) : [...robotRules, r],
            )
          }}
        />
      ) : null}
    </div>
  )
}

const KINDS: { id: RobotRuleKind['kind']; label: string }[] = [
  { id: 'zone', label: '담당 구역(X)' },
  { id: 'item_inner_dia', label: '품목 내경 전담' },
  { id: 'bonus', label: '우선 가산' },
]

function RobotRuleDialog({
  rule,
  takenIds,
  onClose,
  onSave,
}: {
  rule: RobotRule
  takenIds: readonly string[]
  onClose: () => void
  onSave: (r: RobotRule) => void
}) {
  useStore(robots)
  const [r, setR] = useState<RobotRule>(rule)
  const k = r.kind
  const n = (s: string) => (Number.isFinite(Number(s)) ? Number(s) : 0)
  const why = !r.id.trim()
    ? 'Id 가 필요합니다'
    : takenIds.includes(r.id)
      ? `Id ${r.id} 가 이미 있습니다`
      : k.kind === 'zone' && k.x_max <= k.x_min
        ? 'X 끝이 시작보다 커야 합니다'
        : k.kind === 'item_inner_dia' && k.max < k.min
          ? '내경 범위가 거꾸로입니다'
          : undefined
  function setKind(id: RobotRuleKind['kind']) {
    setR({
      ...r,
      kind:
        id === 'zone'
          ? { kind: 'zone', x_min: 0, x_max: 0 }
          : id === 'item_inner_dia'
            ? { kind: 'item_inner_dia', min: 0, max: 0 }
            : { kind: 'bonus', targets: [], bonus: 10 },
    })
  }
  return (
    <FormDialog
      open
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
      title={`로봇 규칙 — ${r.id}`}
      size="sm"
      submitLabel="적용"
      disabledReason={why}
      onSubmit={() => onSave(r)}
      testid="robot-rule"
    >
      <div className="flex flex-col gap-2 text-xs">
        <div className="grid grid-cols-2 gap-2">
          <Input label="Id" mono value={r.id} onValueChange={(x) => setR({ ...r, id: x.trim() })} />
          <Input label="이름" value={r.name} onValueChange={(x) => setR({ ...r, name: x })} />
        </div>
        <div className="grid grid-cols-2 gap-2">
          <Select
            label="로봇"
            value={String(r.robot)}
            onValueChange={(x) => setR({ ...r, robot: Number(x) })}
          >
            {robots.list.map((x) => (
              <option key={x.id} value={x.id}>
                {x.name}
              </option>
            ))}
          </Select>
          <Select
            label="종류"
            value={k.kind}
            onValueChange={(x) => setKind(x as RobotRuleKind['kind'])}
            data-testid="robot-rule-kind"
          >
            {KINDS.map((x) => (
              <option key={x.id} value={x.id}>
                {x.label}
              </option>
            ))}
          </Select>
        </div>
        {k.kind === 'zone' ? (
          <div className="grid grid-cols-2 gap-2">
            <Input
              label="X 시작 (mm)"
              type="number"
              mono
              value={String(k.x_min)}
              onValueChange={(x) => setR({ ...r, kind: { ...k, x_min: n(x) } })}
            />
            <Input
              label="X 끝 (mm)"
              type="number"
              mono
              value={String(k.x_max)}
              onValueChange={(x) => setR({ ...r, kind: { ...k, x_max: n(x) } })}
            />
          </div>
        ) : null}
        {k.kind === 'item_inner_dia' ? (
          <div className="grid grid-cols-2 gap-2">
            <Input
              label="내경 최소 (mm)"
              type="number"
              mono
              value={String(k.min)}
              onValueChange={(x) => setR({ ...r, kind: { ...k, min: n(x) } })}
            />
            <Input
              label="내경 최대 (mm)"
              type="number"
              mono
              value={String(k.max)}
              onValueChange={(x) => setR({ ...r, kind: { ...k, max: n(x) } })}
            />
          </div>
        ) : null}
        {k.kind === 'bonus' ? (
          <div className="grid grid-cols-2 gap-2">
            <Input
              label="가산"
              type="number"
              mono
              value={String(k.bonus)}
              onValueChange={(x) => setR({ ...r, kind: { ...k, bonus: n(x) } })}
            />
            <Input
              label="대상 Id (비우면 모든 작업)"
              mono
              placeholder="2101, 104"
              value={k.targets.join(', ')}
              onValueChange={(x) =>
                setR({
                  ...r,
                  kind: {
                    ...k,
                    targets: x
                      .split(',')
                      .map((p) => Number(p.trim()))
                      .filter((v) => Number.isInteger(v) && v > 0),
                  },
                })
              }
            />
          </div>
        ) : null}
        <Switch
          inline
          label="켬"
          checked={r.enabled}
          onCheckedChange={(v) => setR({ ...r, enabled: v })}
        />
      </div>
    </FormDialog>
  )
}

export default function RulesTab(props: { state: SchedState | null; reload: () => void }) {
  return (
    <ErrorBoundary label="규칙">
      <Body {...props} />
    </ErrorBoundary>
  )
}
