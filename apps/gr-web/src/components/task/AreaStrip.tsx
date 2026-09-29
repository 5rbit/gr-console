// 두 로봇 영역 띠 — 다음 스텝이 왜 영역 대기인지 한눈에: 상대 로봇 위치 · 금지 구간(상대 영역 ± 간격) · 이 Task 구간.
// 이 Task 구간이 금지 구간에 닿으면 붉다. 오른쪽 버튼으로 간격 파라미터를 그 자리에서 고친다(`AnticolDialog`).
import { SlidersHorizontal } from 'lucide-react'
import { robots } from '../../lib/robots'
import { areaStrip, areaSummary } from '../../lib/task/areaStripModel'
import type { AreaView } from '../../lib/types'
import { Button } from '../../lib/ui/Button'
import { cn } from '../../lib/utils'

const mm = (v: number) => v.toFixed(0)

export function AreaStrip({
  view,
  xs,
  onEdit,
}: {
  view: AreaView
  /** 레지스트리 셀·스테이션 X — 축을 설비 전체로. */
  xs: readonly number[]
  onEdit: () => void
}) {
  const s = areaStrip(view, xs)
  const summary = areaSummary(view)
  return (
    <div
      className={cn(
        'flex flex-col gap-1 border-t px-3 py-1.5',
        s.blocked ? 'border-fault bg-fault-soft' : 'border-line-default',
      )}
      data-testid="area-strip"
      data-state={s.blocked ? 'blocked' : 'clear'}
    >
      <div className="flex items-start gap-2">
        <span
          className={cn(
            'min-w-0 flex-1 text-2xs break-words tabular-nums',
            s.blocked ? 'font-medium text-fault-fg' : 'text-content-muted',
          )}
          title={view.check?.reason ?? summary}
          data-testid="area-strip-summary"
        >
          {summary}
        </span>
        <Button
          size="sm"
          intent={s.blocked ? 'primary' : 'ghost'}
          icon={<SlidersHorizontal className="h-3.5 w-3.5" />}
          title={`두 로봇 영역 간격 — 지금 ${mm(view.separation_mm)} mm${view.enabled ? '' : ' (검사 꺼짐)'}`}
          onClick={onEdit}
          data-testid="area-strip-edit"
        >
          간격 {mm(view.separation_mm)}
        </Button>
      </div>
      {/* design-lint-allow: no-control-height — 컨트롤이 아니라 그림(축·마커) 높이 */}
      <div className="relative h-8" aria-hidden="true">
        <div className="absolute inset-x-0 top-2 h-px bg-line-strong" />
        {s.robots.map((r) =>
          r.keepout ? (
            <div
              key={`k${r.id}`}
              className="absolute top-0 h-4 rounded-sm border border-dashed border-fault bg-fault-soft opacity-70"
              style={{ left: `${r.keepout.left}%`, width: `${r.keepout.width}%` }}
            />
          ) : null,
        )}
        {s.gapBar ? (
          <div
            className={cn(
              'absolute top-3.5 h-1.5 border-x border-b',
              s.blocked ? 'border-fault' : 'border-content-muted',
            )}
            style={{ left: `${s.gapBar.left}%`, width: `${s.gapBar.width}%` }}
          />
        ) : null}
        {s.robots.map((r) =>
          r.pos === null ? null : (
            <div
              key={`r${r.id}`}
              className="absolute top-0.5 flex -translate-x-1/2 flex-col items-center gap-1"
              style={{ left: `${r.pos}%` }}
            >
              <span
                className="h-3 w-3 rounded-full border-2 border-surface-raised"
                style={{ background: robots.chipOf(r.id).color }}
              />
              <span className="text-3xs leading-none whitespace-nowrap text-content-secondary tabular-nums">
                {r.name} {r.x === null ? '' : mm(r.x)}
              </span>
            </div>
          ),
        )}
        {/* 이 Task 구간은 마커 위에 — 한 점(목표 = 지금 X)이어도 보이게 최소 폭 */}
        {s.mine ? (
          <div
            className={cn('absolute top-1.5 h-1 rounded-full', s.blocked ? 'bg-fault' : 'bg-ok')}
            style={{ left: `${s.mine.left}%`, width: `max(${s.mine.width}%, 6px)` }}
            data-testid="area-strip-mine"
          />
        ) : null}
      </div>
    </div>
  )
}
