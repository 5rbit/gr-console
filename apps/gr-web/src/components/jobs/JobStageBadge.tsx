// 작업 단계 캡슐 — 표의 단계 열 하나에만 선다(가이드 4절 ④). 할당은 상태 여섯 밖의 예정색(pending)이라
// `StatusBadge` 가 아니라 같은 모양을 토큰으로 직접 그린다.
import { STAGE_LABEL, STAGE_TONE, type JobStage } from '../../lib/jobs/model'
import { StatusBadge } from '../../lib/ui/StatusBadge'

export function JobStageBadge({ stage }: { stage: JobStage }) {
  const tone = STAGE_TONE[stage]
  if (tone === 'pending')
    return (
      <span
        className="inline-flex items-center gap-1 rounded bg-pending-soft px-1.5 py-0.5 text-2xs font-medium whitespace-nowrap text-pending-fg"
        data-stage={stage}
      >
        <span className="h-1.5 w-1.5 rounded-full bg-pending-fg" aria-hidden="true" />
        {STAGE_LABEL[stage]}
      </span>
    )
  return (
    <StatusBadge status={tone} data-stage={stage}>
      {STAGE_LABEL[stage]}
    </StatusBadge>
  )
}
