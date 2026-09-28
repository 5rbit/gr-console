// 알림 규칙 — 목록과 편집이 **한 상자의 두 단계**다(모달 위에 모달을 겹치지 않는다).
// 위 두 스위치(소리 · 브라우저 알림)는 이 브라우저의 설정이다.
import { useEffect, useMemo, useState } from 'react'
import { Pencil, Plus, Trash2 } from 'lucide-react'
import { evtApi, type AlertRule, type EvtCatalog } from '../../lib/evtlog/api'
import { alerts } from '../../lib/evtlog/alerts'
import {
  EMPTY_RULE_FORM,
  formToBody,
  matchSummary,
  ruleFormWhy,
  ruleToForm,
  type RuleForm,
} from '../../lib/evtlog/alertsModel'
import { codeSuggestions } from '../../lib/evtlog/evtFilterModel'
import { EVT_TRANS } from '../../lib/evtlog/evtTypeModel'
import { EvtMultiPick, TYPE_OPTIONS } from './EventFilters'
import { useStore } from '../../lib/store'
import { Button } from '../../lib/ui/Button'
import { DataTable } from '../../lib/ui/DataTable'
import { Dialog } from '../../lib/ui/Dialog'
import { Input } from '../../lib/ui/Input'
import { Select } from '../../lib/ui/Select'
import { Switch } from '../../lib/ui/Switch'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'

const CODE_LIST_ID = 'evt-rule-code-suggest'

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

