// 이벤트 화면의 작은 조각 — 레벨 표시 · PLC 색 칩.
import type { EventRow } from '../../lib/evtlog/api'
import { lvlTone } from '../../lib/evtlog/evtRowsModel'
import { robotColor, robots } from '../../lib/robots'
import { StatusBadge } from '../../lib/ui/StatusBadge'

/** 레벨 — WARN/ERROR 만 캡슐(표의 상태 열 하나), 나머지는 흐린 글자. */
export function EvtLevel({ row }: { row: Pick<EventRow, 'lvl_name'> }) {
  const tone = lvlTone(row.lvl_name)
  if (tone === 'neutral') return <span className="text-2xs text-content-faint">{row.lvl_name}</span>
  return (
    <StatusBadge status={tone} dot={false}>
      {row.lvl_name}
    </StatusBadge>
  )
}

/** PLC 의 데이터 색 — 로봇이면 로봇 식별색(맵·목록과 같은 색), 아니면 없음. */
export function plcColor(plc: string): string | null {
  const r = robots.list.find((x) => x.name === plc || x.plc === plc)
  return r ? robotColor(r.id) : null
}

export function EvtPlcChip({ plc }: { plc: string }) {
  const color = plcColor(plc)
  return (
    <span className="inline-flex min-w-0 items-center gap-1 text-2xs text-content-secondary">
      <span
        className={
          color ? 'h-2 w-2 shrink-0 rounded-sm' : 'h-2 w-2 shrink-0 rounded-sm bg-neutral-dot'
        }
        style={color ? { background: color } : undefined}
      />
      <span className="truncate font-mono">{plc}</span>
    </span>
  )
}
