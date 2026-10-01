// 공통 정책(수요별 켜기 · 점수 · 셀 고르기, 초안 → 저장)과 스테이션 프로파일(줄 편집 → 한 번에 저장) 섹션 — 규칙 탭 · 스테이션 탭이
// 쓴다(옛 정책 탭). 사용자 규칙은 규칙 탭의 한 표(`RulesTab`)로 옮겼다.
import { useCallback, useEffect, useState, type ReactNode } from 'react'
import { Eye, Save, Undo2 } from 'lucide-react'
import type { CellPick, GenConfig } from '../../lib/taskgen'
import {
  DROP_MODE_LABEL,
  ROLE_LABEL,
  schedApi,
  type DropMode,
  type Policy,
  type StationProfile,
  type StationProfileRow,
  type StationRole,
} from '../../lib/sched'
import {
  cellPickText,
  dropsHere,
  guessHint,
  isConflict,
  mergePolicy,
  migratePlanLines,
  pascal,
  policyDiff,
  profileDirty,
  profileDraftOf,
  profilePayload,
  validatePolicy,
  validateProfile,
  withPolicy,
  withRole,
  type ProfileDraft,
} from '../../lib/sched/policyModel'
import { menuItems } from '../../lib/task/menuEntries'
import { Badge } from '../../lib/ui/Badge'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { Input } from '../../lib/ui/Input'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import { Section } from '../../lib/ui/Section'
import { Select } from '../../lib/ui/Select'
import { Switch } from '../../lib/ui/Switch'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'
import { CellPickDialog, SimulateDialog } from './dialogs'

type SchedConfig = GenConfig & { policy: Policy }

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e))

const CONFLICT_MSG =
  '다른 곳에서 먼저 저장했습니다 — 최신 설정을 다시 읽었으니 확인 후 다시 저장하세요'

/** 숫자 입력 — 쓰는 중인 글자(`-`, `1.`)를 지키고 숫자가 될 때만 올린다. 바깥 값이 바뀌면(저장 · 되돌리기) 따라간다. */
function NumInput({
  value,
  onChange,
  disabled,
  title,
  ariaLabel,
}: {
  value: number
  onChange: (n: number) => void
  disabled?: boolean
  title?: string
  ariaLabel: string
}) {
  const [st, setSt] = useState({ text: String(value), committed: value })
  const shown = Object.is(st.committed, value) ? st.text : String(value)
  return (
    <Input
      dense
      mono
      className="ml-auto w-20 text-right"
      value={shown}
      disabled={disabled}
      title={title}
      aria-label={ariaLabel}
      onValueChange={(x) => {
        const n = x.trim() === '' ? 0 : Number(x)
        if (Number.isFinite(n)) {
          setSt({ text: x, committed: n })
          onChange(n)
        } else setSt({ text: x, committed: value })
      }}
    />
  )
}

// ── 정책 ─────────────────────────────────────────────────────────────

type DemandKey = 'inbound' | 'outbound' | 'measure' | 'request' | 'consolidate' | 'multi_pick'
type BoolKey = 'inbound_auto' | 'outbound_auto' | 'measure_first' | 'consolidate' | 'multi_pick'
type PrioKey =
  | 'inbound_priority'
  | 'outbound_priority'
  | 'measure_priority'
  | 'request_priority'
  | 'consolidate_priority'
  | 'merge_priority'
type PickKey = 'inbound_dest' | 'outbound_source'

interface DemandRow {
  key: DemandKey
  name: string
  on: BoolKey | null
  prio: PrioKey
  help: string
}

