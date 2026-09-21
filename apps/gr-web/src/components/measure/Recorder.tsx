// 기록·분석 탭 — 현장 측정 시험(레이저 측정 / Z 오프셋 교정 / 토크 내경 / 그리퍼 파지)의 고속 기록과 분석.
// 기록 : 백엔드(/api/record)가 WEBMON 의 축·그리퍼·측정 영역을 PLC 에서 직접 읽어(기본 50 ms) 콘솔 PC 의 data/records/<id> 에 남긴다.
//        브라우저를 닫아도 기록은 이어지고, 시작·끝에 PARA·LASERDIAG·MEASLOG 스냅샷을 함께 남긴다.
// 분석 : 기록 하나를 불러와 lib/record/analysis 의 순수 함수로 레이저 프로파일, 비드 판정 재현, 그리퍼 상태 변화, 멈춤 구간을 본다.
//
// 자리 정리(2026-09-18): **새 기록 설정 폼**(Label·Kind·Note·RateMs + 안내문)이 기록하지 않는 동안에도
// 카드 하나를 상시로 먹고 있었다 — 하루에 몇 번 누르는 일이다. 폼은 대화상자로 옮기고(`기록 시작…`
// 한 번 → 열리자마자 Label 에 포커스 → Enter 로 시작), 띠에는 **지금 누를 수 있는 것 하나**만 남겼다.
// 기록 중에는 같은 자리가 진행 값 띠 + 표시 입력으로 바뀐다.
import { Fragment, useCallback, useEffect, useMemo, useState } from 'react'
import { api } from '../../lib/api'
import { alarmLabel } from '../../lib/gr/alarms'
import { KIND, STATUS } from '../../lib/gr/const'
import { LASER_DIRS, LASER_SOURCE, ZCAL_ERROR, diagFlagLabels } from '../../lib/laser'
import { delta, pos } from '../../lib/meas/format'
import { visibleInterval } from '../../lib/poll'
import { robots } from '../../lib/robots'
import { useStore } from '../../lib/store'
import {
  LASER_FAR,
  alarmCodes,
  bitEdges,
  dirCount,
  profile,
  replay,
  segments,
  stallEpisodes,
  taskSpec,
  timeSeries,
  type BeadReplay,
  type BitEdge,
  type RecSample,
  type StallEpisode,
} from '../../lib/record/analysis'
import type {
  GripperState,
  LaserDiagEntry,
  LaserZCal,
  RecordMeta,
  RecordOverview,
  RecordSession,
  RecordSnapshot,
} from '../../lib/types'
import { Button } from '../../lib/ui/Button'
import { Card } from '../../lib/ui/Card'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { EmptyState } from '../../lib/ui/EmptyState'
import { Field } from '../../lib/ui/Field'
import { FieldList } from '../../lib/ui/FieldList'
import { FormDialog } from '../../lib/ui/Dialog'
import { HelpTip } from '../../lib/ui/HelpTip'
import { Input } from '../../lib/ui/Input'
import { Select } from '../../lib/ui/Select'
import { StatRow } from '../../lib/ui/StatRow'
import { Toolbar } from '../../lib/ui/Toolbar'
import type { Column } from '../../lib/ui/table'
import { statusTone } from '../../lib/ui/status'
import { toast } from '../../lib/ui/toast'
import { XYChart } from '../../lib/ui/viz/XYChart'
import type { XYRule, XYSeries } from '../../lib/ui/viz/xyChartModel'
import { Section } from '../../lib/ui/Section'
import { KvTable } from './helpers'

const POLL_MS = 1000
/** 분석에 불러오는 최대 데이터 샘플 수 (넘으면 백엔드가 간격을 줄인다. 표시는 모두 온다). 샘플 한 줄 ≈ 2.3 KB, 50 ms 로 10 분 */
const LOAD_MAX = 12000
const EDGE_ROWS = 300

/** 시험 종류 — docs/measure-test 절차서의 네 항목 */
const KINDS: readonly (readonly [string, string])[] = [
  ['laser', '레이저 측정'],
  ['zcal', 'Z 오프셋 교정'],
  ['torque', '토크 내경 측정'],
  ['grip', '그리퍼 파지 판정'],
  ['free', '기타'],
]
const RATES = ['20', '50', '100', '200'] as const
const QUICK_MARKS = ['측정 시작', '타이어 돌림', '다른 타이어', '파라미터 변경', '이상 동작'] as const

const EDGE_BITS: readonly (keyof GripperState)[] = [
  'Commanded',
  'Stopped',
  'AtCommand',
  'TorqueReached',
  'TorqueStop',
  'Stall',
  'StallNoTorque',
]
/** 비트 이름은 PLC `LGR_GripperState` 멤버 그대로, 설명은 툴팁. */
const STATE_HINT: Partial<Record<keyof GripperState, string>> = {
  Commanded: '이동 명령 중',
  Stopped: '멈춤',
  AtCommand: '목표 도착',
  TorqueReached: '토크 검출',
  TorqueStop: '토크 멈춤',
  Stall: 'G 멈춤 (목표 미도달)',
  StallNoTorque: '토크 없이 멈춤',
}
const OFFSET_KEYS = ['GIDL_ZOffset', 'GIDF_ZOffset', 'GIDR_ZOffset', 'GIDB_ZOffset'] as const
/** MEASLOG Item 기록의 Data 인덱스 (MEAS_ITEM_* 와 LGR_MeasureLog 주석) — 이름은 PLC 멤버 그대로. */
const ITEM_DATA: readonly (readonly [number, string])[] = [
  [1, 'InnerDia'],
  [10, 'In.InnerDia'],
  [15, 'Out.InnerDia'],
  [7, 'Torq_InnerDia'],
  [4, 'Torq_Raw1'],
  [5, 'Torq_Raw2'],
  [9, 'Tolerance'],
]

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

