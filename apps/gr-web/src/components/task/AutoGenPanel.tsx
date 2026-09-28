// Task 자동 생성 — 규칙 표(조건 값과 지금 상태를 같은 줄에), 편집은 팝업, 아래는 모니터(점수 근거 · 못 만든 사유 · 예정 큐).
import { useCallback, useEffect, useState } from 'react'
import { Plus, Send, Trash2 } from 'lucide-react'
import {
  ALL_CONDITIONS,
  CONDITION_FIELDS,
  EMPTY_WEIGHTS,
  actionLabel,
  breakdownText,
  condSummary,
  criteriaRows,
  formatMap,
  metricsLine,
  newRule,
  parseMap,
  ruleState,
  EMPTY_INPUTS,
  taskgenApi,
  triggerLabel,
  type CellPick,
  type GenAction,
  type CriteriaRow,
  type GenCandidate,
  type GenConfig,
  type GenInputs,
  type GenItem,
  type GenRule,
  type GenState,
  type GenTrigger,
} from '../../lib/taskgen'
import { loadFailureText } from '../../lib/api'
import { visibleInterval } from '../../lib/poll'
import { menuItems, type MenuEntry } from '../../lib/task/menuEntries'
import { Button } from '../../lib/ui/Button'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { EmptyState } from '../../lib/ui/EmptyState'
import { Dialog, FormDialog } from '../../lib/ui/Dialog'
import { Input } from '../../lib/ui/Input'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import { Section } from '../../lib/ui/Section'
import { Select } from '../../lib/ui/Select'
import { Switch } from '../../lib/ui/Switch'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'
import { api } from '../../lib/api'
import { useRegistry } from '../../lib/registry'
import type { Station, Target } from '../../lib/types'
import { ParamsPanel } from './ParamsPanel'
const num = (s: string, d = 0) => (s.trim() === '' || !Number.isFinite(Number(s)) ? d : Number(s))

