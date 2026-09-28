// 이벤트 — PLC EVTLOG + 콘솔 이벤트. 하위 보기 넷: 목록 · 알람 통계 · 스텝 통계 · 인터록(스윔레인).
// 목록의 행을 누르면 전후 ±30 s(모든 PLC), ctx 를 누르면 스텝 막대.
//
// 필터·목록·라이브 여부·보기는 모듈 스토어(`lib/evtlog/store`)라 탭을 옮겨 다녀도 남는다. 라이브 스트림은
// 화면이 떠 있는 동안만 잡고, 돌아오면 첫 쪽을 다시 받아 빈 구간을 메운다.
// 화면 상태는 URL 의 `e.*` 로도 나간다(링크 복사 · 저장된 필터) — 화면을 떠나면 걷어 낸다.
import { useCallback, useEffect, useMemo, useState } from 'react'
import { ScrollText } from 'lucide-react'
import { evtApi, type EventRow } from '../../lib/evtlog/api'
import {
  EMPTY_FILTER,
  activeCount,
  buildQuery,
  type EvtFilter,
} from '../../lib/evtlog/evtFilterModel'
import { countLevels, sourceBrief } from '../../lib/evtlog/evtRowsModel'
import {
  alarmListFilter,
  fmtDuration,
  periodRange,
  type ParetoMetric,
} from '../../lib/evtlog/evtStatsModel'
import {
  EVT_VIEWS,
  fromQuery,
  mergeSearch,
  toQuery,
  type EvtViewId,
} from '../../lib/evtlog/evtUrlModel'
import { evtView } from '../../lib/evtlog/store'
import { evtStream } from '../../lib/evtlog/stream'
import { swimQuery, type SwimState } from '../../lib/evtlog/swimlaneModel'
import { visibleInterval } from '../../lib/poll'
import { robots } from '../../lib/robots'
import { useStore } from '../../lib/store'
import { Button } from '../../lib/ui/Button'
import { EmptyState } from '../../lib/ui/EmptyState'
import { ErrorBoundary } from '../../lib/ui/ErrorBoundary'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import { ScreenHeader } from '../../lib/ui/ScreenHeader'
import { Segmented } from '../../lib/ui/Segmented'
import { StatRow, type StatItem } from '../../lib/ui/StatRow'
import { StatusDot } from '../../lib/ui/StatusDot'
import { Switch } from '../../lib/ui/Switch'
import { Toolbar } from '../../lib/ui/Toolbar'
import type { MenuItem } from '../../lib/ui/menu'
import { statusTone } from '../../lib/ui/status'
import { AlarmStatsView } from './AlarmStatsView'
import { AlertRulesDialog } from './AlertRulesDialog'
import { AroundDialog } from './AroundDialog'
import { CtxDialog } from './CtxDialog'
import { EventFilters } from './EventFilters'
import { EventList } from './EventList'
import { LoggerCfgDialog } from './LoggerCfgDialog'
import { ReportDialog } from './ReportDialog'
import { SavedFilterMenu } from './SavedFilterMenu'
import { StatsControls } from './StatsControls'
import { StepStatsView } from './StepStatsView'
import { SwimControls } from './SwimControls'
import { SwimlaneView } from './SwimlaneView'

const SOURCES_POLL_MS = 5000
const NOW_TICK_MS = 30_000
const NO_EVTLOG = 'PLC 에 EVTLOG 가 아직 없습니다 — 콘솔 이벤트만 표시'

function download(url: string): void {
  const a = document.createElement('a')
  a.href = url
  a.download = ''
  document.body.appendChild(a)
  a.click()
  a.remove()
}

/** 주소창의 `e.*` 를 화면 상태로 맞춘다(탭 `?tab=` 은 셸 몫이라 그대로). */
function writeUrl(query: string): void {
  const next = location.pathname + mergeSearch(location.search, query)
  if (next !== location.pathname + location.search) history.replaceState(null, '', next)
}