function kindLabel(k: string): string {
  return KINDS.find(([id]) => id === k)?.[1] ?? (k || '-')
}

function clock(s: string | null | undefined): string {
  if (!s) return '-'
  const d = new Date(s)
  return Number.isNaN(d.getTime()) ? s : d.toLocaleString('ko-KR', { hour12: false })
}

function duration(m: RecordMeta): string {
  const end = m.stopped_at ? Date.parse(m.stopped_at) : Date.now()
  const s = Math.max(0, (end - Date.parse(m.started_at)) / 1000)
  return s >= 60 ? `${Math.floor(s / 60)}분 ${Math.round(s % 60)}초` : `${Math.round(s)}초`
}

const sec = (t: number) => (t / 1000).toFixed(2)

function num(v: number | undefined): string {
  if (v === undefined) return '-'
  return Number.isInteger(v) ? String(v) : v.toFixed(3)
}

/** 비드 판정 재현을 읽는 법 — 목록 위에 글머리표 셋으로 서 있던 글(약 300자). */
const REPLAY_HELP =
  'PLC FB_MeasureTireLaser 와 같은 규칙입니다: 본 지름 < 외경인 샘플 중 거리 ≤ 최소 거리 + 여유, 본 지름 < (내경 + 외경) / 2 이면 비드입니다. 높이는 셀 바닥 기준이고 기록 시작 때 PARA 센서 Z 오프셋을 더했습니다(PLC 진단 이력의 경계 높이와 비교하세요). 기록 주기가 PLC 스캔보다 길어 높이는 Z 속도 × 주기만큼 차이 날 수 있습니다.'

/** 멈춤 판정을 읽는 법 — 표 아래 문단으로 서 있던 글. */
const JUDGE_HELP =
  'GripperJudge: 토크 멈춤이 허용 범위(p942) 안이면 토크 파지(정상), 범위 밖 멈춤은 4025 입니다. 판정은 이동 방향을 보지 않는 근사라 목표를 넘어간 멈춤도 범위 밖으로 보일 수 있습니다.'

/** 표시(마크) — 단위는 헤더에, 값은 숫자만. */
const MARK_COLS: Column<RecSample>[] = [
  { key: 't', label: 't (s)', get: (x) => x.t, cell: (x) => sec(x.t), numeric: true, priority: 1 },
  { key: 'at', label: '시각', get: (x) => clock(x.at), priority: 3 },
  { key: 'm', label: '표시', get: (x) => x.mark ?? '', priority: 1 },
]

/** 그리퍼 비트 변화 — 비트 이름은 PLC 멤버 그대로이고 설명은 툴팁이다. */
const EDGE_COLS: Column<BitEdge>[] = [
  { key: 't', label: 't (s)', get: (e) => e.t, cell: (e) => sec(e.t), numeric: true, priority: 1 },
  {
    key: 'bit',
    label: 'Bit',
    get: (e) => e.bit,
    cell: (e) => <span title={STATE_HINT[e.bit]}>{e.bit}</span>,
    priority: 1,
  },
  {
    key: 'on',
    label: 'On/Off',
    get: (e) => (e.on ? '켜짐' : '꺼짐'),
    cell: (e) => {
      const tone = e.bit === 'StallNoTorque' ? 'fault' : e.bit === 'Stall' ? 'warn' : 'info'
      return (
        <span className={e.on ? statusTone(tone).text : 'text-content-muted'}>
          {e.on ? '켜짐' : '꺼짐'}
        </span>
      )
    },
    priority: 1,
  },
  { key: 'z', label: 'Z (mm)', get: (e) => pos(e.z), numeric: true, priority: 2 },
  { key: 'g', label: 'G (mm)', get: (e) => pos(e.g), numeric: true, priority: 2 },
  { key: 'gt', label: 'G.Target (mm)', get: (e) => pos(e.gTarget), numeric: true, priority: 3 },
  { key: 'tq', label: 'Torque (%)', get: (e) => pos(e.torque), numeric: true, priority: 2 },
]

/** 멈춤 판정 — 허용 범위(p942)를 알아야 판정이 나오므로 열을 그때 만든다. */
function stallCols(window: number | undefined): Column<StallEpisode>[] {
  const verdict = (e: StallEpisode) => {
    const short = e.gTarget - e.g
    const inWindow = window !== undefined && window > 0 ? Math.abs(short) < window : null
    if (!e.torqueReached) return { text: '토크 없이 멈춤', tone: 'fault' }
    if (inWindow === false) return { text: '범위 밖 멈춤', tone: 'fault' }
    return { text: inWindow ? '토크 파지' : '토크 멈춤', tone: 'ok' }
  }
  return [
    { key: 't0', label: 't0 (s)', get: (e) => e.t0, cell: (e) => sec(e.t0), numeric: true, priority: 1 },
    {
      key: 'dur',
      label: 'Duration (s)',
      get: (e) => e.t1 - e.t0,
      cell: (e) => sec(e.t1 - e.t0),
      numeric: true,
      priority: 1,
    },
    { key: 'g', label: 'G (mm)', get: (e) => pos(e.g), numeric: true, priority: 2 },
    {
      key: 'short',
      label: 'Target − G (mm)',
      get: (e) => pos(e.gTarget - e.g),
      numeric: true,
      priority: 1,
    },
    { key: 'tq', label: 'Torque (%)', get: (e) => pos(e.torque), numeric: true, priority: 2 },
    {
      key: 'v',
      label: 'Verdict',
      get: (e) => verdict(e).text,
      cell: (e) => {
        const v = verdict(e)
        return (
          <span className={statusTone(v.tone).text}>
            {v.text}
            {e.itemDetect ? ' · 아이템 감지' : ''}
          </span>
        )
      },
      priority: 1,
    },
    {
      key: 'al',
      label: 'Alarms',
      get: (e) =>
        e.alarms.length ? e.alarms.map((c) => `${c} ${alarmLabel(c) ?? ''}`.trim()).join(', ') : null,
      priority: 3,
    },
  ]
}

