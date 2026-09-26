// 로거 설정 — PLC 하나의 `EVTLOG` 머리 숫자와 `Cfg`(MinLevel · CatMask · MaxPerScan) 편집.
import { useEffect, useState } from 'react'
import { evtApi, type EvtCatalog, type EvtSource } from '../../lib/evtlog/api'
import {
  MAX_PER_SCAN_MAX,
  MAX_PER_SCAN_MIN,
  cfgPatch,
  draftOf,
  hasBit,
  maxPerScanError,
  setBit,
  type CfgDraft,
} from '../../lib/evtlog/evtCfgModel'
import { cfgBlockedReason } from '../../lib/evtlog/evtRowsModel'
import { evtView } from '../../lib/evtlog/store'
import { FormDialog } from '../../lib/ui/Dialog'
import { Input } from '../../lib/ui/Input'
import { Select } from '../../lib/ui/Select'
import { StatRow, type StatItem } from '../../lib/ui/StatRow'
import { Switch } from '../../lib/ui/Switch'
import { toast } from '../../lib/ui/toast'

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

export function LoggerCfgDialog({
  open,
  onOpenChange,
  sources,
  catalog,
  initialPlc,
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
  sources: readonly EvtSource[]
  catalog: EvtCatalog | null
  initialPlc: string | null
}) {
  const [plc, setPlc] = useState<string>('')
  const [src, setSrc] = useState<EvtSource | null>(null)
  const [draft, setDraft] = useState<CfgDraft | null>(null)
  const [loadErr, setLoadErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  // 고른 PLC 는 폴이 목록을 갈아도 유지한다 — 비어 있을 때만 채운다.
  useEffect(() => {
    if (!open) {
      setPlc('')
      return
    }
    if (!plc) setPlc(initialPlc ?? sources.find((s) => s.available)?.plc ?? sources[0]?.plc ?? '')
  }, [open, plc, initialPlc, sources])

  useEffect(() => {
    if (!open || !plc) return
    let alive = true
    const cached = sources.find((s) => s.plc === plc) ?? null
    setSrc(cached)
    setDraft(cached?.cfg ? draftOf(cached.cfg) : null)
    setLoadErr(null)
    // 목록은 폴 값이라 늦을 수 있다 — 여는 순간 한 번 더 읽는다.
    evtApi
      .cfg(plc)
      .then((s) => {
        if (!alive) return
        setSrc(s)
        setDraft(s.cfg ? draftOf(s.cfg) : null)
      })
      .catch((e: unknown) => {
        if (alive) setLoadErr(errText(e))
      })
    return () => {
      alive = false
    }
    // sources 는 폴마다 새 배열이다 — PLC 를 바꿀 때만 다시 읽는다.
  }, [open, plc])

  const blocked = cfgBlockedReason(src)
  const patch = src?.cfg && draft ? cfgPatch(src.cfg, draft) : {}
  const dirty = Object.keys(patch).length > 0
  const mpsErr = draft ? maxPerScanError(draft.maxPerScan) : undefined
  const why = blocked ?? mpsErr ?? (dirty ? undefined : '바뀐 값이 없습니다')

  async function submit() {
    if (!src || why) return
    setBusy(true)
    try {
      const next = await evtApi.setCfg(src.plc, patch)
      evtView.adoptSource(next)
      toast.ok(`${src.plc} 로거 설정 적용`)
      onOpenChange(false)
    } catch (e) {
      toast.error(`${src.plc} 로거 설정 실패 — ${errText(e)}`)
    } finally {
      setBusy(false)
    }
  }

  const h = src?.header
  const st = src?.stat
  const c = src?.collector
  const stats: StatItem[] = [
    { label: 'Total', value: h?.total ?? '-' },
    { label: 'BootId', value: h?.boot_id ?? '-' },
    { label: 'Dropped', value: h?.dropped ?? '-', tone: h?.dropped ? 'warn' : undefined },
    { label: 'RunUs', value: st?.run_us ?? '-' },
    { label: 'MaxRunUs', value: st?.max_run_us ?? '-' },
    {
      label: 'MaxPerScanHit',
      value: st?.max_per_scan_hit ?? '-',
      tone: st?.max_per_scan_hit ? 'warn' : undefined,
      help: 'MaxPerScan 에 걸려 다음 스캔으로 미룬 횟수',
    },
    { label: 'epoch', value: c?.epoch ?? '-' },
    { label: 'stored', value: c?.stored ?? '-' },
    { label: 'gaps', value: c?.gaps ?? '-', tone: c?.gaps ? 'warn' : undefined },
  ]

  return (
    <FormDialog
      open={open}
      onOpenChange={onOpenChange}
      title="로거 설정"
      meta={c?.last_read_at ? <span>읽음 {c.last_read_at}</span> : null}
      size="md"
      submitLabel="적용"
      disabledReason={why}
      busy={busy}
      dirty={dirty}
      onSubmit={() => void submit()}
      testid="evt-cfg"
    >
      <Select dense label="PLC" value={plc} onValueChange={setPlc} data-testid="evt-cfg-plc">
        {sources.map((s) => (
          <option key={s.plc} value={s.plc}>
            {s.plc}
            {s.available ? '' : ' (EVTLOG 없음)'}
          </option>
        ))}
      </Select>
      <StatRow items={stats} bordered={false} className="px-0" />
      {c?.error || loadErr || blocked ? (
        <div className="text-2xs text-warn-fg">{loadErr ?? c?.error ?? blocked}</div>
      ) : null}
      {draft ? (
        <>
          <div className="grid grid-cols-2 gap-3">
            <Select
              dense
              label="MinLevel"
              value={String(draft.minLevel)}
              disabled={!!blocked}
              onValueChange={(v) => setDraft({ ...draft, minLevel: Number(v) })}
            >
              {(catalog?.levels ?? []).map((l) => (
                <option key={l.id} value={l.id}>
                  {l.id} {l.name}
                </option>
              ))}
            </Select>
            <Input
              dense
              mono
              label="MaxPerScan"
              inputMode="numeric"
              hint={mpsErr ?? `${MAX_PER_SCAN_MIN}~${MAX_PER_SCAN_MAX}`}
              disabled={!!blocked}
              value={draft.maxPerScan}
              onValueChange={(v) => setDraft({ ...draft, maxPerScan: v })}
            />
          </div>
          <div className="flex flex-col gap-1">
            <span className="text-2xs font-medium text-content-muted">{`CatMask 16#${draft.catMask.toString(16).toUpperCase().padStart(8, '0')}`}</span>
            <div className="grid grid-cols-2 gap-x-4 gap-y-1 sm:grid-cols-3">
              {(catalog?.cats ?? []).map((cat) => (
                <Switch
                  key={cat.id}
                  inline
                  label={`${cat.id} ${cat.name}`}
                  checked={hasBit(draft.catMask, cat.id)}
                  disabled={!!blocked}
                  onCheckedChange={(on) =>
                    setDraft({ ...draft, catMask: setBit(draft.catMask, cat.id, on) })
                  }
                />
              ))}
            </div>
          </div>
        </>
      ) : null}
    </FormDialog>
  )
}