const DEMANDS: DemandRow[] = [
  {
    key: 'request',
    name: 'Request',
    on: null,
    prio: 'request_priority',
    help: '상위 · 사용자 요청 — 늘 켜져 있고 요청 Priority × 10 이 더해진다',
  },
  {
    key: 'multi_pick',
    name: 'MultiPick',
    on: 'multi_pick',
    prio: 'merge_priority',
    help: '같은 품목이 두 PICK 스테이션에 1개씩이면 MergeInto 쪽 타이어 위 2단으로 합친 뒤 2개를 한 번에 빈 셀에 입고한다 — 풀스택은 Consolidate(Split)가 만든다',
  },
  {
    key: 'measure',
    name: 'Measure',
    on: 'measure_first',
    prio: 'measure_priority',
    help: '입고 품목에 비드 프로파일이 없으면 그 스테이션에서 먼저 MEASURE',
  },
  {
    key: 'outbound',
    name: 'Outbound',
    on: 'outbound_auto',
    prio: 'outbound_priority',
    help: '요청이 없어도 DROP 스테이션이 요청하면 가장 오래된 재고를 내보낸다',
  },
  {
    key: 'inbound',
    name: 'Inbound',
    on: 'inbound_auto',
    prio: 'inbound_priority',
    help: '요청이 없어도 PICK 스테이션에 화물이 준비되면 셀로 적재(품목을 알 때만)',
  },
  {
    key: 'consolidate',
    name: 'Consolidate',
    on: 'consolidate',
    prio: 'consolidate_priority',
    help: '로봇이 ConsolidateIdleS 동안 할 일이 없으면 같은 품목을 한 셀로 모은다',
  },
]

