// 측정 모니터 — 머리띠 하나 · 숫자 띠 하나 · 하위 탭 일곱.
//
// 이 화면은 하위 탭이 **열둘**이었고(대시보드·Task·축/센서·측정 진행·레이저·PARA·기록·분석·이력·
// 추세·규격별·통계·원본) 탭마다 제 통계 카드 격자를 따로 그렸다. 같은 값(모드·상태·Task·측정 건수)이
// 탭마다 다른 자리·다른 모양으로 서 있어서, 탭을 옮길 때마다 눈이 처음부터 다시 찾았다.
//
// 지금 구조:
//  - **머리띠**(`ScreenHeader`) — 로봇·PLC·갱신 시각 + SSE 점 + 넘침 메뉴. 한 줄뿐이다.
//  - **숫자 띠**(`StatRow`) — 모드·상태·Task·Step·측정·알람·Z/G·기록 수. **모든 하위 탭에서 같은
//    자리에** 선다. 카드 격자 열여섯 개가 여기로 접혔다.
//  - **하위 탭 일곱** — 현황(대시보드+측정 진행) · Task · 축/센서 · 레이저 · 기록(이력+추세+집계) ·
//    분석 · PARA. 원본 JSON 은 넘침 메뉴의 대화상자로 옮겼다(하루에 한 번 쓸까 한 화면이 상시로
//    탭 한 칸을 먹고 있었다).
//
// 데이터는 **사이드바에서 고른 로봇**의 상태 피드(WEBMON SSE)와 `measlog` 스토어에서 온다.
import { Activity } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useSelectedStatus } from '../../lib/feeds'
import { robots } from '../../lib/robots'
import { KIND } from '../../lib/gr/const'
import { csvFileName, downloadCsv, toCsv } from '../../lib/meas/csv'
import { dtl } from '../../lib/meas/format'
import { codesOf, filt, flatten } from '../../lib/meas/rows'
import { measlog } from '../../lib/measlog'
import { nav } from '../../lib/nav'
import { useStore } from '../../lib/store'
import { Dialog } from '../../lib/ui/Dialog'
import { EmptyState } from '../../lib/ui/EmptyState'
import { Field } from '../../lib/ui/Field'
import { JsonView } from '../../lib/ui/JsonView'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import { ScreenHeader } from '../../lib/ui/ScreenHeader'
import { Segmented } from '../../lib/ui/Segmented'
import { Select } from '../../lib/ui/Select'
import { StatRow } from '../../lib/ui/StatRow'
import { StatusDot } from '../../lib/ui/StatusDot'
import type { MenuItem } from '../../lib/ui/menu'
import { Axes } from './Axes'
import { LaserSensor } from './LaserSensor'
import { Live } from './Live'
import { bandItems } from './MeasureMonitorModel'
import { ParaView } from './ParaView'
import { Recorder } from './Recorder'
import { Records } from './Records'
import { TaskNow } from './TaskNow'

const SUB = [
  ['live', '현황'],
  ['task', 'Task'],
  ['axis', '축 / 센서'],
  ['laser', '레이저'],
  ['hist', '기록'],
  ['rec', '분석'],
  ['para', 'PARA'],
] as const
type Sub = (typeof SUB)[number][0]
const SUB_KEY = 'gr-measure-sub'

/** 옛 탭 id → 지금 탭. 기억해 둔 마지막 탭이 사라진 이름이면 그 내용이 **들어간 자리**로 보낸다. */
const MOVED: Record<string, Sub> = {
  dash: 'live',
  meas: 'live',
  raw: 'live',
  trend: 'hist',
  code: 'hist',
  stat: 'hist',
}

function loadSub(): Sub {
  try {
    const v = localStorage.getItem(SUB_KEY) ?? ''
    if (SUB.some(([id]) => id === v)) return v as Sub
    return MOVED[v] ?? 'live'
  } catch {
    return 'live'
  }
}

/**
 * 필터(Kind·Code)도 **기억한다** — 이 화면은 도킹 존의 탭이라 다른 패널을 보면 통째로 unmount 되고,
 * 그때마다 걸어 둔 필터가 사라졌다("전체"로 돌아온 표를 보고 기록이 사라진 줄 안다). 마지막 탭을
 * 기억하는 `gr-measure-sub` 와 같은 관용구다.
 */