function RuleDialog({
  rule,
  onClose,
  onSave,
}: {
  rule: GenRule
  onClose: () => void
  onSave: (r: GenRule) => void
}) {
  const [r, setR] = useState<GenRule>(rule)
  const cond = { ...ALL_CONDITIONS, ...r.cond }
  const t = r.trigger
  const a = r.action
  const setT = (x: GenTrigger) => setR({ ...r, trigger: x })
  const setA = (x: GenAction) => setR({ ...r, action: x })
  const tgt = (k: 'from' | 'to' | 'target') =>
    (a as Record<string, unknown>)[k] as { kind: 'cell' | 'station'; id: number } | undefined
  const targetInputs = (k: 'from' | 'to' | 'target', label: string) => {
    const v = tgt(k) ?? { kind: 'cell' as const, id: 0 }
    return (
      <div className="grid grid-cols-2 gap-2">
        <Select
          label={`${label}.Kind`}
          value={v.kind}
          onValueChange={(x) => setA({ ...a, [k]: { ...v, kind: x } } as GenAction)}
        >
          <option value="cell">cell</option>
          <option value="station">station</option>
        </Select>
        <Input
          label={`${label}.Id`}
          value={v.id}
          onValueChange={(x) => setA({ ...a, [k]: { ...v, id: num(x) } } as GenAction)}
        />
      </div>
    )
  }
  const error =
    !r.id.trim() || !r.name.trim()
      ? 'Id · Name 이 필요합니다'
      : (a.kind === 'transfer' && ((!a.from_auto && !a.from.id) || (!a.to_auto && !a.to.id))) ||
          (a.kind === 'move' && !a.to.id) ||
          (a.kind === 'measure' && !a.target.id)
        ? '대상 Id 가 필요합니다'
        : undefined
  return (
    <FormDialog
      open
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
      title={`생성 규칙 — ${r.name}`}
      size="md"
      submitLabel="적용"
      disabledReason={error}
      onSubmit={() => onSave(r)}
      testid="taskgen-rule-dialog"
    >
      <div className="grid grid-cols-2 gap-2">
        <Input label="Id" value={r.id} onValueChange={(x) => setR({ ...r, id: x.trim() })} />
        <Input label="Name" value={r.name} onValueChange={(x) => setR({ ...r, name: x })} />
        <Select
          label="Trigger"
          value={t.kind}
          onValueChange={(k) =>
            setT(
              k === 'manual'
                ? { kind: 'manual' }
                : k === 'cell_stock'
                  ? { kind: 'cell_stock', cell: 0, min: 1, item: null }
                  : k === 'unmeasured'
                    ? { kind: 'unmeasured', target: { kind: 'station', id: 0 } }
                    : ({ kind: k, station: 0, require_cvok: true } as GenTrigger),
            )
          }
        >
          <option value="manual">manual</option>
          <option value="station_req">station_req</option>
          <option value="station_item">station_item</option>
          <option value="cell_stock">cell_stock</option>
          <option value="unmeasured">unmeasured (미측정 품목)</option>
        </Select>
        {t.kind === 'station_req' || t.kind === 'station_item' ? (
          <div className="grid grid-cols-2 items-end gap-2">
            <Input
              label="Trigger.Station"
              value={t.station}
              onValueChange={(x) => setT({ ...t, station: num(x) })}
            />
            <Switch
              inline
              label="RequireCVOK"
              title="컨베이어 준비(PI.CVOK)도 켜져 있어야 조건 참"
              checked={t.require_cvok !== false}
              onCheckedChange={(v) => setT({ ...t, require_cvok: v })}
            />
          </div>
        ) : t.kind === 'unmeasured' ? (
          <div className="grid grid-cols-2 gap-2">
            <Select
              label="대상.Kind"
              value={t.target.kind}
              onValueChange={(k) =>
                setT({ ...t, target: { ...t.target, kind: k as Target['kind'] } })
              }
            >
              <option value="station">station</option>
              <option value="cell">cell</option>
            </Select>
            <Input
              label="대상.Id"
              value={t.target.id}
              onValueChange={(x) => setT({ ...t, target: { ...t.target, id: num(x) } })}
            />
          </div>
        ) : t.kind === 'cell_stock' ? (
          <div className="grid grid-cols-3 gap-2">
            <Input
              label="Cell"
              value={t.cell}
              onValueChange={(x) => setT({ ...t, cell: num(x) })}
            />
            <Input
              label="Min"
              value={t.min ?? 1}
              onValueChange={(x) => setT({ ...t, min: num(x, 1) })}
            />
            <Input
              label="Item"
              value={t.item ?? ''}
              onValueChange={(x) => setT({ ...t, item: x.trim() ? num(x) : null })}
            />
          </div>
        ) : (
          <span />
        )}
        <Select
          label="Action"
          value={a.kind}
          onValueChange={(k) =>
            setA(
              k === 'transfer'
                ? {
                    kind: 'transfer',
                    from: { kind: 'cell', id: 0 },
                    to: { kind: 'cell', id: 0 },
                    item: null,
                    count: 1,
                  }
                : k === 'move'
                  ? { kind: 'move', to: { kind: 'cell', id: 0 } }
                  : { kind: 'measure', target: { kind: 'cell', id: 0 }, item: null },
            )
          }
        >
          <option value="transfer">transfer (PICK → DROP)</option>
          <option value="move">move</option>
          <option value="measure">measure</option>
        </Select>
        <Input
          label="Priority"
          value={r.priority}
          onValueChange={(x) => setR({ ...r, priority: num(x) })}
        />
      </div>
      <div className="mt-2 flex flex-col gap-2">
        {a.kind === 'transfer' ? (
          <>
            <PlaceEditor
              label="From"
              auto={a.from_auto ?? null}
              source
              fixed={targetInputs('from', 'From')}
              onAuto={(p) => setA({ ...a, from_auto: p })}
            />
            <PlaceEditor
              label="To"
              auto={a.to_auto ?? null}
              fixed={targetInputs('to', 'To')}
              onAuto={(p) => setA({ ...a, to_auto: p })}
            />
            <Switch
              inline
              label="PalletAuto"
              title="도착이 팔렛 스테이션 — 다음 슬롯을 자동으로(자리 없으면 후보 아님)"
              checked={!!a.pallet_auto}
              onCheckedChange={(v) => setA({ ...a, pallet_auto: v })}
            />
            <div className="grid grid-cols-2 gap-2">
              <Input
                label="Item"
                value={a.item ?? ''}
                onValueChange={(x) => setA({ ...a, item: x.trim() ? num(x) : null })}
              />
              <Input
                label="Count"
                value={a.count ?? 1}
                onValueChange={(x) => setA({ ...a, count: Math.max(1, num(x, 1)) })}
              />
            </div>
          </>
        ) : a.kind === 'move' ? (
          targetInputs('to', 'To')
        ) : (
          targetInputs('target', 'Target')
        )}
        <div className="grid grid-cols-2 gap-2">
          <Input
            label="Robots"
            placeholder="비우면 전부 (예: 1,2)"
            value={r.robots.join(',')}
            onValueChange={(x) =>
              setR({
                ...r,
                robots: x
                  .split(',')
                  .map((p) => Number(p.trim()))
                  .filter((n) => Number.isInteger(n) && n > 0),
              })
            }
          />
          <Switch
            inline
            label="Enabled"
            checked={r.enabled}
            onCheckedChange={(v) => setR({ ...r, enabled: v })}
          />
        </div>
        <div className="rounded border border-line-default p-2">
          <div className="mb-1 text-2xs text-content-muted">
            생성 조건 — 끈 항목은 판단에서 빠집니다(위치 등록은 끌 수 없음)
          </div>
          <div className="grid grid-cols-2 gap-2">
            {CONDITION_FIELDS.map((f) => (
              <Switch
                key={f.key}
                inline
                label={f.label}
                title={f.title}
                checked={cond[f.key]}
                onCheckedChange={(v) => setR({ ...r, cond: { ...cond, [f.key]: v } })}
                testid={`taskgen-cond-${f.key}`}
              />
            ))}
          </div>
        </div>
      </div>
    </FormDialog>
  )
}