function EventsScreen() {
  useStore(evtView, evtStream, robots)
  const v = evtView
  const [now, setNow] = useState(() => Date.now())
  const [picked, setPicked] = useState<EventRow | null>(null)
  const [ctx, setCtx] = useState<number | null>(null)
  const [cfgOpen, setCfgOpen] = useState(false)
  const [rulesOpen, setRulesOpen] = useState(false)
  const [reportOpen, setReportOpen] = useState(false)
  const [metric, setMetric] = useState<ParetoMetric>('count')

  useEffect(() => robots.start(), [])

  useEffect(() => {
    void v.loadCatalog()
    void v.loadSources()
    void v.loadSaved()
    // 링크로 왔으면 그 상태로, 아니면 라이브면 떠나 있던 동안의 빈 구간을, 처음이면 첫 쪽을 받는다.
    const fromLink = fromQuery(location.search)
    if (fromLink) v.applyUrl(fromLink)
    else {
      if (!v.loaded || v.live) void v.reload()
      void v.loadView()
    }
    const t = visibleInterval(() => void v.loadSources(), SOURCES_POLL_MS)
    const n = visibleInterval(() => setNow(Date.now()), NOW_TICK_MS)
    return () => {
      clearInterval(t)
      clearInterval(n)
      writeUrl('')
    }
  }, [v])

  useEffect(() => (v.live ? v.attachLive() : undefined), [v, v.live])

  const urlQuery = toQuery(v.urlState)
  useEffect(() => writeUrl(urlQuery), [urlQuery])

  // 상태바 알림에서 고른 행 — 전후 창으로 연다
  useEffect(() => {
    const r = v.consumeFocus()
    if (r) setPicked(r)
  })

  const setFilter = useCallback((f: EvtFilter) => v.setFilter(f), [v])

  function setLive(on: boolean) {
    v.setLive(on)
    if (on) {
      evtStream.resetLag()
      void v.reload()
    }
  }

  const sources = v.sources ?? []
  const withLog = sources.filter((s) => s.available)
  const noEvtlog = v.sources !== null && withLog.length === 0
  const plcOptions = useMemo(
    () => [
      ...sources.map((s) => ({
        id: s.plc,
        label: s.plc,
        hint: s.available ? undefined : 'EVTLOG 없음',
      })),
      { id: 'console', label: 'console' },
    ],
    [sources],
  )
  const statsPlcs = useMemo(() => plcOptions.filter((p) => p.id !== 'console'), [plcOptions])
  const rows = v.rows
  const lv = useMemo(() => countLevels(rows), [rows])
  const filtered = activeCount(v.filter) > 0
  const view = v.view

  function openSwim(s: SwimState) {
    setPicked(null)
    v.setSwim(s)
    v.setView('ilock')
  }

  const listStats: StatItem[] = [
    { label: 'Rows', value: `${rows.length}${v.hasMore ? '+' : ''}`, testid: 'evt-stat-rows' },
    { label: 'ERROR', value: lv.error, tone: lv.error ? 'fault' : undefined },
    { label: 'WARN', value: lv.warn, tone: lv.warn ? 'warn' : undefined },
    ...withLog.map((s): StatItem => {
      const b = sourceBrief(s)
      return {
        label: s.plc,
        value: b.seq,
        hint: `Dropped ${b.drops} · gaps ${b.gaps}`,
        tone: b.bad ? 'fault' : b.drops || b.gaps ? 'warn' : undefined,
        title: s.collector.error ?? s.detail ?? undefined,
      }
    }),
    ...(noEvtlog ? [{ label: 'EVTLOG', value: '없음', title: NO_EVTLOG } as StatItem] : []),
    ...(v.live && evtStream.lag
      ? [
          {
            label: 'lag',
            value: evtStream.lag,
            tone: 'warn',
            help: '혼잡으로 서버가 라이브에서 건너뛴 행 수 — 새로고침하면 채워진다',
          } as StatItem,
        ]
      : []),
  ]
  const a = v.alarms.data
  const st = v.steps.data
  const sw = v.lanes.data
  const stats: StatItem[] =
    view === 'alarms'
      ? [
          { label: 'Alarms', value: a ? a.rows.length : '-', testid: 'evt-stat-alarms' },
          { label: 'Count', value: a ? a.count : '-' },
          { label: 'Duration', value: a ? fmtDuration(a.total_ms) : '-' },
          {
            label: 'Open',
            value: a ? a.rows.reduce((s, r) => s + r.open, 0) : '-',
            tone: a?.rows.some((r) => r.open) ? 'warn' : undefined,
          },
          ...(a?.by_plc ?? []).map((p): StatItem => ({
            label: p.plc,
            value: p.count,
            hint: fmtDuration(p.total_ms),
          })),
        ]
      : view === 'steps'
        ? [
            { label: 'Samples', value: st ? st.samples : '-', testid: 'evt-stat-samples' },
            { label: 'Steps', value: st ? st.rows.length : '-' },
            {
              label: 'Outliers',
              value: st ? st.outliers_total : '-',
              tone: st?.outliers_total ? 'warn' : undefined,
            },
          ]
        : view === 'ilock'
          ? [
              {
                label: 'Station',
                value: sw ? (sw.station ?? `slot ${sw.slot}`) : (v.swim.station ?? '-'),
                testid: 'evt-stat-station',
              },
              { label: 'Lanes', value: sw ? sw.lanes.length : '-' },
              {
                label: 'Markers',
                value: sw ? sw.markers.length : '-',
                tone: sw?.markers.some((m) => m.kind === 'alarm' || m.kind === 'ilk_timeout')
                  ? 'fault'
                  : undefined,
              },
              {
                label: 'Visits',
                value: sw ? sw.spans.length : '-',
                help: '로봇의 ILK_TARGET 이 이 스테이션이던 구간 수',
              },
            ]
          : listStats

  const menu: MenuItem[] = [
    {
      label: '로거 설정…',
      disabled: withLog.length ? undefined : (v.sourcesError ?? 'EVTLOG 가 있는 PLC 가 없습니다'),
      run: () => setCfgOpen(true),
      testid: 'evt-menu-cfg',
    },
    { label: '알림 규칙…', run: () => setRulesOpen(true), testid: 'evt-menu-rules' },
    {
      label: v.lang === 'en' ? '한국어 문구로 보기' : '영어 문구로 보기',
      run: () => v.setLang(v.lang === 'en' ? 'ko' : 'en'),
      testid: 'evt-menu-lang',
    },
    { label: '일일 보고서…', run: () => setReportOpen(true), testid: 'evt-menu-report' },
    {
      label: 'CSV 내보내기',
      run: () => download(evtApi.csvUrl(buildQuery(v.filter, { now: Date.now() }))),
      testid: 'evt-menu-csv',
    },
    {
      label: '새로고침',
      run: () => {
        void v.reload()
        void v.loadView()
      },
      testid: 'evt-menu-reload',
    },
  ]

  let listBody
  if (rows.length === 0) {
    if (v.error)
      listBody = (
        <EmptyState
          title="이벤트를 불러오지 못했습니다"
          hint={v.error}
          action={
            <Button size="sm" onClick={() => void v.reload()}>
              다시 시도
            </Button>
          }
        />
      )
    else if (!v.loaded || v.loading) listBody = <EmptyState title="불러오는 중" />
    else if (filtered)
      listBody = (
        <EmptyState
          title="조건에 맞는 이벤트 없음"
          hint="필터를 풀거나 기간을 넓혀 보세요."
          action={
            <Button size="sm" onClick={() => v.setFilter(EMPTY_FILTER)}>
              필터 초기화
            </Button>
          }
          testid="evt-empty-filtered"
        />
      )
    else
      listBody = (
        <EmptyState
          title={noEvtlog ? NO_EVTLOG : '이벤트 없음'}
          hint={
            v.live ? '실시간으로 기다리는 중입니다.' : '실시간을 켜면 새 이벤트가 위에 붙습니다.'
          }
          testid="evt-empty"
        />
      )
  } else {
    listBody = (
      <EventList
        rows={rows as EventRow[]}
        now={now}
        selectedId={picked?.id ?? null}
        live={v.live}
        hasMore={v.hasMore}
        loadingMore={v.loadingMore}
        trimmed={v.trimmed}
        lang={v.lang}
        onPick={setPicked}
        onCtx={setCtx}
        onMore={() => void v.more()}
        resetKey={v.filter}
      />
    )
  }

  const reloadView = () => void v.loadView()
  const body =
    view === 'alarms' ? (
      <AlarmStatsView
        data={v.alarms}
        metric={metric}
        now={now}
        onReload={reloadView}
        onPick={(r) => {
          v.setFilter(alarmListFilter(r, periodRange(v.stats, Date.now())))
          v.setView('list')
        }}
      />
    ) : view === 'steps' ? (
      <StepStatsView
        data={v.steps}
        split={v.stats.split}
        now={now}
        onCtx={setCtx}
        onReload={reloadView}
      />
    ) : view === 'ilock' ? (
      <SwimlaneView
        data={v.lanes}
        hasTarget={swimQuery(v.swim, now) !== null}
        onReload={reloadView}
        onZoom={(z) => v.setSwim({ ...v.swim, from: Math.round(z.t0), to: Math.round(z.t1) })}
        onMarker={(m) => setPicked(m)}
      />
    ) : (
      listBody
    )

  const controls =
    view === 'list' ? (
      <EventFilters value={v.filter} onChange={setFilter} plcs={plcOptions} catalog={v.catalog} />
    ) : view === 'ilock' ? (
      <SwimControls value={v.swim} onChange={(s) => v.setSwim(s)} />
    ) : (
      <StatsControls
        value={v.stats}
        onChange={(s) => v.setStats(s)}
        plcs={statsPlcs}
        metric={view === 'alarms' ? metric : undefined}
        onMetric={view === 'alarms' ? setMetric : undefined}
        split={view === 'steps'}
        types={view === 'alarms'}
      />
    )

  const liveDot = v.live ? (
    <StatusDot
      status={evtStream.connected ? 'ok' : 'warn'}
      size="sm"
      label={evtStream.connected ? '수신 중' : '끊김'}
      title={evtStream.error ?? undefined}
    />
  ) : null

  const viewError =
    view === 'alarms'
      ? v.alarms.error
      : view === 'steps'
        ? v.steps.error
        : view === 'ilock'
          ? v.lanes.error
          : null
  const viewStale = (view === 'alarms' && a) || (view === 'steps' && st) || (view === 'ilock' && sw)

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="events-page">
      <ScreenHeader
        title="이벤트"
        icon={<ScrollText size={14} />}
        items={[
          { label: 'EVTLOG', value: v.sources ? `${withLog.length}/${sources.length} PLC` : '-' },
          {
            label: 'catalog',
            value: v.catalog ? String(v.catalog.version) : (v.catalogError ?? '-'),
          },
        ]}
        trailing={
          view === 'list' ? (
            <span className="flex items-center gap-2">
              {liveDot}
              <Switch
                inline
                label="실시간"
                checked={v.live}
                onCheckedChange={setLive}
                testid="evt-live"
              />
            </span>
          ) : undefined
        }
      />
      <StatRow items={stats} testid="evt-stats" />
      <Toolbar
        dense
        lead={
          <div className="flex flex-wrap items-center gap-1.5 py-1">
            <Segmented
              ariaLabel="보기"
              value={view}
              onChange={(id: EvtViewId) => v.setView(id)}
              options={EVT_VIEWS.map((x) => ({
                id: x.id,
                label: x.label,
                testid: `evt-view-${x.id}`,
              }))}
            />
            {controls}
          </div>
        }
      >
        {(view === 'list' && v.error && rows.length) || (viewError && viewStale) ? (
          <span
            className={`text-2xs ${statusTone('fault').text}`}
            title={v.error ?? viewError ?? undefined}
          >
            조회 실패
          </span>
        ) : null}
        <SavedFilterMenu
          saved={v.saved}
          current={urlQuery}
          onApply={(f) => {
            const st2 = fromQuery(`?${f.query}`)
            v.applyUrl(st2 ?? { view: 'list', filter: EMPTY_FILTER, stats: v.stats, swim: v.swim })
          }}
          onSave={(name) => v.saveFilter(name, urlQuery)}
          onDelete={(f) => v.deleteFilter(f.id)}
        />
        <OverflowMenu items={menu} testid="evt-more-menu" />
      </Toolbar>
      <div className="flex min-h-0 flex-1 flex-col bg-surface-panel">{body}</div>

      <AroundDialog
        row={picked}
        now={now}
        onClose={() => setPicked(null)}
        onCtx={(c) => {
          // 모달 위에 모달을 겹치지 않는다 — 전후 창을 닫고 ctx 창으로.
          setPicked(null)
          setCtx(c)
        }}
        onSwim={openSwim}
      />
      <CtxDialog ctx={ctx} catalog={v.catalog} now={now} onClose={() => setCtx(null)} />
      <LoggerCfgDialog
        open={cfgOpen}
        onOpenChange={setCfgOpen}
        sources={sources}
        catalog={v.catalog}
        initialPlc={v.filter.plcs.find((p) => withLog.some((s) => s.plc === p)) ?? null}
      />
      <AlertRulesDialog
        open={rulesOpen}
        onOpenChange={setRulesOpen}
        catalog={v.catalog}
        plcs={[...sources.map((s) => s.plc), 'console']}
      />
      <ReportDialog open={reportOpen} onOpenChange={setReportOpen} onDownload={download} />
    </div>
  )
}

export default function EventsPage() {
  return (
    <ErrorBoundary label="이벤트">
      <EventsScreen />
    </ErrorBoundary>
  )
}