const FILTER_KEY = 'gr-measure-filter'

function loadFilter(): { kind: string; code: string } {
  try {
    const raw = localStorage.getItem(FILTER_KEY)
    if (!raw) return { kind: '', code: '' }
    const v = JSON.parse(raw) as { kind?: unknown; code?: unknown }
    return {
      kind: typeof v.kind === 'string' ? v.kind : '',
      code: typeof v.code === 'string' ? v.code : '',
    }
  } catch {
    return { kind: '', code: '' }
  }
}

function saveFilter(kind: string, code: string): void {
  try {
    localStorage.setItem(FILTER_KEY, JSON.stringify({ kind, code }))
  } catch {
    /* 저장 못 해도 동작 */
  }
}

/** PLC 상태가 있어야 뜻이 있는 탭 — 없으면 왜 없는지 말한다. */
const NEEDS_WEBMON: readonly Sub[] = ['live', 'task', 'axis']

export default function MeasureMonitor() {
  const statusFeed = useSelectedStatus()
  useStore(measlog)
  useEffect(() => measlog.start(), [])
  const [sub, setSub] = useState<Sub>(loadSub)
  const [kind, setKindState] = useState(() => loadFilter().kind)
  const [code, setCodeState] = useState(() => loadFilter().code)
  const [selected, setSelected] = useState<number | null>(null)
  const [raw, setRaw] = useState<string | null>(null)

  const wm = statusFeed.data?.webmon ?? null
  const snap = measlog.snapshot
  const rows = useMemo(() => flatten(measlog.entries), [measlog.entries])
  const filtered = useMemo(() => filt(rows, kind, code), [rows, kind, code])
  const codes = useMemo(() => codesOf(snap?.by_code, rows), [snap, rows])
  const hasFilter = kind !== '' || code !== ''

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

  function setKind(v: string) {
    setKindState(v)
    saveFilter(v, code)
  }
  function setCode(v: string) {
    setCodeState(v)
    saveFilter(kind, v)
  }

  function go(s: Sub) {
    setSub(s)
    try {
      localStorage.setItem(SUB_KEY, s)
    } catch {
      /* 저장 못 해도 동작 */
    }
  }

  // 항목은 **열 때** 만든다 — 모듈 상수로 들면 비활성 사유가 옛 상태에 굳는다.
  const menu: MenuItem[] = [
    {
      label: '측정 기록 CSV 내려받기',
      disabled: filtered.length === 0 ? '내려받을 기록이 없습니다' : undefined,
      run: () => downloadCsv(csvFileName(), toCsv(filtered)),
    },
    {
      label: '이력 다시 읽기',
      disabled: measlog.loading ? '읽는 중입니다' : undefined,
      run: () => void measlog.reload(true),
    },
    {
      label: '필터 지우기',
      disabled: hasFilter ? undefined : '걸린 필터가 없습니다',
      run: () => {
        // 둘을 한 번에 — `setKind('')` → `setCode('')` 로 부르면 두 번째가 **한 렌더 전의** kind 를
        // 들고 저장해 지운 필터가 되살아난다.
        setKindState('')
        setCodeState('')
        saveFilter('', '')
      },
    },
    { label: '원본 JSON…', run: () => setRaw('webmon') },
  ]

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="screen-measure">
      <ScreenHeader
        title="측정 모니터"
        icon={<Activity className="h-4 w-4" />}
        items={[
          {
            label: '로봇',
            value: `${robots.current?.name ?? '기본'}${statusFeed.data?.plc ? ` · ${statusFeed.data.plc}` : ''}`,
          },
          { label: '갱신', value: dtl(wm?.UpdateTime) || '-' },
        ]}
        trailing={
          <span className="flex items-center gap-1">
            <StatusDot
              status={statusFeed.connected ? 'ok' : statusFeed.error ? 'fault' : 'neutral'}
              size="sm"
              label="SSE"
              title={statusFeed.error ?? '상태 스트림'}
            />
            <OverflowMenu items={menu} testid="measure-more" />
          </span>
        }
      />

      {/* 숫자 띠 — 하위 탭과 무관하게 같은 자리에 선다(탭을 옮겨도 눈이 다시 찾지 않는다). */}
      <StatRow items={bandItems(wm, snap)} testid="measure-band" className="flex-none" />

      <div className="flex flex-none flex-wrap items-center gap-2 border-b border-line-default px-3 py-1.5">
        <Segmented
          value={sub}
          onChange={go}
          options={SUB.map(([id, label]) => ({
            id,
            label,
            testid: `measure-sub-${id}`,
          }))}
          ariaLabel="측정 하위 화면"
        />
        {sub === 'hist' ? (
          // 필터는 **기록 탭의 주 조작**이라 접지 않는다(한 번에 닿아야 한다). 나머지 조작은 넘침 메뉴에.
          <span className="ml-auto flex items-center gap-2">
            <Field inline label="Kind" className="w-auto">
              <Select dense value={kind} onValueChange={setKind} aria-label="Kind 필터">
                <option value="">전체</option>
                {KIND.slice(1).map((k, i) => (
                  <option key={k} value={i + 1}>
                    {k}
                  </option>
                ))}
              </Select>
            </Field>
            <Field inline label="Code" className="w-auto">
              <Select dense value={code} onValueChange={setCode} aria-label="Code 필터">
                <option value="">전체</option>
                {codes.map((c) => (
                  <option key={c} value={c}>
                    {c}
                  </option>
                ))}
              </Select>
            </Field>
          </span>
        ) : null}
      </div>

      <div className="min-h-0 flex-1 overflow-auto p-3">
        {!wm && NEEDS_WEBMON.includes(sub) ? (
          <EmptyState
            title="PLC 상태 없음"
            hint={statusFeed.error ?? '상태 스트림(/api/status/stream)을 기다리는 중입니다.'}
          />
        ) : null}
        {wm && sub === 'live' ? <Live wm={wm} /> : null}
        {wm && sub === 'task' ? <TaskNow wm={wm} /> : null}
        {wm && sub === 'axis' ? <Axes wm={wm} axisHist={measlog.axisHist} /> : null}
        {/* 로봇이 바뀌면 탭 안의 확인창·교정 상태를 새로 시작한다(다른 호기 PLC 에 대한 질문이 남지 않게). */}
        {sub === 'laser' ? <LaserSensor key={robots.selected ?? 'default'} /> : null}
        {sub === 'hist' ? (
          <Records
            rows={filtered}
            allCount={rows.length}
            snap={snap}
            kind={kind}
            selected={selected}
            onSelect={setSelected}
            onPickCode={(c) => setCode(String(c))}
            filtered={hasFilter}
          />
        ) : null}
        {sub === 'rec' ? <Recorder /> : null}
        {sub === 'para' ? <ParaView /> : null}
        {measlog.error && sub === 'hist' ? (
          <div className="mt-2 text-xs text-fault-fg">{measlog.error}</div>
        ) : null}
      </div>

      {/* 원본 JSON — 탭 하나를 쓰던 진단 화면. 대화상자로 옮겨도 닿는 걸음 수는 같다(메뉴 → 항목). */}
      <Dialog
        open={raw !== null}
        onOpenChange={(o) => !o && setRaw(null)}
        title="원본 JSON"
        size="xl"
        testid="measure-raw"
        meta={
          <Field inline label="보기" className="w-auto">
            <Select
              dense
              value={raw ?? 'webmon'}
              onValueChange={setRaw}
              aria-label="원본 JSON 보기"
            >
              <option value="webmon">WEBMON</option>
              <option value="header">MEASLOG header</option>
              <option value="last">MEASLOG.Last</option>
              <option value="stat">MEASLOG.Stat</option>
              <option value="by_code">MEASLOG.ByCode</option>
              <option value="sel">선택한 기록</option>
            </Select>
          </Field>
        }
      >
        <JsonView
          value={
            raw === 'webmon'
              ? wm
              : raw === 'header'
                ? snap && {
                    head: snap.head,
                    count: snap.count,
                    total: snap.total,
                    capacity: snap.capacity,
                    at: snap.at,
                  }
                : raw === 'sel'
                  ? (rows.find((r) => r.seq === selected) ?? null)
                  : (snap as unknown as Record<string, unknown> | null)?.[raw ?? '']
          }
        />
      </Dialog>
    </div>
  )
}
