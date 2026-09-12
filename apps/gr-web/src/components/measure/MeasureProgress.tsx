// 측정 진행 탭 — FUNC.Measure* 실시간 값 + OPCUA 보고 영역.
import { STATUS } from '../../lib/gr/const'
import { f1, f2 } from '../../lib/meas/format'
import { Card } from '../../lib/ui/Card'
import { StatusDot } from '../../lib/ui/StatusDot'
import type { MeasLogSnapshot, WebMon } from '../../lib/types'
import { Bits, KvTable, StatCards } from './helpers'

type R = Record<string, unknown>
const num = (o: R | undefined, k: string) => (o ? (o[k] as number | undefined) : undefined)
const bool = (o: R | undefined, k: string) => Boolean(o?.[k])

export function MeasureProgress({ wm, snap }: { wm: WebMon; snap: MeasLogSnapshot | null }) {
  const M = wm.Measure ?? ({} as WebMon['Measure'])
  const mi = M.Item as R | undefined
  const ms = M.Sku as R | undefined
  const mf = M.Floor as R | undefined
  const mb = M.LastBead as R | undefined
  const om = wm.Stat?.Measuring
  const st = (o: R | undefined) => (bool(o, 'Busy') ? '측정중' : bool(o, 'Done') ? '완료' : '-')
  return (
    <div>
      <StatCards
        items={[
          { label: 'Item', value: st(mi), tone: bool(mi, 'Busy') ? 'ok' : '' },
          { label: 'Sku', value: st(ms), tone: bool(ms, 'Busy') ? 'ok' : '' },
          { label: 'Floor', value: st(mf), tone: bool(mf, 'Busy') ? 'ok' : '' },
          { label: 'Pick 비드', value: st(mb), tone: bool(mb, 'Busy') ? 'ok' : '' },
          { label: '이력 Total', value: wm.MeasLog?.Total ?? '' },
          { label: '이력 Count', value: wm.MeasLog?.Count ?? '' },
          {
            label: '콘솔 미러',
            value: (snap as unknown as { mirrored?: number } | null)?.mirrored ?? snap?.total ?? '',
          },
        ]}
      />
      <div className="grid gap-3 lg:grid-cols-3">
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">MeasureItem</h3>
          <KvTable
            rows={[
              [
                '상태',
                <span key="s" className="flex flex-wrap gap-1">
                  <Bits obj={mi} keys={['Busy', 'Done', 'Reported']} />
                  <StatusDot
                    status={num(mi, 'Status') === 3 || num(mi, 'Status') === 4 ? 'fault' : 'info'}
                    size="sm"
                    label={String(STATUS[num(mi, 'Status') ?? 0] ?? num(mi, 'Status') ?? '')}
                  />
                </span>,
              ],
              ['WorkCell / Code', `${num(mi, 'WorkCell') ?? ''} / ${num(mi, 'Code') ?? ''}`],
              [
                '들어갈 때',
                <span key="in" className="flex flex-wrap items-center gap-1">
                  <Bits obj={mi} keys={['InBusy', 'InDone', 'InValid']} /> 내경{' '}
                  {f1(num(mi, 'InInnerDia'))}
                </span>,
              ],
              [
                '나갈 때',
                <span key="out" className="flex flex-wrap items-center gap-1">
                  <Bits obj={mi} keys={['OutBusy', 'OutDone', 'OutValid']} /> 내경{' '}
                  {f1(num(mi, 'OutInnerDia'))}
                </span>,
              ],
              ['토크 내경', f1(num(mi, 'Torq_InnerDia'))],
              ['정합 내경', f1(num(mi, 'InnerDia'))],
              ['편심 X / Y', `${f2(num(mi, 'OffsetX'))} / ${f2(num(mi, 'OffsetY'))}`],
              [
                '상단 비드 / 타이어 높이',
                `${f1(num(mi, 'UpperBeadHeight'))} / ${f1(num(mi, 'TireHeight'))}`,
              ],
            ]}
          />
        </Card>
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">MeasureSku</h3>
          <KvTable
            rows={[
              ['상태', <Bits key="s" obj={ms} keys={['Enable', 'Busy', 'Done']} />],
              ['WorkCell', num(ms, 'WorkCell') ?? ''],
              ['단수', num(ms, 'StackCount') ?? ''],
              ['단당 / 스택 높이', `${f1(num(ms, 'EachHeight'))} / ${f1(num(ms, 'StackHeight'))}`],
              [
                '비드 위치',
                ((ms?.BeadPos as number[] | undefined) ?? [])
                  .filter((v) => v)
                  .map(f1)
                  .join(', ') || '-',
              ],
            ]}
          />
        </Card>
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">
            Floor / Pick 마지막 비드
          </h3>
          <KvTable
            rows={[
              [
                'Floor',
                <span key="f" className="flex flex-wrap items-center gap-1">
                  <Bits obj={mf} keys={['Enable', 'Busy', 'Done']} /> 높이 {f1(num(mf, 'Height'))}
                </span>,
              ],
              ['Pick 마지막 비드', <Bits key="b" obj={mb} keys={['Enable', 'Busy', 'Done']} />],
              [
                'LastBidPos / PickZTarget',
                `${f1(num(mb, 'LastBidPos'))} / ${num(mb, 'PickZTarget') ? f1(num(mb, 'PickZTarget')) : '-'}`,
              ],
            ]}
          />
          <h3 className="mt-3 mb-1 text-xs font-semibold text-content-muted">
            OPCUA Measuring (보고 영역)
          </h3>
          <KvTable
            rows={[
              [
                'SKU',
                `Cell ${om?.SKU?.WorkCell ?? ''} Code ${om?.SKU?.Code ?? ''} Status ${STATUS[om?.SKU?.Data?.[0] ?? 0] ?? ''}`,
              ],
              [
                'Item',
                `Cell ${om?.Item?.WorkCell ?? ''} Code ${om?.Item?.Code ?? ''} Status ${STATUS[om?.Item?.Data?.[0] ?? 0] ?? ''} 내경 ${f1(om?.Item?.Data?.[1])}`,
              ],
              ['Floor', `Cell ${om?.Floor?.WorkCell ?? ''} ${f1(om?.Floor?.Data)}`],
            ]}
          />
        </Card>
      </div>
    </div>
  )
}
