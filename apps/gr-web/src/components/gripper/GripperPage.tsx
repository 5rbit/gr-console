// 그리퍼 — GR2 `FB_Gripper` 토크 제어의 상태·학습 곡선·LEARN 명령 화면(사이드바에서 고른 로봇).
//
// 데이터 두 갈래:
//  - 실시간(`WEBMON.Gripper` + G 축 위치)은 이미 열려 있는 **상태 스트림**(`useSelectedStatus`)에서 — 폴을 하나 더
//    열지 않는다. 화면 머리의 숫자 띠·상태 카드·곡선 위 G 마커가 이것을 쓴다.
//  - 학습 곡선(`GRIP_TUNE.Tune`)·PARA 는 `GET /api/robots/{id}/gripper` 2 초 폴(숨은 탭 스킵). 느린 주기 DB 라 그걸로 충분하다.
// 조작은 띠 하나(LEARN 버튼 + `?` + `⋯`)뿐이고, ScaleByInch 편집은 `⋯` 뒤의 대화상자다.
import { useCallback, useEffect, useMemo, useState } from 'react'
import { Hand } from 'lucide-react'
import { api } from '../../lib/api'
import { useSelectedStatus } from '../../lib/feeds'
import { modeName } from '../../lib/gr/const'
import { delta, dtl, pos } from '../../lib/meas/format'
import { visibleInterval } from '../../lib/poll'
import { robots } from '../../lib/robots'
import { robotChip, robotFailure, withRobotChip } from '../../lib/robotContext'
import { sendRobotAction } from '../../lib/robotCommand'
import { useStore } from '../../lib/store'
import type { GripperSnapshot, WebMonGripper } from '../../lib/types'
import { Button } from '../../lib/ui/Button'
import { Card } from '../../lib/ui/Card'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { DataTable } from '../../lib/ui/DataTable'
import { FormDialog } from '../../lib/ui/Dialog'
import { EmptyState } from '../../lib/ui/EmptyState'
import { ErrorBoundary } from '../../lib/ui/ErrorBoundary'
import { FieldList } from '../../lib/ui/FieldList'
import { HelpTip } from '../../lib/ui/HelpTip'
import { Input } from '../../lib/ui/Input'
import { OverflowMenu } from '../../lib/ui/OverflowMenu'
import { ScreenHeader } from '../../lib/ui/ScreenHeader'
import { Section } from '../../lib/ui/Section'
import { StatRow, type StatItem } from '../../lib/ui/StatRow'
import { Toolbar } from '../../lib/ui/Toolbar'
import type { MenuItem } from '../../lib/ui/menu'
import { statusTone } from '../../lib/ui/status'
import type { Column } from '../../lib/ui/table'
import { toast } from '../../lib/ui/toast'
import { XYChart } from '../../lib/ui/viz/XYChart'
import { robotField } from '../shared/RobotChip'
import { Bits, KvTable } from '../measure/helpers'
import {
  CURVE_LABEL,
  LEARN_ERROR,
  anyValid,
  codeName,
  codeTone,
  errorName,
  gRule,
  learnDisabledReason,
  mechSeries,
  modeLabel,
  paraRows,
  parseScale,
  scaleRows,
  type ParaRowView,
  type ScaleRow,
} from './GripperPageModel'

const POLL_MS = 2000
const CHART_H = 260

const LEARN_HELP =
  'G 축을 RangeMin → RangeMax 로 한 번 쓸며 타이어 없이 드는 기구 부하(링크 · 자중 · 마찰)를 34 칸에 기록합니다. PLC 는 수동/정비 모드 · 그리퍼 Ready · 화물 없음일 때만 받고, 아니면 조용히 무시합니다. 결과는 아래 LearnDone / LearnError 로 옵니다.'

const CURVE_HELP =
  'x = G 위치(mm, RangeMin~RangeMax 를 34 칸), y = 기구 부하 토크(%). 파지 속도 곡선과 측정 느린 속도 곡선이 따로 있고 LearnedSpd 가 지금 속도 등급과 다르면 그 곡선은 무효입니다. 세로선이 지금 G 위치.'

const SCALE_HELP = '인치별 미세 조정(%). 0 또는 100 = 조정 없음. 저장하면 GRIP_TUNE.Tune.ScaleByInch 에 바로 씁니다.'

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

const pct = (v: number | null | undefined) => (v === null || v === undefined ? null : delta(v))