/** 기록 목록 — 행을 누르면 분석이 열린다. 행 끝 조작은 **둘 고정**(CSV · 삭제). */
const LIST_COLS: Column<RecordMeta>[] = [
  { key: 'at', label: 'StartedAt', get: (m) => m.started_at, cell: (m) => clock(m.started_at), priority: 1 },
  { key: 'label', label: 'Label', get: (m) => m.label, priority: 1 },
  { key: 'kind', label: 'Kind', get: (m) => kindLabel(m.kind), priority: 2 },
  { key: 'dur', label: 'Duration', get: (m) => duration(m), numeric: true, priority: 2 },
  { key: 'samples', label: 'Samples', get: (m) => m.samples, numeric: true, priority: 3 },
  { key: 'marks', label: 'Marks', get: (m) => m.marks, numeric: true, priority: 3 },
]

export function Recorder() {
  // 기록은 사이드바에서 고른 로봇 PLC 를 읽는다(한 번에 하나 — 진행 중인 기록은 어느 로봇 것인지 보인다).
  useStore(robots)
  const [ov, setOv] = useState<RecordOverview | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [label, setLabel] = useState('')
  const [kind, setKind] = useState('laser')
  const [note, setNote] = useState('')
  const [rate, setRate] = useState('50')
  const [mark, setMark] = useState('')
  const [busy, setBusy] = useState(false)
  const [session, setSession] = useState<RecordSession | null>(null)
  const [startOpen, setStartOpen] = useState(false)
  const [deleteId, setDeleteId] = useState<string | null>(null)

  const load = useCallback(async () => {
    try {
      setOv(await api.record())
      setError(null)
    } catch (e) {
      setError(errText(e))
    }
  }, [])

  useEffect(() => {
    void load()
    const t = visibleInterval(() => void load(), POLL_MS)
    return () => clearInterval(t)
  }, [load])

  async function act<T>(what: string, fn: () => Promise<T>): Promise<T | undefined> {
    setBusy(true)
    try {
      const r = await fn()
      await load()
      return r
    } catch (e) {
      toast.error(`${what} 실패: ${errText(e)}`)
      return undefined
    } finally {
      setBusy(false)
    }
  }

  async function open(id: string) {
    try {
      setSession(await api.recordSession(id, LOAD_MAX))
    } catch (e) {
      toast.error(`기록 열기 실패: ${errText(e)}`)
    }
  }

  async function start() {
    const m = await act('기록 시작', () =>
      api.recordStart({
        label: label.trim(),
        kind,
        note: note.trim(),
        rate_ms: Number(rate),
        robot: robots.selected,
      }),
    )
    if (m) {
      toast.ok(`기록 시작: ${m.label}`)
      // 시작하면 입력은 소비된 것이다 — 다음 시험을 옛 이름으로 시작하지 않게 비운다.
      setStartOpen(false)
      setLabel('')
      setNote('')
    }
  }

  async function stop() {
    const m = await act('기록 정지', () => api.recordStop())
    if (m) {
      toast.ok(`기록 정지: 샘플 ${m.samples}`)
      void open(m.id)
    }
  }

  async function addMark(text: string) {
    const t = text.trim()
    if (!t) return
    const r = await act('표시', () => api.recordMark(t))
    if (r) {
      setMark('')
      toast.ok(`표시: ${t}`)
    }
  }

  async function remove(id: string) {
    const r = await act('기록 삭제', () => api.recordDelete(id).then(() => true))
    if (r && session?.meta.id === id) setSession(null)
  }

  const active = ov?.active ?? null
  const sessions = ov?.sessions ?? []
  const where = robots.current ? `${robots.current.name} (${robots.current.plc})` : '기본 로봇'

  return (
    <div className="space-y-3" data-testid="measure-record">
      <Toolbar
        title={active ? '기록 중' : '기록'}
        dense
        tone={active ? 'danger' : 'default'}
        meta={
          <span>
            {active
              ? `${active.label} · ${kindLabel(active.kind)} · ${active.plc} · ${active.rate_ms} ms`
              : `${where} WEBMON 을 PLC 에서 직접 읽어 콘솔 PC 에 남깁니다`}
            {error ? <span className={statusTone('fault').text}> · {error}</span> : null}
          </span>
        }
      >
        {active ? (
          <Button size="sm" intent="danger" loading={busy} onClick={() => void stop()}>
            기록 정지
          </Button>
        ) : (
          <Button size="sm" intent="primary" onClick={() => setStartOpen(true)}>
            기록 시작…
          </Button>
        )}
      </Toolbar>

      {active ? (
        <Card padded={false}>
          <StatRow
            bordered={false}
            items={[
              { label: 'Elapsed', value: duration(active), hint: clock(active.started_at) },
              { label: 'Samples', value: String(active.samples) },
              { label: 'Marks', value: String(active.marks) },
              {
                label: 'ReadErrors',
                value: String(active.read_errors),
                tone: active.read_errors > 0 ? 'warn' : undefined,
              },
              {
                label: 'Robot · RateMs',
                value: `${active.robot ? robots.nameOf(active.robot) : active.plc} · ${active.rate_ms}`,
              },
            ]}
          />
          <div className="flex flex-wrap items-center gap-2 border-t border-line-subtle px-3 py-2">
            {/* 시험 단계 표시는 기록 중 가장 자주 하는 일이다 — Enter 하나로 끝난다. */}
            <Input
              value={mark}
              onValueChange={setMark}
              placeholder="표시 (시험 단계 메모, Enter)"
              aria-label="표시"
              className="min-w-0 flex-1"
              onKeyDown={(e) => {
                if (e.key === 'Enter') void addMark(mark)
              }}
            />
            {QUICK_MARKS.map((q) => (
              <Button key={q} size="sm" intent="outline" disabled={busy} onClick={() => void addMark(q)}>
                {q}
              </Button>
            ))}
          </div>
        </Card>
      ) : null}

      <Card padded={false}>
        <div className="flex items-baseline gap-2 border-b border-line-default px-3 py-1.5">
          <h3 className="text-xs font-semibold text-content-muted">기록 목록</h3>
          <HelpTip
            title="기록 목록"
            text="행을 누르면 아래에 그 기록의 분석이 열립니다. 행 끝의 CSV 는 샘플 전체를 내려받고, 삭제는 콘솔 PC 의 파일만 지웁니다."
          />
          <span className="ml-auto text-2xs text-content-faint tabular-nums">
            {sessions.length}
          </span>
        </div>
        <DataTable
          rows={sessions}
          columns={LIST_COLS}
          rowKey={(m) => m.id}
          onPick={(m) => void open(m.id)}
          selected={session?.meta.id ?? null}
          density="compact"
          stickyHeader
          className="max-h-96 overflow-y-auto"
          empty="기록 없음"
          emptyHint="위의 기록 시작으로 첫 기록을 남기세요."
          actions={(m) => (
            <span className="flex justify-end gap-1">
              <Button
                size="sm"
                intent="ghost"
                onClick={() => window.location.assign(api.recordCsvUrl(m.id))}
              >
                CSV
              </Button>
              <Button size="sm" intent="ghost" disabled={busy} onClick={() => setDeleteId(m.id)}>
                삭제
              </Button>
            </span>
          )}
        />
      </Card>

      {session ? <SessionAnalysis key={session.meta.id} s={session} /> : null}

      {/* 새 기록 설정 — 열리자마자 Label 에 포커스, Enter 로 시작, Escape 로 닫되 입력이 남아 있으면 되묻는다. */}
      <FormDialog
        open={startOpen}
        onOpenChange={setStartOpen}
        title="기록 시작"
        meta={
          <>
            <span>{where}</span>
            <span>30 분 후 자동 정지</span>
            <HelpTip
              title="기록"
              text="축·그리퍼·측정 영역을 PLC 에서 직접 읽어 콘솔 PC 에 저장합니다. 시작과 끝에 PARA·LASERDIAG·MEASLOG 스냅샷을 함께 남기고, 브라우저를 닫아도 기록은 이어집니다."
            />
          </>
        }
        size="md"
        testid="record-start"
        dirty={label.trim() !== '' || note.trim() !== ''}
        submitLabel="기록 시작"
        busy={busy}
        disabledReason={label.trim() ? undefined : 'Label 을 적어야 시작할 수 있습니다'}
        onSubmit={() => void start()}
      >
        <div className="grid gap-3 sm:grid-cols-2">
          <Input label="Label" value={label} onValueChange={setLabel} placeholder="예: 1001 들어갈 때 3회" />
          <Select label="Kind" value={kind} onValueChange={setKind}>
            {KINDS.map(([id, l]) => (
              <option key={id} value={id}>
                {l}
              </option>
            ))}
          </Select>
          <Input label="Note" value={note} onValueChange={setNote} placeholder="타이어·셀·파라미터" />
          <Field label="RateMs" hint="20 ms 는 필요한 구간만 — 샘플 한 줄이 약 2.3 KB 입니다">
            <Select value={rate} onValueChange={setRate} aria-label="RateMs">
              {RATES.map((r) => (
                <option key={r} value={r}>
                  {`${r} ms`}
                </option>
              ))}
            </Select>
          </Field>
        </div>
      </FormDialog>

      <ConfirmDialog
        open={deleteId !== null}
        onOpenChange={(o) => !o && setDeleteId(null)}
        scope="single"
        title="기록 삭제"
        danger
        confirmLabel="삭제"
        onConfirm={() => {
          const id = deleteId
          setDeleteId(null)
          if (id) void remove(id)
        }}
      >
        {/* 질문 한 줄 + 무엇을 지우는지 알아볼 라벨+값 짝. */}
        <p className="m-0 text-content-primary">이 기록을 정말로 삭제하시겠습니까? 되돌릴 수 없습니다.</p>
        <FieldList
          className="mt-3"
          columns={2}
          dense
          labelWidth={72}
          items={(() => {
            const m = sessions.find((x) => x.id === deleteId)
            return [
              { label: 'Label', value: m?.label ?? null },
              { label: 'Kind', value: m ? kindLabel(m.kind) : null },
              { label: 'StartedAt', value: m ? clock(m.started_at) : null },
              { label: 'Samples', value: m?.samples ?? null },
              { label: '지우는 것', value: '콘솔 PC 의 샘플·스냅샷 파일', wide: true },
              { label: 'PLC', value: '값이 바뀌지 않습니다', wide: true },
            ]
          })()}
        />
      </ConfirmDialog>
    </div>
  )
}

