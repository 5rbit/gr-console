// 기록 탭 — KPI(1h · 8h · 24h) · 스테이션별 대기 · 로봇별 건수 · 판정 기록. 보이는 동안 5 초마다 다시 읽는다.
import { useCallback, useEffect, useState } from 'react'
import { schedApi, type Kpi, type LogRow, type SchedState } from '../../lib/sched'
import {
  KPI_HOURS,
  LOG_KINDS,
  fmtAt,
  fmtSeconds,
  kindLabel,
  kindTone,
  kpiTiles,
  normalizeKpi,
  sortLog,
  type KpiHours,
} from '../../lib/sched/historyModel'
import { visibleInterval } from '../../lib/poll'
import { DataTable } from '../../lib/ui/DataTable'
import { ErrorBoundary } from '../../lib/ui/ErrorBoundary'
import { Section } from '../../lib/ui/Section'
import { Segmented } from '../../lib/ui/Segmented'
import { Select } from '../../lib/ui/Select'
import { StatRow } from '../../lib/ui/StatRow'
import type { Column } from '../../lib/ui/table'

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e))

const KIND_CLASS = {
  ok: 'text-ok-fg',
  warn: 'text-warn-fg',
  fault: 'text-fault-fg',
  neutral: 'text-content-secondary',
} as const

type StationKpi = Kpi['stations'][number]
type RobotKpi = Kpi['robots'][number]

const mono = (v: string | number) => <span className="font-mono tabular-nums">{v}</span>

const stationCols: Column<StationKpi>[] = [
  { key: 'id', label: 'Id', get: (s) => s.id, numeric: true, priority: 1, cell: (s) => mono(s.id) },
  {
    key: 'gen',
    label: 'Generated',
    get: (s) => s.generated,
    numeric: true,
    priority: 1,
    cell: (s) => mono(s.generated),
  },
  {
    key: 'avg',
    label: 'Wait avg (s)',
    get: (s) => s.wait_avg_s ?? -1,
    numeric: true,
    priority: 1,
    cell: (s) => mono(fmtSeconds(s.wait_avg_s)),
  },
  {
    key: 'max',
    label: 'Wait max (s)',
    get: (s) => s.wait_max_s ?? -1,
    numeric: true,
    priority: 2,
    cell: (s) => mono(fmtSeconds(s.wait_max_s)),
  },
]

const robotCols: Column<RobotKpi>[] = [
  { key: 'name', label: 'Robot', get: (r) => r.name || String(r.id), priority: 1 },
  {
    key: 'gen',
    label: 'Generated',
    get: (r) => r.generated,
    numeric: true,
    priority: 1,
    cell: (r) => mono(r.generated),
  },
  {
    key: 'done',
    label: 'Completed',
    get: (r) => r.completed,
    numeric: true,
    priority: 1,
    cell: (r) => mono(r.completed),
  },
  {
    key: 'abort',
    label: 'Aborted',
    get: (r) => r.aborted,
    numeric: true,
    priority: 1,
    cell: (r) => (
      <span className={`font-mono tabular-nums ${r.aborted ? 'text-warn-fg' : ''}`}>
        {r.aborted}
      </span>
    ),
  },
]

const logCols: Column<LogRow>[] = [
  {
    key: 'at',
    label: 'At',
    get: (l) => l.at,
    priority: 1,
    cell: (l) => (
      <span className="font-mono tabular-nums" title={l.at}>
        {fmtAt(l.at)}
      </span>
    ),
  },
  {
    key: 'kind',
    label: 'Kind',
    get: (l) => l.kind,
    priority: 1,
    cell: (l) => (
      <span className={KIND_CLASS[kindTone(l.kind)]} title={l.kind}>
        {kindLabel(l.kind)}
      </span>
    ),
  },
  {
    key: 'key',
    label: 'Key',
    get: (l) => l.key,
    priority: 2,
    cell: (l) => (
      <span className="truncate font-mono text-2xs" title={l.key}>
        {l.key}
      </span>
    ),
  },
  {
    key: 'robot',
    label: 'Robot',
    get: (l) => l.robot ?? '',
    numeric: true,
    priority: 2,
    cell: (l) => mono(l.robot ?? '—'),
  },
  {
    key: 'detail',
    label: 'Detail',
    sortable: false,
    priority: 1,
    cell: (l) => (
      <span className="truncate text-2xs text-content-muted" title={l.detail}>
        {l.detail}
      </span>
    ),
  },
]

