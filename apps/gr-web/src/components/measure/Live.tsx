// 현황 — 지금 이 로봇이 무엇을 하고 있나. 대시보드와 측정 진행 **두 탭을 하나로** 합친 것이다.
//
// 합친 이유는 둘이 같은 질문에 답하고 있었기 때문이다. 대시보드가 상태 비트·알람·인터록을 말하고
// 측정 진행이 FUNC.Measure* 를 말했는데, 측정 중인지 아닌지는 둘 다 봐야 알 수 있었다.
// 합치면서 **두 번 그리던 것**을 뺐다:
//  - 통계 카드 열여섯 개 → 화면 머리의 숫자 띠 하나(`MeasureMonitorModel.bandItems`)
//  - 진행 중 Task 상세 → Task 탭에만(여기서는 띠가 Work/Task 를 말한다)
//  - 축 표 → 축·센서 탭에만(거기 표가 이 표의 상위 집합이었다)
import { alarmLabel } from '../../lib/gr/alarms'
import { STATUS } from '../../lib/gr/const'
import { delta, pos } from '../../lib/meas/format'
import { Card } from '../../lib/ui/Card'
import { StatusDot } from '../../lib/ui/StatusDot'
import { statusTone } from '../../lib/ui/status'
import type { WebMon } from '../../lib/types'
import { Section } from '../../lib/ui/Section'
import { Bits, KvTable } from './helpers'

type R = Record<string, unknown>
const num = (o: R | undefined, k: string) => (o ? (o[k] as number | undefined) : undefined)

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

export function Live({ wm }: { wm: WebMon }) {
  const S = wm.Stat
  const st = S?.Status ?? {}
  const ts = S?.Task?.Status ?? {}
  const al = wm.Alarm
  const il = S?.Interlock
  const M = wm.Measure ?? ({} as WebMon['Measure'])
  const mi = M.Item as R | undefined
  const ms = M.Sku as R | undefined
  const mf = M.Floor as R | undefined
  const mb = M.LastBead as R | undefined
  const om = S?.Measuring

  return (
    <div className="grid gap-3 lg:grid-cols-3">
      <Card>
        <Section title="장비 상태 (Status)" first>
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
        </Section>
        <Section title="모드 (OPCUA.STAT.Mode)">
          <Bits
            obj={S?.Mode}
            keys={['Init', 'Maint', 'Manual', 'AutoReady', 'Auto', 'Fault']}
            bad={['Fault']}
          />
        </Section>
        <Section title="Task 상태">
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
        </Section>
      </Card>

      <Card>
        <Section title="MeasureItem" first>
          <KvTable
            rows={[
              [
                'Status',
                <span key="s" className="flex flex-wrap gap-1">
                  <Bits obj={mi} keys={['Busy', 'Done', 'Reported']} inline />
                  <StatusDot
                    status={num(mi, 'Status') === 3 || num(mi, 'Status') === 4 ? 'fault' : 'info'}
                    size="sm"
                    label={String(STATUS[num(mi, 'Status') ?? 0] ?? num(mi, 'Status') ?? '')}
                  />
                </span>,
              ],
              ['WorkCell / Code', `${num(mi, 'WorkCell') ?? ''} / ${num(mi, 'Code') ?? ''}`],
              [
                'In',
                <span key="in" className="flex flex-wrap items-center gap-1">
                  <Bits obj={mi} keys={['InBusy', 'InDone', 'InValid']} inline /> InInnerDia{' '}
                  {pos(num(mi, 'InInnerDia'))}
                </span>,
              ],
              [
                'Out',
                <span key="out" className="flex flex-wrap items-center gap-1">
                  <Bits obj={mi} keys={['OutBusy', 'OutDone', 'OutValid']} inline /> OutInnerDia{' '}
                  {pos(num(mi, 'OutInnerDia'))}
                </span>,
              ],
              ['Torq_InnerDia / InnerDia', `${pos(num(mi, 'Torq_InnerDia'))} / ${pos(num(mi, 'InnerDia'))}`],
              ['OffsetX / OffsetY', `${delta(num(mi, 'OffsetX'))} / ${delta(num(mi, 'OffsetY'))}`],
              [
                'UpperBeadHeight / TireHeight',
                `${pos(num(mi, 'UpperBeadHeight'))} / ${pos(num(mi, 'TireHeight'))}`,
              ],
            ]}
          />
        </Section>
        <Section title="MeasureSku / Floor / LastBead">
          <KvTable
            rows={[
              [
                'Sku',
                <span key="k" className="flex flex-wrap items-center gap-1">
                  <Bits obj={ms} keys={['Enable', 'Busy', 'Done']} inline /> Cell{' '}
                  {num(ms, 'WorkCell') ?? ''} · Stack {num(ms, 'StackCount') ?? ''}
                </span>,
              ],
              [
                'EachHeight / StackHeight',
                `${pos(num(ms, 'EachHeight'))} / ${pos(num(ms, 'StackHeight'))}`,
              ],
              [
                'BeadPos',
                ((ms?.BeadPos as number[] | undefined) ?? [])
                  .filter((v) => v)
                  .map(pos)
                  .join(', ') || '-',
              ],
              [
                'Floor',
                <span key="f" className="flex flex-wrap items-center gap-1">
                  <Bits obj={mf} keys={['Enable', 'Busy', 'Done']} inline /> Height {pos(num(mf, 'Height'))}
                </span>,
              ],
              [
                'LastBead',
                <span key="b" className="flex flex-wrap items-center gap-1">
                  <Bits obj={mb} keys={['Enable', 'Busy', 'Done']} inline /> LastBeadPos{' '}
                  {pos(num(mb, 'LastBeadPos'))} · PickZTarget{' '}
                  {num(mb, 'PickZTarget') ? pos(num(mb, 'PickZTarget')) : '-'}
                </span>,
              ],
            ]}
          />
        </Section>
        <Section title="OPCUA Measuring (보고 영역)">
          <KvTable
            rows={[
              [
                'SKU',
                `Cell ${om?.SKU?.WorkCell ?? ''} Code ${om?.SKU?.Code ?? ''} Status ${STATUS[om?.SKU?.Data?.[0] ?? 0] ?? ''}`,
              ],
              [
                'Item',
                `Cell ${om?.Item?.WorkCell ?? ''} Code ${om?.Item?.Code ?? ''} Status ${STATUS[om?.Item?.Data?.[0] ?? 0] ?? ''} InnerDia ${pos(om?.Item?.Data?.[1])}`,
              ],
              ['Floor', `Cell ${om?.Floor?.WorkCell ?? ''} ${pos(om?.Floor?.Data)}`],
            ]}
          />
        </Section>
      </Card>

      <Card>
        <Section title="알람 (최근 코드)" first>
          <KvTable
            rows={[
              [
                'Fault',
                <span key="f" className="inline-flex items-center gap-1.5">
                  {al?.Fault ? <StatusDot status="fault" size="sm" title="Fault ON" /> : null}
                  {codes(al?.FaultCode, 'fault')}
                </span>,
              ],
              [
                'Warn',
                <span key="w" className="inline-flex items-center gap-1.5">
                  {al?.Warn ? <StatusDot status="warn" size="sm" title="Warn ON" /> : null}
                  {codes(al?.WarnCode, 'warn')}
                </span>,
              ],
              ['Event', codes(al?.EventCode, 'info')],
              ['Task', codes(al?.TaskCode, 'info')],
            ]}
          />
        </Section>
        <Section title="C/V 인터록">
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
        </Section>
      </Card>
    </div>
  )
}
