// 사용자 규칙 편집 팝업 — 조건(Trigger) · 만들 것(Action) · 로봇 · 생성 조건 스위치. 적용하면 `onSave`(저장은 호출자).
import { useState, type ReactNode } from 'react'
import {
  ALL_CONDITIONS,
  CONDITION_FIELDS,
  type CellPick,
  type GenAction,
  type GenRule,
  type GenTrigger,
} from '../../../lib/taskgen'
import type { Target } from '../../../lib/types'
import { FormDialog } from '../../../lib/ui/Dialog'
import { Input } from '../../../lib/ui/Input'
import { Select } from '../../../lib/ui/Select'
import { Switch } from '../../../lib/ui/Switch'
import { CellPickFields } from './CellPickFields'

const num = (s: string, d = 0) => (s.trim() === '' || !Number.isFinite(Number(s)) ? d : Number(s))

/** 규칙이 가리킬 수 없는 값을 막는다(없으면 undefined). */
function ruleError(r: GenRule, taken: readonly string[]): string | undefined {
  const a = r.action
  if (!r.id.trim() || !r.name.trim()) return 'Id · Name 이 필요합니다'
  if (taken.includes(r.id)) return `Id '${r.id}' 가 이미 있습니다`
  if (
    (a.kind === 'transfer' && ((!a.from_auto && !a.from.id) || (!a.to_auto && !a.to.id))) ||
    (a.kind === 'move' && !a.to.id) ||
    (a.kind === 'measure' && !a.target.id)
  )
    return '대상 Id 가 필요합니다'
  return undefined
}

export function RuleDialog({
  rule,
  takenIds = [],
  onClose,
  onSave,
}: {
  rule: GenRule
  /** 다른 규칙이 쓰는 Id(자기 자신 제외) — 겹치면 적용을 막는다. */
  takenIds?: readonly string[]
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
    (a as Record<string, unknown>)[k] as Target | undefined
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
  const error = ruleError(r, takenIds)
  return (
    <FormDialog
      open
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
      title={`생성 규칙 — ${r.name}`}
      size="md"
      submitLabel="적용"
      dirty={JSON.stringify(r) !== JSON.stringify(rule)}
      disabledReason={error}
      onSubmit={() => onSave(r)}
      testid="sched-rule-dialog"
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
              label="Target.Kind"
              value={t.target.kind}
              onValueChange={(k) => setT({ ...t, target: { ...t.target, kind: k as Target['kind'] } })}
            >
              <option value="station">station</option>
              <option value="cell">cell</option>
            </Select>
            <Input
              label="Target.Id"
              value={t.target.id}
              onValueChange={(x) => setT({ ...t, target: { ...t.target, id: num(x) } })}
            />
          </div>
        ) : t.kind === 'cell_stock' ? (
          <div className="grid grid-cols-3 gap-2">
            <Input label="Cell" value={t.cell} onValueChange={(x) => setT({ ...t, cell: num(x) })} />
            <Input
              label="Min"
              value={t.min ?? 1}
              onValueChange={(x) => setT({ ...t, min: num(x, 1) })}
            />
            <Input
              label="ItemCode"
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
      <div className="flex flex-col gap-2">
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
                label="ItemCode"
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
        <fieldset
          className="rounded border border-line-default p-2"
          title="끈 조건은 판정에서 빠집니다(위치 등록은 끌 수 없음)"
        >
          <legend className="px-1 text-2xs text-content-muted">Conditions</legend>
          <div className="grid grid-cols-2 gap-2">
            {CONDITION_FIELDS.map((f) => (
              <Switch
                key={f.key}
                inline
                label={f.label}
                title={f.title}
                checked={cond[f.key]}
                onCheckedChange={(v) => setR({ ...r, cond: { ...cond, [f.key]: v } })}
                testid={`sched-cond-${f.key}`}
              />
            ))}
          </div>
        </fieldset>
      </div>
    </FormDialog>
  )
}

/** 출발 · 도착 — 고정 대상 또는 자동 선택(구역 · 행 · 열 필터, 순서). */
export function PlaceEditor({
  label,
  auto,
  fixed,
  source = false,
  onAuto,
}: {
  label: string
  auto: CellPick | null
  fixed: ReactNode
  source?: boolean
  onAuto: (p: CellPick | null) => void
}) {
  return (
    <div className="flex flex-col gap-2 rounded border border-line-default p-2">
      <Select
        label={`${label}.Mode`}
        value={auto ? 'auto' : 'fixed'}
        onValueChange={(m) => onAuto(m === 'auto' ? { order: source ? 'oldest' : 'nearest' } : null)}
      >
        <option value="fixed">fixed</option>
        <option value="auto">auto</option>
      </Select>
      {auto ? <CellPickFields value={auto} source={source} onChange={onAuto} /> : fixed}
    </div>
  )
}
