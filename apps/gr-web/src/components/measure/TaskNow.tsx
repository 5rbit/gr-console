// Task 탭 — 진행 중 작업의 상세와 **PLC 작업 배열 넷을 한 표로**.
//
// 전에는 카드 일곱이었다(Now · Proc · RES.Header · Queue · Completed · Canceled · Rejected).
// 뒤의 넷은 열이 완전히 같은 표라 머리글을 네 번 읽어야 했고, 셋이 비어 있어도 빈 카드가 자리를
// 먹었다. 구분 열 하나를 앞에 세우니 표 하나로 줄고, 정렬로 묶어 볼 수 있게 됐다.
import { dtl, f1, hex } from '../../lib/meas/format'
import { Card } from '../../lib/ui/Card'
import type { WebMon } from '../../lib/types'
import { Section } from '../../lib/ui/Section'
import { KvTable, TaskKv, TaskTables } from './helpers'

export function TaskNow({ wm }: { wm: WebMon }) {
  const S = wm.Stat
  const T = S?.Task
  const pr = wm.Proc
  const rh = S?.RES?.Header
  const groups = [
    { label: '대기열', list: T?.Queue },
    { label: '완료', list: T?.Completed },
    { label: '취소', list: T?.Canceled },
    { label: '거부', list: T?.Rejected },
  ]
  const count = groups.reduce(
    (n, g) => n + (g.list ?? []).filter((t) => t && (t.WorkId || t.TaskId)).length,
    0,
  )

  return (
    <div className="space-y-3">
      <div className="grid gap-3 lg:grid-cols-2">
        <Card>
          <Section title="진행 중 Task (Stat.Task.Now)" first>
            <TaskKv t={T?.Now} />
          </Section>
        </Card>
        <Card>
          <Section title="Proc" first>
            <KvTable
              rows={[
                ['ProcNo', pr?.ProcNo ?? ''],
                ['Step Now / Prev', `${pr?.Step?.Now ?? ''} / ${pr?.Step?.Prev ?? ''}`],
                ['ElapseTime (s)', f1(pr?.Step?.ElapseTime)],
                ['Msg', pr?.Msg ?? ''],
                ['Task.Status.Step', T?.Status?.Step ?? ''],
              ]}
            />
          </Section>
          <Section title="RES.Header (OPCUA 응답)">
            <KvTable
              rows={[
                ['CMD', rh ? hex(rh.CMD) : ''],
                ['CMD_ID / SEQ', `${rh?.CMD_ID ?? ''} / ${rh?.SEQ ?? ''}`],
                ['SRC → DST', `${rh?.SRC ?? ''} → ${rh?.DST ?? ''}`],
                ['RES.Data', (S?.RES?.Data ?? []).slice(0, 8).join(' ')],
                ['NTP / ComponentID', `${dtl(S?.NTP)} · ${S?.ComponentID ?? ''}`],
              ]}
            />
          </Section>
        </Card>
      </div>
      <Card padded={false}>
        <div className="flex items-baseline gap-2 border-b border-line-default px-3 py-1.5">
          <h3 className="text-xs font-semibold text-content-muted">
            대기열 · 완료 · 취소 · 거부
          </h3>
          <span className="ml-auto text-2xs text-content-faint tabular-nums">{count}</span>
        </div>
        <TaskTables groups={groups} empty="PLC 작업 배열이 모두 비어 있습니다" />
      </Card>
    </div>
  )
}