function SessionAnalysis({ s }: { s: RecordSession }) {
  const { meta, samples } = s
  const segs = useMemo(() => segments(samples), [samples])
  const [segKey, setSegKey] = useState('all')
  const [band, setBand] = useState('10')
  const seg = segKey === 'all' ? null : (segs[Number(segKey)] ?? null)
  const i0 = seg?.i0 ?? 0
  const i1 = seg?.i1 ?? samples.length - 1
  const t0 = samples[i0]?.t ?? 0
  const t1 = samples[i1]?.t ?? 0
  const spec = useMemo(() => taskSpec(samples, i0, i1), [samples, i0, i1])
  const nDir = useMemo(() => dirCount(samples), [samples])
  const bandMm = Number(band) > 0 ? Number(band) : 10
  const descending = seg?.kind !== 'out'
  const dataCount = useMemo(() => samples.filter((x) => !x.mark).length, [samples])
  const zoff = (k: number): number => meta.start?.para_sensor?.[OFFSET_KEYS[k] ?? ''] ?? 0
  const window = meta.start?.para_task?.G_TorqLimitAllowDistance

  const replays = useMemo(
    () =>
      spec
        ? Array.from({ length: nDir }, (_, k) => replay(samples, k, spec, { band: bandMm, descending, i0, i1 }))
        : [],
    [samples, spec, nDir, bandMm, descending, i0, i1],
  )
  const edges = useMemo(() => bitEdges(samples, EDGE_BITS, i0, i1), [samples, i0, i1])
  const stalls = useMemo(() => stallEpisodes(samples, i0, i1), [samples, i0, i1])
  const alarms = useMemo(() => alarmCodes(samples.slice(i0, i1 + 1)), [samples, i0, i1])
  const marks = useMemo(() => samples.filter((x) => x.mark), [samples])

  const markRules = useMemo(
    () =>
      marks
        .filter((x) => x.t >= t0 && x.t <= t1)
        .map((x): XYRule => ({ axis: 'x', value: x.t / 1000, label: x.mark ?? '', tone: 4 })),
    [marks, t0, t1],
  )
  const profileSeries = useMemo(
    () =>
      Array.from(
        { length: nDir },
        (_, k): XYSeries => ({
          name: LASER_DIRS[k] ?? String(k),
          tone: k,
          dots: true,
          points: profile(samples, k, i0, i1).map((p) => ({ x: p.dia, y: p.z })),
        }),
      ),
    [samples, nDir, i0, i1],
  )
  const profileRules = useMemo(() => {
    const r: XYRule[] = []
    if (!spec) return r
    r.push({ axis: 'x', value: spec.id, label: 'InnerDiameter', tone: 4 })
    if (spec.od > spec.id) {
      r.push({ axis: 'x', value: (spec.id + spec.od) / 2, label: 'Mid', tone: 4 })
      r.push({ axis: 'x', value: spec.od, label: 'OuterDiameter', tone: 4 })
    }
    return r
  }, [spec])
  const series = useMemo(() => {
    const ts = (fn: Parameters<typeof timeSeries>[1]) => timeSeries(samples, fn, i0, i1)
    const z: XYSeries[] = [
      { name: 'Z.Position', tone: 0, points: ts((x) => x.Axis[2]?.Position) },
      { name: 'Z.Target', tone: 4, points: ts((x) => x.Axis[2]?.Target) },
    ]
    const g: XYSeries[] = [
      { name: 'G.Position', tone: 0, points: ts((x) => x.Axis[3]?.Position) },
      { name: 'G.Target', tone: 4, points: ts((x) => x.Axis[3]?.Target) },
    ]
    const torque: XYSeries[] = [{ name: 'G.Torque %', tone: 3, points: ts((x) => x.Axis[3]?.Torque) }]
    const dist = Array.from(
      { length: nDir },
      (_, k): XYSeries => ({
        name: `GID[${LASER_DIRS[k] ?? k}]`,
        tone: k,
        dots: true,
        points: ts((x) => {
          const v = x.Gripper?.GID?.[k]
          return v !== undefined && v > 0 && v < LASER_FAR ? v : undefined
        }),
      }),
    )
    return { z, g, torque, dist }
  }, [samples, nDir, i0, i1])


  /** 방향별 높이는 기록 시작 때의 PARA 센서 Z 오프셋을 더해 셀 바닥·목표 기준으로 환산한다. */
  const rel = (k: number, z: number | null, ref: number) => (z === null ? null : pos(z + zoff(k) - ref))
  const replayCols: Column<BeadReplay>[] = [
    {
      key: 'dir',
      label: 'Dir',
      get: (r) => LASER_DIRS[r.dir] ?? String(r.dir),
      class: 'font-semibold',
      priority: 1,
    },
    { key: 'seen', label: 'Seen', get: (r) => r.seen, numeric: true, priority: 1 },
    { key: 'min', label: 'MinDist (mm)', get: (r) => pos(r.minDist), numeric: true, priority: 1 },
    {
      key: 'beadz',
      label: 'BeadZ (mm)',
      get: (r) => rel(r.dir, r.zAtMin, spec?.cellZ ?? 0),
      numeric: true,
      priority: 2,
    },
    {
      key: 'lastz',
      label: 'LastBeadZ (mm)',
      get: (r) => pos(r.lastBeadZ),
      numeric: true,
      priority: 2,
    },
    {
      key: 'dz',
      label: 'LastBeadZ − TargetZ (mm)',
      get: (r) => rel(r.dir, r.lastBeadZ, spec?.targetZ ?? 0),
      numeric: true,
      priority: 1,
    },
    {
      key: 'edge',
      label: 'EdgeZ (mm)',
      get: (r) => rel(r.dir, r.edgeZ, spec?.cellZ ?? 0),
      numeric: true,
      priority: 3,
    },
  ]

  return (
    <div className="space-y-3" data-testid="record-analysis">
      {/* ① 이 기록이 무엇인가 + 언제 무엇을 표시했나 — 같은 질문이라 한 면에 둔다. */}
      <Card>
        <div className="mb-2 flex flex-wrap items-end gap-3">
          <h3 className="mr-auto text-sm font-semibold">분석: {meta.label}</h3>
          <Select dense label="구간" value={segKey} onValueChange={setSegKey}>
            <option value="all">{`전체 (${dataCount} 샘플)`}</option>
            {segs.map((g, i) => (
              <option key={`${g.kind}-${g.i0}`} value={String(i)}>
                {`${g.label} ${sec(g.t0)}–${sec(g.t1)} s`}
              </option>
            ))}
          </Select>
          <Input label="비드 여유 (mm)" value={band} onValueChange={setBand} mono className="w-28" />
        </div>
        <div className="grid gap-3 xl:grid-cols-2">
          <Section title="기록" first>
            <KvTable
              labelWidth={120}
              rows={[
                ['Kind', `${kindLabel(meta.kind)}${meta.note ? ` — ${meta.note}` : ''}`],
                ['Plc / RateMs', `${meta.plc} / ${meta.rate_ms} ms`],
                [
                  'StartedAt',
                  `${clock(meta.started_at)} ~ ${clock(meta.stopped_at)} (${duration(meta)})`,
                  '시작 ~ 끝 (걸린 시간)',
                ],
                [
                  'Samples',
                  `${meta.samples}${dataCount < meta.samples ? ` (${dataCount} 불러옴)` : ''} · Marks ${meta.marks} · ReadErrors ${meta.read_errors}`,
                  dataCount < meta.samples ? '최대치를 넘어 간격을 줄여 불러왔습니다' : undefined,
                ],
                [
                  'Item',
                  spec
                    ? `Code ${spec.code} · ID ${pos(spec.id)} · OD ${pos(spec.od)} · H ${pos(spec.height)} · Cell.Z ${pos(spec.cellZ)} · TargetZ ${pos(spec.targetZ)}`
                    : '작업 없음',
                  'Code · InnerDiameter · OuterDiameter · Height · Cell.Z · TargetZ (mm)',
                ],
                [
                  'Alarms',
                  alarms.length ? (
                    <span className={statusTone('fault').text}>
                      {alarms.map((c) => `${c} ${alarmLabel(c) ?? ''}`.trim()).join(', ')}
                    </span>
                  ) : (
                    '없음'
                  ),
                ],
              ]}
            />
          </Section>
          <Section title="표시" right={`${marks.length} 건`} first>
            <DataTable
              rows={marks}
              columns={MARK_COLS}
              rowKey={(x) => `${x.t}-${x.mark ?? ''}`}
              density="compact"
              className="max-h-72 overflow-auto"
              empty="표시 없음"
              emptyHint="기록 중 표시 입력이나 빠른 버튼으로 시험 단계를 남깁니다."
            />
          </Section>
        </div>
      </Card>

      {/* ② 레이저가 무엇을 봤나 — 프로파일과 그 프로파일로 다시 돌린 판정. */}
      <Card>
        <div className="grid gap-3 xl:grid-cols-2">
          <Section title="레이저 프로파일" right="가로 G + 2×GID · 세로 그리퍼 Z" first>
            {profileSeries.some((x) => x.points.length) ? (
              <XYChart
                series={profileSeries}
                rules={profileRules}
                xLabel="G + 2×GID (mm)"
                yLabel="Z (mm)"
                height={360}
              />
            ) : (
              <EmptyState
                title="유효 거리 없음"
                hint="이 구간의 거리 값이 타이어 밖이거나 800 mm 클램프입니다. 구간을 바꿔 보세요."
              />
            )}
          </Section>
          <Section
            title="비드 판정 재현"
            right={`${descending ? '하강' : '상승'} · 여유 ${bandMm} mm`}
            help={REPLAY_HELP}
            first
          >
            {!spec ? (
              <EmptyState title="작업 규격 없음" hint="이 구간에는 내경이 있는 작업이 없습니다." />
            ) : (
              <DataTable
                rows={replays}
                columns={replayCols}
                rowKey={(r) => String(r.dir)}
                density="compact"
                empty="방향 없음"
              />
            )}
          </Section>
        </div>
      </Card>

      {/* ③ 시계열 넷 — 같은 x축(t)에 세로선(표시)이 같은 자리에 선다. 카드 넷이던 것을 한 면에. */}
      <Card>
        <div className="grid gap-3 xl:grid-cols-2">
          <Section title="Z 축" right="세로선 = 표시" first>
            <XYChart series={series.z} rules={markRules} xLabel="t (s)" yLabel="mm" height={220} />
          </Section>
          <Section title="G 축" first>
            <XYChart series={series.g} rules={markRules} xLabel="t (s)" yLabel="mm" height={220} />
          </Section>
          <Section title="G 토크" first>
            <XYChart
              series={series.torque}
              rules={markRules}
              xLabel="t (s)"
              yLabel="%"
              height={220}
            />
          </Section>
          <Section title="레이저 거리" right="유효값만" first>
            <XYChart
              series={series.dist}
              rules={markRules}
              xLabel="t (s)"
              yLabel="mm"
              height={220}
            />
          </Section>
        </div>
      </Card>

      {/* ④ 그리퍼가 무엇을 했나 — 비트 변화와 그 변화가 만든 멈춤 구간. */}
      <Card>
        <div className="grid gap-3 xl:grid-cols-2">
          <Section
            title="그리퍼 상태 변화"
            right={
              edges.length > EDGE_ROWS
                ? `앞 ${EDGE_ROWS} / ${edges.length} 건 — 구간을 좁히세요`
                : `${edges.length} 건`
            }
            first
          >
            <DataTable
              rows={edges.slice(0, EDGE_ROWS)}
              columns={EDGE_COLS}
              rowKey={(e) => `${e.t}-${e.bit}-${e.on ? 1 : 0}`}
              density="compact"
              stickyHeader
              className="max-h-96 overflow-auto"
              empty="변화 없음"
            />
          </Section>
          <Section
            title="G 멈춤 구간"
            right={`${stalls.length} 건 · 허용 범위 p942 ${window ?? '-'} mm`}
            help={JUDGE_HELP}
            first
          >
            <DataTable
              rows={stalls}
              columns={stallCols(window)}
              rowKey={(e) => String(e.t0)}
              density="compact"
              stickyHeader
              className="max-h-96 overflow-auto"
              empty="멈춤 없음"
            />
          </Section>
        </div>
      </Card>

      <PlcRecords meta={meta} nDir={nDir} />

      {/* ⑤ 스냅샷 셋은 같은 질문("기록 도중 값이 바뀌었나")이라 카드 하나 안에 나란히 둔다. */}
      <Card>
        <div className="grid gap-3 xl:grid-cols-3">
          <Section title="PARA.Sensor" right="시작 → 끝" first>
            <ParaDiff a={meta.start?.para_sensor} b={meta.end?.para_sensor} />
          </Section>
          <Section title="PARA.Task" right="시작 → 끝" first>
            <ParaDiff a={meta.start?.para_task} b={meta.end?.para_task} />
          </Section>
          <Section title="Z 오프셋 교정" right="시작 → 끝" first>
            <ZCalDiff a={meta.start} b={meta.end} nDir={nDir} />
          </Section>
        </div>
      </Card>
    </div>
  )
}

