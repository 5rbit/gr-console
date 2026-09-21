// 축 / 센서 탭 — 축 한 줄에 **PLC 축과 OPCUA 드라이브를 함께**, 그리퍼·센서, 선택 축 위치 추이.
//
// 전에는 표가 둘이었다(MACHINE.Axis 와 OPCUA STAT.Drive). 행이 같은 축 넷인데 표가 갈려 있어서
// "Z 가 왜 안 움직이나"를 볼 때 두 표 사이에서 눈이 왕복했다. 한 표로 합치고 드라이브 쪽 열은
// 우선순위 3(넓을 때만)으로 내렸다 — 좁은 존에서는 접히고 행을 펼치면 라벨+값 짝으로 나온다.
import { useState } from 'react'
import { AXIS } from '../../lib/gr/const'
import { delta, f1, pos } from '../../lib/meas/format'
import { AXIS_HIST } from '../../lib/measlog'
import { Card } from '../../lib/ui/Card'
import { DataTable } from '../../lib/ui/DataTable'
import { Field } from '../../lib/ui/Field'
import { Select } from '../../lib/ui/Select'
import type { Column } from '../../lib/ui/table'
import { SeriesChart } from '../../lib/ui/viz/LineChart'
import type { PlcAxis, PlcDrive, WebMon } from '../../lib/types'
import { Section } from '../../lib/ui/Section'
import { Bits, KvTable } from './helpers'

interface AxisRow {
  i: number
  a: PlcAxis
  d: PlcDrive | undefined
}

// 단위는 값이 아니라 머리글에 붙인다(값이 1초에 여러 번 갱신되는 열에서 단위가 값에 붙으면
// 폭이 흔들린다 — `docs/DESIGN.md` 4절 ③).
const COLS: Column<AxisRow>[] = [
  { key: 'ax', label: 'Axis', get: (r) => AXIS[r.i] ?? String(r.i), priority: 1 },
  { key: 'pos', label: 'Position (mm)', get: (r) => pos(r.a?.Position), numeric: true, priority: 1 },
  { key: 'tgt', label: 'Target (mm)', get: (r) => pos(r.a?.Target), numeric: true, priority: 1 },
  { key: 'spd', label: 'Speed', get: (r) => f1(r.a?.Speed), numeric: true, priority: 2 },
  { key: 'trq', label: 'Torque (%)', get: (r) => f1(r.a?.Torque), numeric: true, priority: 2 },
  { key: 'lag', label: 'Lag (mm)', get: (r) => delta(r.a?.LagError), numeric: true, priority: 3 },
  {
    key: 'state',
    label: 'State',
    priority: 2,
    get: (r) => (r.a?.Running ? 'Running' : r.a?.Enabled ? 'Enabled' : ''),
    cell: (r) => (
      <Bits
        obj={r.a as unknown as Record<string, unknown>}
        keys={['Ready', 'Enabled', 'Running', 'StandStill']}
        inline
      />
    ),
  },
  { key: 'code', label: 'Main/Sub', get: (r) => `${r.a?.MainCode}/${r.a?.SubCode}`, priority: 3 },
  { key: 'op', label: 'Drv.OpMode', get: (r) => r.d?.OpMode, numeric: true, priority: 3 },
  { key: 'dpos', label: 'Drv.Position', get: (r) => pos(r.d?.Position), numeric: true, priority: 3 },
  { key: 'dtrq', label: 'Drv.Torque', get: (r) => f1(r.d?.Torque), numeric: true, priority: 3 },
  { key: 'lim', label: 'Drv.TorqLimit', get: (r) => f1(r.d?.TorqLimit), numeric: true, priority: 3 },
  {
    key: 'dcode',
    label: 'Drv.Main/Sub',
    get: (r) => (r.d ? `${r.d.MainCode}/${r.d.SubCode}` : ''),
    priority: 3,
  },
]

export function Axes({ wm, axisHist }: { wm: WebMon; axisHist: readonly (readonly number[])[] }) {
  const [sel, setSel] = useState(2)
  const axis = wm.Axis ?? []
  const drv = wm.Stat?.Drive ?? []
  const g = wm.Gripper
  const rows: AxisRow[] = axis.map((a, i) => ({ i, a, d: drv[i] }))

  return (
    <div className="space-y-3">
      <Card padded={false}>
        <DataTable
          rows={rows}
          columns={COLS}
          rowKey={(r) => String(r.i)}
          fit
          density="compact"
          empty="축 값 없음"
        />
      </Card>
      <div className="grid gap-3 lg:grid-cols-2">
        <Card>
          <Section title="그리퍼 / 센서" first>
            <KvTable
              rows={[
                ['ItemDetect', g?.ItemDetect ? '감지' : '없음'],
                ['GID L / F / R / B (mm)', (g?.GID ?? []).map(pos).join(' / ')],
                ['FLD (mm)', pos(g?.FLD)],
                ['TorqueReachedPosition', pos(g?.TorqueReachedPosition)],
                [
                  'TorqueReached / TorqueStop',
                  g?.State
                    ? `${g.State.TorqueReached ? '검출' : '-'} / ${g.State.TorqueStop ? '멈춤' : '-'}`
                    : '-',
                ],
                ['StallTime (s)', g?.State ? f1(g.State.StallTime) : '-'],
              ]}
            />
          </Section>
          <Section title="그리퍼 상태 (MACHINE.Gripper.State)">
            <Bits
              obj={g?.State as unknown as Record<string, unknown>}
              keys={[
                'Commanded',
                'Stopped',
                'AtCommand',
                'TorqueReached',
                'TorqueStop',
                'Stall',
                'StallNoTorque',
              ]}
              warn={['Stall']}
              bad={['StallNoTorque']}
            />
          </Section>
        </Card>
        <Card>
          <div className="mb-2 flex items-end gap-3">
            <Field label="위치 추이" className="w-28">
              <Select
                dense
                value={sel}
                onValueChange={(v) => setSel(Number(v))}
                aria-label="위치 추이 축"
              >
                {AXIS.map((a, i) => (
                  <option key={a} value={i}>
                    {a}
                  </option>
                ))}
              </Select>
            </Field>
            <span className="pb-1.5 text-2xs text-content-muted">
              최근 {AXIS_HIST} 샘플 (브라우저 메모리)
            </span>
          </div>
          <SeriesChart
            values={axisHist[sel] ?? []}
            capacity={AXIS_HIST}
            label={`${AXIS[sel]}.Position (mm)`}
          />
        </Card>
      </div>
    </div>
  )
}
