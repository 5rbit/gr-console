// 트레이스 — PLC 가 **매 스캔 주기**로 채널 32개까지를 표본해 소켓 링크로 밀어 올리는 파형을
// 실시간으로 보고, 저장된 세션을 같은 차트로 되감아 보는 화면.
//
// 다른 화면의 값은 폴이나 20 Hz 상태 스트림에서 온다 — 그건 "지금 어떤가"를 본다. 여기 값은 PLC
// 스캔과 **같은 박자**라 "그 한 스캔에 무슨 일이 있었나"를 본다. 그래서 브라우저 쪽 손실이 보이는
// 자리(overrun · gaps · lag)를 값과 같은 크기로 낸다: 무손실이 아니면 파형으로 판단하면 안 된다.
//
// 데이터 경로: SSE `trace` → `traceStream`(링 30초) → `traceChartModel`(엔벨로프 다운샘플) → 캔버스.
// 재생 경로:   `/api/trace/<id>?max=budgetFor(폭)` → `RowArray` → 같은 차트.
import { useCallback, useEffect, useMemo, useState } from 'react'
import { Waves } from 'lucide-react'
import { robots } from '../../lib/robots'
import { visibleInterval } from '../../lib/poll'
import { useStore } from '../../lib/store'
import {
  traceApi,
  type TraceChannelMeta,
  type TraceMark,
  type TraceMeta,
} from '../../lib/trace/api'
import { Button } from '../../lib/ui/Button'
import { Card } from '../../lib/ui/Card'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { ErrorBoundary } from '../../lib/ui/ErrorBoundary'
import { FieldList, type FieldItem } from '../../lib/ui/FieldList'
import { HelpTip } from '../../lib/ui/HelpTip'
import { Input } from '../../lib/ui/Input'
import { ScreenHeader } from '../../lib/ui/ScreenHeader'
import { StatRow, type StatItem } from '../../lib/ui/StatRow'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'
import { budgetFor } from '../../lib/ui/viz/downsample'
import { traceColor } from '../../lib/ui/viz/traceSeries'
import { ChannelPicker } from './ChannelPicker'
import { SessionList, clock, durationSec } from './SessionList'
import { TraceChart } from './TraceChart'
import type { SeriesSpec } from './traceChartModel'
import { traceLive } from './traceStream'
import {
  CHAN_MAX,
  FLUSH_MIN_MS,
  RING_MS,
  RowArray,
  fmtT,
  fmtValue,
  health,
  parseSelection,
  shortPath,
  startDisabledReason,
  stripCycle,
  type RowSource,
} from './traceModel'

const POLL_MS = 1000
const LS_SEL = 'gr-trace-channels'
const LS_CFG = 'gr-trace-config'
const CHART_H = 340

interface Replay {
  meta: TraceMeta
  marks: TraceMark[]
  src: RowSource
}

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

function loadSelection(): string[] {
  try {
    return parseSelection(localStorage.getItem(LS_SEL))
  } catch {
    return []
  }
}

function loadConfig(): { divider: string; flushMs: string } {
  try {
    const raw = JSON.parse(localStorage.getItem(LS_CFG) ?? '{}') as Record<string, unknown>
    return {
      divider: typeof raw.divider === 'string' ? raw.divider : '1',
      flushMs: typeof raw.flushMs === 'string' ? raw.flushMs : '100',
    }
  } catch {
    return { divider: '1', flushMs: '100' }
  }
}

function save(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value))
  } catch {
    /* 저장 못 해도 동작한다 */
  }
}

export default function TracePage() {
  return (
    <ErrorBoundary label="트레이스">
      <Trace />
    </ErrorBoundary>
  )
}

