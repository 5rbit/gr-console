// 레이저 센서 탭 — 센서별 상태(LASERDIAG.Sensor), Z 오프셋 자동 교정(ZCal), 측정별 진단 이력.
//
// 데이터는 GET /api/laser?robot= (2 초 폴링, 숨은 탭 스킵) — 사이드바에서 고른 로봇 PLC 것. 교정 시작·취소·초기화는
// 그 PLC LASERDIAG 의 Bool 하나를 쓰고, 나머지는 PLC(UL_LaserDiag)가 한다. LASERDIAG 가 없는 PLC(GR1)는 백엔드 사유를 그대로 보인다.
//
// 자리 정리(2026-09-18): **주 조작은 띠에 하나**(교정 시작/취소)뿐이고, 기준값 표(PARA.Sensor 열한 줄)와
// 초기화는 넘침 메뉴 뒤로 갔다. 기준값은 교정 전에 한 번 확인하는 값이라 상시로 카드 하나를 먹을 이유가
// 없었고, 안내문 다섯 줄은 `?` 도움말과 확인 대화상자로 옮겼다(읽어야 할 순간에 그 자리에 있게).
//
// 2차 정리(같은 날): 남아 있던 문장 자리를 마저 걷었다 — ① `반영 규칙` 묶음(값이 문장 셋이던 표)은
// ZCal 제목의 `?` 로, ② 확인 대화상자의 두 문단은 **질문 한 줄 + 라벨+값 짝**으로, 절차(타이어를
// 돌려 놓기)는 `교정 시작` 옆 `?` 로 갔다. 표 셋은 전부 `DataTable`(정렬·좁은 폭 열 접기·빈 상태)이고,
// 이력 표 높이는 `max-h-[46vh]` 대신 **남는 높이**다(도킹 존에서 뷰포트 비율은 늘 어긋난다).
import { useCallback, useEffect, useState } from 'react'
import { api } from '../../lib/api'
import { robots } from '../../lib/robots'
import { robotChip, robotFailure, withRobotChip } from '../../lib/robotContext'
import { robotField } from '../shared/RobotChip'
import { useStore } from '../../lib/store'
import { ALARM_LABEL } from '../../lib/gr/alarms'
import {
  LASER_DIRS,
  LASER_SOURCE,
  ZCAL_ERROR,
  activeDirs,
  diagFlagLabels,
  zcalPhase,
} from '../../lib/laser'
import { delta, dtl, f2 } from '../../lib/meas/format'
import { visibleInterval } from '../../lib/poll'
import type { LaserDiagEntry as LaserEntry, LaserSnapshot } from '../../lib/types'
import { Button } from '../../lib/ui/Button'
import { Card } from '../../lib/ui/Card'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { Dialog } from '../../lib/ui/Dialog'
import { FieldList } from '../../lib/ui/FieldList'
import { EmptyState } from '../../lib/ui/EmptyState'
import { HelpTip } from '../../lib/ui/HelpTip'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import { Toolbar } from '../../lib/ui/Toolbar'
import { statusTone } from '../../lib/ui/status'
import { toast } from '../../lib/ui/toast'
import type { Column } from '../../lib/ui/table'
import type { MenuItem } from '../../lib/ui/menu'
import { KvTable } from './helpers'

const POLL_MS = 2000
const OFFSET_KEYS = ['GIDL_ZOffset', 'GIDF_ZOffset', 'GIDR_ZOffset', 'GIDB_ZOffset'] as const

/** PARA.Sensor 기준값 — 0 이면 PLC 가 괄호 안 기본값을 쓴다. */
const PARA_ROWS: readonly (readonly [string, string])[] = [
  ['LaserDiag_BiasLimit', '높이 편차 알람 기준 mm (p422, 기본 5)'],
  ['LaserDiag_BiasMinSamples', '높이 편차 판정 최소 측정 수 (p423, 기본 20)'],
  ['LaserDiag_BiasAlpha', '편차 이동평균 계수 (p424, 기본 0.1)'],
  ['LaserDiag_ConsecLimit', '무응답·불안정 연속 기준 (p408, 기본 3)'],
  ['LaserDiag_JumpDist', '거리 급변 기준 mm (p406, 기본 20)'],
  ['LaserDiag_JumpCountLimit', '측정당 급변 횟수 기준 (p407, 기본 5)'],
  ['LaserDiag_SpreadLimit', '편차 폭 플래그 기준 mm (p404, 기본 8)'],
  ['LaserDiag_PlanarLimit', 'TBR 비평면 플래그 기준 mm (p405, 기본 4)'],
  ['LaserZCal_SampleCount', '교정 측정 수 (p425, 기본 5)'],
  ['LaserZCal_MaxStdDev', '교정 표준편차 허용 mm (p426, 기본 2)'],
  ['LaserZCal_MaxCorrection', '교정 보정량 허용 mm (p427, 기본 20)'],
]

