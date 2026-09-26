// 이벤트 — PLC EVTLOG + 콘솔 이벤트를 한 목록으로. 행을 누르면 전후 ±30 s(모든 PLC), ctx 를 누르면 스텝 막대.
//
// 필터·목록·라이브 여부는 모듈 스토어(`lib/evtlog/store`)라 탭을 옮겨 다녀도 남는다. 라이브 스트림은
// 화면이 떠 있는 동안만 잡고, 돌아오면 첫 쪽을 다시 받아 빈 구간을 메운다.
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
import { evtView } from '../../lib/evtlog/store'
import { evtStream } from '../../lib/evtlog/stream'
import { visibleInterval } from '../../lib/poll'
import { robots } from '../../lib/robots'
import { useStore } from '../../lib/store'
import { Button } from '../../lib/ui/Button'
import { EmptyState } from '../../lib/ui/EmptyState'
import { ErrorBoundary } from '../../lib/ui/ErrorBoundary'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import { ScreenHeader } from '../../lib/ui/ScreenHeader'
import { StatRow, type StatItem } from '../../lib/ui/StatRow'
import { StatusDot } from '../../lib/ui/StatusDot'
import { Switch } from '../../lib/ui/Switch'
import { Toolbar } from '../../lib/ui/Toolbar'
import type { MenuItem } from '../../lib/ui/menu'
import { statusTone } from '../../lib/ui/status'
import { AroundDialog } from './AroundDialog'
import { CtxDialog } from './CtxDialog'
import { EventFilters } from './EventFilters'
import { EventList } from './EventList'
import { LoggerCfgDialog } from './LoggerCfgDialog'

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

function EventsScreen() {
  useStore(evtView, evtStream, robots)
  const v = evtView
  const [now, setNow] = useState(() => Date.now())
  const [picked, setPicked] = useState<EventRow | null>(null)
  const [ctx, setCtx] = useState<number | null>(null)
  const [cfgOpen, setCfgOpen] = useState(false)

  useEffect(() => robots.start(), [])

  useEffect(() => {
    void v.loadCatalog()
    void v.loadSources()
    // 라이브면 떠나 있던 동안의 빈 구간을, 처음이면 첫 쪽을 받는다.
    if (!v.loaded || v.live) void v.reload()
    const t = visibleInterval(() => void v.loadSources(), SOURCES_POLL_MS)
    const n = visibleInterval(() => setNow(Date.now()), NOW_TICK_MS)
    return () => {
      clearInterval(t)
      clearInterval(n)
    }
  }, [v])

  useEffect(() => (v.live ? v.attachLive() : undefined), [v, v.live])

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
  const rows = v.rows
  const lv = useMemo(() => countLevels(rows), [rows])
  const filtered = activeCount(v.filter) > 0

  const stats: StatItem[] = [
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

  const menu: MenuItem[] = [
    {
      label: '로거 설정…',
      disabled: withLog.length ? undefined : (v.sourcesError ?? 'EVTLOG 가 있는 PLC 가 없습니다'),
      run: () => setCfgOpen(true),
      testid: 'evt-menu-cfg',
    },
    {
      label: 'CSV 내보내기',
      run: () => download(evtApi.csvUrl(buildQuery(v.filter, { now: Date.now() }))),
      testid: 'evt-menu-csv',
    },
    { label: '새로고침', run: () => void v.reload(), testid: 'evt-menu-reload' },
  ]

  let body
  if (rows.length === 0) {
    if (v.error)
      body = (
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
    else if (!v.loaded || v.loading) body = <EmptyState title="불러오는 중" />
    else if (filtered)
      body = (
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
      body = (
        <EmptyState
          title={noEvtlog ? NO_EVTLOG : '이벤트 없음'}
          hint={
            v.live ? '실시간으로 기다리는 중입니다.' : '실시간을 켜면 새 이벤트가 위에 붙습니다.'
          }
          testid="evt-empty"
        />
      )
  } else {
    body = (
      <EventList
        rows={rows as EventRow[]}
        now={now}
        selectedId={picked?.id ?? null}
        live={v.live}
        hasMore={v.hasMore}
        loadingMore={v.loadingMore}
        trimmed={v.trimmed}
        onPick={setPicked}
        onCtx={setCtx}
        onMore={() => void v.more()}
        resetKey={v.filter}
      />
    )
  }

  const liveDot = v.live ? (
    <StatusDot
      status={evtStream.connected ? 'ok' : 'warn'}
      size="sm"
      label={evtStream.connected ? '수신 중' : '끊김'}
      title={evtStream.error ?? undefined}
    />
  ) : null

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
        }
      />
      <StatRow items={stats} testid="evt-stats" />
      <Toolbar
        dense
        lead={
          <EventFilters
            value={v.filter}
            onChange={setFilter}
            plcs={plcOptions}
            catalog={v.catalog}
          />
        }
      >
        {v.error && rows.length ? (
          <span className={`text-2xs ${statusTone('fault').text}`} title={v.error}>
            조회 실패
          </span>
        ) : null}
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
      />
      <CtxDialog ctx={ctx} catalog={v.catalog} now={now} onClose={() => setCtx(null)} />
      <LoggerCfgDialog
        open={cfgOpen}
        onOpenChange={setCfgOpen}
        sources={sources}
        catalog={v.catalog}
        initialPlc={v.filter.plcs.find((p) => withLog.some((s) => s.plc === p)) ?? null}
      />
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
