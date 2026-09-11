// 측정 모니터 — GrWeb(`tools/GrWeb/index.html`) 이식. 데이터는 `statusFeed`(WEBMON SSE)와 `measlog`
// 스토어(MEASLOG 스냅샷 + 이력)에서 온다. 하위 탭: 대시보드/Task/축·센서/측정진행/이력/추세/규격별/통계/원본.
import { Activity } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { statusFeed } from '../../lib/feeds'
import { KIND } from '../../lib/gr/const'
import { csvFileName, downloadCsv, toCsv } from '../../lib/meas/csv'
import { dtl } from '../../lib/meas/format'
import { codesOf, filt, flatten } from '../../lib/meas/rows'
import { measlog } from '../../lib/measlog'
import { nav } from '../../lib/nav'
import { useSse } from '../../lib/sse'
import { useStore } from '../../lib/store'
import { Button } from '../../lib/ui/Button'
import { EmptyState } from '../../lib/ui/EmptyState'
import { JsonView } from '../../lib/ui/JsonView'
import { ScreenHeader } from '../../lib/ui/ScreenHeader'
import { Select } from '../../lib/ui/Select'
import { Axes } from './Axes'
import { ByCode } from './ByCode'
import { Dashboard } from './Dashboard'
import { History } from './History'
import { MeasureProgress } from './MeasureProgress'
import { Stats } from './Stats'
import { TaskNow } from './TaskNow'
import { Trend } from './Trend'

const SUB = [
  ['dash', '대시보드'],
  ['task', 'Task'],
  ['axis', '축 / 센서'],
  ['meas', '측정 진행'],
  ['hist', '이력'],
  ['trend', '추세'],
  ['code', '규격별'],
  ['stat', '통계'],
  ['raw', '원본'],
] as const
type Sub = (typeof SUB)[number][0]
const SUB_KEY = 'gr-measure-sub'

export default function MeasureMonitor() {
  useSse(statusFeed)
  useStore(measlog)
  useEffect(() => measlog.start(), [])
  const [sub, setSub] = useState<Sub>(() => {
    try {
      return (localStorage.getItem(SUB_KEY) as Sub) || 'dash'
    } catch {
      return 'dash'
    }
  })
  const [kind, setKind] = useState('')
  const [code, setCode] = useState('')
  const [selected, setSelected] = useState<number | null>(null)
  const [rawSel, setRawSel] = useState('webmon')

  const wm = statusFeed.data?.webmon ?? null
  const snap = measlog.snapshot
  const rows = useMemo(() => flatten(measlog.entries), [measlog.entries])
  const filtered = useMemo(() => filt(rows, kind, code), [rows, kind, code])
  const codes = useMemo(() => codesOf(snap?.by_code, rows), [snap, rows])

  // 다른 화면에서 온 원샷 신호(추세 점 클릭 등).
  useEffect(() => {
    const seq = nav.consumeMeasSeq()
    if (seq !== null) {
      setSelected(seq)
      go('hist')
    }
    const c = nav.consumeMeasCode()
    if (c !== null) setCode(String(c))
  })

  function go(s: Sub) {
    setSub(s)
    try {
      localStorage.setItem(SUB_KEY, s)
    } catch {
      /* 저장 못 해도 동작 */
    }
  }

  const header = (
    <ScreenHeader
      title="측정 모니터"
      icon={<Activity className="h-4 w-4" />}
      items={[
        { label: 'PLC', value: dtl(wm?.UpdateTime) || '-' },
        { label: 'MeasLog', value: String(wm?.MeasLog?.Total ?? snap?.total ?? '-') },
        { label: 'SSE', value: statusFeed.connected ? '연결' : statusFeed.error ? '오류' : '대기' },
      ]}
    />
  )

  const showFilters = sub === 'hist' || sub === 'trend'
  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="screen-measure">
      {header}
      <div className="flex flex-wrap items-center gap-1 border-b border-line-default px-3 py-1.5">
        {SUB.map(([id, label]) => (
          <Button key={id} size="sm" intent={sub === id ? 'primary' : 'ghost'} active={sub === id} onClick={() => go(id)} data-testid={`measure-sub-${id}`}>
            {label}
          </Button>
        ))}
        {showFilters ? (
          <div className="ml-auto flex items-center gap-2">
            <Select dense value={kind} onValueChange={setKind} aria-label="종류">
              <option value="">전체 종류</option>
              {KIND.slice(1).map((k, i) => (
                <option key={k} value={i + 1}>
                  {k}
                </option>
              ))}
            </Select>
            <Select dense value={code} onValueChange={setCode} aria-label="Code">
              <option value="">전체 Code</option>
              {codes.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </Select>
            <Button size="sm" onClick={() => downloadCsv(csvFileName(), toCsv(filtered))} disabled={filtered.length === 0}>
              CSV
            </Button>
            <Button size="sm" loading={measlog.loading} onClick={() => void measlog.reload(true)}>
              이력 다시 읽기
            </Button>
          </div>
        ) : null}
      </div>
      <div className="min-h-0 flex-1 overflow-auto p-3">
        {!wm && (sub === 'dash' || sub === 'task' || sub === 'axis' || sub === 'meas') ? (
          <EmptyState title="PLC 상태 없음" hint={statusFeed.error ?? '상태 스트림(/api/status/stream)을 기다리는 중입니다.'} />
        ) : null}
        {wm && sub === 'dash' ? <Dashboard wm={wm} /> : null}
        {wm && sub === 'task' ? <TaskNow wm={wm} /> : null}
        {wm && sub === 'axis' ? <Axes wm={wm} axisHist={measlog.axisHist} /> : null}
        {wm && sub === 'meas' ? <MeasureProgress wm={wm} snap={snap} /> : null}
        {sub === 'hist' ? <History rows={filtered} snap={snap} selected={selected} onSelect={setSelected} allCount={rows.length} /> : null}
        {sub === 'trend' ? (
          <Trend
            rows={filtered}
            kind={kind}
            onPick={(seq) => {
              setSelected(seq)
              go('hist')
            }}
          />
        ) : null}
        {sub === 'code' ? (
          <ByCode
            snap={snap}
            onPick={(c) => {
              setCode(String(c))
              go('hist')
            }}
          />
        ) : null}
        {sub === 'stat' ? <Stats snap={snap} /> : null}
        {sub === 'raw' ? (
          <div>
            <div className="mb-2">
              <Select dense label="보기" value={rawSel} onValueChange={setRawSel}>
                <option value="webmon">WEBMON</option>
                <option value="header">MEASLOG header</option>
                <option value="last">MEASLOG.Last</option>
                <option value="stat">MEASLOG.Stat</option>
                <option value="by_code">MEASLOG.ByCode</option>
                <option value="sel">선택한 기록</option>
              </Select>
            </div>
            <JsonView
              value={
                rawSel === 'webmon'
                  ? wm
                  : rawSel === 'header'
                    ? snap && { head: snap.head, count: snap.count, total: snap.total, capacity: snap.capacity, at: snap.at }
                    : rawSel === 'sel'
                      ? (rows.find((r) => r.seq === selected) ?? null)
                      : (snap as unknown as Record<string, unknown> | null)?.[rawSel]
              }
            />
          </div>
        ) : null}
        {measlog.error && (sub === 'hist' || sub === 'trend' || sub === 'code' || sub === 'stat') ? <div className="mt-2 text-xs text-red-600">{measlog.error}</div> : null}
      </div>
    </div>
  )
}