const BIAS_HELP =
  '편차 평균이 같은 쪽으로 계속 크면 설치 높이 오차입니다. 타이어 기울기·변형은 타이어마다 방향이 달라 평균에서 줄어듭니다.'

/** 교정을 시작하기 전에 한 번 읽는 절차 — 버튼 옆 `?` 뒤에 둔다(상시 문단이 아니라). */
const START_HELP =
  '측정 사이에 타이어를 돌려 놓거나 다른 타이어를 쓰세요 (PCR 3방향은 필수). 표본이 다 모이면 PARA 센서 Z 오프셋을 보정하고 CSV 저장을 요청합니다.'

/** 교정 결과를 PARA 에 반영하는 규칙 — 표 셋으로 풀어 쓰던 것. */
function zcalRule(maxStd: number, maxCorr: number): string {
  return `반영하는 측정: MeasureItem 들어갈 때·나갈 때, 수동 측정 (PICK 하강 제외). 건너뛰는 경우: StdDev > ${maxStd} mm 또는 보정량 > ${maxCorr} mm. 맞추는 것: 센서끼리의 상대 높이만 — 오프셋 평균은 그대로입니다.`
}

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

/** 표 한 줄 = 센서 방향 하나(L·F·R·B). 값은 방향 인덱스로 각 배열에서 꺼낸다. */
interface DirRow {
  dir: string
  k: number
}

/** 그 방향에 켜진 알람 코드 — 편차 4201+ · 무응답 4205+ · 불안정 4209+. */
function alarmsOf(data: LaserSnapshot, k: number): number[] {
  const s = data.sensor?.[k]
  return [
    s?.BiasAlarm ? 4201 + k : 0,
    s?.NoRespAlarm ? 4205 + k : 0,
    s?.UnstableAlarm ? 4209 + k : 0,
  ].filter((c) => c)
}