const SCALE_COLS: Column<ScaleRow>[] = [
  { key: 'inch', label: 'Inch', get: (r) => r.inch, numeric: true },
  { key: 'v', label: 'ScaleByInch (%)', get: (r) => delta(r.value), numeric: true },
]

const PARA_COLS: Column<ParaRowView>[] = [
  { key: 'name', label: 'Name', get: (r) => r.name, class: 'font-mono', priority: 1 },
  { key: 'p', label: 'p', get: (r) => r.param, numeric: true, priority: 2 },
  { key: 'grp', label: 'Group', get: (r) => r.group, priority: 3 },
  {
    key: 'v',
    label: 'Value',
    get: (r) => (r.value === null ? null : delta(r.value)),
    numeric: true,
    priority: 1,
  },
]

function GripperScreen() {
  useStore(robots)
  const robot = robots.selected
  const feed = useSelectedStatus()
  const wm = feed.data?.webmon ?? null
  const [snap, setSnap] = useState<GripperSnapshot | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [confirm, setConfirm] = useState(false)
  const [sending, setSending] = useState(false)
  const [scaleOpen, setScaleOpen] = useState(false)
  const [scaleIn, setScaleIn] = useState<string[]>([])
  const [scaleBusy, setScaleBusy] = useState(false)

  const load = useCallback(async () => {
    if (robot === null) return
    try {
      const d = await api.gripper(robot)
      // 로봇을 바꾼 직후 늦게 온 옛 로봇 응답은 버린다.
      if (d.robot !== robot) return
      setSnap(d)
      setError(null)
    } catch (e) {
      setError(errText(e))
    }
  }, [robot])

  useEffect(() => {
    setSnap(null)
    setError(null)
    void load()
    const t = visibleInterval(() => void load(), POLL_MS)
    return () => clearInterval(t)
  }, [load])

  // 실시간은 스트림, 없으면(스트림 전) 폴 응답의 것.
  const live: WebMonGripper | null = wm?.Gripper ?? snap?.live ?? null
  const gPos = wm?.Axis?.[3]?.Position ?? snap?.live.GPos ?? null
  const modeText = wm ? modeName(wm.Mode) : null
  const chip = robotChip(robots.byId(snap?.robot ?? robot), {
    name: snap?.robot_name ?? null,
    plc: snap?.plc ?? null,
    id: snap?.robot ?? robot,
  })
  const tune = snap?.tune ?? null
  const bins = snap?.bins ?? { range_min: 295, range_max: 630, count: 34, width: 335 / 34 }
  const series = useMemo(() => mechSeries(tune, bins), [tune, bins])
  const rules = useMemo(() => gRule(gPos), [gPos])
  const isDemo = feed.data?.source === 'demo'
  const learnWhy = learnDisabledReason(
    modeText,
    live
      ? {
          ItemPresent: live.ItemPresent ?? false,
          ItemDetect: live.ItemDetect,
          Busy: live.Busy ?? false,
          Code: live.Code ?? 0,
        }
      : null,
    robots.current?.cmd_ready ?? false,
    isDemo,
  )

  async function learn() {
    const r = robots.current
    if (!r) return
    setSending(true)
    try {
      await sendRobotAction(r, 'gripper-learn')
      await load()
    } finally {
      setSending(false)
    }
  }

  function openScale() {
    setScaleIn(scaleRows(tune).map((r) => String(r.value)))
    setScaleOpen(true)
  }

  async function saveScale() {
    const parsed = parseScale(scaleIn)
    if ('error' in parsed) {
      toast.error(parsed.error)
      return
    }
    if (robot === null) return
    setScaleBusy(true)
    try {
      await api.gripperScaleByInch(robot, parsed.values)
      toast.ok(withRobotChip(chip, 'ScaleByInch 저장'))
      setScaleOpen(false)
      await load()
    } catch (e) {
      toast.error(robotFailure(chip.name, 'ScaleByInch 저장 실패', errText(e)))
    } finally {
      setScaleBusy(false)
    }
  }

  const menu: MenuItem[] = [
    {
      label: 'ScaleByInch 편집…',
      disabled: tune ? undefined : (snap?.tune_error ?? 'GRIP_TUNE 를 아직 읽지 못했습니다'),
      run: openScale,
      testid: 'gripper-scale-edit',
    },
  ]

  if (!live && !snap) {
    return (
      <EmptyState
        title={`${robots.current?.name ?? ''} 그리퍼 상태 없음`.trim()}
        hint={error ?? feed.error ?? 'WEBMON · GRIP_TUNE 를 읽는 중입니다.'}
        testid="gripper-empty"
      />
    )
  }

  const code = live?.Code
  const stats: StatItem[] = [
    { label: 'Code', value: codeName(code), tone: codeTone(code), testid: 'gripper-code' },
    { label: 'Mode / Step', value: `${modeLabel(live?.Mode)} / ${live?.Step ?? '-'}` },
    { label: 'LimitNow (%)', value: pct(live?.LimitNow) ?? '-', help: '지금 드라이브에 보내는 토크 제한' },
    { label: 'TorqPct (%)', value: pct(live?.TorqPct) ?? '-', help: '적용 중인 실측 토크' },
    { label: 'Mech (%)', value: pct(live?.Mech) ?? '-', help: '이 위치의 기구 부하(곡선값)' },
    { label: 'Rise (%)', value: pct(live?.Rise) ?? '-', help: '|PV.Torque| − Mech — 타이어 몫' },
    { label: 'Force_N (N)', value: pct(live?.Force_N) ?? '-' },
    { label: 'G (mm)', value: gPos === null ? '-' : pos(gPos) },
    { label: 'Inch', value: live?.Inch ?? '-' },
  ]

  const err = live?.Error
  const learnDone = tune?.LearnDone ?? false
  const learnErr = tune?.LearnError ?? 0

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="gripper-page">
      <ScreenHeader
        title="그리퍼"
        icon={<Hand size={14} />}
        items={[
          { label: '로봇', value: chip.name },
          { label: 'PLC', value: snap?.plc ?? chip.plc ?? '-' },
          { label: 'WEBMON', value: dtl(feed.data?.at ?? snap?.at) },
          { label: 'GRIP_TUNE', value: snap?.tune_at ? dtl(snap.tune_at) : (snap?.tune_error ?? '-') },
        ]}
      />
      <StatRow items={stats} testid="gripper-stats" />
      <Toolbar
        title="LEARN"
        dense
        meta={
          error ? <span className={statusTone('fault').text}>{error}</span> : null
        }
      >
        <Button
          size="sm"
          intent="primary"
          loading={sending}
          disabled={learnWhy !== undefined}
          title={learnWhy}
          onClick={() => setConfirm(true)}
          data-testid="gripper-learn"
        >
          LEARN
        </Button>
        <HelpTip title="그리퍼 LEARN" text={LEARN_HELP} />
        <OverflowMenu items={menu} testid="gripper-more" />
      </Toolbar>

      <div className="grid min-h-0 flex-1 gap-3 overflow-auto p-3 lg:grid-cols-2">
        <Card>
          <Section title="상태 (WEBMON.Gripper)" first>
            <Bits
              obj={live as unknown as Record<string, unknown>}
              keys={[
                'Busy',
                'Done',
                'Error',
                'GripOk',
                'ItemPresent',
                'ItemDetect',
                'Obstacle',
                'Thermal',
                'AtSpeed',
                'Contact',
              ]}
              bad={['Error', 'Thermal']}
              warn={['Obstacle']}
            />
          </Section>
          <Section title="결과 (Result)">
            <KvTable
              rows={[
                [
                  'ErrorCode',
                  <span key="e" className={err ? statusTone('fault').text : undefined}>
                    {live?.ErrorCode ?? 0} {errorName(live?.ErrorCode)}
                  </span>,
                ],
                ['Timeout', String(live?.Timeout ?? 0)],
                ['ContactPos (mm)', pos(live?.ContactPos)],
                ['ReachedPos (mm)', pos(live?.ReachedPos)],
                ['GTarget (mm)', pos(wm?.Axis?.[3]?.Target ?? snap?.live.GTarget)],
                ['TorqueReachedPosition (mm)', pos(live?.TorqueReachedPosition)],
              ]}
            />
          </Section>
        </Card>

        <Card>
          <Section
            title="Mech 곡선 (GRIP_TUNE.Tune.Mech)"
            first
            help={CURVE_HELP}
            right={
              tune
                ? `Accel ${delta(tune.Accel?.[0])} / ${delta(tune.Accel?.[1])} · TorqSign ${tune.TorqSign} · Drift ${tune.DriftCount}`
                : undefined
            }
          >
            {anyValid(tune) ? (
              <XYChart
                series={series}
                rules={rules}
                xLabel="G (mm)"
                yLabel="Mech (%)"
                height={CHART_H}
                xDigits={0}
                yDigits={1}
              />
            ) : (
              <EmptyState
                compact
                title="미학습"
                hint={snap?.tune_error ?? 'LEARN 을 실행하면 두 곡선이 채워집니다.'}
                testid="gripper-unlearned"
              />
            )}
            <FieldList
              className="mt-2"
              columns={2}
              dense
              labelWidth={150}
              items={[
                { label: `Valid[1] ${CURVE_LABEL[0]}`, value: tune ? (tune.Valid?.[0] ? '학습됨' : '미학습') : null },
                { label: `Valid[2] ${CURVE_LABEL[1]}`, value: tune ? (tune.Valid?.[1] ? '학습됨' : '미학습') : null },
                { label: 'LearnedSpd[1] / [2]', value: tune ? `${tune.LearnedSpd?.[0] ?? '-'} / ${tune.LearnedSpd?.[1] ?? '-'}` : null },
                {
                  label: 'LearnDone / LearnError',
                  value: tune
                    ? `${learnDone ? 'Done' : '-'} / ${learnErr} ${LEARN_ERROR[learnErr] ?? ''}`.trim()
                    : null,
                  status: learnErr ? 'fault' : learnDone ? 'ok' : undefined,
                },
                { label: 'LearnErrorPos (mm)', value: tune && learnErr ? pos(tune.LearnErrorPos) : null, missing: '실패 없음' },
              ]}
            />
          </Section>
        </Card>

        <Card padded={false}>
          <div className="flex items-baseline gap-2 border-b border-line-default px-3 py-1.5">
            <h3 className="text-xs font-semibold text-content-muted">ScaleByInch (GRIP_TUNE)</h3>
            <HelpTip title="ScaleByInch" text={SCALE_HELP} />
          </div>
          <DataTable
            rows={scaleRows(tune)}
            columns={SCALE_COLS}
            rowKey={(r) => String(r.inch)}
            density="compact"
            fit
            empty="GRIP_TUNE 없음"
            emptyHint={snap?.tune_error ?? undefined}
          />
        </Card>

        <Card padded={false}>
          <div className="flex items-baseline gap-2 border-b border-line-default px-3 py-1.5">
            <h3 className="text-xs font-semibold text-content-muted">PARA (Machine.G_* · Task.G_*)</h3>
            <span className="ml-auto text-2xs text-content-faint">읽기 전용 · 0 = PLC 기본값</span>
          </div>
          <DataTable
            rows={paraRows(snap?.para)}
            columns={PARA_COLS}
            rowKey={(r) => r.name}
            density="compact"
            stickyHeader
            className="max-h-80 overflow-auto"
            empty="PARA 없음"
          />
        </Card>
      </div>

      <ConfirmDialog
        open={confirm}
        onOpenChange={setConfirm}
        scope="single-robot"
        title={`그리퍼 LEARN — ${chip.name}`}
        confirmLabel="LEARN"
        confirmDisabled={learnWhy}
        onConfirm={() => void learn()}
      >
        <div className="text-content-primary">
          그리퍼 기구 부하 곡선을 다시 학습하시겠습니까? G 축이 전 범위를 한 번 움직입니다.
        </div>
        <FieldList
          className="mt-3"
          columns={2}
          dense
          labelWidth={96}
          items={[
            robotField(chip, '대상'),
            { label: '모드', value: modeText ?? '모름', status: learnWhy ? 'warn' : undefined },
            { label: '조건', value: '수동/정비 모드 · 화물 없음 · 그리퍼 Ready', wide: true },
            { label: '경로', value: 'Command.B3_Spare.Spare_X0 1 s 펄스', wide: true, mono: true },
          ]}
        />
      </ConfirmDialog>

      <FormDialog
        open={scaleOpen}
        onOpenChange={setScaleOpen}
        title={`ScaleByInch 편집 — ${chip.name}`}
        meta="12~24 인치 · % · 0 또는 100 = 조정 없음"
        size="md"
        onSubmit={() => void saveScale()}
        submitLabel="저장"
        busy={scaleBusy}
        dirty={scaleIn.some((v, i) => v !== String(scaleRows(tune)[i]?.value ?? ''))}
        testid="gripper-scale-dialog"
      >
        <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
          {scaleIn.map((v, i) => (
            <Input
              key={i}
              dense
              label={`${12 + i} in`}
              value={v}
              inputMode="decimal"
              onValueChange={(s) => setScaleIn((prev) => prev.map((p, k) => (k === i ? s : p)))}
            />
          ))}
        </div>
      </FormDialog>
    </div>
  )
}

export default function GripperPage() {
  return (
    <ErrorBoundary label="그리퍼">
      <GripperScreen />
    </ErrorBoundary>
  )
}
