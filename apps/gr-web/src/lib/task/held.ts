// 지금 고른 로봇의 손 화물로 다음 맵 클릭이 DROP 이 되나 — 맵 클릭(`TaskIssue`)과 [작업 추가] 표시(`LayoutTab`)가 같이 쓴다.
import { groupOf, isPlanned } from '../jobs/model'
import { jobs as jobStore } from '../jobs/store'
import { robots } from '../robots'
import { stock as stockStore } from '../stock'
import { heldCargo, type PlanStep } from './plan'

export function heldForPlan(
  steps: readonly PlanStep[],
): { item_code: number; count: number } | null {
  const r = robots.current
  if (!r) return null
  const dropQueued = jobStore.all.some(
    (j) =>
      j.robot === r.id &&
      !isPlanned(j) &&
      groupOf(j.stage) !== 'ended' &&
      j.steps.length === 1 &&
      j.steps[0].request.type === 'DROP',
  )
  return heldCargo(steps, stockStore.hand(r.plc), dropQueued)
}
