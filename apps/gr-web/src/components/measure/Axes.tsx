// 축 / 센서 탭 — 축 표, 그리퍼 센서, OPCUA Drive, 선택 축 위치 추이.
import { useState } from 'react'
import { AXIS } from '../../lib/gr/const'
import { f1, f2 } from '../../lib/meas/format'
import { AXIS_HIST } from '../../lib/measlog'
import { Card } from '../../lib/ui/Card'
import { Select } from '../../lib/ui/Select'
import { SeriesChart } from '../../lib/ui/viz/LineChart'
import type { WebMon } from '../../lib/types'
import { Chips, KvTable } from './helpers'

export function Axes({ wm, axisHist }: { wm: WebMon; axisHist: readonly (readonly number[])[] }) {
  const [sel, setSel] = useState(2)
  const axis = wm.Axis ?? []
  const g = wm.Gripper
  const drv = wm.Stat?.Drive ?? []
  return (
    <div className="space-y-3">
      <Card>
        <table className="w-full text-xs">
          <thead>
            <tr className="text-content-muted">
              <th className="py-1 text-left">축</th>
              <th className="text-right">위치 (mm)</th>
              <th className="text-right">목표 (mm)</th>
              <th className="text-right">속도</th>
              <th className="text-right">토크 %</th>
              <th className="text-right">Lag</th>
              <th className="pl-3 text-left">상태</th>
              <th className="text-right">Main/Sub</th>
            </tr>
          </thead>
          <tbody>
            {axis.map((a, i) => (
              <tr key={i} className="border-t border-line-default">
                <td className="py-1 font-semibold">{AXIS[i]}</td>
                <td className="text-right font-mono tabular-nums">{f1(a.Position)}</td>
                <td className="text-right font-mono tabular-nums">{f1(a.Target)}</td>
                <td className="text-right font-mono tabular-nums">{f1(a.Speed)}</td>
                <td className="text-right font-mono tabular-nums">{f1(a.Torque)}</td>
                <td className="text-right font-mono tabular-nums">{f2(a.LagError)}</td>
                <td className="pl-3">
                  <Chips obj={a as unknown as Record<string, unknown>} keys={['Ready', 'Enabled', 'Running', 'StandStill']} />
                </td>
                <td className="text-right font-mono tabular-nums">
                  {a.MainCode}/{a.SubCode}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
      <div className="grid gap-3 lg:grid-cols-2">
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">그리퍼 / 센서</h3>
          <KvTable
            rows={[
              ['ItemDetect', g?.ItemDetect ? '감지' : '없음'],
              ['GID L / F / R / B (mm)', (g?.GID ?? []).map(f1).join(' / ')],
              ['FLD (mm)', f1(g?.FLD)],
              ['토크 도달 G 위치', f1(g?.TorqueReachedPosition)],
            ]}
          />
        </Card>
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">OPCUA Drive (STAT.Drive)</h3>
          <table className="w-full text-xs">
            <thead>
              <tr className="text-content-muted">
                <th className="text-left">축</th>
                <th className="text-right">OpMode</th>
                <th className="text-right">Speed</th>
                <th className="text-right">Torque</th>
                <th className="text-right">Position</th>
                <th className="text-right">TorqLimit</th>
                <th className="text-right">Main/Sub</th>
              </tr>
            </thead>
            <tbody>
              {drv.map((d, i) => (
                <tr key={i} className="border-t border-line-default font-mono tabular-nums">
                  <td className="text-left font-sans font-semibold">{AXIS[i]}</td>
                  <td className="text-right">{d.OpMode}</td>
                  <td className="text-right">{f1(d.Speed)}</td>
                  <td className="text-right">{f1(d.Torque)}</td>
                  <td className="text-right">{f1(d.Position)}</td>
                  <td className="text-right">{f1(d.TorqLimit)}</td>
                  <td className="text-right">
                    {d.MainCode}/{d.SubCode}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      </div>
      <Card>
        <div className="mb-2 flex items-center gap-3">
          <Select dense label="위치 추이" value={sel} onValueChange={(v) => setSel(Number(v))}>
            {AXIS.map((a, i) => (
              <option key={a} value={i}>
                {a}
              </option>
            ))}
          </Select>
          <span className="text-2xs text-content-muted">최근 {AXIS_HIST} 샘플 (브라우저 메모리)</span>
        </div>
        <SeriesChart values={axisHist[sel] ?? []} capacity={AXIS_HIST} label={`${AXIS[sel]} 위치 (mm)`} />
      </Card>
    </div>
  )
}
