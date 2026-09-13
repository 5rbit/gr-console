// 레이저 센서 탭 — 그리퍼 레이저 센서별 상태(LASERDIAG.Sensor), Z 오프셋 자동 교정(ZCal), 측정별 진단 이력.
// 데이터는 GET /api/laser (2 초 폴링, 숨은 탭 스킵). 교정 시작·취소·초기화는 LASERDIAG 의 Bool 하나를 쓰고, 나머지는 PLC(UL_LaserDiag)가 한다.
import { useCallback, useEffect, useState } from 'react'
import { api } from '../../lib/api'
import { ALARM_LABEL } from '../../lib/gr/alarms'
import { LASER_DIRS, LASER_SOURCE, ZCAL_ERROR, activeDirs, diagFlagLabels, zcalPhase } from '../../lib/laser'
import { dtl, f2 } from '../../lib/meas/format'
import { visibleInterval } from '../../lib/poll'
import type { LaserSnapshot } from '../../lib/types'
import { Button } from '../../lib/ui/Button'
import { Card } from '../../lib/ui/Card'
import { ConfirmDialog } from '../../lib/ui/ConfirmDialog'
import { EmptyState } from '../../lib/ui/EmptyState'
import { StatusBadge } from '../../lib/ui/StatusBadge'
import { statusTone } from '../../lib/ui/status'
import { toast } from '../../lib/ui/toast'
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

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