function HistoryBody() {
  const [hours, setHours] = useState<KpiHours>(24)
  const [kind, setKind] = useState<string>('')
  const [kpi, setKpi] = useState<Kpi | null>(null)
  const [log, setLog] = useState<LogRow[] | null>(null)
  const [err, setErr] = useState<string | null>(null)

  const load = useCallback(() => {
    Promise.all([schedApi.kpi(hours), schedApi.log(200, kind || undefined)])
      .then(([k, l]) => {
        setKpi(normalizeKpi(k, hours))
        setLog(sortLog(l))
        setErr(null)
      })
      .catch((e: unknown) => setErr(errText(e)))
  }, [hours, kind])

  useEffect(() => {
    load()
    const t = visibleInterval(load, 5000)
    return () => clearInterval(t)
  }, [load])

  const tiles = kpiTiles(kpi)

  return (
    <div className="flex min-h-0 flex-col gap-3 text-xs" data-testid="sched-history">
      <div className="flex flex-wrap items-center gap-2">
        <Segmented<string>
          value={String(hours)}
          onChange={(v) => setHours(Number(v) as KpiHours)}
          ariaLabel="Hours"
          options={KPI_HOURS.map((h) => ({ id: String(h), label: `${h}h` }))}
        />
        {err ? (
          <span className="min-w-0 flex-1 truncate text-2xs text-warn-fg" title={err}>
            기록을 읽지 못함 — {err}
          </span>
        ) : null}
      </div>

      <StatRow
        testid="sched-kpi"
        items={tiles.map((t) => ({
          label: t.label,
          value: t.value,
          tone: t.tone,
          testid: `sched-kpi-${t.key}`,
        }))}
      />

      <div className="grid gap-3 lg:grid-cols-2">
        <Section title="스테이션" first>
          <DataTable
            rows={kpi?.stations ?? []}
            columns={stationCols}
            rowKey={(s) => String(s.id)}
            density="compact"
            emptyDense
            loading={!kpi && !err}
            empty="이 기간 기록 없음"
            testid="sched-kpi-stations"
          />
        </Section>
        <Section title="로봇" first>
          <DataTable
            rows={kpi?.robots ?? []}
            columns={robotCols}
            rowKey={(r) => String(r.id)}
            density="compact"
            emptyDense
            loading={!kpi && !err}
            empty="이 기간 기록 없음"
            testid="sched-kpi-robots"
          />
        </Section>
      </div>

      <Section
        title="판정 기록"
        right={
          <Select
            dense
            aria-label="Kind"
            value={kind}
            onValueChange={setKind}
            data-testid="sched-log-kind"
          >
            <option value="">전체</option>
            {LOG_KINDS.map((k) => (
              <option key={k} value={k}>
                {kindLabel(k)}
              </option>
            ))}
          </Select>
        }
      >
        <DataTable
          rows={log ?? []}
          columns={logCols}
          rowKey={(l) => String(l.id)}
          density="compact"
          emptyDense
          stickyHeader
          loading={log === null && !err}
          empty="기록 없음"
          testid="sched-log"
        />
      </Section>
    </div>
  )
}

export default function HistoryTab(_props: { state: SchedState | null; reload: () => void }) {
  return (
    <ErrorBoundary label="기록">
      <HistoryBody />
    </ErrorBoundary>
  )
}