export function PolicySection({
  cfg,
  saveConfig,
}: {
  cfg: SchedConfig
  saveConfig: (c: SchedConfig, what: string) => Promise<boolean>
}) {
  const base = mergePolicy(cfg.policy)
  const [draft, setDraft] = useState<Policy | null>(null)
  const [pick, setPick] = useState<PickKey | null>(null)
  const [sim, setSim] = useState(false)
  const [saving, setSaving] = useState(false)
  const cur = draft ?? base
  const changed = draft ? policyDiff(base, draft) : []
  const invalid = validatePolicy(cur)
  const set = (patch: Partial<Policy>) => setDraft({ ...cur, ...patch })
  const mark = (k: keyof Policy) => (changed.includes(k) ? 'rounded ring-1 ring-accent' : '')

  const save = async () => {
    setSaving(true)
    const ok = await saveConfig(withPolicy(cfg, cur), '정책')
    setSaving(false)
    if (ok) setDraft(null)
  }

  const pickButton = (k: PickKey) => (
    <span className={mark(k)}>
      <Button
        type="button"
        size="sm"
        intent="ghost"
        title={`${pascal(k)} — 셀 고르기`}
        onClick={() => setPick(k)}
        data-testid={`sched-policy-${k}`}
      >
        <span className="text-content-muted">{pascal(k)}</span>
        <span className="font-mono text-2xs">{cellPickText(cur[k])}</span>
      </Button>
    </span>
  )

  const options: Record<DemandKey, ReactNode> = {
    request: null,
    multi_pick: (
      <span className={`inline-flex items-center gap-1 ${mark('multi_pick_max')}`}>
        <span className="text-2xs text-content-muted">MultiPickMax</span>
        <NumInput
          value={cur.multi_pick_max}
          ariaLabel="MultiPickMax"
          title="한 번에 집을 최대 개수 (2 ~ 3)"
          onChange={(n) => set({ multi_pick_max: n })}
        />
      </span>
    ),
    measure: null,
    outbound: pickButton('outbound_source'),
    inbound: (
      <span className="flex flex-wrap items-center gap-2">
        {pickButton('inbound_dest')}
        <span className={mark('inbound_near_drop')}>
          <Switch
            inline
            label="InboundNearDrop"
            title="적재 셀을 가장 가까운 DROP 스테이션 기준으로 고른다"
            checked={cur.inbound_near_drop}
            onCheckedChange={(v) => set({ inbound_near_drop: v })}
          />
        </span>
      </span>
    ),
    consolidate: (
      <span className={`inline-flex items-center gap-1 ${mark('consolidate_idle_s')}`}>
        <span className="text-2xs text-content-muted">ConsolidateIdleS (s)</span>
        <NumInput
          value={cur.consolidate_idle_s}
          ariaLabel="ConsolidateIdleS"
          onChange={(n) => set({ consolidate_idle_s: n })}
        />
      </span>
    ),
  }

  const cols: Column<DemandRow>[] = [
    {
      key: 'name',
      label: 'Demand',
      priority: 1,
      sortable: false,
      cell: (d) => <span title={d.help}>{d.name}</span>,
    },
    {
      key: 'on',
      label: 'Enabled',
      priority: 1,
      sortable: false,
      cell: (d) =>
        d.on ? (
          <span className={mark(d.on)}>
            <Switch
              checked={cur[d.on]}
              title={`${pascal(d.on)} — ${d.help}`}
              onCheckedChange={(v) => set({ [d.on as BoolKey]: v } as Partial<Policy>)}
              testid={`sched-policy-${d.on}`}
            />
          </span>
        ) : (
          <span className="text-content-faint" title={d.help}>
            늘
          </span>
        ),
    },
    {
      key: 'prio',
      label: 'Priority',
      priority: 1,
      sortable: false,
      numeric: true,
      cell: (d) => (
        <span className={`flex justify-end ${mark(d.prio)}`}>
          <NumInput
            value={cur[d.prio]}
            ariaLabel={pascal(d.prio)}
            title={pascal(d.prio)}
            onChange={(n) => set({ [d.prio]: n } as Partial<Policy>)}
          />
        </span>
      ),
    },
    {
      key: 'opt',
      label: 'Options',
      priority: 2,
      sortable: false,
      cell: (d) => options[d.key],
    },
  ]

  return (
    <Section
      title="정책"
      first
      help="모든 셀 · 스테이션에 공통으로 도는 수요. 점수가 높은 수요가 먼저 — 요청은 늘 정책 수요보다 먼저다. 저장 전에는 미리보기로 판정만 해 볼 수 있다."
      right={
        <span className="flex items-center gap-1">
          {changed.length ? (
            <span className="text-2xs text-accent-text" title={changed.map(pascal).join(' · ')}>
              변경 {changed.length}
            </span>
          ) : null}
          {draft ? (
            <Button
              type="button"
              size="sm"
              intent="ghost"
              icon={<Undo2 className="h-3.5 w-3.5" />}
              onClick={() => setDraft(null)}
              data-testid="sched-policy-discard"
            >
              되돌리기
            </Button>
          ) : null}
          <Button
            type="button"
            size="sm"
            intent="outline"
            icon={<Eye className="h-3.5 w-3.5" />}
            disabled={!!invalid}
            title={invalid ?? '저장 전 설정으로 한 번 판정'}
            onClick={() => setSim(true)}
            data-testid="sched-policy-preview"
          >
            미리보기
          </Button>
          <Button
            type="button"
            size="sm"
            intent="primary"
            icon={<Save className="h-3.5 w-3.5" />}
            loading={saving}
            disabled={!changed.length || !!invalid}
            title={invalid ?? (changed.length ? undefined : '바뀐 것 없음')}
            onClick={() => void save()}
            data-testid="sched-policy-save"
          >
            저장
          </Button>
        </span>
      }
      testid="sched-policy"
    >
      <DataTable
        rows={DEMANDS}
        columns={cols}
        rowKey={(d) => d.key}
        density="compact"
        testid="sched-policy-table"
      />
      {pick ? (
        <CellPickDialog
          title={pascal(pick)}
          value={cur[pick] as CellPick}
          source={pick === 'outbound_source'}
          onClose={() => setPick(null)}
          onApply={(p) => {
            set({ [pick]: p } as Partial<Policy>)
            setPick(null)
          }}
        />
      ) : null}
      {sim ? (
        <SimulateDialog
          title="미리보기 — 저장 전 정책"
          config={withPolicy(cfg, cur)}
          onClose={() => setSim(false)}
        />
      ) : null}
    </Section>
  )
}

// ── 스테이션 프로파일 ────────────────────────────────────────────────

const ROLES: StationRole[] = ['pick', 'drop', 'both', 'off']
const MODES: DropMode[] = ['single', 'stack', 'pallet']