export function LaserSensor() {
  const [data, setData] = useState<LaserSnapshot | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [confirm, setConfirm] = useState<'start' | 'reset' | null>(null)

  const load = useCallback(async () => {
    try {
      setData(await api.laser())
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

  async function run(label: string, fn: () => Promise<unknown>) {
    setBusy(true)
    try {
      await fn()
      toast.ok(label)
      await load()
    } catch (e) {
      toast.error(`${label} 실패: ${errText(e)}`)
    } finally {
      setBusy(false)
    }
  }

  if (!data) return <EmptyState title="레이저 진단 없음" hint={error ?? 'LASERDIAG 를 읽는 중입니다.'} />

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

  return (
    <div className="space-y-3" data-testid="measure-laser">
      {error ? <div className={`text-xs ${statusTone('fault').text}`}>{error}</div> : null}
      <div className="grid gap-3 lg:grid-cols-2">
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">
            센서별 상태 (LASERDIAG.Sensor, {nDir}방향)
          </h3>
          <table className="w-full text-xs">
            <thead>
              <tr className="text-content-muted">
                <th className="py-1 text-left">방향</th>
                <th className="text-right">Z 오프셋</th>
                <th className="text-right">편차 평균</th>
                <th className="text-right">측정 수</th>
                <th className="text-right">마지막 편차</th>
                <th className="text-right">무응답 연속</th>
                <th className="text-right">불안정 연속</th>
                <th className="pl-2 text-left">경고</th>
              </tr>
            </thead>
            <tbody>
              {dirs.map((d, k) => {
                const s = data.sensor?.[k]
                const alarms = [
                  s?.BiasAlarm ? 4201 + k : 0,
                  s?.NoRespAlarm ? 4205 + k : 0,
                  s?.UnstableAlarm ? 4209 + k : 0,
                ].filter((c) => c)
                return (
                  <tr key={d} className="border-t border-line-default font-mono tabular-nums">
                    <td className="py-1 font-sans font-semibold">{d}</td>
                    <td className="text-right">{f2(para[OFFSET_KEYS[k] ?? ''])}</td>
                    <td className="text-right">{f2(s?.BiasEma)}</td>
                    <td className="text-right">{s?.BiasSamples ?? '-'}</td>
                    <td className="text-right">{f2(s?.LastDev)}</td>
                    <td className="text-right">{s?.NoRespConsec ?? '-'}</td>
                    <td className="text-right">{s?.UnstableConsec ?? '-'}</td>
                    <td className="pl-2 font-sans">
                      {alarms.length ? (
                        <span className="flex flex-wrap gap-1">
                          {alarms.map((c) => (
                            <span key={c} title={ALARM_LABEL[c]}>
                              <StatusBadge status="warn">{c}</StatusBadge>
                            </span>
                          ))}
                        </span>
                      ) : (
                        <span className="text-content-muted">-</span>
                      )}
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
          <p className="mt-2 text-2xs text-content-muted">
            편차 평균이 같은 쪽으로 계속 크면 설치 높이 오차입니다. 타이어 기울기·변형은 타이어마다 방향이 달라 평균에서 줄어듭니다.
          </p>
          <div className="mt-2 flex justify-end">
            <Button size="sm" intent="outline" disabled={busy} onClick={() => setConfirm('reset')}>
              상태·이력 초기화
            </Button>
          </div>
        </Card>

        <Card>
          <div className="mb-1 flex items-center gap-2">
            <h3 className="text-xs font-semibold text-content-muted">Z 오프셋 자동 교정 (LASERDIAG.ZCal)</h3>
            <StatusBadge status={phase === 'busy' ? 'info' : phase === 'done' ? 'ok' : phase === 'error' ? 'fault' : 'neutral'}>
              {phaseText}
            </StatusBadge>
          </div>
          <KvTable
            rows={[
              ['측정 수 / 필요', `${z?.Count ?? 0} / ${z?.Target ?? '-'}  (방향 누락으로 건너뜀 ${z?.Skipped ?? 0})`],
              ['반영하는 측정', 'MeasureItem 들어갈 때·나갈 때, 수동 측정 (PICK 하강 제외)'],
            ]}
          />
          <table className="mt-2 w-full text-xs">
            <thead>
              <tr className="text-content-muted">
                <th className="py-1 text-left">방향</th>
                <th className="text-right">편차 평균</th>
                <th className="text-right">표준편차</th>
                <th className="text-right">이전 오프셋</th>
                <th className="text-right">새 오프셋</th>
              </tr>
            </thead>
            <tbody>
              {dirs.map((d, k) => (
                <tr key={d} className="border-t border-line-default font-mono tabular-nums">
                  <td className="py-1 font-sans font-semibold">{d}</td>
                  <td className="text-right">{f2(z?.Mean?.[k])}</td>
                  <td className="text-right">{f2(z?.StdDev?.[k])}</td>
                  <td className="text-right">{f2(z?.OldOffset?.[k])}</td>
                  <td className="text-right">{f2(z?.NewOffset?.[k])}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <ul className="mt-2 list-disc space-y-0.5 pl-4 text-2xs text-content-muted">
            <li>측정 사이에 타이어를 돌려 놓거나 다른 타이어를 쓰세요. 기울어짐이 평균에서 줄어듭니다 (PCR 3방향은 필수).</li>
            <li>센서끼리의 상대 높이만 맞춥니다. 오프셋 평균은 그대로입니다.</li>
            <li>
              표준편차가 {para.LaserZCal_MaxStdDev || 2} mm 를 넘거나 보정량이 {para.LaserZCal_MaxCorrection || 20} mm 를 넘으면
              PARA 를 바꾸지 않습니다.
            </li>
          </ul>
          <div className="mt-2 flex justify-end gap-2">
            {phase === 'busy' ? (
              <Button size="sm" intent="danger" loading={busy} onClick={() => void run('Z 오프셋 교정 취소', () => api.laserZCal(false))}>
                교정 취소
              </Button>
            ) : (
              <Button size="sm" intent="primary" loading={busy} onClick={() => setConfirm('start')}>
                교정 시작
              </Button>
            )}
          </div>
        </Card>
      </div>

      <div className="grid gap-3 lg:grid-cols-3">
        <Card className="lg:col-span-2">
          <h3 className="mb-1 text-xs font-semibold text-content-muted">
            측정별 진단 이력 (최근 {entries.length} 건 / 누적 {data.total ?? 0})
          </h3>
          {entries.length === 0 ? (
            <div className="text-xs text-content-muted">기록 없음</div>
          ) : (
            <div className="overflow-auto">
              <table className="w-full text-xs">
                <thead>
                  <tr className="text-content-muted">
                    <th className="py-1 text-right">Seq</th>
                    <th className="pl-2 text-left">시각</th>
                    <th className="text-left">측정</th>
                    <th className="text-right">Code</th>
                    <th className="text-center">원 맞춤</th>
                    {dirs.map((d) => (
                      <th key={d} className="text-right">
                        편차 {d}
                      </th>
                    ))}
                    <th className="text-right">편차 폭</th>
                    {nDir === 4 ? <th className="text-right">비평면</th> : null}
                    <th className="pl-2 text-left">플래그</th>
                  </tr>
                </thead>
                <tbody>
                  {entries.map((e) => (
                    <tr key={e.Seq} className="border-t border-line-default font-mono tabular-nums">
                      <td className="py-1 text-right">{e.Seq}</td>
                      <td className="pl-2 font-sans">{dtl(e.TimeStamp)}</td>
                      <td className="font-sans">{LASER_SOURCE[e.Source] ?? e.Source}</td>
                      <td className="text-right">{e.Code || '-'}</td>
                      <td className="text-center font-sans">{e.Valid ? 'OK' : `실패 ${e.FitError}`}</td>
                      {dirs.map((d, k) => (
                        <td key={d} className="text-right">
                          {f2(e.BeadDev?.[k])}
                        </td>
                      ))}
                      <td className="text-right">{f2(e.Spread)}</td>
                      {nDir === 4 ? <td className="text-right">{f2(e.PlanarResidual)}</td> : null}
                      <td className="pl-2 font-sans">{diagFlagLabels(e.DiagFlags).join(', ') || '-'}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Card>
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">기준값 (PARA.Sensor, 0 = 기본값)</h3>
          <KvTable rows={PARA_ROWS.map(([key, label]) => [label, f2(para[key])] as const)} />
        </Card>
      </div>

      <ConfirmDialog
        open={confirm !== null}
        onOpenChange={(o) => !o && setConfirm(null)}
        scope="single-robot"
        title={confirm === 'reset' ? '레이저 진단 초기화' : 'Z 오프셋 교정 시작'}
        danger={confirm === 'reset'}
        confirmLabel={confirm === 'reset' ? '초기화' : '교정 시작'}
        onConfirm={() => {
          const c = confirm
          setConfirm(null)
          if (c === 'reset') void run('레이저 진단 초기화', () => api.laserReset())
          else if (c === 'start') void run('Z 오프셋 교정 시작', () => api.laserZCal(true))
        }}
      >
        {confirm === 'reset' ? (
          <p className="text-sm">센서별 편차 평균·연속 횟수와 진단 이력을 지웁니다. 센서 교체·재취부 후에 사용하세요.</p>
        ) : (
          <div className="space-y-1 text-sm">
            <p>
              {data.plc} 에서 교정을 켭니다. 이후 타이어 측정 {para.LaserZCal_SampleCount || 5} 회의 방향별 비드 높이 편차로 PARA 센서 Z 오프셋을
              보정하고 CSV 저장을 요청합니다.
            </p>
            <p>측정 사이에 타이어를 돌려 놓으세요.</p>
          </div>
        )}
      </ConfirmDialog>
    </div>
  )
}