/** 출발·도착 — 고정 대상 또는 자동 선택(구역·행·열 필터, 순서). */
function PlaceEditor({
  label,
  auto,
  fixed,
  source = false,
  onAuto,
}: {
  label: string
  auto: CellPick | null
  fixed: React.ReactNode
  source?: boolean
  onAuto: (p: CellPick | null) => void
}) {
  const p = auto ?? {}
  const n = (x: string) => (x.trim() === '' ? null : num(x))
  return (
    <div className="flex flex-col gap-2 rounded border border-line-default p-2">
      <Select
        label={`${label}.Mode`}
        value={auto ? 'auto' : 'fixed'}
        onValueChange={(m) =>
          onAuto(m === 'auto' ? { order: source ? 'oldest' : 'nearest' } : null)
        }
      >
        <option value="fixed">fixed</option>
        <option value="auto">auto</option>
      </Select>
      {auto ? (
        <div className="grid grid-cols-3 gap-2">
          <Input
            label="Section"
            value={p.section ?? ''}
            onValueChange={(x) => onAuto({ ...p, section: n(x) })}
          />
          <Input
            label="RowMin"
            value={p.row_min ?? ''}
            onValueChange={(x) => onAuto({ ...p, row_min: n(x) })}
          />
          <Input
            label="RowMax"
            value={p.row_max ?? ''}
            onValueChange={(x) => onAuto({ ...p, row_max: n(x) })}
          />
          <Input
            label="ColMin"
            value={p.col_min ?? ''}
            onValueChange={(x) => onAuto({ ...p, col_min: n(x) })}
          />
          <Input
            label="ColMax"
            value={p.col_max ?? ''}
            onValueChange={(x) => onAuto({ ...p, col_max: n(x) })}
          />
          {source ? null : (
            <Input
              label="Near"
              placeholder="출고 Station Id (비우면 로봇 기준)"
              value={p.near ?? ''}
              onValueChange={(x) => onAuto({ ...p, near: n(x) })}
            />
          )}
          {source ? (
            <Select
              label="Order"
              value={p.order ?? 'oldest'}
              onValueChange={(o) => onAuto({ ...p, order: o })}
            >
              <option value="oldest">oldest</option>
              <option value="nearest">nearest</option>
            </Select>
          ) : (
            <Switch
              inline
              label="SameItemFirst"
              title="같은 품목이 쌓인 셀 먼저(아니면 빈 셀 먼저)"
              checked={!!p.same_item_first}
              onCheckedChange={(v) => onAuto({ ...p, same_item_first: v })}
            />
          )}
        </div>
      ) : (
        fixed
      )}
    </div>
  )
}

function WeightsDialog({
  cfg,
  onClose,
  onSave,
}: {
  cfg: GenConfig
  onClose: () => void
  onSave: (c: GenConfig) => void
}) {
  const w = { ...EMPTY_WEIGHTS, ...cfg.weights }
  const [target, setTarget] = useState(formatMap(w.target))
  const [item, setItem] = useState(formatMap(w.item))
  const [robot, setRobot] = useState(formatMap(w.robot))
  const maps = [parseMap(target), parseMap(item), parseMap(robot)]
  const bad = maps.find((m) => 'error' in m) as { error: string } | undefined
  return (
    <FormDialog
      open
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
      title="생성 우선순위 가중치"
      size="md"
      submitLabel="적용"
      disabledReason={bad?.error}
      onSubmit={() => {
        const [t, i, r] = maps as { ok: Record<string, number> }[]
        onSave({
          ...cfg,
          weights: { target: t.ok, item: i.ok, robot: r.ok },
        })
      }}
      testid="taskgen-weights-dialog"
    >
      <div className="grid grid-cols-1 gap-2">
        <Input
          label="Weights.Target"
          placeholder="2101:10, 401:2"
          value={target}
          onValueChange={setTarget}
        />
        <Input label="Weights.Item" placeholder="2011:5" value={item} onValueChange={setItem} />
        <Input label="Weights.Robot" placeholder="2:1" value={robot} onValueChange={setRobot} />
      </div>
    </FormDialog>
  )
}
function ParamsDialog({ onClose }: { onClose: () => void }) {
  return (
    <Dialog
      open
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
      title="스케줄링 · 생성 파라미터"
      size="lg"
      closeLabel="닫기"
      testid="taskgen-params-dialog"
    >
      <ParamsPanel />
    </Dialog>
  )
}