function Trace() {
  useStore(robots, traceLive)
  useEffect(() => robots.start(), [])
  useEffect(() => traceLive.start(), [])

  const [ov, setOv] = useState<{
    plc?: string
    link_ready: boolean
    current: TraceMeta | null
    sessions: TraceMeta[]
  } | null>(null)
  const [pollError, setPollError] = useState<string | null>(null)
  const [startError, setStartError] = useState<string | null>(null)

  const [selection, setSelection] = useState<string[]>(loadSelection)
  const [cfg, setCfg] = useState(loadConfig)
  const [label, setLabel] = useState('')
  const [note, setNote] = useState('')
  const [markText, setMarkText] = useState('')
  const [busy, setBusy] = useState(false)

  const [hidden, setHidden] = useState<ReadonlySet<string>>(new Set())
  const [right, setRight] = useState<ReadonlySet<string>>(new Set())
  const [replay, setReplay] = useState<Replay | null>(null)
  const [loadingId, setLoadingId] = useState<string | null>(null)
  const [deleteId, setDeleteId] = useState<string | null>(null)
  const [cursor, setCursor] = useState<number | null>(null)
  const [plotPx, setPlotPx] = useState(900)

  const load = useCallback(async () => {
    try {
      const r = await traceApi.overview()
      setOv(r)
      traceLive.adopt(r.current)
      setPollError(null)
    } catch (e) {
      setPollError(errText(e))
    }
  }, [])

  useEffect(() => {
    void load()
    const t = visibleInterval(() => void load(), POLL_MS)
    return () => clearInterval(t)
  }, [load])

  useEffect(() => save(LS_SEL, selection), [selection])
  useEffect(() => save(LS_CFG, cfg), [cfg])

  const current = ov?.current ?? null
  const running = current !== null
  const chanMax = CHAN_MAX

  // 차트가 무엇을 그리는가 — 재생 중이면 그 세션, 아니면 라이브 링.
  const source: RowSource = replay ? replay.src : traceLive.ring
  const channels: readonly TraceChannelMeta[] = replay
    ? replay.meta.channels
    : (current?.channels ?? [])
  const marks = replay ? replay.marks : traceLive.marks
  const version = replay ? replay.meta.rows : traceLive.version

  const specs = useMemo<SeriesSpec[]>(
    () =>
      channels
        .map((c, i) => ({
          path: c.path,
          name: shortPath(c.path),
          index: i,
          axis: right.has(c.path) ? ('R' as const) : ('L' as const),
        }))
        .filter((s) => !hidden.has(s.path)),
    [channels, hidden, right],
  )

  const lastRow = source.lastRow()
  const cursorRow = useMemo(() => {
    if (cursor === null) return null
    const i = source.indexAtTime(cursor)
    if (i < 0) return null
    const out = [source.time(i)]
    for (let c = 0; c < source.chan; c++) out.push(source.value(i, c))
    return out
    // `version` 은 링이 제자리에서 바뀌는 것을 알리는 신호다(의존성으로 쓴다).
  }, [cursor, source, version])

  const h = health(replay ? replay.meta : current, {
    rows: replay ? replay.meta.rows : traceLive.rows,
    chunks: replay ? replay.meta.chunks : traceLive.chunks,
    overrun: replay ? replay.meta.overrun : traceLive.overrun,
    lag: replay ? 0 : traceLive.lag,
  })

  // 트레이스는 서버가 정한 PLC 로만 간다(TRACE_LNK 가 있는 로봇) — 사이드바에서 GR1 을 골라도 GR2 레이아웃 채널을
  // GR1 로 보내지 않는다. 옛 서버(plc 없음)면 선택 로봇.
  const plc = ov?.plc ?? robots.current?.plc ?? ''
  const traceRobot = robots.list.find((r) => r.plc === plc)?.id ?? robots.selected
  const blocked = startDisabledReason({
    linkReady: ov?.link_ready ?? false,
    running,
    plc,
    channels: selection,
    divider: cfg.divider,
    flushMs: cfg.flushMs,
    chanMax,
    flushMin: FLUSH_MIN_MS,
  })

  async function start() {
    setBusy(true)
    setStartError(null)
    try {
      const r = await traceApi.start({
        plc,
        robot: traceRobot,
        channels: selection,
        divider: Number(cfg.divider),
        flush_ms: Number(cfg.flushMs),
        label: label.trim(),
        note: note.trim(),
      })
      traceLive.reset(r.current.id)
      setReplay(null)
      setCursor(null)
      toast.ok(`트레이스 시작: 채널 ${r.current.channels.length} · 분주 ${r.current.divider}`)
      await load()
    } catch (e) {
      // 백엔드 문장을 **그대로** 낸다 — 110·111 은 PLC 에서 무엇을 고쳐야 하는지 이름으로 말한다.
      setStartError(errText(e))
    } finally {
      setBusy(false)
    }
  }

  async function stop() {
    setBusy(true)
    try {
      const r = await traceApi.stop()
      toast.ok(`트레이스 정지: 행 ${r.session.rows}`)
      await load()
      void open(r.session)
    } catch (e) {
      toast.error(`정지 실패: ${errText(e)}`)
    } finally {
      setBusy(false)
    }
  }

  async function mark(text: string) {
    const t = text.trim()
    if (!t) return
    try {
      await traceApi.mark(t)
      setMarkText('')
      toast.ok(`마크: ${t}`)
    } catch (e) {
      toast.error(`마크 실패: ${errText(e)}`)
    }
  }

  const open = useCallback(
    async (m: TraceMeta) => {
      setLoadingId(m.id)
      try {
        // 재생 행 수는 **차트 폭**이 정한다 — 화면보다 촘촘한 행은 그려도 보이지 않는다.
        const s = await traceApi.session(m.id, budgetFor(plotPx))
        setReplay({
          meta: s.meta,
          marks: s.marks ?? [],
          src: new RowArray(stripCycle(s.rows), s.meta.channels.length),
        })
        setCursor(null)
        setHidden(new Set())
      } catch (e) {
        toast.error(`세션 열기 실패: ${errText(e)}`)
      } finally {
        setLoadingId(null)
      }
    },
    [plotPx],
  )

  async function remove(id: string) {
    try {
      await traceApi.remove(id)
      if (replay?.meta.id === id) setReplay(null)
      await load()
      toast.ok('세션을 지웠습니다')
    } catch (e) {
      toast.error(`삭제 실패: ${errText(e)}`)
    }
  }

  function toggle(path: string) {
    const next = new Set(hidden)
    if (next.has(path)) next.delete(path)
    else next.add(path)
    setHidden(next)
  }

  function toggleAxis(path: string) {
    const next = new Set(right)
    if (next.has(path)) next.delete(path)
    else next.add(path)
    setRight(next)
  }

  const [t0, t1] = source.bounds()
  const deleted = deleteId === null ? null : (ov?.sessions.find((s) => s.id === deleteId) ?? null)

  /** 손실 집계 — 값이 알람인 자리만 색을 받는다(Rows 는 화면 머리띠가 말한다). */
  const lossStats: StatItem[] = [
    { label: 'Chunks', value: h.chunks },
    {
      label: 'Overrun',
      value: h.overrun,
      tone: h.overrun > 0 ? 'fault' : 'neutral',
      title: 'PLC 가 버린 표본 수',
    },
    {
      label: 'Gaps',
      value: h.gaps,
      tone: h.gaps > 0 ? 'warn' : 'neutral',
      title: '청크 사이가 끊긴 횟수',
    },
    {
      label: 'SSE lag',
      value: h.lag,
      tone: h.lag > 0 ? 'warn' : 'neutral',
      title: '브라우저가 건너뛴 이벤트 수',
    },
  ]

  /** 채널별 현재·커서 값 — 읽는 표. 색 막대는 파형 범례와 같은 채널 색이다. */
  const valueColumns: Column<TraceChannelMeta & { index: number }>[] = [
    {
      key: 'path',
      label: 'Channel',
      get: (c) => c.path,
      cell: (c) => (
        <span className="font-mono">
          <span
            className="mr-1 inline-block h-0.5 w-3 align-middle"
            style={{ background: traceColor(c.index) }}
            aria-hidden="true"
          />
          {c.path}
        </span>
      ),
      priority: 1,
    },
    { key: 'type', label: 'Type', get: (c) => c.type_name, priority: 3 },
    {
      key: 'now',
      label: replay ? '마지막' : '현재',
      get: (c) => (lastRow ? lastRow[c.index + 1] : null),
      cell: (c) => (lastRow ? fmtValue(c.kind, lastRow[c.index + 1]) : '-'),
      numeric: true,
      priority: 1,
    },
    {
      key: 'cursor',
      label: '커서',
      get: (c) => (cursorRow ? cursorRow[c.index + 1] : null),
      cell: (c) => (cursorRow ? fmtValue(c.kind, cursorRow[c.index + 1]) : '-'),
      numeric: true,
      priority: 2,
    },
  ]

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="screen-trace">
      <ScreenHeader
        title="트레이스"
        icon={<Waves className="h-4 w-4" />}
        items={[
          { label: '링크', value: ov?.link_ready ? '준비' : '없음' },
          { label: '세션', value: current ? current.label || current.id : '정지' },
          { label: 'Rows', value: String(h.rows) },
          { label: 'SSE', value: traceLive.connected ? '연결' : traceLive.error ? '오류' : '대기' },
        ]}
      />

      <div className="min-h-0 flex-1 space-y-3 overflow-auto p-3">
        {pollError ? <div className="text-xs text-fault-fg">{pollError}</div> : null}

        <div className="grid gap-3 xl:grid-cols-2">
          <ErrorBoundary label="트레이스 설정">
            <Card>
              <h3 className="mb-2 text-xs font-semibold text-content-muted">
                {running ? '기록 중' : '새 트레이스'}
              </h3>
              {running && current ? (
                <div className="space-y-2">
                  <FieldList items={runningInfo(current)} columns={2} dense />
                  <div className="flex items-end gap-2">
                    <Input
                      label="마크 (Enter)"
                      value={markText}
                      onValueChange={setMarkText}
                      className="min-w-0 flex-1"
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') void mark(markText)
                      }}
                    />
                    <Button
                      size="sm"
                      disabled={!markText.trim()}
                      onClick={() => void mark(markText)}
                    >
                      마크
                    </Button>
                    <Button size="sm" intent="danger" loading={busy} onClick={() => void stop()}>
                      정지
                    </Button>
                  </div>
                </div>
              ) : (
                <div className="space-y-2">
                  <ChannelPicker
                    selected={selection}
                    onChange={setSelection}
                    chanMax={chanMax}
                    disabled={busy}
                  />
                  <div className="grid gap-2 sm:grid-cols-2">
                    <Input
                      label="Divider"
                      value={cfg.divider}
                      mono
                      onValueChange={(v) => setCfg({ ...cfg, divider: v })}
                      hint="1 = 매 스캔"
                      title="표본 분주 — 2 면 한 스캔 걸러 하나를 뜹니다"
                    />
                    <Input
                      label="Flush (ms)"
                      value={cfg.flushMs}
                      mono
                      onValueChange={(v) => setCfg({ ...cfg, flushMs: v })}
                      hint={`≥ ${FLUSH_MIN_MS}`}
                      title="PLC 가 청크를 올려 보내는 주기"
                    />
                    <Input
                      label="Label"
                      value={label}
                      onValueChange={setLabel}
                      placeholder="예: 400 단계 멈춤 재현"
                    />
                    <Input
                      label="Note"
                      value={note}
                      onValueChange={setNote}
                      placeholder="조건 · 품목 · 파라미터"
                    />
                  </div>
                  <dl className="flex flex-wrap items-center gap-x-4 text-2xs">
                    <div className="flex items-center gap-1">
                      <dt className="text-content-faint">대상</dt>
                      <dd className="font-mono text-content-secondary">
                        {robots.current ? `${robots.current.name} · ${plc}` : '로봇 미선택'}
                      </dd>
                    </div>
                    <div className="flex items-center gap-1">
                      <dt className="text-content-faint">브라우저 링 (s)</dt>
                      <dd className="font-mono tabular-nums text-content-secondary">
                        {RING_MS / 1000}
                      </dd>
                      <HelpTip
                        title="브라우저 링"
                        text={`라이브 차트는 최근 ${RING_MS / 1000}초만 들고 있습니다. 그 앞 구간은 정지한 뒤 세션 재생으로 봅니다(세션 파일에는 다 남습니다).`}
                      />
                    </div>
                  </dl>
                  {blocked ? <p className="text-2xs text-warn-fg">{blocked}</p> : null}
                  {startError ? (
                    <p
                      className="rounded border border-fault bg-fault-soft p-2 font-mono text-2xs break-all text-fault-fg"
                      data-testid="trace-start-error"
                    >
                      {startError}
                    </p>
                  ) : null}
                  <div className="flex justify-end">
                    <Button
                      size="sm"
                      intent="primary"
                      loading={busy}
                      disabled={blocked !== null}
                      title={blocked ?? undefined}
                      onClick={() => void start()}
                      data-testid="trace-start"
                    >
                      시작
                    </Button>
                  </div>
                </div>
              )}
            </Card>
          </ErrorBoundary>

          <ErrorBoundary label="세션 목록">
            <Card>
              <h3 className="mb-1 text-xs font-semibold text-content-muted">
                저장된 세션 ({ov?.sessions.length ?? 0})
              </h3>
              <SessionList
                sessions={ov?.sessions ?? []}
                openId={replay?.meta.id ?? ''}
                loadingId={loadingId}
                onOpen={(m) => void open(m)}
                onDelete={setDeleteId}
              />
            </Card>
          </ErrorBoundary>
        </div>

        <ErrorBoundary label="파형">
          <Card>
            <div className="mb-2 flex flex-wrap items-center gap-2">
              <h3 className="text-xs font-semibold text-content-muted">
                {replay ? `재생 — ${replay.meta.label || replay.meta.id}` : '라이브'}
              </h3>
              <dl className="flex flex-wrap items-center gap-x-3 text-2xs">
                <div className="flex items-center gap-1">
                  <dt className="text-content-faint">Rows</dt>
                  <dd className="font-mono tabular-nums text-content-secondary">{source.length}</dd>
                </div>
                <div className="flex items-center gap-1">
                  <dt className="text-content-faint">구간</dt>
                  <dd className="font-mono tabular-nums text-content-secondary">
                    {fmtT(t0)} ~ {fmtT(t1)}
                  </dd>
                </div>
                {cursor !== null && (
                  <div className="flex items-center gap-1">
                    <dt className="text-content-faint">커서</dt>
                    <dd className="font-mono tabular-nums text-content-secondary">
                      {fmtT(cursor)}
                    </dd>
                  </div>
                )}
              </dl>
              {replay ? (
                <Button size="sm" intent="ghost" onClick={() => setReplay(null)}>
                  라이브로
                </Button>
              ) : null}
            </div>

            <TraceChart
              source={source}
              specs={specs}
              marks={marks.map((m) => ({ t_ms: m.t_ms, text: m.text }))}
              version={version}
              height={CHART_H}
              empty={
                running ? '첫 청크를 기다리는 중…' : '세션을 시작하거나 저장된 세션을 재생하세요'
              }
              onCursor={setCursor}
              onWidth={setPlotPx}
            />

            {channels.length > 0 ? (
              <ul className="mt-2 m-0 flex list-none flex-wrap gap-x-3 gap-y-1 p-0">
                {channels.map((c, i) => {
                  const off = hidden.has(c.path)
                  return (
                    <li key={c.path} className="flex items-center gap-1">
                      <button
                        type="button"
                        className={`flex items-center gap-1 text-2xs ${off ? 'text-content-disabled' : 'text-content-secondary'}`}
                        onClick={() => toggle(c.path)}
                        title={`${c.path} — 누르면 ${off ? '보이기' : '숨기기'}`}
                      >
                        <span
                          className="inline-block h-0.5 w-4"
                          style={{ background: off ? 'currentColor' : traceColor(i) }}
                          aria-hidden="true"
                        />
                        <span className="font-mono">{shortPath(c.path)}</span>
                      </button>
                      <button
                        type="button"
                        className={`rounded border border-line-default px-1 font-mono text-3xs ${
                          right.has(c.path) ? 'text-accent-text' : 'text-content-faint'
                        }`}
                        onClick={() => toggleAxis(c.path)}
                        title="왼쪽 / 오른쪽 축"
                      >
                        {right.has(c.path) ? 'R' : 'L'}
                      </button>
                    </li>
                  )
                })}
              </ul>
            ) : null}
          </Card>
        </ErrorBoundary>

        <div className="grid gap-3 lg:grid-cols-2">
          <ErrorBoundary label="현재 값">
            <Card>
              <h3 className="mb-1 text-xs font-semibold text-content-muted">
                현재 값 {cursor === null ? '' : `· 커서 ${fmtT(cursor)}`}
              </h3>
              <DataTable
                rows={channels.map((c, i) => ({ ...c, index: i }))}
                columns={valueColumns}
                rowKey={(c) => c.path}
                density="compact"
                stickyHeader
                className="max-h-80 overflow-y-auto"
                empty="채널 없음"
                emptyHint={
                  running ? '기록 중인 세션의 채널을 기다립니다' : '새 트레이스에서 채널을 고르세요'
                }
                testid="trace-values"
              />
            </Card>
          </ErrorBoundary>

          <ErrorBoundary label="상태">
            <Card>
              <div className="mb-1 flex items-center gap-1">
                <h3 className="text-xs font-semibold text-content-muted">손실</h3>
                <HelpTip
                  title="손실"
                  text="Overrun 이 0 이 아니면 PLC 가 표본을 버린 것이라 더는 무손실이 아닙니다 — 분주를 키우거나 채널을 줄이세요. Gaps 는 청크 사이가 끊긴 횟수, SSE lag 는 브라우저가 건너뛴 이벤트 수입니다(그 구간도 세션 파일에는 남습니다)."
                />
              </div>
              {/* Rows · Overrun 은 화면 머리띠가 이미 말한다 — 여기서는 되풀이하지 않는다. */}
              <StatRow items={lossStats} bordered={false} className="px-0" testid="trace-health" />
              {(replay ? replay.meta.errors : (current?.errors ?? [])).length > 0 ? (
                <ul className="mt-2 m-0 list-none space-y-0.5 p-0">
                  {(replay ? replay.meta.errors : (current?.errors ?? [])).map((e, i) => (
                    <li key={i} className="font-mono text-2xs break-all text-fault-fg">
                      {e}
                    </li>
                  ))}
                </ul>
              ) : null}
              {traceLive.error && !replay ? (
                <p className="mt-1 text-2xs text-warn-fg">{traceLive.error}</p>
              ) : null}
            </Card>
          </ErrorBoundary>
        </div>
      </div>

      <ConfirmDialog
        open={deleteId !== null}
        onOpenChange={(o) => !o && setDeleteId(null)}
        scope="single"
        title="트레이스 세션 삭제"
        danger
        confirmLabel="삭제"
        onConfirm={() => {
          const id = deleteId
          setDeleteId(null)
          if (id) void remove(id)
        }}
      >
        <p>콘솔 PC 의 세션 파일을 지울까요?</p>
        <dl className="mt-2 flex flex-wrap gap-x-4 text-xs">
          {(
            [
              ['Label', deleted ? deleted.label || deleted.id : '—'],
              ['StartedAt', deleted ? clock(deleted.started_at) : '—'],
              ['Rows', deleted ? String(deleted.rows) : '—'],
            ] as [string, string][]
          ).map(([k, v]) => (
            <div key={k}>
              <dt className="text-3xs text-content-faint">{k}</dt>
              <dd className="font-mono tabular-nums">{v}</dd>
            </div>
          ))}
        </dl>
      </ConfirmDialog>
    </div>
  )
}

/** 기록 중인 세션의 값 — 단위는 라벨에 붙는다(`Flush (ms)`), 값은 숫자만 남는다. */
function runningInfo(m: TraceMeta): FieldItem[] {
  return [
    { label: 'Label', value: m.label || m.id, mono: true },
    { label: 'Plc', value: m.plc, mono: true },
    { label: 'Divider', value: m.divider, tooltip: '1 = 매 스캔' },
    { label: 'Flush (ms)', value: m.flush_ms, tooltip: 'PLC 가 청크를 올려 보내는 주기' },
    { label: 'StartedAt', value: clock(m.started_at) },
    { label: 'Elapsed (s)', value: durationSec(m) },
    { label: 'Channels', value: m.channels.length },
  ]
}