type MigratePlan = { profiles: StationProfile[]; disabled_rules: string[]; policy: Policy }

export function StationsSection({ policy, reload }: { policy: Policy; reload: () => void }) {
  const [rows, setRows] = useState<StationProfileRow[] | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const [drafts, setDrafts] = useState<Record<number, ProfileDraft>>({})
  const [saving, setSaving] = useState(false)
  const [plan, setPlan] = useState<MigratePlan | null>(null)

  const load = useCallback(() => {
    schedApi
      .stations()
      .then((r) => {
        setRows(r ?? [])
        setErr(null)
      })
      .catch((e: unknown) => setErr(errText(e)))
  }, [])
  useEffect(load, [load])

  const list = rows ?? []
  const draftOf = (r: StationProfileRow) => drafts[r.id] ?? profileDraftOf(r)
  const put = (r: StationProfileRow, d: ProfileDraft) => setDrafts((m) => ({ ...m, [r.id]: d }))
  const dirty = list.filter((r) => drafts[r.id] && profileDirty(r, drafts[r.id]))
  const dirtyIds = new Set(dirty.map((r) => r.id))
  const invalid = dirty.map((r) => validateProfile(draftOf(r))).find(Boolean)
  const noProfile = list.filter((r) => !r.profile).length

  const saveDirty = async () => {
    setSaving(true)
    const res = await Promise.allSettled(
      dirty.map((r) =>
        schedApi.saveStation(r.id, profilePayload(draftOf(r), r.profile?.note ?? '')),
      ),
    )
    setSaving(false)
    const okIds = new Set(dirty.filter((_, i) => res[i].status === 'fulfilled').map((r) => r.id))
    const failed = dirty.filter((r) => !okIds.has(r.id))
    setDrafts((m) => Object.fromEntries(Object.entries(m).filter(([k]) => !okIds.has(Number(k)))))
    if (failed.length) {
      const why = res.find((x): x is PromiseRejectedResult => x.status === 'rejected')?.reason
      toast.error(`스테이션 ${failed.map((r) => r.id).join(', ')} 저장 실패 — ${errText(why)}`)
    } else toast.ok(`스테이션 프로파일 ${okIds.size}곳 저장`)
    load()
    reload()
  }

  const applyGuess = () =>
    void schedApi
      .applyGuess()
      .then((r) => {
        toast.ok(`추정 적용 — ${r.applied?.length ? r.applied.join(', ') : '없음'}`)
        load()
        reload()
      })
      .catch((e: unknown) => toast.error(`추정 적용 실패 — ${errText(e)}`))

  const dryMigrate = () =>
    void schedApi
      .migrateRules(true)
      .then((p) => setPlan(p))
      .catch((e: unknown) => toast.error(`이관 계획 실패 — ${errText(e)}`))

  const runMigrate = () =>
    void schedApi
      .migrateRules(false)
      .then((p) => {
        toast.ok(
          `이관 — 프로파일 ${p.profiles?.length ?? 0}곳 · 규칙 끔 ${p.disabled_rules?.length ?? 0}개`,
        )
        load()
        reload()
      })
      .catch((e: unknown) => toast.error(`이관 실패 — ${errText(e)}`))

  const cols: Column<StationProfileRow>[] = [
    {
      key: 'id',
      label: 'Id',
      get: (r) => r.id,
      numeric: true,
      priority: 1,
      cell: (r) => (
        <span className="inline-flex items-center gap-1 font-mono tabular-nums">
          {dirtyIds.has(r.id) ? (
            <span className="h-1.5 w-1.5 rounded-full bg-accent" title="저장 안 한 변경" />
          ) : null}
          {r.id}
          {r.pallet ? <Badge>pallet</Badge> : null}
        </span>
      ),
    },
    { key: 'conv', label: 'ConvNo', get: (r) => r.conv_no, numeric: true, priority: 3 },
    {
      key: 'role',
      label: 'Role',
      sortable: false,
      priority: 1,
      cell: (r) => {
        const d = draftOf(r)
        return (
          <Select
            dense
            aria-label={`Role ${r.id}`}
            value={d.role ?? ''}
            onValueChange={(v) => put(r, withRole(r, d, v ? (v as StationRole) : null))}
          >
            <option value="">— (정책 제외)</option>
            {ROLES.map((x) => (
              <option key={x} value={x}>
                {ROLE_LABEL[x]}
              </option>
            ))}
          </Select>
        )
      },
    },
    {
      key: 'mode',
      label: 'DropMode',
      sortable: false,
      priority: 1,
      cell: (r) => {
        const d = draftOf(r)
        return (
          <Select
            dense
            aria-label={`DropMode ${r.id}`}
            value={d.drop_mode}
            disabled={!dropsHere(d.role)}
            title={dropsHere(d.role) ? undefined : 'DROP 을 하지 않는 역할'}
            onValueChange={(v) => put(r, { ...d, drop_mode: v as DropMode })}
          >
            {MODES.map((x) => (
              <option key={x} value={x}>
                {DROP_MODE_LABEL[x]}
              </option>
            ))}
          </Select>
        )
      },
    },
    {
      key: 'merge',
      label: 'MergeInto',
      sortable: false,
      priority: 2,
      cell: (r) => {
        const d = draftOf(r)
        const picks = (x: StationRole | null) => x === 'pick' || x === 'both'
        const targets = list.filter((o) => o.id !== r.id && picks(draftOf(o).role))
        return (
          <Select
            dense
            aria-label={`MergeInto ${r.id}`}
            value={d.merge_into == null ? '' : String(d.merge_into)}
            disabled={!picks(d.role)}
            title={
              picks(d.role)
                ? '멀티 피킹: 이 스테이션 타이어를 고른 스테이션 타이어 위에 얹는다'
                : 'PICK 스테이션에만'
            }
            onValueChange={(v) => put(r, { ...d, merge_into: v ? Number(v) : null })}
          >
            <option value="">—</option>
            {targets.map((o) => (
              <option key={o.id} value={o.id}>
                {o.id}
              </option>
            ))}
          </Select>
        )
      },
    },
    {
      key: 'height',
      label: 'MaxHeight (mm)',
      sortable: false,
      numeric: true,
      priority: 2,
      cell: (r) => {
        const d = draftOf(r)
        const on = dropsHere(d.role) && d.drop_mode === 'stack'
        return (
          <NumInput
            value={d.max_height_mm}
            ariaLabel={`MaxHeight ${r.id}`}
            disabled={!on}
            title={on ? '0 = 높이 제한 없음 · 단수 한도는 품목 StackMax' : '스택 DROP 에만'}
            onChange={(n) => put(r, { ...d, max_height_mm: n })}
          />
        )
      },
    },
    {
      key: 'weight',
      label: 'Weight',
      sortable: false,
      numeric: true,
      priority: 2,
      cell: (r) => {
        const d = draftOf(r)
        return (
          <NumInput
            value={d.weight}
            ariaLabel={`Weight ${r.id}`}
            disabled={d.role === null}
            onChange={(n) => put(r, { ...d, weight: n })}
          />
        )
      },
    },
    {
      key: 'wait',
      label: 'MaxWait (s)',
      sortable: false,
      numeric: true,
      priority: 3,
      cell: (r) => {
        const d = draftOf(r)
        return (
          <NumInput
            value={d.max_wait_s}
            ariaLabel={`MaxWait ${r.id}`}
            disabled={d.role === null}
            title="0 = 경고 없음"
            onChange={(n) => put(r, { ...d, max_wait_s: n })}
          />
        )
      },
    },
    {
      key: 'guess',
      label: 'Guess',
      sortable: false,
      priority: 2,
      cell: (r) => {
        const g = guessHint(r)
        return g ? (
          <span className="text-2xs text-content-faint" title={g.title}>
            {g.text}
          </span>
        ) : null
      },
    },
  ]

  const lines = plan ? migratePlanLines(plan) : null
  const policyChanges = plan?.policy ? policyDiff(policy, mergePolicy(plan.policy)) : []

  return (
    <Section
      title="스테이션"
      help="프로파일이 없는 스테이션은 정책이 보지 않는다(사용자 규칙은 신호만 본다). Guess = GRM 연결로 추정한 값(저장 안 됨). MaxHeight 는 스택 DROP 의 최대 높이, 단수 한도는 품목 StackMax."
      right={
        <span className="flex items-center gap-1">
          <Button
            type="button"
            size="sm"
            intent="primary"
            icon={<Save className="h-3.5 w-3.5" />}
            loading={saving}
            disabled={!dirty.length || !!invalid}
            title={invalid ?? (dirty.length ? dirty.map((r) => r.id).join(', ') : '바뀐 줄 없음')}
            onClick={() => void saveDirty()}
            data-testid="sched-stations-save"
          >
            저장{dirty.length ? ` (${dirty.length})` : ''}
          </Button>
          <OverflowMenu
            title="스테이션 도구"
            testid="sched-stations-more"
            items={menuItems([
              {
                label: `추정 적용 (프로파일 없는 스테이션 ${noProfile})`,
                disabled: noProfile ? undefined : '프로파일 없는 스테이션이 없습니다',
                run: applyGuess,
                testid: 'sched-apply-guess',
              },
              { label: '기본 규칙 → 정책으로 옮기기…', run: dryMigrate, testid: 'sched-migrate' },
              dirty.length
                ? {
                    label: '편집 버리기',
                    run: () => setDrafts({}),
                    testid: 'sched-stations-discard',
                  }
                : null,
            ])}
          />
        </span>
      }
      testid="sched-stations"
    >
      {err ? (
        <span className="truncate text-2xs text-warn-fg" title={err}>
          스테이션을 읽지 못함 — {err}
        </span>
      ) : null}
      <DataTable
        rows={list}
        columns={cols}
        rowKey={(r) => String(r.id)}
        density="compact"
        emptyDense
        loading={rows === null && !err}
        empty="등록된 스테이션 없음"
        testid="sched-stations-table"
      />
      <ConfirmDialog
        open={!!plan}
        onOpenChange={(o) => {
          if (!o) setPlan(null)
        }}
        scope="console-data"
        title="기본 규칙 → 정책으로 옮기기"
        confirmLabel="옮기기"
        confirmDisabled={
          lines && !lines.profiles.length && !lines.rules.length && !policyChanges.length
            ? '옮길 것이 없습니다'
            : undefined
        }
        onConfirm={runMigrate}
      >
        {lines ? (
          <div className="flex flex-col gap-1 text-xs">
            <span className="text-content-muted">프로파일 {lines.profiles.length}</span>
            {lines.profiles.map((l) => (
              <span key={l} className="font-mono text-2xs">
                {l}
              </span>
            ))}
            <span className="text-content-muted">끄는 규칙 {lines.rules.length}</span>
            {lines.rules.length ? (
              <span className="font-mono text-2xs">{lines.rules.join(' · ')}</span>
            ) : null}
            {policyChanges.length ? (
              <span className="text-content-muted">
                정책 {policyChanges.map(pascal).join(' · ')}
              </span>
            ) : null}
          </div>
        ) : null}
      </ConfirmDialog>
    </Section>
  )
}

// ── 탭 ───────────────────────────────────────────────────────────────

/** 설정 저장(버전 충돌이면 알리고 다시 읽는다) — 규칙 · 정책 · 규칙 세트 탭이 같이 쓴다. */
export function useSaveConfig(reload: () => void) {
  return useCallback(
    async (c: SchedConfig, what: string) => {
      try {
        const saved = await schedApi.save(c)
        toast.ok(`${what} 저장 (v${saved.version})`)
        reload()
        return true
      } catch (e) {
        if (isConflict(e)) toast.error(CONFLICT_MSG)
        else toast.error(`${what} 저장 실패 — ${errText(e)}`)
        reload()
        return false
      }
    },
    [reload],
  )
}