export function AlertRulesDialog({
  open,
  onOpenChange,
  catalog,
  plcs,
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
  catalog: EvtCatalog | null
  plcs: readonly string[]
}) {
  useStore(alerts)
  const [rules, setRules] = useState<AlertRule[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  /** 편집 중인 규칙 — `0` = 새 규칙, `null` = 목록. */
  const [editId, setEditId] = useState<number | null>(null)
  const [form, setForm] = useState<RuleForm>(EMPTY_RULE_FORM)
  const [busy, setBusy] = useState(false)
  /** 삭제는 두 번 누른다(상자 안에서 확인 — 두 번째 모달을 띄우지 않는다). */
  const [confirmDel, setConfirmDel] = useState<number | null>(null)

  async function load() {
    try {
      setRules(await evtApi.rules())
      setError(null)
    } catch (e) {
      setError(errText(e))
    }
  }

  useEffect(() => {
    if (!open) return
    setEditId(null)
    setConfirmDel(null)
    void load()
  }, [open])

  const suggestions = useMemo(
    () => codeSuggestions(form.codes, catalog?.events ?? []),
    [form.codes, catalog],
  )
  const why = ruleFormWhy(form, catalog)
  const dirty = editId !== null

  async function toggle(r: AlertRule, on: boolean) {
    try {
      await evtApi.updateRule(r.id, {
        name: r.name,
        enabled: on,
        match: r.match,
        cooldown_s: r.cooldown_s,
      })
      await load()
    } catch (e) {
      toast.error(errText(e))
    }
  }

  async function remove(r: AlertRule) {
    try {
      setConfirmDel(null)
      await evtApi.deleteRule(r.id)
      toast.ok(`규칙 삭제: ${r.name}`)
      await load()
    } catch (e) {
      toast.error(errText(e))
    }
  }

  async function save() {
    if (why || editId === null) return
    setBusy(true)
    try {
      const body = formToBody(form)
      if (editId === 0) await evtApi.createRule(body)
      else await evtApi.updateRule(editId, body)
      toast.ok(`규칙 저장: ${body.name}`)
      setEditId(null)
      await load()
    } catch (e) {
      toast.error(errText(e))
    } finally {
      setBusy(false)
    }
  }

  async function setNotify(on: boolean) {
    const err = await alerts.setNotify(on)
    if (err) toast.warn(err)
  }

  const cols: Column<AlertRule>[] = [
    {
      key: 'on',
      label: 'Enabled',
      cell: (r) => (
        <Switch
          checked={r.enabled}
          label={r.name}
          onCheckedChange={(on) => void toggle(r, on)}
          testid={`evt-rule-on-${r.id}`}
        />
      ),
    },
    { key: 'name', label: 'Name', get: (r) => r.name },
    { key: 'match', label: 'Match', get: (r) => matchSummary(r.match) },
    {
      key: 'cooldown',
      label: 'Cooldown (s)',
      get: (r) => r.cooldown_s,
      numeric: true,
      priority: 2,
    },
  ]

  const set = (patch: Partial<RuleForm>) => setForm((f) => ({ ...f, ...patch }))

  const footer =
    editId === null ? (
      <Button
        size="sm"
        icon={<Plus size={12} />}
        onClick={() => {
          setForm(EMPTY_RULE_FORM)
          setEditId(0)
        }}
        data-testid="evt-rule-add"
      >
        규칙 추가
      </Button>
    ) : (
      <>
        <Button size="sm" intent="ghost" onClick={() => setEditId(null)}>
          목록으로
        </Button>
        <Button
          size="sm"
          intent="primary"
          disabled={!!why || busy}
          title={why}
          onClick={() => void save()}
          data-testid="evt-rule-save"
        >
          저장
        </Button>
      </>
    )

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={editId === null ? '알림 규칙' : editId === 0 ? '알림 규칙 추가' : '알림 규칙 편집'}
      size="lg"
      dirty={dirty}
      footer={footer}
      testid="evt-rules-dialog"
    >
      {editId === null ? (
        <div className="space-y-3">
          <div className="flex flex-wrap items-center gap-4">
            <Switch
              inline
              label="소리"
              checked={alerts.beepOn}
              onCheckedChange={(on) => alerts.setBeep(on)}
              testid="evt-alert-beep"
            />
            <Switch
              inline
              label="브라우저 알림"
              checked={alerts.notifyOn}
              onCheckedChange={(on) => void setNotify(on)}
              testid="evt-alert-notify"
            />
          </div>
          <DataTable
            rows={rules ?? []}
            columns={cols}
            rowKey={(r) => String(r.id)}
            loading={rules === null && !error}
            density="compact"
            empty={error ?? '규칙 없음'}
            emptyDense
            actions={(r) => (
              <span className="flex gap-1">
                <Button
                  size="sm"
                  intent="ghost"
                  icon={<Pencil size={12} />}
                  onClick={() => {
                    setForm(ruleToForm(r))
                    setEditId(r.id)
                  }}
                >
                  편집
                </Button>
                <Button
                  size="sm"
                  intent={confirmDel === r.id ? 'danger' : 'ghost'}
                  icon={<Trash2 size={12} />}
                  onClick={() => (confirmDel === r.id ? void remove(r) : setConfirmDel(r.id))}
                  data-testid={`evt-rule-del-${r.id}`}
                >
                  {confirmDel === r.id ? '삭제 확인' : '삭제'}
                </Button>
              </span>
            )}
            testid="evt-rules"
          />
        </div>
      ) : (
        <div className="grid gap-3 sm:grid-cols-2" data-testid="evt-rule-form">
          <Input label="Name" value={form.name} onValueChange={(v) => set({ name: v })} />
          <Input
            label="Cooldown (s)"
            inputMode="numeric"
            value={form.cooldown}
            onValueChange={(v) => set({ cooldown: v })}
            hint="같은 규칙이 다시 울리기까지"
          />
          <Select label="PLC" value={form.plc} onValueChange={(v) => set({ plc: v })}>
            <option value="">전체</option>
            {plcs.map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
          </Select>
          <div className="flex flex-col gap-1">
            <span className="text-xs font-medium text-content-secondary">Types</span>
            <EvtMultiPick
              label="Type"
              options={TYPE_OPTIONS}
              value={form.types}
              onChange={(types) => set({ types })}
              testid="evt-rule-types"
            />
          </div>
          <Select label="Trans" value={form.trans} onValueChange={(v) => set({ trans: v })}>
            <option value="">전체</option>
            {EVT_TRANS.map((t) => (
              <option key={t.id} value={t.id}>
                {t.id} · {t.label}
              </option>
            ))}
          </Select>
          <Select label="Cat" value={form.cat} onValueChange={(v) => set({ cat: v })}>
            <option value="">전체</option>
            {(catalog?.cats ?? []).map((c) => (
              <option key={c.id} value={c.name}>
                {c.name}
              </option>
            ))}
          </Select>
          <Select label="MinLevel" value={form.minLvl} onValueChange={(v) => set({ minLvl: v })}>
            <option value="">전체</option>
            {(catalog?.levels ?? []).map((l) => (
              <option key={l.id} value={l.name}>
                ≥ {l.name}
              </option>
            ))}
          </Select>
          <Input
            label="Codes"
            mono
            list={CODE_LIST_ID}
            placeholder="CMD_EMS, F0202"
            value={form.codes}
            onValueChange={(v) => set({ codes: v })}
            hint="이벤트 이름 · 코드 · ErrorList 코드, 쉼표로 여럿"
          />
          <datalist id={CODE_LIST_ID}>
            {suggestions.map((s) => (
              <option key={s} value={s} />
            ))}
          </datalist>
          <Input
            label="TextContains"
            value={form.text}
            onValueChange={(v) => set({ text: v })}
            hint="렌더된 문구 · 이름 · detail 에서"
          />
          <Input
            label="A"
            value={form.aEq}
            onValueChange={(v) => set({ aEq: v })}
            hint="값이 이것일 때만 (on/off 이벤트는 1 = ON), 비우면 무관"
          />
          <div className="flex items-end">
            <Switch
              inline
              label="Enabled"
              checked={form.enabled}
              onCheckedChange={(on) => set({ enabled: on })}
            />
          </div>
        </div>
      )}
    </Dialog>
  )
}