/**
 * 기록이 끝난 뒤 PLC 가 남긴 것 — 레이저 진단 이력과 MEASLOG 종류별 마지막 건.
 *
 * MEASLOG 표만 손으로 짠 `<table>` 로 남는다: Item(Kind 1) 행 아래에 `Data` 일곱 값을 **보조 줄**로
 * 펴는데, 한 행이 두 줄인 모양은 `DataTable` 의 계약(행 하나 = `<tr>` 하나 + 접힌 열 펼치기) 밖이다.
 */
function PlcRecords({ meta, nDir }: { meta: RecordMeta; nDir: number }) {
  const dirs = LASER_DIRS.slice(0, nDir)
  const laser = meta.end?.laser_entries ?? []
  const meas = meta.end?.measlog_last ?? []
  const pendingHint = '기록을 정지하면 채워집니다.'
  const laserCols: Column<LaserDiagEntry>[] = [
    { key: 'seq', label: 'Seq', get: (e) => e.Seq, numeric: true, priority: 1 },
    { key: 'src', label: 'Source', get: (e) => LASER_SOURCE[e.Source] ?? e.Source, priority: 2 },
    { key: 'code', label: 'Code', get: (e) => e.Code || null, numeric: true, priority: 2 },
    {
      key: 'valid',
      label: 'Valid / FitError',
      get: (e) => (e.Valid ? 'OK' : `실패 ${e.FitError}`),
      priority: 1,
    },
    ...dirs.map(
      (d, k): Column<LaserDiagEntry> => ({
        key: `dev${k}`,
        label: `BeadDev[${d}] (mm)`,
        get: (e) => delta(e.BeadDev?.[k]),
        numeric: true,
        priority: 2,
      }),
    ),
    ...dirs.map(
      (d, k): Column<LaserDiagEntry> => ({
        key: `edge${k}`,
        label: `EdgeHeight[${d}] (mm)`,
        get: (e) => pos(e.EdgeHeight?.[k]),
        numeric: true,
        priority: 3,
      }),
    ),
    { key: 'spread', label: 'Spread (mm)', get: (e) => delta(e.Spread), numeric: true, priority: 3 },
    {
      key: 'flags',
      label: 'DiagFlags',
      get: (e) => diagFlagLabels(e.DiagFlags).join(', ') || null,
      priority: 3,
    },
  ]

  return (
    <Card>
      <div className="grid gap-3 xl:grid-cols-2">
        <Section title="PLC 레이저 진단" right={`기록 중 새로 ${laser.length} 건`} first>
          {!meta.end ? (
            <EmptyState title="기록 중" hint={pendingHint} />
          ) : (
            <DataTable
              rows={laser}
              columns={laserCols}
              rowKey={(e) => String(e.Seq)}
              density="compact"
              stickyHeader
              className="max-h-96 overflow-auto"
              empty="새 기록 없음"
            />
          )}
        </Section>
        <Section
          title="PLC 측정 이력 MEASLOG"
          right="종류별 마지막 1 건"
          help="모든 회차는 측정 모니터의 기록 탭에 있습니다. Item(Kind 1) 행 아래 줄은 그 회차의 Data 값입니다."
          first
        >
          {!meta.end ? (
            <EmptyState title="기록 중" hint={pendingHint} />
          ) : meas.length === 0 ? (
            <EmptyState title="새 기록 없음" hint="기록 중에 끝난 측정이 없습니다." />
          ) : (
            <div className="overflow-auto">
              <table className="w-full text-xs">
                <thead>
                  <tr className="whitespace-nowrap text-content-muted">
                    <th className="py-1 text-right">Seq</th>
                    <th className="pl-3 text-left">Kind</th>
                    <th className="pl-3 text-left">Status</th>
                    <th className="pl-3 text-right">Code</th>
                    <th className="pl-3 text-right">Cmd.InnerDiameter (mm)</th>
                    <th className="pl-3 text-right">Delta.InnerDia (mm)</th>
                    <th className="pl-3 text-right">Delta.Z (mm)</th>
                  </tr>
                </thead>
                <tbody>
                  {meas.map((e) => (
                    <Fragment key={e.Seq}>
                      <tr className="border-t border-line-default font-mono whitespace-nowrap tabular-nums">
                        <td className="py-1 text-right">{e.Seq}</td>
                        <td className="pl-3 font-sans">{KIND[e.Kind] ?? e.Kind}</td>
                        <td className="pl-3 font-sans">{STATUS[e.Status] ?? e.Status}</td>
                        <td className="pl-3 text-right">{e.Cmd?.Item?.Code ?? '-'}</td>
                        <td className="pl-3 text-right">{pos(e.Cmd?.Item?.InnerDiameter)}</td>
                        <td className="pl-3 text-right">{delta(e.Delta?.InnerDia)}</td>
                        <td className="pl-3 text-right">{delta(e.Delta?.Z)}</td>
                      </tr>
                      {e.Kind === 1 ? (
                        <tr>
                          <td />
                          <td colSpan={6} className="pb-1 pl-3">
                            <span className="flex flex-wrap gap-x-4 gap-y-0.5 font-mono tabular-nums">
                              {ITEM_DATA.map(([i, l]) => (
                                <span key={i} className="whitespace-nowrap">
                                  <span className="font-sans text-content-muted">{l}</span>{' '}
                                  {pos(e.Data?.[i])}
                                </span>
                              ))}
                            </span>
                          </td>
                        </tr>
                      ) : null}
                    </Fragment>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Section>
      </div>
    </Card>
  )
}

interface ParaRow {
  key: string
  a: number | undefined
  b: number | undefined
}

const PARA_COLS: Column<ParaRow>[] = [
  { key: 'k', label: 'Member', get: (r) => r.key, priority: 1 },
  { key: 'a', label: 'Start', get: (r) => num(r.a), numeric: true, priority: 1 },
  {
    key: 'b',
    label: 'End',
    get: (r) => num(r.b),
    // 바뀐 값만 색을 받는다 — 같은 값을 칠하면 바뀐 하나가 묻힌다.
    cell: (r) => {
      const changed = r.a !== undefined && r.b !== undefined && r.a !== r.b
      return <span className={changed ? statusTone('warn').text : ''}>{num(r.b)}</span>
    },
    numeric: true,
    priority: 1,
  },
]

function ParaDiff({
  a,
  b,
}: {
  a: Record<string, number> | undefined
  b: Record<string, number> | undefined
}) {
  const rows: ParaRow[] = [...new Set([...Object.keys(a ?? {}), ...Object.keys(b ?? {})])].map(
    (key) => ({ key, a: a?.[key], b: b?.[key] }),
  )
  return (
    <DataTable
      rows={rows}
      columns={PARA_COLS}
      rowKey={(r) => r.key}
      density="compact"
      stickyHeader
      className="max-h-96 overflow-auto"
      empty="스냅샷 없음"
    />
  )
}

function zcalState(z: LaserZCal | undefined): string {
  if (!z) return '-'
  if (z.Busy) return `수집 중 ${z.Count}/${z.Target}`
  if (z.Done) return '완료'
  if (z.Error) return `실패: ${ZCAL_ERROR[z.ErrorCode] ?? z.ErrorCode}`
  return '대기'
}

function ZCalDiff({ a, b, nDir }: { a: RecordSnapshot | null; b: RecordSnapshot | null; nDir: number }) {
  const za = a?.laser?.ZCal
  const zb = b?.laser?.ZCal
  if (!za && !zb) return <EmptyState title="LASERDIAG 스냅샷 없음" hint="이 PLC 는 레이저 진단 DB 가 없습니다." />
  return (
    <KvTable
      labelWidth={140}
      rows={[
        ['State', `${zcalState(za)} → ${zcalState(zb)}`],
        ['Count (Skipped)', `${za?.Count ?? '-'} → ${zb?.Count ?? '-'} (${zb?.Skipped ?? '-'})`],
        ...LASER_DIRS.slice(0, nDir).map((d, k): [string, string, string] => [
          `[${d}] (mm)`,
          `${delta(zb?.Mean?.[k])} / ${delta(zb?.StdDev?.[k])} / ${delta(zb?.NewOffset?.[k])}`,
          'Mean / StdDev / NewOffset',
        ]),
        ['LASERDIAG.Total', `${a?.laser?.Total ?? '-'} → ${b?.laser?.Total ?? '-'}`],
      ]}
    />
  )
}