/** 자동 생성 탭과 같은 내용을 팝업으로 — 다른 탭에서 작업하다가 규칙·판단 기준을 볼 때. */
export function AutoGenDialog({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="자동 생성 — 규칙 · 판단 기준"
      size="lg"
      closeLabel="닫기"
      testid="autogen-dialog"
    >
      {open ? <AutoGenPanel /> : null}
    </Dialog>
  )
}

/**
 * 기본 규칙 만들기 — 출고 스테이션만 고르면 한 벌이 생긴다.
 * 측정 먼저(미측정 규격) · 출고 우선 · 적재는 출고에 가까운 셀. 영역·회피는 엔진이 늘 본다(규칙 아님).
 */
function SeedDialog({ onClose, onDone }: { onClose: () => void; onDone: () => void }) {
  const stations = useRegistry<Station>(api.stations)
  const ids = stations.items.map((s) => s.id).sort((a, b) => a - b)
  // 라인 끝(다음 연결 없음)을 출고로 먼저 고른다 — 아니면 첫 스테이션.
  const guess = stations.items.find((s) => !s.connection_next)?.id ?? ids[0]
  const [out, setOut] = useState<number>(0)
  const [busy, setBusy] = useState(false)
  const outId = out || guess || 0
  const ins = ids.filter((id) => id !== outId)
  return (
    <FormDialog
      open
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
      title="기본 규칙 만들기"
      size="md"
      submitLabel="만들기"
      disabledReason={
        busy ? '만드는 중' : ids.length === 0 ? '등록된 스테이션이 없습니다' : undefined
      }
      onSubmit={() => {
        setBusy(true)
        void taskgenApi
          .seedDefaults(outId, ins)
          .then((r) => {
            toast.ok(`기본 규칙 ${r.added}개 (v${r.config.version}) — 자동 생성은 꺼진 채입니다`)
            onDone()
          })
          .catch((e) => toast.error(`기본 규칙 — ${e instanceof Error ? e.message : String(e)}`))
          .finally(() => setBusy(false))
      }}
      testid="taskgen-seed-dialog"
    >
      <div className="flex flex-col gap-2 text-xs">
        <Select
          label="출고 스테이션"
          value={String(outId)}
          onValueChange={(v) => setOut(Number(v))}
        >
          {ids.map((id) => (
            <option key={id} value={String(id)}>
              {id}
            </option>
          ))}
        </Select>
        <div className="text-2xs text-content-muted">
          입고 스테이션: {ins.length ? ins.join(' · ') : '없음'} (출고를 뺀 등록 스테이션 전부)
        </div>
        <ul className="ml-4 list-disc text-2xs text-content-muted">
          <li>
            측정 먼저 — 입고에 올라온 화물의 품목에 비드 프로파일이 없으면 MEASURE (우선순위 80)
          </li>
          <li>출고 우선 — 출고가 요청하면 셀에서 꺼내 내보낸다 (60, 대상 가중 +20)</li>
          <li>적재 — 입고 → 출고에 가까운 셀 (40)</li>
          <li>
            영역이 겹치지 않는 명령 우선 · 회피 최우선은 엔진이 늘 본다(파라미터 gen_avoid_bonus)
          </li>
        </ul>
        <div className="text-2xs text-content-faint">
          같은 이름의 규칙은 새로 만든 것으로 바뀝니다. 만든 뒤에도 자동 생성은 꺼진 채이니 규칙을
          확인하고 켜세요.
        </div>
      </div>
    </FormDialog>
  )
}

const TONE = {
  ok: 'text-ok-fg',
  warn: 'text-warn-fg',
  muted: 'text-content-faint',
} as const

