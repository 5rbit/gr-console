// 기록·분석 탭 — 현장 측정 시험(레이저 측정 / Z 오프셋 교정 / 토크 내경 / 그리퍼 파지)의 고속 기록과 분석.
// 기록 : 백엔드(/api/record)가 WEBMON 의 축·그리퍼·측정 영역을 PLC 에서 직접 읽어(기본 50 ms) 콘솔 PC 의 data/records/<id> 에 남긴다.
//        브라우저를 닫아도 기록은 이어지고, 시작·끝에 PARA·LASERDIAG·MEASLOG 스냅샷을 함께 남긴다.
// 분석 : 기록 하나를 불러와 lib/record/analysis 의 순수 함수로 레이저 프로파일, 비드 판정 재현, 그리퍼 상태 변화, 멈춤 구간을 본다.
import { Fragment, useCallback, useEffect, useMemo, useState } from 'react'
import { api } from '../../lib/api'
import { alarmLabel } from '../../lib/gr/alarms'
import { KIND, STATUS } from '../../lib/gr/const'
import { LASER_DIRS, LASER_SOURCE, ZCAL_ERROR, diagFlagLabels } from '../../lib/laser'
import { f1, f2 } from '../../lib/meas/format'
import { visibleInterval } from '../../lib/poll'
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
} from '../../lib/record/analysis'
import type {
  GripperState,
  LaserZCal,
  RecordMeta,
  RecordOverview,
  RecordSession,
  RecordSnapshot,
} from '../../lib/types'
import { Button } from '../../lib/ui/Button'
import { Card } from '../../lib/ui/Card'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { Input } from '../../lib/ui/Input'
import { Select } from '../../lib/ui/Select'
import { statusTone } from '../../lib/ui/status'
import { toast } from '../../lib/ui/toast'
import { XYChart } from '../../lib/ui/viz/XYChart'
import type { XYRule, XYSeries } from '../../lib/ui/viz/xyChartModel'
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
const STATE_LABEL: Partial<Record<keyof GripperState, string>> = {
  Commanded: '이동 명령',
  Stopped: '멈춤',
  AtCommand: '목표 도착',
  TorqueReached: '토크 검출',
  TorqueStop: '토크 멈춤',
  Stall: 'G 멈춤 (목표 미도달)',
  StallNoTorque: '토크 없이 멈춤',
}
const OFFSET_KEYS = ['GIDL_ZOffset', 'GIDF_ZOffset', 'GIDR_ZOffset', 'GIDB_ZOffset'] as const
/** MEASLOG Item 기록의 Data 인덱스 (MEAS_ITEM_* 와 LGR_MeasureLog 주석) */
const ITEM_DATA: readonly (readonly [number, string])[] = [
  [1, '내경'],
  [10, '들어갈 때'],
  [15, '나갈 때'],
  [7, '토크'],
  [4, '토크 1회 G'],
  [5, '토크 2회 G'],
  [9, '허용 편차'],
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

export function Recorder() {
  const [ov, setOv] = useState<RecordOverview | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [label, setLabel] = useState('')
  const [kind, setKind] = useState('laser')
  const [note, setNote] = useState('')
  const [rate, setRate] = useState('50')
  const [mark, setMark] = useState('')
  const [busy, setBusy] = useState(false)
  const [session, setSession] = useState<RecordSession | null>(null)
  const [loadingId, setLoadingId] = useState<string | null>(null)
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
    setLoadingId(id)
    try {
      setSession(await api.recordSession(id, LOAD_MAX))
    } catch (e) {
      toast.error(`기록 열기 실패: ${errText(e)}`)
    } finally {
      setLoadingId(null)
    }
  }

  async function start() {
    const m = await act('기록 시작', () =>
      api.recordStart({ label: label.trim(), kind, note: note.trim(), rate_ms: Number(rate) }),
    )
    if (m) toast.ok(`기록 시작: ${m.label}`)
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

  return (
    <div className="space-y-3" data-testid="measure-record">
      {error ? <div className={`text-xs ${statusTone('fault').text}`}>{error}</div> : null}
      <div className="grid gap-3 lg:grid-cols-2">
        <Card>
          <h3 className="mb-2 text-xs font-semibold text-content-muted">{active ? '기록 중' : '새 기록'}</h3>
          {active ? (
            <div className="space-y-2">
              <KvTable
                rows={[
                  ['이름', `${active.label} (${kindLabel(active.kind)})`],
                  ['PLC / 주기', `${active.plc} / ${active.rate_ms} ms`],
                  ['시작 / 경과', `${clock(active.started_at)} / ${duration(active)}`],
                  ['샘플 / 표시 / 읽기 실패', `${active.samples} / ${active.marks} / ${active.read_errors}`],
                ]}
              />
              <div className="flex items-end gap-2">
                <Input
                  label="표시 (시험 단계 메모, Enter)"
                  value={mark}
                  onValueChange={setMark}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') void addMark(mark)
                  }}
                  className="min-w-0 flex-1"
                />
                <Button size="sm" disabled={busy || !mark.trim()} onClick={() => void addMark(mark)}>
                  표시
                </Button>
              </div>
              <div className="flex flex-wrap gap-1">
                {QUICK_MARKS.map((q) => (
                  <Button key={q} size="sm" intent="ghost" disabled={busy} onClick={() => void addMark(q)}>
                    {q}
                  </Button>
                ))}
              </div>
              <div className="flex justify-end">
                <Button size="sm" intent="danger" loading={busy} onClick={() => void stop()}>
                  기록 정지
                </Button>
              </div>
            </div>
          ) : (
            <div className="space-y-2">
              <div className="grid gap-2 sm:grid-cols-2">
                <Input label="이름" value={label} onValueChange={setLabel} placeholder="예: 1001 들어갈 때 3회" />
                <Select label="시험" value={kind} onValueChange={setKind}>
                  {KINDS.map(([id, l]) => (
                    <option key={id} value={id}>
                      {l}
                    </option>
                  ))}
                </Select>
                <Input label="메모" value={note} onValueChange={setNote} placeholder="타이어·셀·파라미터" />
                <Select label="주기" value={rate} onValueChange={setRate}>
                  {RATES.map((r) => (
                    <option key={r} value={r}>
                      {`${r} ms`}
                    </option>
                  ))}
                </Select>
              </div>
              <p className="text-2xs text-content-muted">
                GR2 WEBMON 의 축·그리퍼·측정 영역을 PLC 에서 직접 읽어 콘솔 PC 에 저장합니다. 시작과 끝에 PARA·LASERDIAG·MEASLOG 를
                함께 남기고, 30 분이 지나면 스스로 멈춥니다. 20 ms 는 필요한 구간만 쓰세요.
              </p>
              <div className="flex justify-end">
                <Button
                  size="sm"
                  intent="primary"
                  loading={busy}
                  disabled={!label.trim()}
                  onClick={() => void start()}
                >
                  기록 시작
                </Button>
              </div>
            </div>
          )}
        </Card>

        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">기록 목록 ({sessions.length})</h3>
          {sessions.length === 0 ? (
            <div className="text-xs text-content-muted">기록 없음</div>
          ) : (
            <div className="max-h-80 overflow-auto">
              <table className="w-full text-xs">
                <thead>
                  <tr className="text-content-muted">
                    <th className="py-1 text-left">시작</th>
                    <th className="pl-2 text-left">이름</th>
                    <th className="text-left">시험</th>
                    <th className="text-right">길이</th>
                    <th className="text-right">샘플</th>
                    <th className="text-right">표시</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {sessions.map((m) => (
                    <tr
                      key={m.id}
                      className={`border-t border-line-default ${session?.meta.id === m.id ? 'bg-surface-inset' : ''}`}
                    >
                      <td className="py-1 font-mono tabular-nums">{clock(m.started_at)}</td>
                      <td className="pl-2">{m.label}</td>
                      <td>{kindLabel(m.kind)}</td>
                      <td className="text-right font-mono tabular-nums">{duration(m)}</td>
                      <td className="text-right font-mono tabular-nums">{m.samples}</td>
                      <td className="text-right font-mono tabular-nums">{m.marks}</td>
                      <td className="whitespace-nowrap text-right">
                        <Button size="sm" intent="ghost" loading={loadingId === m.id} onClick={() => void open(m.id)}>
                          분석
                        </Button>
                        <Button size="sm" intent="ghost" onClick={() => window.location.assign(api.recordCsvUrl(m.id))}>
                          CSV
                        </Button>
                        <Button size="sm" intent="ghost" disabled={busy} onClick={() => setDeleteId(m.id)}>
                          삭제
                        </Button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Card>
      </div>

      {session ? <SessionAnalysis key={session.meta.id} s={session} /> : null}

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
        <p className="text-sm">콘솔 PC 의 기록 파일(샘플·스냅샷)을 지웁니다. PLC 값은 바뀌지 않습니다.</p>
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
    r.push({ axis: 'x', value: spec.id, label: '규격 내경', tone: 4 })
    if (spec.od > spec.id) {
      r.push({ axis: 'x', value: (spec.id + spec.od) / 2, label: '중간', tone: 4 })
      r.push({ axis: 'x', value: spec.od, label: '외경', tone: 4 })
    }
    return r
  }, [spec])
  const series = useMemo(() => {
    const ts = (fn: Parameters<typeof timeSeries>[1]) => timeSeries(samples, fn, i0, i1)
    const z: XYSeries[] = [
      { name: 'Z 위치', tone: 0, points: ts((x) => x.Axis[2]?.Position) },
      { name: 'Z 목표', tone: 4, points: ts((x) => x.Axis[2]?.Target) },
    ]
    const g: XYSeries[] = [
      { name: 'G 위치', tone: 0, points: ts((x) => x.Axis[3]?.Position) },
      { name: 'G 목표', tone: 4, points: ts((x) => x.Axis[3]?.Target) },
    ]
    const torque: XYSeries[] = [{ name: 'G 토크 %', tone: 3, points: ts((x) => x.Axis[3]?.Torque) }]
    const dist = Array.from(
      { length: nDir },
      (_, k): XYSeries => ({
        name: `${LASER_DIRS[k] ?? k} 거리`,
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

  return (
    <div className="space-y-3" data-testid="record-analysis">
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
          <Input label="비드 여유 mm" value={band} onValueChange={setBand} mono className="w-28" />
        </div>
        <KvTable
          rows={[
            ['시험', `${kindLabel(meta.kind)}${meta.note ? ` — ${meta.note}` : ''}`],
            ['PLC / 주기', `${meta.plc} / ${meta.rate_ms} ms`],
            ['시각', `${clock(meta.started_at)} ~ ${clock(meta.stopped_at)} (${duration(meta)})`],
            [
              '샘플',
              `${meta.samples} 기록${dataCount < meta.samples ? `, ${dataCount} 불러옴 (간격 줄임)` : ''} · 표시 ${meta.marks} · 읽기 실패 ${meta.read_errors}`,
            ],
            [
              '규격 (구간 첫 작업)',
              spec
                ? `Code ${spec.code} · 내경 ${f1(spec.id)} · 외경 ${f1(spec.od)} · 높이 ${f1(spec.height)} · 셀 Z ${f1(spec.cellZ)} · 목표 Z ${f1(spec.targetZ)}`
                : '작업 없음',
            ],
            [
              '알람 (구간)',
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
      </Card>

      <div className="grid gap-3 xl:grid-cols-2">
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">
            레이저 프로파일 (가로 = 본 지름 G + 2 × 거리, 세로 = 그리퍼 Z)
          </h3>
          {profileSeries.some((x) => x.points.length) ? (
            <XYChart series={profileSeries} rules={profileRules} xLabel="본 지름 mm" yLabel="Z mm" height={360} />
          ) : (
            <div className="text-xs text-content-muted">구간에 유효 거리가 없습니다 (타이어 밖 또는 800 클램프).</div>
          )}
        </Card>
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">
            비드 판정 재현 ({descending ? '하강' : '상승'}, 여유 {bandMm} mm)
          </h3>
          {!spec ? (
            <div className="text-xs text-content-muted">구간에 작업 규격이 없습니다.</div>
          ) : (
            <div className="overflow-auto">
              <table className="w-full text-xs">
                <thead>
                  <tr className="text-content-muted">
                    <th className="py-1 text-left">방향</th>
                    <th className="text-right">본 샘플</th>
                    <th className="text-right">최소 거리</th>
                    <th className="text-right">비드 높이</th>
                    <th className="text-right">마지막 비드 Z</th>
                    <th className="text-right">마지막 비드 − 목표</th>
                    <th className="text-right">경계 높이</th>
                  </tr>
                </thead>
                <tbody>
                  {replays.map((r) => {
                    const k = r.dir
                    const rel = (z: number | null, ref: number) => (z === null ? '-' : f1(z + zoff(k) - ref))
                    return (
                      <tr key={k} className="border-t border-line-default font-mono tabular-nums">
                        <td className="py-1 font-sans font-semibold">{LASER_DIRS[k]}</td>
                        <td className="text-right">{r.seen}</td>
                        <td className="text-right">{f1(r.minDist)}</td>
                        <td className="text-right">{rel(r.zAtMin, spec.cellZ)}</td>
                        <td className="text-right">{f1(r.lastBeadZ)}</td>
                        <td className="text-right">{rel(r.lastBeadZ, spec.targetZ)}</td>
                        <td className="text-right">{rel(r.edgeZ, spec.cellZ)}</td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            </div>
          )}
          <ul className="mt-2 list-disc space-y-0.5 pl-4 text-2xs text-content-muted">
            <li>
              PLC FB_MeasureTireLaser 와 같은 규칙: 본 지름 {'<'} 외경인 샘플 중 거리 ≤ 최소 거리 + 여유, 본 지름 {'<'} (내경 + 외경) / 2
              이면 비드입니다.
            </li>
            <li>높이는 셀 바닥 기준이고 기록 시작 때 PARA 의 센서 Z 오프셋을 더했습니다. 아래 PLC 진단 이력의 경계 높이와 비교하세요.</li>
            <li>기록 주기({meta.rate_ms} ms)가 PLC 스캔보다 길어 높이는 Z 속도 × 주기만큼 차이 날 수 있습니다.</li>
          </ul>
        </Card>
      </div>

      <div className="grid gap-3 xl:grid-cols-2">
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">Z 축 (세로선 = 표시)</h3>
          <XYChart series={series.z} rules={markRules} xLabel="시간 s" yLabel="mm" height={220} />
        </Card>
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">G 축</h3>
          <XYChart series={series.g} rules={markRules} xLabel="시간 s" yLabel="mm" height={220} />
        </Card>
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">G 토크</h3>
          <XYChart series={series.torque} rules={markRules} xLabel="시간 s" yLabel="%" height={220} />
        </Card>
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">레이저 거리 (유효값만)</h3>
          <XYChart series={series.dist} rules={markRules} xLabel="시간 s" yLabel="mm" height={220} />
        </Card>
      </div>

      <div className="grid gap-3 xl:grid-cols-2">
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">그리퍼 상태 변화 ({edges.length})</h3>
          {edges.length === 0 ? (
            <div className="text-xs text-content-muted">변화 없음</div>
          ) : (
            <div className="max-h-96 overflow-auto">
              <table className="w-full text-xs">
                <thead>
                  <tr className="text-content-muted">
                    <th className="py-1 text-right">시간 s</th>
                    <th className="pl-2 text-left">상태</th>
                    <th className="text-left">변화</th>
                    <th className="text-right">Z</th>
                    <th className="text-right">G</th>
                    <th className="text-right">G 목표</th>
                    <th className="text-right">토크 %</th>
                  </tr>
                </thead>
                <tbody>
                  {edges.slice(0, EDGE_ROWS).map((e, i) => {
                    const tone = e.bit === 'StallNoTorque' ? 'fault' : e.bit === 'Stall' ? 'warn' : 'info'
                    return (
                      <tr key={i} className="border-t border-line-default font-mono tabular-nums">
                        <td className="py-1 text-right">{sec(e.t)}</td>
                        <td className="pl-2 font-sans">{STATE_LABEL[e.bit] ?? e.bit}</td>
                        <td className={`font-sans ${e.on ? statusTone(tone).text : 'text-content-muted'}`}>
                          {e.on ? '켜짐' : '꺼짐'}
                        </td>
                        <td className="text-right">{f1(e.z)}</td>
                        <td className="text-right">{f1(e.g)}</td>
                        <td className="text-right">{f1(e.gTarget)}</td>
                        <td className="text-right">{f1(e.torque)}</td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
              {edges.length > EDGE_ROWS ? (
                <div className="mt-1 text-2xs text-content-muted">앞 {EDGE_ROWS} 건만 표시합니다. 구간을 좁히세요.</div>
              ) : null}
            </div>
          )}
        </Card>
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">
            G 멈춤 구간 ({stalls.length}, 허용 범위 p942 = {window ?? '-'} mm)
          </h3>
          {stalls.length === 0 ? (
            <div className="text-xs text-content-muted">멈춤 없음</div>
          ) : (
            <div className="max-h-96 overflow-auto">
              <table className="w-full text-xs">
                <thead>
                  <tr className="text-content-muted">
                    <th className="py-1 text-right">시작 s</th>
                    <th className="text-right">길이 s</th>
                    <th className="text-right">G</th>
                    <th className="text-right">목표 − G</th>
                    <th className="text-right">토크 %</th>
                    <th className="pl-2 text-left">판정</th>
                    <th className="text-left">알람</th>
                  </tr>
                </thead>
                <tbody>
                  {stalls.map((e) => {
                    const short = e.gTarget - e.g
                    const inWindow = window !== undefined && window > 0 ? Math.abs(short) < window : null
                    const verdict = !e.torqueReached
                      ? { text: '토크 없이 멈춤', tone: 'fault' }
                      : inWindow === false
                        ? { text: '범위 밖 멈춤', tone: 'fault' }
                        : { text: inWindow ? '토크 파지' : '토크 멈춤', tone: 'ok' }
                    return (
                      <tr key={e.t0} className="border-t border-line-default font-mono tabular-nums">
                        <td className="py-1 text-right">{sec(e.t0)}</td>
                        <td className="text-right">{sec(e.t1 - e.t0)}</td>
                        <td className="text-right">{f1(e.g)}</td>
                        <td className="text-right">{f1(short)}</td>
                        <td className="text-right">{f1(e.torque)}</td>
                        <td className={`pl-2 font-sans ${statusTone(verdict.tone).text}`}>
                          {verdict.text}
                          {e.itemDetect ? ' · 아이템 감지' : ''}
                        </td>
                        <td className="font-sans">
                          {e.alarms.length ? e.alarms.map((c) => `${c} ${alarmLabel(c) ?? ''}`.trim()).join(', ') : '-'}
                        </td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            </div>
          )}
          <p className="mt-2 text-2xs text-content-muted">
            GripperJudge: 토크 멈춤이 허용 범위 안이면 토크 파지(정상), 범위 밖 멈춤은 4025 입니다. 판정은 이동 방향을 보지 않는 근사라
            목표를 넘어간 멈춤도 범위 밖으로 보일 수 있습니다.
          </p>
        </Card>
      </div>

      <PlcRecords meta={meta} nDir={nDir} />

      <div className="grid gap-3 xl:grid-cols-3">
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">PARA.Sensor (시작 → 끝)</h3>
          <ParaDiff a={meta.start?.para_sensor} b={meta.end?.para_sensor} />
        </Card>
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">PARA.Task (시작 → 끝)</h3>
          <ParaDiff a={meta.start?.para_task} b={meta.end?.para_task} />
        </Card>
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">Z 오프셋 교정 (시작 → 끝)</h3>
          <ZCalDiff a={meta.start} b={meta.end} nDir={nDir} />
        </Card>
      </div>

      <Card>
        <h3 className="mb-1 text-xs font-semibold text-content-muted">표시 ({marks.length})</h3>
        {marks.length === 0 ? (
          <div className="text-xs text-content-muted">표시 없음</div>
        ) : (
          <table className="w-full text-xs">
            <tbody>
              {marks.map((x, i) => (
                <tr key={i} className="border-t border-line-default">
                  <td className="w-20 whitespace-nowrap py-1 text-right font-mono tabular-nums">{sec(x.t)} s</td>
                  <td className="w-56 whitespace-nowrap pl-4 font-mono tabular-nums text-content-muted">{clock(x.at)}</td>
                  <td className="pl-4">{x.mark}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>
    </div>
  )
}

function PlcRecords({ meta, nDir }: { meta: RecordMeta; nDir: number }) {
  const dirs = LASER_DIRS.slice(0, nDir)
  const laser = meta.end?.laser_entries ?? []
  const meas = meta.end?.measlog_last ?? []
  const pending = <div className="text-xs text-content-muted">기록을 정지하면 채워집니다.</div>
  return (
    <div className="grid gap-3 xl:grid-cols-2">
      <Card>
        <h3 className="mb-1 text-xs font-semibold text-content-muted">
          PLC 레이저 진단 이력 (기록 중 새로 {laser.length} 건)
        </h3>
        {!meta.end ? (
          pending
        ) : laser.length === 0 ? (
          <div className="text-xs text-content-muted">새 기록 없음</div>
        ) : (
          <div className="overflow-auto">
            <table className="w-full text-xs">
              <thead>
                <tr className="text-content-muted">
                  <th className="py-1 text-right">Seq</th>
                  <th className="pl-2 text-left">측정</th>
                  <th className="text-right">Code</th>
                  <th className="text-center">원 맞춤</th>
                  {dirs.map((d) => (
                    <th key={`dev-${d}`} className="text-right">
                      편차 {d}
                    </th>
                  ))}
                  {dirs.map((d) => (
                    <th key={`edge-${d}`} className="text-right">
                      경계 {d}
                    </th>
                  ))}
                  <th className="text-right">편차 폭</th>
                  <th className="pl-2 text-left">플래그</th>
                </tr>
              </thead>
              <tbody>
                {laser.map((e) => (
                  <tr key={e.Seq} className="border-t border-line-default font-mono tabular-nums">
                    <td className="py-1 text-right">{e.Seq}</td>
                    <td className="pl-2 font-sans">{LASER_SOURCE[e.Source] ?? e.Source}</td>
                    <td className="text-right">{e.Code || '-'}</td>
                    <td className="text-center font-sans">{e.Valid ? 'OK' : `실패 ${e.FitError}`}</td>
                    {dirs.map((d, k) => (
                      <td key={`dev-${d}`} className="text-right">
                        {f2(e.BeadDev?.[k])}
                      </td>
                    ))}
                    {dirs.map((d, k) => (
                      <td key={`edge-${d}`} className="text-right">
                        {f1(e.EdgeHeight?.[k])}
                      </td>
                    ))}
                    <td className="text-right">{f2(e.Spread)}</td>
                    <td className="pl-2 font-sans">{diagFlagLabels(e.DiagFlags).join(', ') || '-'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
      <Card>
        <h3 className="mb-1 text-xs font-semibold text-content-muted">
          PLC 측정 이력 MEASLOG (종류별 마지막 1 건, 모든 회차는 이력 탭)
        </h3>
        {!meta.end ? (
          pending
        ) : meas.length === 0 ? (
          <div className="text-xs text-content-muted">새 기록 없음</div>
        ) : (
          <div className="overflow-auto">
            <table className="w-full text-xs">
              <thead>
                <tr className="whitespace-nowrap text-content-muted">
                  <th className="py-1 text-right">Seq</th>
                  <th className="pl-3 text-left">종류</th>
                  <th className="pl-3 text-left">상태</th>
                  <th className="pl-3 text-right">Code</th>
                  <th className="pl-3 text-right">규격 내경</th>
                  <th className="pl-3 text-right">Δ 내경</th>
                  <th className="pl-3 text-right">Δ Z</th>
                </tr>
              </thead>
              <tbody>
                {meas.map((e) => (
                  <Fragment key={e.Seq}>
                    <tr className="whitespace-nowrap border-t border-line-default font-mono tabular-nums">
                      <td className="py-1 text-right">{e.Seq}</td>
                      <td className="pl-3 font-sans">{KIND[e.Kind] ?? e.Kind}</td>
                      <td className="pl-3 font-sans">{STATUS[e.Status] ?? e.Status}</td>
                      <td className="pl-3 text-right">{e.Cmd?.Item?.Code ?? '-'}</td>
                      <td className="pl-3 text-right">{f1(e.Cmd?.Item?.InnerDiameter)}</td>
                      <td className="pl-3 text-right">{f2(e.Delta?.InnerDia)}</td>
                      <td className="pl-3 text-right">{f2(e.Delta?.Z)}</td>
                    </tr>
                    {e.Kind === 1 ? (
                      <tr>
                        <td />
                        <td colSpan={6} className="pb-1 pl-3">
                          <span className="flex flex-wrap gap-x-4 gap-y-0.5 font-mono tabular-nums">
                            {ITEM_DATA.map(([i, l]) => (
                              <span key={i} className="whitespace-nowrap">
                                <span className="font-sans text-content-muted">{l}</span> {f1(e.Data?.[i])}
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
      </Card>
    </div>
  )
}

function ParaDiff({ a, b }: { a: Record<string, number> | undefined; b: Record<string, number> | undefined }) {
  const keys = [...new Set([...Object.keys(a ?? {}), ...Object.keys(b ?? {})])]
  if (keys.length === 0) return <div className="text-xs text-content-muted">스냅샷 없음</div>
  return (
    <table className="w-full text-xs">
      <thead>
        <tr className="text-content-muted">
          <th className="py-1 text-left">멤버</th>
          <th className="text-right">시작</th>
          <th className="text-right">끝</th>
        </tr>
      </thead>
      <tbody>
        {keys.map((k) => {
          const x = a?.[k]
          const y = b?.[k]
          const changed = x !== undefined && y !== undefined && x !== y
          return (
            <tr key={k} className="border-t border-line-default font-mono tabular-nums">
              <td className="py-1 font-sans">{k}</td>
              <td className="text-right">{num(x)}</td>
              <td className={`text-right ${changed ? statusTone('warn').text : ''}`}>{num(y)}</td>
            </tr>
          )
        })}
      </tbody>
    </table>
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
  if (!za && !zb) return <div className="text-xs text-content-muted">LASERDIAG 스냅샷 없음</div>
  return (
    <KvTable
      rows={[
        ['상태', `${zcalState(za)} → ${zcalState(zb)}`],
        ['측정 수 (건너뜀)', `${za?.Count ?? '-'} → ${zb?.Count ?? '-'} (${zb?.Skipped ?? '-'})`],
        ...LASER_DIRS.slice(0, nDir).map((d, k): [string, string] => [
          `${d} 평균 / 표준편차 / 새 오프셋`,
          `${f2(zb?.Mean?.[k])} / ${f2(zb?.StdDev?.[k])} / ${f2(zb?.NewOffset?.[k])}`,
        ]),
        ['LASERDIAG 누적', `${a?.laser?.Total ?? '-'} → ${b?.laser?.Total ?? '-'}`],
      ]}
    />
  )
}
