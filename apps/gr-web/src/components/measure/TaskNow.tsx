// Task 탭 — Now 상세, Proc, OPCUA 응답 헤더, 대기열/완료/취소/거부 표.
import { dtl, f1, hex } from '../../lib/meas/format'
import { Card } from '../../lib/ui/Card'
import type { WebMon } from '../../lib/types'
import { KvTable, TaskKv, TaskTableMini } from './helpers'

export function TaskNow({ wm }: { wm: WebMon }) {
  const S = wm.Stat
  const T = S?.Task
  const pr = wm.Proc
  const rh = S?.RES?.Header
  return (
    <div className="space-y-3">
      <div className="grid gap-3 lg:grid-cols-2">
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">진행 중 (Now)</h3>
          <TaskKv t={T?.Now} />
        </Card>
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">진행 (Proc)</h3>
          <KvTable
            rows={[
              ['ProcNo', pr?.ProcNo ?? ''],
              ['Step Now / Prev', `${pr?.Step?.Now ?? ''} / ${pr?.Step?.Prev ?? ''}`],
              ['경과 (s)', f1(pr?.Step?.ElapseTime)],
              ['Msg', pr?.Msg ?? ''],
              ['Task.Status.Step', T?.Status?.Step ?? ''],
            ]}
          />
          <h3 className="mt-3 mb-1 text-xs font-semibold text-content-muted">OPCUA 응답 (RES.Header)</h3>
          <KvTable
            rows={[
              ['CMD', rh ? hex(rh.CMD) : ''],
              ['CMD_ID / SEQ', `${rh?.CMD_ID ?? ''} / ${rh?.SEQ ?? ''}`],
              ['SRC → DST', `${rh?.SRC ?? ''} → ${rh?.DST ?? ''}`],
              ['RES.Data', (S?.RES?.Data ?? []).slice(0, 8).join(' ')],
              ['NTP', dtl(S?.NTP)],
              ['ComponentID', S?.ComponentID ?? ''],
            ]}
          />
        </Card>
      </div>
      <Card>
        <h3 className="mb-1 text-xs font-semibold text-content-muted">대기열 (Queue[0..3])</h3>
        <TaskTableMini list={T?.Queue} />
      </Card>
      <Card>
        <h3 className="mb-1 text-xs font-semibold text-content-muted">완료 (Completed[0..9])</h3>
        <TaskTableMini list={T?.Completed} />
      </Card>
      <div className="grid gap-3 lg:grid-cols-2">
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">취소 (Canceled)</h3>
          <TaskTableMini list={T?.Canceled} />
        </Card>
        <Card>
          <h3 className="mb-1 text-xs font-semibold text-content-muted">거부 (Rejected)</h3>
          <TaskTableMini list={T?.Rejected} />
        </Card>
      </div>
    </div>
  )
}