export function AutoGenPanel() {
  const [state, setState] = useState<GenState | null>(null)
  const [editing, setEditing] = useState<GenRule | null>(null)
  const [weightsOpen, setWeightsOpen] = useState(false)
  const [paramsOpen, setParamsOpen] = useState(false)
  const [seedOpen, setSeedOpen] = useState(false)
  const [confirmAuto, setConfirmAuto] = useState(false)
  // 2 초마다 조회한다 — 실패는 **한 번만** 토스트하고 그 뒤로는 띠에 남긴다(예전에는 2 초마다 토스트가 쌓였다).
  const [err, setErr] = useState<string | null>(null)
  const load = useCallback(() => {
    taskgenApi
      .get()
      .then((s) => {
        setState(s)
        setErr(null)
      })
      .catch((e: unknown) => {
        setErr((prev) => {
          const why = loadFailureText(e, '생성 규칙', false)
          if (prev === null) toast.error(why)
          return why
        })
      })
  }, [])
  useEffect(() => {
    load()
    const t = visibleInterval(load, 2000)
    return () => clearInterval(t)
  }, [load])

  const save = useCallback(
    async (c: GenConfig) => {
      try {
        const saved = await taskgenApi.save(c)
        toast.ok(`생성 규칙 저장 (v${saved.version})`)
        load()
      } catch (e) {
        toast.error(`저장 실패 — ${e instanceof Error ? e.message : String(e)}`)
      }
    },
    [load],
  )
  const cfg = state?.config ?? null
  const sepText = state ? `간격 ${state.separation_mm.toFixed(0)} mm` : ''
  const statusOf = (id: string) => state?.rules.find((x) => x.rule_id === id)

  const inputs = state?.inputs ?? EMPTY_INPUTS
  const crit = criteriaRows(cfg?.rules ?? [], state?.rules ?? [])
  const limitsText = `영역 간격 ${inputs.separation_mm.toFixed(0)} mm · 로봇당 큐 ${inputs.queue_depth}건${inputs.updated_at ? ` · ${inputs.updated_at}` : ''}`
  const yn = (v: boolean) => (
    <span className={v ? 'text-ok-fg' : 'text-content-faint'}>{v ? '1' : '0'}</span>
  )

  const critCols: Column<CriteriaRow>[] = [
    {
      key: 'rule',
      label: 'Rule',
      get: (c) => c.ruleName,
      priority: 1,
      cell: (c) => (
        <span
          className={c.enabled ? '' : 'text-content-faint'}
          title={c.enabled ? '' : '규칙 꺼짐'}
        >
          {c.ruleName}
        </span>
      ),
    },
    { key: 'label', label: '보는 값', get: (c) => c.label, priority: 1 },
    {
      key: 'value',
      label: '지금',
      get: (c) => c.value,
      priority: 1,
      cell: (c) => <span className="font-mono tabular-nums">{c.value}</span>,
    },
    {
      key: 'ok',
      label: '충족',
      sortable: false,
      priority: 1,
      cell: (c) => (
        <span className={c.ok ? 'text-ok-fg' : 'text-warn-fg'}>{c.ok ? '충족' : '아직'}</span>
      ),
    },
  ]

  const stationCols: Column<GenInputs['stations'][number]>[] = [
    {
      key: 'id',
      label: 'Station',
      get: (s) => s.id,
      numeric: true,
      priority: 1,
      cell: (s) => (
        <span className="font-mono tabular-nums" title={s.watched ? '켠 규칙이 보는 스테이션' : ''}>
          {s.watched ? '● ' : ''}
          {s.id}
        </span>
      ),
    },
    { key: 'cvok', label: 'CVOK', sortable: false, priority: 1, cell: (s) => yn(s.cvok) },
    { key: 'req', label: 'Req', sortable: false, priority: 1, cell: (s) => yn(s.req) },
    {
      key: 'item',
      label: 'ItemExist',
      sortable: false,
      priority: 1,
      cell: (s) => yn(s.item_exist),
    },
  ]

  const cellCols: Column<GenInputs['cells'][number]>[] = [
    {
      key: 'id',
      label: 'Cell',
      get: (c) => c.id,
      numeric: true,
      priority: 1,
      cell: (c) => (
        <span className="font-mono tabular-nums" title={c.watched ? '켠 규칙이 보는 셀' : ''}>
          {c.watched ? '● ' : ''}
          {c.id}
        </span>
      ),
    },
    { key: 'item', label: 'Item', get: (c) => c.item_code, numeric: true, priority: 2 },
    { key: 'count', label: '재고', get: (c) => c.count, numeric: true, priority: 1 },
    {
      key: 'room',
      label: '남은 칸',
      get: (c) => c.room ?? -1,
      numeric: true,
      priority: 1,
      cell: (c) => (c.room === null ? '—' : String(c.room)),
    },
  ]

  const robotCols: Column<GenInputs['robots'][number]>[] = [
    { key: 'name', label: 'Robot', get: (r) => r.name, priority: 1 },
    {
      key: 'x',
      label: 'X (mm)',
      get: (r) => r.x ?? 0,
      numeric: true,
      priority: 1,
      cell: (r) => (r.x === null ? '—' : r.x.toFixed(0)),
    },
    {
      key: 'busy',
      label: '생성 작업',
      sortable: false,
      priority: 1,
      cell: (r) => (
        <span className={r.busy ? 'text-ok-fg' : 'text-content-faint'}>
          {r.busy ? '진행 중' : '없음'}
        </span>
      ),
    },
  ]

  const ruleCols: Column<GenRule>[] = [
    { key: 'name', label: 'Name', get: (r) => r.name, priority: 1 },
    {
      key: 'state',
      label: '상태',
      sortable: false,
      priority: 1,
      cell: (r) => {
        const s = ruleState(statusOf(r.id), !!cfg?.auto)
        return (
          <span className={TONE[s.tone]} title={s.title}>
            {s.label}
          </span>
        )
      },
    },
    {
      key: 'inputs',
      label: '조건 값',
      sortable: false,
      priority: 2,
      cell: (r) => {
        const s = statusOf(r.id)
        return (
          <span className="truncate text-2xs text-content-muted" title={s?.inputs ?? ''}>
            {s?.inputs ?? ''}
          </span>
        )
      },
    },
    {
      key: 'cond',
      label: '조건',
      get: (r) => condSummary(r.cond).label,
      // 완화한 규칙은 좁은 칸에서도 보여야 한다(안전 신호).
      priority: 1,
      cell: (r) => {
        const c = condSummary(r.cond)
        return (
          <span
            className={c.off.length ? 'text-warn-fg' : 'text-content-faint'}
            title={c.off.length ? `끈 조건: ${c.off.join(' · ')}` : '생성 조건 모두 켜짐'}
          >
            {c.label}
          </span>
        )
      },
    },
    { key: 'trigger', label: 'Trigger', get: (r) => triggerLabel(r.trigger), priority: 3 },
    { key: 'action', label: 'Action', get: (r) => actionLabel(r.action), priority: 3 },
    {
      key: 'robots',
      label: 'Robots',
      get: (r) => (r.robots.length ? r.robots.join(',') : 'All'),
      priority: 3,
    },
    { key: 'prio', label: 'Priority', get: (r) => r.priority, numeric: true, priority: 3 },
    {
      key: 'made',
      label: '만든 건',
      get: (r) => statusOf(r.id)?.generated ?? 0,
      numeric: true,
      priority: 2,
      cell: (r) => {
        const s = statusOf(r.id)
        return (
          <span className="font-mono tabular-nums" title={s?.last_generated_at ?? '아직 없음'}>
            {s?.generated ?? 0}
          </span>
        )
      },
    },
    {
      key: 'on',
      label: 'Enabled',
      sortable: false,
      priority: 1,
      cell: (r) => (
        <Switch
          checked={r.enabled}
          title={r.enabled ? '규칙 켜짐' : '규칙 꺼짐'}
          onCheckedChange={(v) =>
            cfg &&
            void save({
              ...cfg,
              rules: cfg.rules.map((x) => (x.id === r.id ? { ...x, enabled: v } : x)),
            })
          }
        />
      ),
    },
  ]
  const candCols: Column<GenCandidate>[] = [
    { key: 'rule', label: 'Rule', get: (c) => c.rule_name, priority: 1 },
    { key: 'robot', label: 'Robot', get: (c) => c.robot_name, priority: 1 },
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
      label: 'X',
      get: (c) => c.area.lo,
      priority: 3,
      cell: (c) => `${c.area.lo.toFixed(0)}..${c.area.hi.toFixed(0)}`,
    },
    {
      key: 'reason',
      label: 'State',
      sortable: false,
      priority: 1,
      cell: (c) =>
        c.reason ? (
          <span className="text-warn-fg" title={c.reason}>
            대기
          </span>
        ) : (
          <span className="text-ok-fg">생성</span>
        ),
    },
  ]
  const queueCols: Column<GenItem>[] = [
    { key: 'rule', label: 'Rule', get: (g) => g.rule_name, priority: 1 },
    { key: 'robot', label: 'Robot', get: (g) => g.robot, priority: 1 },
    {
      key: 'step',
      label: 'Next',
      get: (g) => g.next,
      priority: 1,
      cell: (g) => `${g.steps[g.next]?.type ?? '-'} (${g.next + 1}/${g.steps.length})`,
    },
    {
      key: 'note',
      label: 'Note',
      sortable: false,
      priority: 2,
      cell: (g) => (
        <span className="truncate text-2xs" title={g.note ?? ''}>
          {g.note ?? ''}
        </span>
      ),
    },
  ]

  const tools: MenuEntry[] = [
    { label: '기본 규칙 만들기…', run: () => setSeedOpen(true), testid: 'taskgen-seed' },
    { label: '우선순위 가중치…', run: () => setWeightsOpen(true), testid: 'taskgen-weights' },
    {
      label: '제출 · 스케줄링 파라미터…',
      run: () => setParamsOpen(true),
      testid: 'taskgen-params',
    },
  ]

  // 조회가 실패해도 머리띠(스위치 · 규칙 추가 · 도구)는 남는다 — 엔진을 못 읽는 동안에도 자동 생성을 끌 수 있어야 한다.
  const head = (
    <div className="flex flex-wrap items-center gap-2">
      <Switch
        inline
        label="자동 생성"
        checked={!!cfg?.auto}
        disabled={!cfg}
        title={
          cfg
            ? '켜면 조건이 참이고 영역이 비는 후보를 예정으로 만들어 보냅니다'
            : '생성 엔진을 읽지 못해 지금 값을 모릅니다'
        }
        onCheckedChange={(v) =>
          v ? setConfirmAuto(true) : cfg && void save({ ...cfg, auto: false })
        }
        testid="taskgen-auto"
      />
      {cfg ? (
        <span className="text-2xs text-content-faint">
          v{cfg.version} · {sepText}
        </span>
      ) : null}
      {state?.note ? <span className="text-warn-fg">{state.note}</span> : null}
      <span className="flex-1" />
      <Button
        size="sm"
        intent="outline"
        icon={<Plus className="h-3.5 w-3.5" />}
        disabled={!cfg}
        onClick={() => cfg && setEditing(newRule(cfg.rules))}
        data-testid="taskgen-add"
      >
        규칙 추가
      </Button>
      <OverflowMenu items={menuItems(tools)} title="도구" testid="taskgen-more" />
    </div>
  )

  const errBand = err ? (
    <div className="flex items-center gap-2 rounded border border-warn-border bg-warn-soft px-2 py-1 text-2xs text-warn-fg">
      <span className="min-w-0 flex-1 truncate" title={err}>
        {err} · 2초마다 다시 시도{state ? ' · 아래는 마지막으로 읽은 값' : ''}
      </span>
      <Button size="sm" intent="ghost" onClick={load} data-testid="taskgen-retry">
        다시 시도
      </Button>
    </div>
  ) : null

  if (!cfg) {
    return (
      <div className="flex min-h-0 flex-col gap-3 text-xs" data-testid="autogen-panel">
        {head}
        {errBand ?? <span className="text-content-faint">읽는 중…</span>}
        {/* 규칙을 못 읽는 동안에도 열리는 팝업 — 둘 다 제 값을 따로 읽는다(가중치·규칙 편집은 설정이 있어야 한다). */}
        {paramsOpen ? <ParamsDialog onClose={() => setParamsOpen(false)} /> : null}
        {seedOpen ? (
          <SeedDialog
            onClose={() => setSeedOpen(false)}
            onDone={() => {
              setSeedOpen(false)
              load()
            }}
          />
        ) : null}
      </div>
    )
  }

  return (
    <div className="flex min-h-0 flex-col gap-3 text-xs" data-testid="autogen-panel">
      {head}
      {errBand}

      {cfg.rules.length === 0 ? (
        <EmptyState
          title="생성 규칙이 없습니다"
          hint="규칙 하나 = 조건(스테이션 요청 · 셀 재고 · 운전자 요청) + 만들 것(PICK→DROP 짝 · MOVE · MEASURE) + 보낼 로봇. 조건이 참이 되면 콘솔이 그 작업을 만듭니다."
          action={
            <Button
              size="sm"
              intent="primary"
              icon={<Plus className="h-3.5 w-3.5" />}
              onClick={() => setEditing(newRule(cfg.rules))}
              data-testid="taskgen-add-empty"
            >
              첫 규칙 만들기
            </Button>
          }
          testid="taskgen-empty"
        />
      ) : (
        <DataTable
          rows={cfg.rules}
          columns={ruleCols}
          rowKey={(r) => r.id}
          density="compact"
          emptyDense
          empty="규칙 없음"
          testid="taskgen-rules"
          onPick={(r) => setEditing(r)}
          actions={(r) => (
            <>
              {r.trigger.kind === 'manual' ? (
                <Button
                  size="icon-sm"
                  intent="ghost"
                  icon={<Send className="h-3.5 w-3.5" />}
                  title="요청 1건 (수동 규칙)"
                  onClick={() =>
                    void taskgenApi
                      .request(r.id)
                      .then((x) => toast.info(`${r.name}: 요청 ${x.requests}건`))
                  }
                />
              ) : null}
              <Button
                size="icon-sm"
                intent="ghost"
                icon={<Trash2 className="h-3.5 w-3.5" />}
                title="삭제"
                onClick={() => void save({ ...cfg, rules: cfg.rules.filter((x) => x.id !== r.id) })}
              />
            </>
          )}
        />
      )}

      <Section title="판단 기준" right={`${crit.filter((c) => c.ok).length} / ${crit.length} 충족`}>
        <DataTable
          rows={crit}
          columns={critCols}
          rowKey={(c) => c.key}
          density="compact"
          emptyDense
          empty="규칙이 없어 판단할 기준이 없습니다"
          testid="taskgen-criteria"
          onPick={(c) => {
            const r = cfg.rules.find((x) => x.id === c.ruleId)
            if (r) setEditing(r)
          }}
        />
      </Section>

      <Section
        title="엔진이 보는 값"
        right={`${inputs.tick_ms} ms 마다`}
        help="규칙과 무관하게 엔진이 매 판정에 읽는 값이다. 규칙이 겨냥한 줄에는 눈표를 단다. 셀은 재고가 있거나 규칙이 겨냥한 것만 보인다."
      >
        <div className="flex flex-col gap-2">
          <span className="text-2xs text-content-faint" data-testid="taskgen-limits">
            {limitsText}
          </span>
          <DataTable
            rows={inputs.stations}
            columns={stationCols}
            rowKey={(s) => String(s.id)}
            density="compact"
            emptyDense
            empty="GRM STATION 을 읽지 못함 (연결 · 계약 확인)"
            testid="taskgen-inputs-stations"
          />
          <DataTable
            rows={inputs.cells}
            columns={cellCols}
            rowKey={(c) => String(c.id)}
            density="compact"
            emptyDense
            empty="재고가 있거나 규칙이 겨냥한 셀 없음"
            testid="taskgen-inputs-cells"
          />
          <DataTable
            rows={inputs.robots}
            columns={robotCols}
            rowKey={(r) => String(r.id)}
            density="compact"
            emptyDense
            empty="로봇 설정 없음"
            testid="taskgen-inputs-robots"
          />
        </div>
      </Section>

      <Section title="모니터">
        <div className="flex flex-col gap-2">
          {state?.metrics ? (
            <span className="text-2xs text-content-faint" data-testid="taskgen-metrics">
              {metricsLine(state.metrics)}
            </span>
          ) : null}
          {state?.skipped.length ? (
            <span
              className="truncate text-2xs text-warn-fg"
              title={state.skipped.map((x) => `${x.rule}: ${x.reason}`).join('\n')}
            >
              후보 못 됨 {state.skipped.length}건 — {state.skipped[0].rule}:{' '}
              {state.skipped[0].reason}
            </span>
          ) : null}
          <DataTable
            rows={state?.candidates ?? []}
            columns={candCols}
            rowKey={(c) => `${c.rule_id}-${c.robot}`}
            density="compact"
            emptyDense
            empty="후보 없음 (참인 규칙 없음)"
            testid="taskgen-candidates"
          />
          <DataTable
            rows={state?.queue ?? []}
            columns={queueCols}
            rowKey={(g) => g.id}
            density="compact"
            emptyDense
            empty="예정 없음"
            testid="taskgen-queue"
            actions={(g) =>
              g.next === 0 ? (
                <Button
                  size="icon-sm"
                  intent="ghost"
                  icon={<Trash2 className="h-3.5 w-3.5" />}
                  title="예정 지우기 (안 보냄)"
                  onClick={() => void taskgenApi.removeQueued(g.id).then(load)}
                />
              ) : null
            }
          />
        </div>
      </Section>

      <ConfirmDialog
        open={confirmAuto}
        onOpenChange={setConfirmAuto}
        scope="fleet"
        title="자동 생성 켜기"
        confirmLabel="켜기"
        onConfirm={() => {
          setConfirmAuto(false)
          void save({ ...cfg, auto: true })
        }}
      >
        <div className="flex flex-col gap-1 text-xs">
          <div>
            켠 규칙의 조건이 참이 되면 콘솔이 스스로 Task 를 만들어 로봇에 보냅니다. 로봇이 AUTO
            이면 그대로 움직입니다.
          </div>
          <div className="text-content-faint">
            켜진 규칙 {cfg.rules.filter((r) => r.enabled).length} / {cfg.rules.length}개 · {sepText}
          </div>
        </div>
      </ConfirmDialog>

      {editing ? (
        <RuleDialog
          key={editing.id}
          rule={editing}
          onClose={() => setEditing(null)}
          onSave={(r) => {
            const exists = cfg.rules.some((x) => x.id === editing.id)
            const rules = exists
              ? cfg.rules.map((x) => (x.id === editing.id ? r : x))
              : [...cfg.rules, r]
            setEditing(null)
            void save({ ...cfg, rules })
          }}
        />
      ) : null}
      {weightsOpen ? (
        <WeightsDialog
          cfg={cfg}
          onClose={() => setWeightsOpen(false)}
          onSave={(c) => {
            setWeightsOpen(false)
            void save(c)
          }}
        />
      ) : null}
      {paramsOpen ? <ParamsDialog onClose={() => setParamsOpen(false)} /> : null}
      {seedOpen ? (
        <SeedDialog
          onClose={() => setSeedOpen(false)}
          onDone={() => {
            setSeedOpen(false)
            load()
          }}
        />
      ) : null}
    </div>
  )
}
