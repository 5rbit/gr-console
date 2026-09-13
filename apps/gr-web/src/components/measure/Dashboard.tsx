// 대시보드 — 모드/상태/Task 칩, 진행 중 Task, 알람, 인터록, 축 요약.
import { alarmLabel } from '../../lib/gr/alarms'
import { AXIS, modeName } from '../../lib/gr/const'
import { f1, tt } from '../../lib/meas/format'
import { Card } from '../../lib/ui/Card'
import { StatusDot } from '../../lib/ui/StatusDot'
import { statusTone } from '../../lib/ui/status'
import type { WebMon } from '../../lib/types'
import { Bits, KvTable, StatCards, TaskKv } from './helpers'

function codes(arr: readonly number[] | undefined, tone: 'fault' | 'warn' | 'info') {
  const list = (arr ?? []).filter((c) => c)
  if (!list.length) return <span className="text-content-muted">-</span>
  // 코드는 값이다 — 캡슐로 싸면 개수에 따라 줄이 흔들린다. 색 글자로 나열한다. 알려진 코드는 툴팁으로 설명.
  return (
    <span className={`tabular-nums ${statusTone(tone).text}`}>
      {list.map((c, i) => (
        <span key={i} title={alarmLabel(c)}>
          {i > 0 ? ' · ' : ''}
          {c}
        </span>
      ))}
    </span>
  )
}

export function Dashboard({ wm }: { wm: WebMon }) {
  const S = wm.Stat
  const st = S?.Status ?? {}
  const ts = S?.Task?.Status ?? {}
  const now = S?.Task?.Now
  const pr = wm.Proc
  const al = wm.Alarm
  const il = S?.Interlock
  const axis = wm.Axis ?? []
  const fault = Boolean(st.Fault)
  const warn = Boolean(st.Warn)
  return (
    <div>
      <StatCards
        items={[
          {
            label: '모드',
            value: modeName(wm.Mode),
            tone: wm.Mode === 32 ? 'ok' : wm.Mode === 128 ? 'bad' : 'warn',
            hint: 'MACHINE.Mode',
            big: true,
          },
          {
            label: '상태',
            value: fault ? 'FAULT' : warn ? 'WARN' : st.Busy ? 'BUSY' : st.Idle ? 'IDLE' : '-',
            tone: fault ? 'bad' : warn ? 'warn' : 'ok',
            hint: `StatusCode ${S?.StatusCode?.Main ?? ''}/${S?.StatusCode?.Sub ?? ''}`,
            big: true,
          },
          {
            label: 'Task',
            value: ts.Inprogress ? '진행' : ts.Complete ? '완료' : ts.Idle ? '대기' : '-',
            tone: ts.Inprogress ? 'ok' : '',
            hint: `Step ${ts.Step ?? ''}`,
            big: true,
          },
          {
            label: 'Proc Step',
            value: pr?.Step?.Now ?? '',
            hint: `${f1(pr?.Step?.ElapseTime)} s  ${pr?.Msg ?? ''}`,
            big: true,
          },
          {
            label: '진행 Task',
            value: now?.WorkId ? `${now.WorkId}/${now.TaskId}` : '-',
            hint: `${tt(now?.TaskType)} Cell ${now?.Cell?.Id ?? ''} Code ${now?.Item?.Code ?? ''}`,
            big: true,
          },
          { label: '속도', value: String(st.Speed ?? ''), hint: '%', big: true },
          {
            label: 'Z',
            value: f1(axis[2]?.Position),
            tone: axis[2]?.Running ? 'ok' : '',
            hint: `→ ${f1(axis[2]?.Target)}`,
            big: true,
          },
          {
            label: 'G',
            value: f1(axis[3]?.Position),
            tone: axis[3]?.Running ? 'ok' : '',
            hint: `→ ${f1(axis[3]?.Target)}`,
            big: true,
          },
          {
            label: '아이템',
            value: wm.Gripper?.ItemDetect ? '감지' : '없음',
            tone: wm.Gripper?.ItemDetect ? 'ok' : '',
            hint: 'Gripper',
            big: true,
          },
        ]}
      />
      <div className="grid gap-3 lg:grid-cols-3">
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">장비 상태 (Status)</h3>
          <Bits
            obj={st}
            keys={[
              'Normal',
              'Idle',
              'Busy',
              'Stopped',
              'Online',
              'Warn',
              'Fault',
              'PowerOn',
              'ItemDectect',
              'HeartBeat',
              'DoorOpenErrorGR1',
              'DoorOpenErrorGR2',
              'BuzzerStop',
            ]}
            bad={['Fault', 'DoorOpenErrorGR1', 'DoorOpenErrorGR2', 'Stopped']}
            warn={['Warn']}
          />
          <h3 className="mt-3 mb-1 text-xs font-semibold text-content-muted">
            모드 (OPCUA.STAT.Mode)
          </h3>
          <Bits
            obj={S?.Mode}
            keys={['Init', 'Maint', 'Manual', 'AutoReady', 'Auto', 'Fault']}
            bad={['Fault']}
          />
          <h3 className="mt-3 mb-1 text-xs font-semibold text-content-muted">Task 상태</h3>
          <Bits
            obj={ts}
            keys={[
              'Accept',
              'Idle',
              'Assigned',
              'Inprogress',
              'AvoidReq',
              'HoldItem',
              'Complete',
              'Canceled',
            ]}
            bad={['Canceled']}
            warn={['AvoidReq']}
          />
        </Card>
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">진행 중 Task</h3>
          <TaskKv t={now} />
        </Card>
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">알람 (최근 코드)</h3>
          <KvTable
            rows={[
              [
                <span key="f" className="inline-flex items-center gap-1.5">
                  Fault {al?.Fault ? <StatusDot status="fault" size="sm" title="Fault ON" /> : null}
                </span>,
                codes(al?.FaultCode, 'fault'),
              ],
              [
                <span key="w" className="inline-flex items-center gap-1.5">
                  Warn {al?.Warn ? <StatusDot status="warn" size="sm" title="Warn ON" /> : null}
                </span>,
                codes(al?.WarnCode, 'warn'),
              ],
              ['Event', codes(al?.EventCode, 'info')],
              ['Task', codes(al?.TaskCode, 'info')],
            ]}
          />
          <h3 className="mt-3 mb-1 text-xs font-semibold text-content-muted">C/V 인터록</h3>
          <KvTable
            rows={[
              ['WorkCell / Type', `${il?.WorkCell ?? ''} / ${il?.WorkType ?? ''}`],
              [
                'PI (C/V → GR)',
                <Bits key="pi" obj={il?.PI} keys={['CVOK', 'Req', 'MeasReq', 'ItemExist']} />,
              ],
              [
                'PO (GR → C/V)',
                <Bits
                  key="po"
                  obj={il?.PO}
                  keys={['CVNO', 'Comp', 'MeasComp', 'MeasErr']}
                  bad={['MeasErr']}
                />,
              ],
            ]}
          />
        </Card>
      </div>
      <Card className="mt-3">
        <h3 className="mb-1 text-xs font-semibold text-content-muted">축</h3>
        <table className="w-full text-xs">
          <thead>
            <tr className="text-content-muted">
              <th className="py-1 text-left">축</th>
              <th className="text-right">위치</th>
              <th className="text-right">목표</th>
              <th className="text-right">속도</th>
              <th className="text-right">토크%</th>
              <th className="pl-3 text-left">상태</th>
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
                <td className="pl-3">
                  <Bits
                    obj={a as unknown as Record<string, unknown>}
                    keys={['Ready', 'Enabled', 'Running', 'StandStill']}
                  />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  )
}
