// 스케줄링·생성 파라미터 — 한 곳에서 보고 고친다(단위·범위·기본값·출처·도움말은 서버 스펙). 고친 값만 한 번에
// 저장하고, 누가/언제/무엇을 바꿨는지는 이력으로 남는다. PLC PARA 를 따라가는 값은 로봇별 실측과 나란히 보인다.
import { useCallback, useEffect, useMemo, useState } from 'react'
import { RotateCcw } from 'lucide-react'
import {
  fromText,
  paramsApi,
  toText,
  type ParamSpec,
  type ParamsHistory,
  type ParamsState,
} from '../../lib/params'
import { Button } from '../../lib/ui/Button'
import { DataTable } from '../../lib/ui/DataTable'
import { HelpTip } from '../../lib/ui/HelpTip'
import { Input } from '../../lib/ui/Input'
import { Segmented } from '../../lib/ui/Segmented'
import { Switch } from '../../lib/ui/Switch'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'

type View = 'values' | 'history'

export function ParamsPanel() {
  const [state, setState] = useState<ParamsState | null>(null)
  const [edits, setEdits] = useState<Record<string, string>>({})
  const [view, setView] = useState<View>('values')
  const [hist, setHist] = useState<ParamsHistory[]>([])
  const [busy, setBusy] = useState(false)
  const load = useCallback(() => {
    paramsApi
      .get()
      .then((s) => {
        setState(s)
        setEdits({})
      })
      .catch((e) => toast.error(`파라미터 — ${e instanceof Error ? e.message : String(e)}`))
  }, [])
  useEffect(load, [load])
  useEffect(() => {
    if (view === 'history')
      void paramsApi
        .history()
        .then(setHist)
        .catch(() => {})
  }, [view, state?.version])

  const parsed = useMemo(() => {
    const out: Record<string, unknown> = {}
    const errors: string[] = []
    if (!state) return { out, errors }
    for (const [k, text] of Object.entries(edits)) {
      const sp = state.spec.find((s) => s.key === k)
      if (!sp) continue
      const r = fromText(text, sp, state.defaults[k])
      if ('error' in r) errors.push(r.error)
      else out[k] = r.ok
    }
    return { out, errors }
  }, [edits, state])

  if (!state) return <span className="text-content-faint">읽는 중…</span>

  const value = (k: string) => (k in edits ? edits[k] : toText(state.params[k]))
  const dirty = Object.keys(edits).length > 0
  const plcSep = state.plc.filter((p) => p.separation !== null)

  async function save() {
    if (!state || parsed.errors.length) return
    setBusy(true)
    try {
      const r = await paramsApi.save({ ...state.params, ...parsed.out })
      toast.ok(`파라미터 저장 v${r.version} (${r.changes.length}개 바뀜)`)
      load()
    } catch (e) {
      toast.error(`저장 실패 — ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setBusy(false)
    }
  }
  async function reset(keys: string[]) {
    setBusy(true)
    try {
      const r = await paramsApi.reset(keys)
      toast.ok(`기본값으로 (v${r.version}, ${r.changes}개)`)
      load()
    } catch (e) {
      toast.error(`되돌리기 실패 — ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  const cols: Column<ParamSpec>[] = [
    { key: 'group', label: 'Group', get: (s) => s.group, priority: 3 },
    {
      key: 'key',
      label: 'Name',
      get: (s) => s.key,
      priority: 1,
      cell: (s) => (
        <span className="flex items-center gap-1 font-mono text-2xs">
          {s.key}
          <HelpTip
            title={s.key}
            text={`${s.help}${s.source ? ` — 출처: ${s.source}` : ''}${s.locked ? ' (고정)' : ''}`}
          />
        </span>
      ),
    },
    {
      key: 'value',
      label: 'Value',
      sortable: false,
      priority: 1,
      cell: (s) =>
        typeof state.defaults[s.key] === 'boolean' ? (
          <Switch
            checked={value(s.key) === 'true'}
            disabled={s.locked}
            title={s.locked ? '고정값' : s.key}
            onCheckedChange={(v) => setEdits({ ...edits, [s.key]: String(v) })}
          />
        ) : (
          <Input
            dense
            value={value(s.key)}
            disabled={s.locked}
            placeholder={
              s.key === 'echo_timeout_ms' ? `toml ${state.config.echo_timeout_ms}` : undefined
            }
            onValueChange={(v) => setEdits({ ...edits, [s.key]: v })}
            aria-label={s.key}
          />
        ),
    },
    { key: 'unit', label: 'Unit', get: (s) => s.unit, priority: 2 },
    {
      key: 'default',
      label: 'Default',
      sortable: false,
      priority: 3,
      cell: (s) => <span className="font-mono text-2xs">{toText(state.defaults[s.key])}</span>,
    },
    {
      key: 'plc',
      label: 'PLC',
      sortable: false,
      priority: 2,
      cell: (s) =>
        s.key === 'anticol_separation_mm' && plcSep.length ? (
          <span
            className="text-2xs"
            title={plcSep
              .map(
                (p) =>
                  `${p.name}: XLength ${p.x_length_front ?? '?'}/${p.x_length_rear ?? '?'} + Avoid ${p.margin_avoid ?? '?'} + Default ${p.margin_default ?? '?'} (Pos ${p.margin_pos ?? '?'})`,
              )
              .join('\n')}
          >
            {plcSep.map((p) => `${p.name} ${p.separation}`).join(' · ')}
            {plcSep.some((p) => p.separation !== state.params[s.key]) ? (
              <span className="text-warn-fg"> ≠</span>
            ) : null}
          </span>
        ) : null,
    },
  ]

  return (
    <div className="flex flex-col gap-2 text-xs">
      <div className="flex flex-wrap items-center gap-2">
        <Segmented
          ariaLabel="파라미터 보기"
          value={view}
          onChange={setView}
          options={[
            { id: 'values', label: 'Values' },
            { id: 'history', label: 'History' },
          ]}
        />
        <span className="text-content-faint">v{state.version}</span>
        {parsed.errors.length ? (
          <span className="truncate text-fault-fg" title={parsed.errors.join('\n')}>
            {parsed.errors[0]}
          </span>
        ) : null}
        <span className="flex-1" />
        <Button
          size="sm"
          intent="ghost"
          icon={<RotateCcw className="h-3.5 w-3.5" />}
          disabled={busy}
          onClick={() => void reset([])}
          title="모든 값을 기본값으로 (이력에 남는다)"
        >
          전체 기본값
        </Button>
        <Button size="sm" intent="ghost" disabled={!dirty || busy} onClick={() => setEdits({})}>
          되돌리기
        </Button>
        <Button
          size="sm"
          intent="primary"
          disabled={!dirty || busy || parsed.errors.length > 0}
          loading={busy}
          onClick={() => void save()}
          data-testid="params-save"
        >
          저장
        </Button>
      </div>
      {view === 'values' ? (
        <DataTable
          rows={state.spec}
          columns={cols}
          rowKey={(s) => s.key}
          density="compact"
          testid="params-table"
          actions={(s) =>
            s.locked ? null : (
              <Button
                size="icon-sm"
                intent="ghost"
                icon={<RotateCcw className="h-3.5 w-3.5" />}
                title="이 값만 기본값으로"
                disabled={busy}
                onClick={() => void reset([s.key])}
              />
            )
          }
        />
      ) : (
        <DataTable
          rows={hist}
          columns={[
            { key: 'v', label: 'Version', get: (h) => h.version, numeric: true, priority: 1 },
            {
              key: 'at',
              label: 'SavedAt',
              get: (h) => h.saved_at,
              cell: (h) => h.saved_at.replace('T', ' ').slice(0, 19),
              priority: 2,
            },
            { key: 'by', label: 'SavedBy', get: (h) => h.saved_by, priority: 2 },
            {
              key: 'ch',
              label: 'Changes',
              sortable: false,
              priority: 1,
              cell: (h) => (
                <span className="font-mono text-2xs">
                  {(h.changes ?? [])
                    .map((c) => `${c.key}: ${toText(c.old)} → ${toText(c.new)}`)
                    .join(' · ')}
                </span>
              ),
            },
          ]}
          rowKey={(h) => String(h.version)}
          density="compact"
          emptyDense
          empty="이력 없음"
          testid="params-history"
        />
      )}
    </div>
  )
}