export function LaserSensor() {
  useStore(robots)
  const robot = robots.selected
  const [data, setData] = useState<LaserSnapshot | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [confirm, setConfirm] = useState<'start' | 'reset' | null>(null)
  const [paraOpen, setParaOpen] = useState(false)

  const load = useCallback(async () => {
    try {
      const d = await api.laser(robot)
      // 로봇을 바꾼 직후 늦게 온 옛 로봇 응답은 버린다 — 칩(확인 창)이 옛 로봇을 말하는데 쓰기는 새 로봇으로 가던 경합.
      if (robot !== null && d.robot != null && d.robot !== robot) return
      setData(d)
      setError(null)
    } catch (e) {
      setError(errText(e))
    }
  }, [robot])

  useEffect(() => {
    // 다른 로봇으로 바뀌면 옛 로봇 값을 들고 있지 않는다.
    setData(null)
    setError(null)
    void load()
    const t = visibleInterval(() => void load(), POLL_MS)
    return () => clearInterval(t)
  }, [load])

  // 교정·초기화는 **이 로봇의** 센서를 바꾼다 — 응답이 말한 로봇·PLC 가 있으면 그것, 없으면 선택.
  const chip = robotChip(robots.byId(data?.robot ?? robot), {
    name: data?.robot ? robots.nameOf(data.robot) : null,
    plc: data?.plc ?? null,
  })

  async function run(label: string, fn: () => Promise<unknown>) {
    setBusy(true)
    try {
      await fn()
      toast.ok(withRobotChip(chip, label))
      await load()
    } catch (e) {
      toast.error(robotFailure(chip.name, `${label} 실패`, errText(e)))
    } finally {
      setBusy(false)
    }
  }

  if (!data)
    return (
      <EmptyState
        title={`${robots.current?.name ?? ''} 레이저 진단 없음`.trim()}
        hint={error ?? 'LASERDIAG 를 읽는 중입니다.'}
      />
    )

  const z = data.zcal
  const phase = zcalPhase(z)
  const nDir = activeDirs(data.entries ?? [])
  const dirs = LASER_DIRS.slice(0, nDir)
  const para = data.para ?? {}
  const entries = data.entries ?? []
  const phaseText =
    phase === 'busy'
      ? `수집 중 ${z?.Count ?? 0} / ${z?.Target ?? '-'}`
      : phase === 'done'
        ? '완료 (PARA 반영, CSV 저장 요청)'
        : phase === 'error'
          ? `실패 : ${ZCAL_ERROR[z?.ErrorCode ?? 0] ?? z?.ErrorCode}`
          : '대기'

  const menu: MenuItem[] = [
    { label: '기준값 (PARA.Sensor)…', run: () => setParaOpen(true) },
    {
      label: '상태·이력 초기화…',
      danger: true,
      disabled: busy ? '진행 중입니다' : undefined,
      run: () => setConfirm('reset'),
    },
  ]

  /** 값이 없으면 `null` — 빈 문자열로 두면 표가 빈 칸을 그려 "0 인가"로 읽힌다. */
  const mm = (v: number | null | undefined) => (v === null || v === undefined ? null : delta(v))
  const dirRows: DirRow[] = dirs.map((d, k) => ({ dir: d, k }))
  const sensorCols: Column<DirRow>[] = [
    { key: 'dir', label: 'Dir', get: (r) => r.dir, class: 'font-semibold', priority: 1 },
    {
      key: 'off',
      label: 'GIDx_ZOffset (mm)',
      get: (r) => mm(para[OFFSET_KEYS[r.k] ?? '']),
      numeric: true,
      priority: 1,
    },
    {
      key: 'bias',
      label: 'BiasEma (mm)',
      get: (r) => mm(data.sensor?.[r.k]?.BiasEma),
      numeric: true,
      priority: 1,
    },
    {
      key: 'n',
      label: 'BiasSamples',
      get: (r) => data.sensor?.[r.k]?.BiasSamples ?? null,
      numeric: true,
      priority: 2,
    },
    {
      key: 'dev',
      label: 'LastDev (mm)',
      get: (r) => mm(data.sensor?.[r.k]?.LastDev),
      numeric: true,
      priority: 2,
    },
    {
      key: 'nr',
      label: 'NoRespConsec',
      get: (r) => data.sensor?.[r.k]?.NoRespConsec ?? null,
      numeric: true,
      priority: 3,
    },
    {
      key: 'un',
      label: 'UnstableConsec',
      get: (r) => data.sensor?.[r.k]?.UnstableConsec ?? null,
      numeric: true,
      priority: 3,
    },
    {
      key: 'al',
      label: 'Alarms',
      get: (r) => alarmsOf(data, r.k).join(' · ') || null,
      cell: (r) => {
        const codes = alarmsOf(data, r.k)
        if (!codes.length) return <span className="text-content-muted">-</span>
        return (
          <span className={`tabular-nums ${statusTone('warn').text}`}>
            {codes.map((c, i) => (
              <span key={c} title={ALARM_LABEL[c]}>
                {i > 0 ? ' · ' : ''}
                {c}
              </span>
            ))}
          </span>
        )
      },
      priority: 1,
    },
  ]
  const zcalCols: Column<DirRow>[] = [
    { key: 'dir', label: 'Dir', get: (r) => r.dir, class: 'font-semibold', priority: 1 },
    { key: 'mean', label: 'Mean (mm)', get: (r) => mm(z?.Mean?.[r.k]), numeric: true, priority: 1 },
    {
      key: 'sd',
      label: 'StdDev (mm)',
      get: (r) => mm(z?.StdDev?.[r.k]),
      numeric: true,
      priority: 1,
    },
    {
      key: 'old',
      label: 'OldOffset (mm)',
      get: (r) => mm(z?.OldOffset?.[r.k]),
      numeric: true,
      priority: 2,
    },
    {
      key: 'new',
      label: 'NewOffset (mm)',
      get: (r) => mm(z?.NewOffset?.[r.k]),
      numeric: true,
      priority: 1,
    },
  ]
  // 진단 이력 — 행을 알아보는 Seq·시각·판정이 1, 방향별 편차가 2, 나머지가 3이다.
  const entryCols: Column<LaserEntry>[] = [
    { key: 'seq', label: 'Seq', get: (e) => e.Seq, numeric: true, priority: 1 },
    { key: 'ts', label: 'TimeStamp', get: (e) => dtl(e.TimeStamp), priority: 1 },
    { key: 'src', label: 'Source', get: (e) => LASER_SOURCE[e.Source] ?? e.Source, priority: 2 },
    { key: 'code', label: 'Code', get: (e) => e.Code || null, numeric: true, priority: 2 },
    {
      key: 'valid',
      label: 'Valid / FitError',
      get: (e) => (e.Valid ? 'OK' : `실패 ${e.FitError}`),
      priority: 1,
    },
    ...dirs.map((d, k): Column<LaserEntry> => ({
      key: `dev${k}`,
      label: `BeadDev[${d}] (mm)`,
      get: (e) => mm(e.BeadDev?.[k]),
      numeric: true,
      priority: 2,
    })),
    { key: 'spread', label: 'Spread (mm)', get: (e) => mm(e.Spread), numeric: true, priority: 3 },
    ...(nDir === 4
      ? [
          {
            key: 'planar',
            label: 'PlanarResidual (mm)',
            get: (e: LaserEntry) => mm(e.PlanarResidual),
            numeric: true,
            priority: 3 as const,
          },
        ]
      : []),
    {
      key: 'flags',
      label: 'DiagFlags',
      get: (e) => diagFlagLabels(e.DiagFlags).join(', ') || null,
      priority: 3,
    },
  ]

  return (
    <div className="flex h-full min-h-0 flex-col gap-3" data-testid="measure-laser">
      <Toolbar
        title="Z 오프셋 자동 교정"
        dense
        meta={
          <span data-testid="laser-robot">
            로봇 {robots.nameOf(data.robot ?? robot)} · PLC {data.plc} · 읽은 시각 {dtl(data.at)}
            {error ? <span className={statusTone('fault').text}> · {error}</span> : null}
          </span>
        }
      >
        <StatusBadge
          status={
            phase === 'busy'
              ? 'info'
              : phase === 'done'
                ? 'ok'
                : phase === 'error'
                  ? 'fault'
                  : 'neutral'
          }
        >
          {phaseText}
        </StatusBadge>
        {phase === 'busy' ? (
          <Button
            size="sm"
            intent="danger"
            loading={busy}
            onClick={() => void run('Z 오프셋 교정 취소', () => api.laserZCal(false, robot))}
          >
            교정 취소
          </Button>
        ) : (
          <>
            <Button size="sm" intent="primary" loading={busy} onClick={() => setConfirm('start')}>
              교정 시작
            </Button>
            {/* 시작 전에 한 번 읽는 절차 — 상시 문단이 아니라 누르는 손 옆의 `?` 로. */}
            <HelpTip title="교정 시작" text={START_HELP} />
          </>
        )}
        <OverflowMenu items={menu} testid="laser-more" />
      </Toolbar>

      <div className="grid flex-none gap-3 lg:grid-cols-2">
        <Card padded={false}>
          <div className="flex items-baseline gap-2 border-b border-line-default px-3 py-1.5">
            <h3 className="text-xs font-semibold text-content-muted">
              센서별 상태 (LASERDIAG.Sensor, {nDir}방향)
            </h3>
            <HelpTip title="BiasEma" text={BIAS_HELP} />
          </div>
          <DataTable
            rows={dirRows}
            columns={sensorCols}
            rowKey={(r) => r.dir}
            density="compact"
            empty="센서 값 없음"
          />
        </Card>

        <Card padded={false}>
          <div className="flex items-baseline gap-2 border-b border-line-default px-3 py-1.5">
            <h3 className="text-xs font-semibold text-content-muted">ZCal 값 (LASERDIAG.ZCal)</h3>
            {/* 반영 규칙 표(문장 셋)가 서 있던 자리 — 교정 결과를 볼 때만 읽는 글이다. */}
            <HelpTip
              title="PARA 반영 규칙"
              text={zcalRule(para.LaserZCal_MaxStdDev || 2, para.LaserZCal_MaxCorrection || 20)}
            />
            <span className="ml-auto text-2xs text-content-faint tabular-nums">
              Count {z?.Count ?? 0} / {z?.Target ?? '-'} (Skipped {z?.Skipped ?? 0})
            </span>
          </div>
          <DataTable
            rows={dirRows}
            columns={zcalCols}
            rowKey={(r) => r.dir}
            density="compact"
            empty="교정 값 없음"
          />
        </Card>
      </div>

      <Card padded={false} className="flex min-h-0 flex-1 flex-col">
        <div className="flex flex-none items-baseline gap-2 border-b border-line-default px-3 py-1.5">
          <h3 className="text-xs font-semibold text-content-muted">측정별 진단 이력</h3>
          <span className="ml-auto text-2xs text-content-faint tabular-nums">
            최근 {entries.length} / 누적 {data.total ?? 0}
          </span>
        </div>
        <DataTable
          rows={entries}
          columns={entryCols}
          rowKey={(e) => String(e.Seq)}
          density="compact"
          stickyHeader
          zebra
          className="min-h-0 flex-1 overflow-auto"
          empty="진단 기록 없음"
          emptyHint="타이어를 측정하면 회차마다 한 줄씩 쌓입니다."
        />
      </Card>

      {/* 기준값 — 교정 전에 한 번 보는 값이라 대화상자로 옮겼다(카드 하나가 상시로 서 있을 이유가 없다). */}
      <Dialog
        open={paraOpen}
        onOpenChange={setParaOpen}
        title="기준값 (PARA.Sensor)"
        meta="0 = PLC 기본값"
        size="md"
        testid="laser-para"
      >
        {/* 설명(p번호·기본값)은 라벨 아래 두 번째 줄이 아니라 **툴팁**으로 — 값 열이 한 줄에 선다. */}
        <KvTable
          labelWidth={196}
          rows={PARA_ROWS.map(([key, desc]) => [key, f2(para[key]), desc] as const)}
        />
      </Dialog>

      <ConfirmDialog
        open={confirm !== null}
        onOpenChange={(o) => !o && setConfirm(null)}
        scope="single-robot"
        title={`${confirm === 'reset' ? '레이저 진단 초기화' : 'Z 오프셋 교정 시작'} — ${chip.name}`}
        danger={confirm === 'reset'}
        confirmLabel={confirm === 'reset' ? '초기화' : '교정 시작'}
        onConfirm={() => {
          const c = confirm
          setConfirm(null)
          if (c === 'reset') void run('레이저 진단 초기화', () => api.laserReset(robot))
          else if (c === 'start') void run('Z 오프셋 교정 시작', () => api.laserZCal(true, robot))
        }}
      >
        {/* 질문 한 줄 + 대상을 알아볼 라벨+값 짝. 절차 안내(타이어 돌리기)는 시작 버튼 옆 `?` 에 있다. */}
        <p className="m-0 text-content-primary">
          {confirm === 'reset'
            ? '레이저 진단 상태와 이력을 정말로 지우시겠습니까? 되돌릴 수 없습니다.'
            : 'Z 오프셋 자동 교정을 시작하시겠습니까?'}
        </p>
        <FieldList
          className="mt-3"
          columns={2}
          dense
          labelWidth={96}
          items={
            confirm === 'reset'
              ? [
                  robotField(chip, '대상'),
                  { label: '쓰는 때', value: '센서 교체 · 재취부 후' },
                  { label: '지우는 것', value: '편차 평균 · 연속 횟수 · 진단 이력', wide: true },
                ]
              : [
                  robotField(chip, '대상'),
                  { label: '표본', value: `${para.LaserZCal_SampleCount || 5} 회` },
                  { label: '반영', value: 'PARA 센서 Z 오프셋 · CSV 저장 요청', wide: true },
                ]
          }
        />
      </ConfirmDialog>
    </div>
  )
}
